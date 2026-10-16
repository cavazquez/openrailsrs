//! Original rolling sound catalog (`ttype.dat`) and WORLD sound boundaries (`.ws`).
//! These are distinct from stationary `SoundSource` emitters and from rail geometry.
use std::path::Path;

use crate::{Ast, Atom, FormatError, parse_named_stf, read_msts_file_to_string};

#[derive(Clone, Debug, PartialEq)]
pub struct TrackSoundType {
    pub label: String,
    pub inside: String,
    pub outside: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackSoundTypes {
    pub types: Vec<TrackSoundType>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldSoundRegion {
    pub track_type: u32,
    pub rotation_y_rad: f64,
    pub track_item_ids: Vec<u32>,
}

fn items(ast: &Ast) -> &[Ast] {
    match ast {
        Ast::List(items) => items,
        _ => &[],
    }
}
fn text(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(Atom::String(s) | Atom::Symbol(s)) => Some(s.clone()),
        Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
        Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}
fn number(ast: &Ast) -> Option<f64> {
    text(ast)?.parse::<f64>().ok().filter(|v| v.is_finite())
}
fn uint(ast: &Ast) -> Option<u32> {
    let n = number(ast)?;
    (n >= 0. && n.fract() == 0. && n <= u32::MAX as f64).then_some(n as u32)
}
fn field<'a>(ast: &'a Ast, key: &str) -> Option<&'a Ast> {
    items(ast).iter().find(|v| {
        items(v)
            .first()
            .and_then(text)
            .is_some_and(|s| s.eq_ignore_ascii_case(key))
    })
}
fn missing(key: &str, context: &str) -> FormatError {
    FormatError::MissingField {
        key: key.into(),
        context: context.into(),
    }
}

impl TrackSoundTypes {
    pub fn from_path(path: &Path) -> Result<Self, FormatError> {
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > 16 * 1024 * 1024 {
            return Err(FormatError::UnexpectedAtom {
                key: "size".into(),
                context: path.display().to_string(),
                expected: "ttype.dat at most 16 MiB".into(),
            });
        }
        Self::parse(&read_msts_file_to_string(path)?)
    }

    pub fn parse(source: &str) -> Result<Self, FormatError> {
        // Unlike other STF files, this file starts with a bare count, not a root block.
        let body = source
            .lines()
            .filter(|l| !l.trim_start().starts_with("SIMISA"))
            .collect::<Vec<_>>()
            .join("\n");
        let ast = parse_named_stf(&format!("TrackTypes ( {body} )"))?;
        let count = items(&ast)
            .get(1)
            .and_then(uint)
            .ok_or_else(|| missing("count", "ttype.dat"))? as usize;
        let mut result = Self::default();
        for entry in super::named_blocks(&ast, "TrackType") {
            if result.types.len() >= count {
                result.warnings.push("Skipped extra TrackType".into());
                continue;
            }
            let values = items(entry);
            let value = |index| {
                values
                    .get(index)
                    .and_then(text)
                    .ok_or_else(|| missing("label/inside/outside", "TrackType"))
            };
            result.types.push(TrackSoundType {
                label: value(1)?,
                inside: value(2)?,
                outside: value(3)?,
            });
        }
        if result.types.len() < count {
            result.warnings.push(format!(
                "{} missing TrackType(s)",
                count - result.types.len()
            ));
        }
        Ok(result)
    }
}

impl WorldSoundRegion {
    pub fn from_path(path: &Path) -> Result<Vec<Self>, FormatError> {
        if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > 16 * 1024 * 1024 {
            return Err(FormatError::UnexpectedAtom {
                key: "size".into(),
                context: path.display().to_string(),
                expected: "WORLD sound file at most 16 MiB".into(),
            });
        }
        Self::parse(&read_msts_file_to_string(path)?)
    }

    pub fn parse(source: &str) -> Result<Vec<Self>, FormatError> {
        let ast = parse_named_stf(source)?;
        let mut result = vec![];
        for region in super::named_blocks(&ast, "SoundRegion") {
            let track_type = field(region, "SoundRegionTrackType")
                .and_then(|a| items(a).get(1))
                .and_then(uint)
                .ok_or_else(|| missing("SoundRegionTrackType", "SoundRegion"))?;
            let rotation_y_rad = field(region, "SoundRegionRoty")
                .and_then(|a| items(a).get(1))
                .and_then(number)
                .ok_or_else(|| missing("SoundRegionRoty", "SoundRegion"))?;
            let mut track_item_ids = vec![];
            for reference in super::named_blocks(region, "TrItemId") {
                // Table zero is the railway TDB. Road DB references cannot bind rolling sound.
                if items(reference).get(1).and_then(uint) == Some(0) {
                    let id = items(reference)
                        .get(2)
                        .and_then(uint)
                        .ok_or_else(|| missing("TrItemId", "SoundRegion"))?;
                    track_item_ids.push(id);
                }
            }
            result.push(Self {
                track_type,
                rotation_y_rad,
                track_item_ids,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn track_types_preserve_order_and_native_declared_count() {
        let t = TrackSoundTypes::parse("SIMISA@@@@@@@@@@JINX0T0t______\n2\nTrackType ( Default in.sms out.sms ) TrackType ( Bridge bridgein.sms bridgeout.sms ) TrackType ( Extra x y )").unwrap();
        assert_eq!(t.types.len(), 2);
        assert_eq!(t.types[1].outside, "bridgeout.sms");
        assert_eq!(t.warnings, ["Skipped extra TrackType"]);
        assert!(TrackSoundTypes::parse("1 TrackType ( Broken only.sms )").is_err());
        assert_eq!(
            TrackSoundTypes::parse("2 TrackType ( Default in.sms out.sms )")
                .unwrap()
                .warnings,
            ["1 missing TrackType(s)"]
        );
    }
    #[test]
    fn world_regions_bind_all_railway_items_and_ignore_ambient_and_road_sources() {
        let regions = WorldSoundRegion::parse("Tr_Worldsoundfile ( SoundSource ( FileName ( town.sms ) ) SoundRegion ( SoundRegionTrackType ( 3 ) SoundRegionRoty ( 1.2 ) TrItemId ( 0 17 ) TrItemId ( 0 18 ) TrItemId ( 1 9 ) ) )").unwrap();
        assert_eq!(
            regions,
            [WorldSoundRegion {
                track_type: 3,
                rotation_y_rad: 1.2,
                track_item_ids: vec![17, 18]
            }]
        );
        assert!(
            WorldSoundRegion::parse(
                "Tr_Worldsoundfile ( SoundRegion ( SoundRegionTrackType ( -1 ) ) )"
            )
            .is_err()
        );
    }
}
