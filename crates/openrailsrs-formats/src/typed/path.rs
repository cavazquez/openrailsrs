//! Parser for MSTS Path (`.pat`) files.
//!
//! A `.pat` file describes track points and the links ordering their traversal.
//! Native MSTS editor files use `TrackPDP (tileX tileZ x y z flag1 flag2)` where
//! `flag1`/`flag2` are junction/invalid flags (Open Rails `PathFile.cs`) — **not**
//! TDB node IDs. Compact fixtures may use `TrPathPDP (node_id junction_flag)`.
//!
//! Example (simplified TrPathPDP fixture):
//! ```text
//! SIMISA@@@@@@@@@@JINX0T0t______
//! (TrPathNode
//!     (TrPathName "Retiro to Victoria")
//!     (TrPathPDPs 4
//!         (TrPathPDP  1  0)
//!         (TrPathPDP  3  0)
//!         (TrPathPDP  5  1)
//!         (TrPathPDP  7  0)
//!     )
//! )
//! ```

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::parser::parse_named_stf;

use super::track_db::TrackVectorPoint;
use super::{atom_to_number, atom_to_string};

/// One point along the path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathDataPoint {
    /// TDB `TrackNode` ID when present (`TrPathPDP` / legacy). Native `TrackPDP` has none.
    pub node_id: Option<u32>,
    /// Junction flag (OR `flag1`): 2 ≈ junction, 1 ≈ endpoint / intermediate, etc.
    pub junction_flag: i32,
    /// Invalid / broken-path flag (OR `flag2`). Bit 3 set (8/9/12/13) often means broken.
    pub invalid_flag: i32,
    /// World position from native `TrackPDP` lines, when present.
    pub world: Option<TrackVectorPoint>,
}

impl PathDataPoint {
    /// True when Open Rails would treat this PDP as invalid (`invalidFlag == 9`).
    pub fn is_invalid(&self) -> bool {
        self.invalid_flag == 9
    }
}

/// Parsed representation of a `.pat` file.
#[derive(Clone, Debug, Default)]
pub struct PathFile {
    /// Human-readable name of the path (from `TrPathName`).
    pub name: String,
    /// Ordered sequence of path data points.
    pub pdps: Vec<PathDataPoint>,
}

impl PathFile {
    /// Parse from a pre-built AST.
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let name = extract_path_name(ast).unwrap_or_default();
        let mut pdps = extract_pdps(ast);
        if let Some(nodes) = find_path_node_table(ast) {
            pdps = ordered_main_path(&pdps, nodes)?;
        }
        Ok(Self { name, pdps })
    }

    /// Convenience: read and parse a `.pat` file from disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let text = crate::encoding::read_msts_file_to_string(path.as_ref())?;
        // Native files have Serial, TrackPDPs and TrackPath as sibling roots.
        // Parsing just the first parenthesis loses the TrPathNode links.
        let mut file = Self::from_ast(&parse_named_stf(&text)?)?;
        if file.pdps.is_empty() {
            file.pdps = extract_track_pdps_from_text(&text);
        }
        if file.pdps.is_empty() {
            return Err(FormatError::MissingField {
                key: "TrackPDP".into(),
                context: path.as_ref().display().to_string(),
            });
        }
        Ok(file)
    }

    /// First TDB node ID in the path, when PDPs carry node ids (`TrPathPDP`).
    pub fn start_node(&self) -> Option<u32> {
        self.pdps.iter().find_map(|p| p.node_id)
    }

    /// Last TDB node ID in the path, when PDPs carry node ids (`TrPathPDP`).
    pub fn end_node(&self) -> Option<u32> {
        self.pdps.iter().rev().find_map(|p| p.node_id)
    }

    /// True when any PDP carries a native world position.
    pub fn has_world_pdps(&self) -> bool {
        self.pdps.iter().any(|p| p.world.is_some())
    }
}

fn find_path_node_table(ast: &Ast) -> Option<&[Ast]> {
    let Ast::List(items) = ast else { return None };
    if matches!(items.first(), Some(Ast::Atom(Atom::Symbol(tag))) if tag.eq_ignore_ascii_case("TrPathNodes"))
    {
        return Some(items);
    }
    items.iter().find_map(find_path_node_table)
}

fn path_link_error(expected: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: "TrPathNodes".into(),
        context: "native PAT main path".into(),
        expected: expected.into(),
    }
}

fn path_index(value: Option<&Ast>) -> Option<u32> {
    let Ast::Atom(atom) = value? else { return None };
    let number = atom_to_number(atom)?;
    (number.is_finite() && number.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&number))
        .then_some(number as u32)
}

/// OR 1.6.1 PathFile: begin at TrPathNode[0], follow nextMainNode, and
/// resolve each fromPDP. The PDP table itself is neither ordered nor a path;
/// unused points and optional sidings must not become player waypoints.
fn ordered_main_path(
    points: &[PathDataPoint],
    table: &[Ast],
) -> Result<Vec<PathDataPoint>, FormatError> {
    let count =
        path_index(table.get(1)).ok_or_else(|| path_link_error("an unsigned node count"))? as usize;
    let mut nodes = Vec::new();
    for value in table.iter().skip(2) {
        let Ast::List(items) = value else { continue };
        if !matches!(items.first(), Some(Ast::Atom(Atom::Symbol(tag))) if tag.eq_ignore_ascii_case("TrPathNode"))
        {
            continue;
        }
        let next = path_index(items.get(2));
        let pdp = path_index(items.get(4));
        let (Some(next), Some(pdp)) = (next, pdp) else {
            return Err(path_link_error(
                "unsigned nextMainNode and correspondingPDP indices",
            ));
        };
        nodes.push((next, pdp));
    }
    if nodes.is_empty() || nodes.len() != count {
        return Err(path_link_error("the declared nonempty TrPathNode table"));
    }
    let mut ordered = Vec::new();
    let mut visited = vec![false; nodes.len()];
    let mut index = 0usize;
    loop {
        let Some(&(next, pdp)) = nodes.get(index) else {
            return Err(path_link_error("nextMainNode within the node table"));
        };
        if visited[index] {
            return Err(path_link_error(
                "a main path ending without cyclic node links",
            ));
        }
        visited[index] = true;
        ordered.push(
            points
                .get(pdp as usize)
                .ok_or_else(|| path_link_error("correspondingPDP within the point table"))?
                .clone(),
        );
        if next == u32::MAX {
            return Ok(ordered);
        }
        index = next as usize;
    }
}

fn extract_path_name(ast: &Ast) -> Option<String> {
    let Ast::List(items) = ast else { return None };
    for item in items {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("TrPathName")
                && let Some(Ast::Atom(a)) = sub.get(1)
            {
                return atom_to_string(a);
            }
            if let Some(n) = extract_path_name(item) {
                return Some(n);
            }
        }
    }
    None
}

fn extract_pdps(ast: &Ast) -> Vec<PathDataPoint> {
    let mut out = Vec::new();
    collect_pdps(ast, &mut out);
    out
}

fn collect_pdps(ast: &Ast, out: &mut Vec<PathDataPoint>) {
    let Ast::List(items) = ast else { return };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        // TrPathPDP <node_id> <junction_flag>  (compact / fixture format with real TDB ids)
        if head.eq_ignore_ascii_case("TrPathPDP")
            && items.len() >= 3
            && let Some(pdp) = pdp_from_tr_path_pdp(items.get(1), items.get(2))
        {
            out.push(pdp);
            return;
        }
        // TrackPDP tileX tileZ x y z junction_flag invalid_flag  (native MSTS / OR PathFile.cs)
        if head.eq_ignore_ascii_case("TrackPDP") && items.len() >= 8 {
            if let Some(pdp) = pdp_from_track_pdp(items) {
                out.push(pdp);
            }
            return;
        }
    }

    for child in items {
        collect_pdps(child, out);
    }
}

fn pdp_from_tr_path_pdp(node: Option<&Ast>, flag: Option<&Ast>) -> Option<PathDataPoint> {
    let node_id = node.and_then(|a| match a {
        Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
        _ => None,
    })?;
    let junction_flag = flag.and_then(|a| match a {
        Ast::Atom(at) => atom_to_number(at).map(|n| n as i32),
        _ => None,
    })?;
    Some(PathDataPoint {
        node_id: Some(node_id),
        junction_flag,
        invalid_flag: 0,
        world: None,
    })
}

fn pdp_from_track_pdp(items: &[Ast]) -> Option<PathDataPoint> {
    let nums: Vec<f64> = items
        .iter()
        .skip(1)
        .take(7)
        .filter_map(|a| match a {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        })
        .collect();
    if nums.len() < 7 {
        return None;
    }
    Some(PathDataPoint {
        node_id: None,
        junction_flag: nums[5] as i32,
        invalid_flag: nums[6] as i32,
        world: Some(TrackVectorPoint {
            tile_x: nums[0] as i32,
            tile_z: nums[1] as i32,
            x: nums[2],
            y: nums[3],
            z: nums[4],
        }),
    })
}

/// Fallback when the `.pat` preamble parses as a tiny `( 1 )` stub instead of the full path.
fn extract_track_pdps_from_text(text: &str) -> Vec<PathDataPoint> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("TrackPDP") else {
            continue;
        };
        if rest.starts_with('s') || rest.starts_with('S') {
            continue;
        }
        let inner = rest
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
        let nums: Vec<f64> = inner
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        // Native: tileX tileZ x y z junction_flag invalid_flag
        if nums.len() >= 7 {
            out.push(PathDataPoint {
                node_id: None,
                junction_flag: nums[5] as i32,
                invalid_flag: nums[6] as i32,
                world: Some(TrackVectorPoint {
                    tile_x: nums[0] as i32,
                    tile_z: nums[1] as i32,
                    x: nums[2],
                    y: nums[3],
                    z: nums[4],
                }),
            });
        } else if nums.len() >= 2 {
            // Legacy compact fallback: node_id junction_flag (no world).
            out.push(PathDataPoint {
                node_id: Some(nums[nums.len() - 2] as u32),
                junction_flag: nums[nums.len() - 1] as i32,
                invalid_flag: 0,
                world: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    const LINKED_NATIVE_PATH: &str = "SIMISA@@@@@@@@@@JINX0P0t______\nSerial ( 1 )
        TrackPDPs (
            TrackPDP ( 0 0 100 0 0 2 0 )
            TrackPDP ( 0 0 900 0 0 2 0 )
            TrackPDP ( 0 0 0 0 0 1 1 )
            TrackPDP ( 0 0 200 0 0 1 1 )
        )
        TrackPath ( TrPathName ( Ordered ) TrPathNodes ( 4
            TrPathNode ( 00000000 2 1 2 )
            TrPathNode ( 00000000 4294967295 4294967295 1 )
            TrPathNode ( 00000000 3 4294967295 0 )
            TrPathNode ( 00000000 4294967295 4294967295 3 )
        ) )";

    #[test]
    fn native_path_follows_main_links_from_node_zero_and_ignores_optional_siding() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), LINKED_NATIVE_PATH).unwrap();
        let path = PathFile::from_path(file.path()).unwrap();
        assert_eq!(path.name, "Ordered");
        assert_eq!(
            path.pdps
                .iter()
                .map(|p| p.world.unwrap().x)
                .collect::<Vec<_>>(),
            [0.0, 100.0, 200.0]
        );
    }

    #[test]
    fn native_path_rejects_dangling_links_points_and_cycles() {
        for (old, new, message) in [
            ("00000000 2 1 2", "00000000 99 1 2", "nextMainNode within"),
            (
                "00000000 2 1 2",
                "00000000 2 1 99",
                "correspondingPDP within",
            ),
            (
                "00000000 3 4294967295 0",
                "00000000 0 4294967295 0",
                "cyclic node links",
            ),
            ("TrPathNodes ( 4", "TrPathNodes ( 5", "declared nonempty"),
        ] {
            let source = LINKED_NATIVE_PATH.replace(old, new);
            let ast = parse_named_stf(&source).unwrap();
            assert!(
                PathFile::from_ast(&ast)
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
    }

    #[test]
    fn parse_track_pdp_native_pat_flags_not_node_ids() {
        let text = r#"
( TrPathNode
    ( TrPathName "Test" )
    ( TrackPDPs 2
        ( TrackPDP -6079 14925 -961.337 28.558 -71.912 1 0 )
        ( TrackPDP -6080 14925 998.528 28.558 306.463 2 0 )
    )
)"#;
        let ast = parse_from_first_paren(text).expect("parse pat");
        let path = PathFile::from_ast(&ast).expect("path");
        assert_eq!(path.pdps.len(), 2);
        assert_eq!(path.pdps[0].node_id, None);
        assert_eq!(path.pdps[0].junction_flag, 1);
        assert_eq!(path.pdps[0].invalid_flag, 0);
        let w = path.pdps[0].world.expect("world");
        assert_eq!(w.tile_x, -6079);
        assert!((w.x + 961.337).abs() < 0.01);
        assert_eq!(path.pdps[1].node_id, None);
        assert_eq!(path.pdps[1].junction_flag, 2);
        assert_eq!(path.pdps[1].invalid_flag, 0);
        assert!(path.has_world_pdps());
        assert_eq!(path.start_node(), None);
        assert_eq!(path.end_node(), None);
    }

    #[test]
    fn parse_tr_path_pdp_keeps_real_node_ids() {
        let text = r#"
( TrPathNode
    ( TrPathName "Fixture" )
    ( TrPathPDPs 2
        ( TrPathPDP 1 0 )
        ( TrPathPDP 3 1 )
    )
)"#;
        let ast = parse_from_first_paren(text).expect("parse pat");
        let path = PathFile::from_ast(&ast).expect("path");
        assert_eq!(path.pdps.len(), 2);
        assert_eq!(path.pdps[0].node_id, Some(1));
        assert_eq!(path.pdps[0].junction_flag, 0);
        assert_eq!(path.pdps[1].node_id, Some(3));
        assert_eq!(path.pdps[1].junction_flag, 1);
        assert_eq!(path.start_node(), Some(1));
        assert_eq!(path.end_node(), Some(3));
        assert!(!path.has_world_pdps());
    }
}
