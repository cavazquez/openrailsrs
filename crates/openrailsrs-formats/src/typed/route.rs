use std::path::Path;

use crate::ast::Ast;
use crate::dispatch::parse_msts_file;
use crate::error::FormatError;
use crate::typed::TrackVectorPoint;

use super::find_list_value;
use super::{atom_to_number, find_optional_string_field};

/// MSTS `.trk` `RouteStart ( tileX tileZ localX localZ )` — default player spawn tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteStart {
    pub tile_x: i32,
    pub tile_z: i32,
    pub local_x_m: f64,
    pub local_z_m: f64,
}

impl RouteStart {
    /// Bevy world X/Y/Z (Y = 0; elevation unknown from RouteStart alone).
    pub fn bevy_position(self) -> (f32, f32, f32) {
        TrackVectorPoint {
            tile_x: self.tile_x,
            tile_z: self.tile_z,
            x: self.local_x_m,
            y: 0.0,
            z: self.local_z_m,
        }
        .bevy_position()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteFile {
    pub route_id: String,
    pub name: String,
    pub route_start: Option<RouteStart>,
}

impl RouteFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let route_id =
            find_optional_string_field(ast, &["RouteID"], "Tr_RouteFile")?.ok_or_else(|| {
                FormatError::MissingField {
                    key: "RouteID".to_string(),
                    context: "Tr_RouteFile".to_string(),
                }
            })?;
        let name = find_optional_string_field(ast, &["Name"], "Tr_RouteFile")?
            .unwrap_or_else(|| route_id.clone());
        let route_start = parse_route_start(ast);
        Ok(Self {
            route_id,
            name,
            route_start,
        })
    }

    /// Load the first `.trk` in `route_dir` (same search roots as `.tdb`).
    pub fn from_route_dir(route_dir: &Path) -> Result<Self, FormatError> {
        let path = find_trk_path(route_dir).ok_or_else(|| FormatError::MissingField {
            key: ".trk".to_string(),
            context: route_dir.display().to_string(),
        })?;
        let file = parse_msts_file(&path)?;
        match file {
            crate::MstsFile::Route(route) => Ok(route),
            _ => Err(FormatError::UnexpectedAtom {
                key: "extension".into(),
                context: path.display().to_string(),
                expected: "Route .trk".into(),
            }),
        }
    }
}

fn find_trk_path(route_dir: &Path) -> Option<std::path::PathBuf> {
    for dir in trk_search_dirs(route_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("trk"))
            {
                return Some(path);
            }
        }
    }
    None
}

fn trk_search_dirs(route_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(parent) = route_dir.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs
}

fn parse_route_start(ast: &Ast) -> Option<RouteStart> {
    let value = find_list_value(ast, "RouteStart")?;
    let nums = collect_numbers(value);
    if nums.len() < 4 {
        return None;
    }
    Some(RouteStart {
        tile_x: nums[0].round() as i32,
        tile_z: nums[1].round() as i32,
        local_x_m: nums[2],
        local_z_m: nums[3],
    })
}

fn collect_numbers(ast: &Ast) -> Vec<f64> {
    match ast {
        Ast::Atom(atom) => atom_to_number(atom).into_iter().collect(),
        Ast::List(items) => items.iter().flat_map(collect_numbers).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn parse_route_start_from_trk() {
        let src = r#"
SIMISA@@@@@@@@@@JINX0t0t______
(Tr_RouteFile
  (RouteID "chiltern")
  (Name "Chiltern")
  (RouteStart ( -6080 14925 891.831 582.756 ))
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let route = RouteFile::from_ast(&ast).expect("route");
        let start = route.route_start.expect("RouteStart");
        assert_eq!(start.tile_x, -6080);
        assert_eq!(start.tile_z, 14925);
        assert!((start.local_x_m - 891.831).abs() < 0.001);
        assert!((start.local_z_m - 582.756).abs() < 0.001);
    }

    #[test]
    fn typed_minimal_trk_has_no_route_start() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/typed_minimal.trk");
        let text = std::fs::read_to_string(&path).expect("read");
        let ast = parse_from_first_paren(&text).expect("parse");
        let route = RouteFile::from_ast(&ast).expect("route");
        assert!(route.route_start.is_none());
    }
}
