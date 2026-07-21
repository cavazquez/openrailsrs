use std::path::{Path, PathBuf};

use crate::ast::{Ast, Atom};
use crate::encoding::{read_msts_file_to_string, resolve_path_case_insensitive};
use crate::error::FormatError;
use crate::parser::{parse_all_top_level_lenient, parse_first};
use crate::typed::TrackVectorPoint;

use super::activity::find_string_field;
use super::{atom_to_number, atom_to_string};

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

/// Visual overhead-wire parameters from `.trk` (`Electrified` / `OverheadWireHeight` / ORTS*).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverheadWireParams {
    /// `Electrified` — Open Rails default is `true` when the token is absent.
    pub electrified: bool,
    /// Contact-wire height above rail (metres). OR default `6.0`.
    pub height_m: f32,
    /// `ORTSDoubleWireEnabled` interpreted as on (`"On"` / non-empty truthy).
    pub double_wire: bool,
    /// Vertical offset of the messenger wire above the contact wire (metres).
    pub double_wire_height_m: f32,
}

impl Default for OverheadWireParams {
    fn default() -> Self {
        Self {
            electrified: true,
            height_m: 6.0,
            double_wire: false,
            double_wire_height_m: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteFile {
    pub route_id: String,
    pub name: String,
    pub route_start: Option<RouteStart>,
    /// Path of the `.trk` that was loaded (OpenRails override when present).
    pub source_path: Option<PathBuf>,
    /// Overhead wire / electrification visual flags (#36).
    pub overhead_wire: OverheadWireParams,
}

impl RouteFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let route_id =
            find_string_field(ast, &["RouteID"]).ok_or_else(|| FormatError::MissingField {
                key: "RouteID".to_string(),
                context: "Tr_RouteFile".to_string(),
            })?;
        let name = find_string_field(ast, &["Name"]).unwrap_or_else(|| route_id.clone());
        let route_start = parse_route_start(ast);
        Ok(Self {
            route_id,
            name,
            route_start,
            source_path: None,
            overhead_wire: parse_overhead_wire_params(ast),
        })
    }

    /// Load the route `.trk` for `route_dir` (OpenRails override + includes when present).
    pub fn from_route_dir(route_dir: &Path) -> Result<Self, FormatError> {
        let path = find_trk_path(route_dir).ok_or_else(|| FormatError::MissingField {
            key: ".trk".to_string(),
            context: route_dir.display().to_string(),
        })?;
        Self::from_trk_path(&path)
    }

    /// Load a concrete `.trk`, resolving `include ( ... )` like Open Rails STF.
    pub fn from_trk_path(path: &Path) -> Result<Self, FormatError> {
        let mut visited = Vec::new();
        let ast = load_trk_ast_with_includes(path, &mut visited)?;
        let mut route = Self::from_ast(&ast)?;
        route.source_path = Some(path.to_path_buf());
        Ok(route)
    }
}

/// Deterministic `.trk` selection for a route directory.
///
/// Preference (Open Rails `ORFileHelper.FindORTSFile` + stable fallback):
/// 1. `OpenRails/<routeName>.trk` / `openrails/<routeName>.trk`
/// 2. `<routeDir>/<routeName>.trk`
/// 3. First `*.trk` in those directories, sorted by path (case-insensitive)
pub fn find_trk_path(route_dir: &Path) -> Option<PathBuf> {
    let route_name = route_dir.file_name()?.to_string_lossy();
    let named = format!("{route_name}.trk");

    let preferred = [
        route_dir.join("OpenRails").join(&named),
        route_dir.join("openrails").join(&named),
        route_dir.join(&named),
    ];
    for candidate in &preferred {
        if let Some(resolved) = resolve_path_case_insensitive(candidate) {
            if resolved.is_file() {
                return Some(resolved);
            }
        }
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }

    let mut found = Vec::new();
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
                found.push(path);
            }
        }
    }
    found.sort_by(|a, b| {
        a.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&b.to_string_lossy().to_ascii_lowercase())
    });
    found.into_iter().next()
}

fn trk_search_dirs(route_dir: &Path) -> Vec<PathBuf> {
    vec![
        route_dir.join("OpenRails"),
        route_dir.join("openrails"),
        route_dir.to_path_buf(),
    ]
}

fn load_trk_ast_with_includes(path: &Path, visited: &mut Vec<PathBuf>) -> Result<Ast, FormatError> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if visited.iter().any(|p| p == &canonical) {
        return Err(FormatError::UnexpectedAtom {
            key: "include".into(),
            context: path.display().to_string(),
            expected: "acyclic include graph".into(),
        });
    }
    visited.push(canonical);

    let text = read_msts_file_to_string(path)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut trees = Vec::new();
    for include_path in scan_trk_includes(&text, base) {
        let resolved = resolve_path_case_insensitive(&include_path).unwrap_or(include_path);
        if resolved.is_file() {
            trees.push(load_trk_ast_with_includes(&resolved, visited)?);
        }
    }
    // Route overlays are multiple top-level `Token ( ... )` blocks (include + ORTS*).
    // Base `.trk` files are usually a single `Tr_RouteFile ( ... )` expression.
    let blocks = parse_all_top_level_lenient(&text);
    if blocks.is_empty() {
        return Err(FormatError::UnexpectedEof);
    }
    trees.extend(blocks);
    Ok(Ast::List(trees))
}

fn scan_trk_includes(text: &str, base: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut pos = 0usize;
    while let Some(found) = text[pos..].find("include") {
        let abs = pos + found;
        if !tag_at_boundary(text, abs, "include".len()) {
            pos = abs + "include".len();
            continue;
        }
        let after_tag = abs + "include".len();
        let Some(rel_paren) = text[after_tag..].find('(') else {
            pos = abs + 1;
            continue;
        };
        let open = after_tag + rel_paren;
        if let Ok(Ast::List(items)) = parse_first(&text[open..]) {
            if let Some(rel) = items.first().and_then(ast_to_include_path) {
                paths.push(base.join(rel.trim()));
            }
        }
        pos = open + 1;
    }
    paths
}

fn tag_at_boundary(text: &str, start: usize, len: usize) -> bool {
    let before_ok = start == 0
        || text.as_bytes()[start - 1].is_ascii_whitespace()
        || matches!(text.as_bytes()[start - 1], b'(' | b')');
    let end = start + len;
    let after_ok = end >= text.len()
        || text.as_bytes()[end].is_ascii_whitespace()
        || matches!(text.as_bytes()[end], b'(' | b')');
    before_ok && after_ok
}

fn ast_to_include_path(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(atom) => atom_to_string(atom),
        Ast::List(items) => items
            .get(1)
            .or_else(|| items.first())
            .and_then(|a| match a {
                Ast::Atom(atom) => atom_to_string(atom),
                _ => None,
            }),
    }
}

fn parse_route_start(ast: &Ast) -> Option<RouteStart> {
    let nums = find_numeric_field(ast, "RouteStart")?;
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

fn parse_overhead_wire_params(ast: &Ast) -> OverheadWireParams {
    let mut params = OverheadWireParams::default();
    if let Some(nums) = find_numeric_field(ast, "Electrified") {
        if let Some(v) = nums.first() {
            // MSTS often stores flags as hex ints (`00000001`); non-zero ⇒ true.
            params.electrified = *v != 0.0;
        }
    }
    if let Some(nums) = find_numeric_field(ast, "OverheadWireHeight") {
        if let Some(v) = nums.first() {
            if v.is_finite() && *v > 0.0 {
                params.height_m = *v as f32;
            }
        }
    }
    if let Some(enabled) = find_string_field(ast, &["ORTSDoubleWireEnabled"]) {
        params.double_wire = enabled.eq_ignore_ascii_case("On")
            || enabled.eq_ignore_ascii_case("true")
            || enabled == "1";
    }
    if let Some(nums) = find_numeric_field(ast, "ORTSDoubleWireHeight") {
        if let Some(v) = nums.first() {
            if v.is_finite() && *v > 0.0 {
                params.double_wire_height_m = *v as f32;
            }
        }
    }
    params
}

/// Nested `(RouteStart n n n n)` / `(RouteStart (n n n n))` and flat `RouteStart ( n n n n )`.
fn find_numeric_field(ast: &Ast, key: &str) -> Option<Vec<f64>> {
    let Ast::List(items) = ast else {
        return None;
    };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case(key) {
            let nums = collect_numbers_from_tail(&items[1..]);
            if !nums.is_empty() {
                return Some(nums);
            }
        }
    }

    for i in 0..items.len().saturating_sub(1) {
        if let Ast::Atom(Atom::Symbol(sym)) = &items[i] {
            if sym.eq_ignore_ascii_case(key) {
                let nums = collect_numbers(&items[i + 1]);
                if !nums.is_empty() {
                    return Some(nums);
                }
            }
        }
    }

    for child in items {
        if let Some(nums) = find_numeric_field(child, key) {
            return Some(nums);
        }
    }
    None
}

fn collect_numbers_from_tail(items: &[Ast]) -> Vec<f64> {
    items.iter().flat_map(collect_numbers).collect()
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
    fn parse_route_start_from_nested_trk() {
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
    fn parse_route_start_from_flat_msts_trk() {
        // Real Chiltern / MSTS layout: `Token ( values )` siblings inside Tr_RouteFile.
        let src = r#"
SIMISA@@@@@@@@@@JINX0r0t______

Tr_RouteFile (
	RouteID ( Chiltern )
	Name ( "Chiltern" )
	RouteStart ( -6079 14925 -896 182 )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let route = RouteFile::from_ast(&ast).expect("route");
        assert_eq!(route.route_id, "Chiltern");
        assert_eq!(route.name, "Chiltern");
        let start = route.route_start.expect("RouteStart");
        assert_eq!(start.tile_x, -6079);
        assert_eq!(start.tile_z, 14925);
        assert!((start.local_x_m - -896.0).abs() < 0.001);
        assert!((start.local_z_m - 182.0).abs() < 0.001);
    }

    #[test]
    fn parse_overhead_wire_from_trk() {
        let src = r#"
Tr_RouteFile (
	RouteID ( Demo )
	Name ( "Demo" )
	Electrified ( 00000001 )
	OverheadWireHeight ( 7.23 )
	ORTSDoubleWireEnabled ( On )
	ORTSDoubleWireHeight ( 1.5 )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let route = RouteFile::from_ast(&ast).expect("route");
        assert!(route.overhead_wire.electrified);
        assert!((route.overhead_wire.height_m - 7.23).abs() < 0.001);
        assert!(route.overhead_wire.double_wire);
        assert!((route.overhead_wire.double_wire_height_m - 1.5).abs() < 0.001);
    }

    #[test]
    fn electrified_zero_disables_wire() {
        let src = r#"
Tr_RouteFile (
	RouteID ( Demo )
	Name ( "Demo" )
	Electrified ( 00000000 )
	OverheadWireHeight ( 6.0 )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let route = RouteFile::from_ast(&ast).expect("route");
        assert!(!route.overhead_wire.electrified);
    }

    #[test]
    fn typed_minimal_trk_has_no_route_start() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/typed_minimal.trk");
        let route = RouteFile::from_trk_path(&path).expect("route");
        assert!(route.route_start.is_none());
        assert_eq!(route.route_id, "typed_route");
    }

    #[test]
    fn openrails_override_include_supplies_route_start() {
        let dir = tempfile::tempdir().expect("tempdir");
        let route_dir = dir.path().join("Chiltern");
        let openrails = route_dir.join("OpenRails");
        std::fs::create_dir_all(&openrails).expect("mkdir");

        let base = route_dir.join("Chiltern.trk");
        std::fs::write(
            &base,
            r#"SIMISA@@@@@@@@@@JINX0r0t______

Tr_RouteFile (
	RouteID ( Chiltern )
	Name ( "Chiltern" )
	RouteStart ( -6079 14925 -896 182 )
)
"#,
        )
        .expect("write base");

        let overlay = openrails.join("Chiltern.trk");
        std::fs::write(
            &overlay,
            r#"
include ( "../Chiltern.trk" )
ORTSDefaultTurntableSMS ( turntable.sms )
"#,
        )
        .expect("write overlay");

        let selected = find_trk_path(&route_dir).expect("trk path");
        assert!(
            selected.ends_with("OpenRails/Chiltern.trk")
                || selected.ends_with("OpenRails\\Chiltern.trk"),
            "selected={selected:?}"
        );

        let route = RouteFile::from_route_dir(&route_dir).expect("route");
        let start = route.route_start.expect("RouteStart via include");
        assert_eq!(
            (start.tile_x, start.tile_z, start.local_x_m, start.local_z_m),
            (-6079, 14925, -896.0, 182.0)
        );
        assert_eq!(route.source_path.as_deref(), Some(selected.as_path()));
    }

    #[test]
    fn find_trk_path_is_deterministic_without_openrails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let route_dir = dir.path().join("DemoRoute");
        std::fs::create_dir_all(&route_dir).expect("mkdir");
        std::fs::write(
            route_dir.join("DemoRoute.trk"),
            r#"(Tr_RouteFile (RouteID "demo") (Name "Demo"))"#,
        )
        .expect("write");
        std::fs::write(
            route_dir.join("zzz_other.trk"),
            r#"(Tr_RouteFile (RouteID "other") (Name "Other"))"#,
        )
        .expect("write other");

        let a = find_trk_path(&route_dir).expect("a");
        let b = find_trk_path(&route_dir).expect("b");
        assert_eq!(a, b);
        assert!(a.ends_with("DemoRoute.trk"));
    }
}
