//! Native Open Rails timetable matrix. Unknown commands remain explicit data
//! for the executor to reject; reading a file never enables an approximation.
use crate::{FormatError, read_msts_file_to_string};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct NativeTimetable {
    pub description: String,
    pub services: Vec<NativeTimetableService>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NativeTimetableService {
    pub id: String,
    pub path: String,
    pub consist: String,
    pub start: String,
    pub stops: Vec<NativeTimetableStop>,
    pub dispose: String,
    pub properties: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NativeTimetableStop {
    pub station: String,
    pub timing: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TimetableCommand {
    pub name: String,
    pub value: Option<String>,
    pub qualifiers: Vec<(String, Option<String>)>,
}
fn error(message: impl Into<String>) -> FormatError {
    FormatError::UnexpectedToken {
        offset: 0,
        message: message.into(),
    }
}
impl NativeTimetable {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let length = std::fs::metadata(path)
            .map_err(|source| error(format!("{}: {source}", path.display())))?
            .len();
        if length > 8 * 1024 * 1024 {
            return Err(error("Native timetable exceeds 8 MiB"));
        }
        Self::from_text(&read_msts_file_to_string(path)?)
    }
    pub fn from_text(text: &str) -> Result<Self, FormatError> {
        let text = text.trim_start_matches('\u{feff}');
        let delimiter = text
            .chars()
            .next()
            .filter(|c| matches!(c, ';' | ',' | '\t'))
            .ok_or_else(|| error("Native timetable starts with its delimiter"))?;
        let rows = matrix(text, delimiter)?;
        let header = rows
            .first()
            .ok_or_else(|| error("Empty native timetable"))?;
        if header.len() > 2048 || rows.len() > 4096 {
            return Err(error("Native timetable matrix exceeds its budget"));
        }
        let descriptor = |row: &Vec<String>| {
            row.first()
                .map(|s| {
                    s.split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                })
                .unwrap_or_default()
        };
        let property = |name: &str, column: usize| {
            rows.iter()
                .find(|r| descriptor(r) == name)
                .and_then(|r| r.get(column))
                .cloned()
                .unwrap_or_default()
        };
        let description = rows
            .iter()
            .find(|r| descriptor(r) == "#comment")
            .and_then(|r| r.iter().skip(1).find(|s| !s.is_empty()))
            .cloned()
            .unwrap_or_default();
        let mut services = vec![];
        let mut ids = std::collections::HashSet::new();
        for (column, id) in header.iter().enumerate().skip(1) {
            if id.is_empty() || id.starts_with('#') {
                continue;
            }
            if !ids.insert(id.to_ascii_lowercase()) {
                return Err(error(format!("Duplicate native timetable service {id}")));
            }
            let path = property("#path", column);
            let consist = property("#consist", column);
            let start = property("#start", column);
            if path.is_empty() || start.is_empty() {
                return Err(error(format!("Service {id} lacks its original path/start")));
            }
            let mut stops = vec![];
            let mut properties = vec![];
            for row in rows.iter().skip(1) {
                let name = row.first().cloned().unwrap_or_default();
                let cell = row.get(column).cloned().unwrap_or_default();
                if name.is_empty() || cell.is_empty() || name.starts_with("#comment") {
                    continue;
                }
                if name.starts_with('#') {
                    properties.push((name, cell));
                } else {
                    stops.push(NativeTimetableStop {
                        station: name,
                        timing: cell,
                    });
                }
            }
            services.push(NativeTimetableService {
                id: id.clone(),
                path,
                consist,
                start,
                stops,
                dispose: property("#dispose", column),
                properties,
            });
        }
        Ok(Self {
            description,
            services,
        })
    }
}
// OR accepts quoted cells and doubled quotes. Embedded line breaks are part of
// a cell, and must never create extra train columns.
fn matrix(text: &str, delimiter: char) -> Result<Vec<Vec<String>>, FormatError> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if quoted && chars.peek() == Some(&'"') {
                chars.next();
                cell.push('"');
            } else {
                quoted = !quoted;
            }
        } else if !quoted && c == delimiter {
            row.push(cell.trim().to_string());
            cell.clear();
        } else if !quoted && c == '\n' {
            row.push(cell.trim().to_string());
            cell.clear();
            rows.push(std::mem::take(&mut row));
            if rows.len() > 4096 {
                return Err(error("Too many native timetable rows"));
            }
        } else if c != '\r' {
            cell.push(c);
        }
        if cell.len() > 64 * 1024 || row.len() > 2048 {
            return Err(error("Native timetable cell/column budget exceeded"));
        }
    }
    if quoted {
        return Err(error("Unclosed native timetable cell"));
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell.trim().to_string());
        rows.push(row);
    }
    Ok(rows)
}
pub fn timetable_commands(text: &str) -> Result<Vec<TimetableCommand>, FormatError> {
    let mut commands = vec![];
    for piece in text.split('$').skip(1) {
        // OR separates qualifiers with '/', with or without surrounding spaces.
        let mut words = piece.split('/').map(str::trim);
        let head = words
            .next()
            .ok_or_else(|| error("Empty timetable command"))?;
        let (name, value) = head
            .split_once('=')
            .map_or((head, None), |(n, v)| (n, Some(v.to_string())));
        if name.is_empty() {
            return Err(error("Empty timetable command name"));
        }
        let mut qualifiers = vec![];
        for word in words {
            let qualifier = word;
            if qualifier.is_empty() {
                return Err(error("Empty timetable qualifier"));
            }
            let (key, value) = qualifier
                .split_once('=')
                .map_or((qualifier, None), |(k, v)| (k, Some(v.to_string())));
            qualifiers.push((key.to_ascii_lowercase(), value));
        }
        commands.push(TimetableCommand {
            name: name.to_ascii_lowercase(),
            value,
            qualifiers,
        });
    }
    Ok(commands)
}
pub fn timetable_time_s(text: &str) -> Result<f64, FormatError> {
    let parts = text.trim().split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return Err(error(format!("Invalid timetable time {text}")));
    }
    let values = parts
        .iter()
        .map(|p| {
            p.parse::<u32>()
                .map_err(|_| error("Invalid timetable time"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values[0] >= 48 || values[1] >= 60 || values.get(2).is_some_and(|s| *s >= 60) {
        return Err(error("Timetable time outside two-day horizon"));
    }
    Ok(f64::from(
        values[0] * 3600 + values[1] * 60 + values.get(2).copied().unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matrix_preserves_dispose_detach_attach_forms_and_comment_columns() {
        let text = ";;#comment;Inbound;Return\n#comment;;Example;First;Second\n#path /binary;;;in;back\n#consist;;;<loco+cars>;loco\n#start;;;9:05;9:35\nStation;;;9:20-9:25 $detach /units=2 /forms=Return;$attach=Inbound\n#dispose;;;$forms=Return /reverse;$static\n";
        let file = NativeTimetable::from_text(text).unwrap();
        assert_eq!(file.services.len(), 2);
        let train = &file.services[0];
        assert_eq!(train.id, "Inbound");
        assert_eq!(train.consist, "<loco+cars>");
        let dispose = timetable_commands(&train.dispose).unwrap();
        assert_eq!(dispose[0].name, "forms");
        assert_eq!(dispose[0].value.as_deref(), Some("Return"));
        let detach = timetable_commands(&train.stops[0].timing).unwrap();
        assert_eq!(detach[0].name, "detach");
        assert_eq!(detach[0].qualifiers[0], ("units".into(), Some("2".into())));
        assert_eq!(timetable_time_s("09:05:30").unwrap(), 32730.0);
    }
    #[test]
    fn quoted_cells_do_not_change_the_number_of_services() {
        let file=NativeTimetable::from_text(";\"Train;A\"\n#path;p\n#consist;c\n#start;09:00\nStation;\"09:10 $unknown /text=abc\"\n").unwrap();
        assert_eq!(file.services[0].id, "Train;A");
        assert_eq!(
            timetable_commands(&file.services[0].stops[0].timing).unwrap()[0].name,
            "unknown"
        );
        assert!(NativeTimetable::from_text(";Train\n#path;p\n#start;\"broken").is_err());
    }
}
