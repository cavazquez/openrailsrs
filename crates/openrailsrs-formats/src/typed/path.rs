//! Parser for MSTS Path (`.pat`) files.
//!
//! A `.pat` file describes an ordered sequence of track nodes that constitute
//! a train path through the route.  The key section is `TrackPDP` (Track Path
//! Data Points), each of which references a `TrackNode` ID and carries a
//! junction-direction flag.
//!
//! Example (simplified):
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
use crate::parser::parse_from_first_paren;

use super::{atom_to_number, atom_to_string};

/// One point along the path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathDataPoint {
    /// `TrackNode` ID referenced by this data point.
    pub node_id: u32,
    /// Junction direction:  0 = straight / main, 1 = diverging, -1 = reverse.
    pub junction_flag: i32,
}

/// Parsed representation of a `.pat` file.
#[derive(Clone, Debug, Default)]
pub struct PathFile {
    /// Human-readable name of the path (from `TrPathName`).
    pub name: String,
    /// Ordered sequence of node references.
    pub pdps: Vec<PathDataPoint>,
}

impl PathFile {
    /// Parse from a pre-built AST.
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let name = extract_path_name(ast).unwrap_or_default();
        let pdps = extract_pdps(ast);
        Ok(Self { name, pdps })
    }

    /// Convenience: read and parse a `.pat` file from disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let text = crate::encoding::read_msts_file_to_string(path.as_ref())?;
        let mut file = match parse_from_first_paren(&text) {
            Ok(ast) => Self::from_ast(&ast)?,
            Err(_) => Self::default(),
        };
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

    /// First node ID in the path (start).
    pub fn start_node(&self) -> Option<u32> {
        self.pdps.first().map(|p| p.node_id)
    }

    /// Last node ID in the path (destination).
    pub fn end_node(&self) -> Option<u32> {
        self.pdps.last().map(|p| p.node_id)
    }
}

fn extract_path_name(ast: &Ast) -> Option<String> {
    let Ast::List(items) = ast else { return None };
    for item in items {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("TrPathName") {
                if let Some(Ast::Atom(a)) = sub.get(1) {
                    return atom_to_string(a);
                }
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
        // TrPathPDP <node_id> <junction_flag>
        if head.eq_ignore_ascii_case("TrPathPDP") && items.len() >= 3 {
            if let Some(pdp) = pdp_from_node_and_flag(items.get(1), items.get(2)) {
                out.push(pdp);
                return;
            }
        }
        // TrackPDP tileX tileZ x y z node_id junction_flag  (native MSTS editor format)
        if head.eq_ignore_ascii_case("TrackPDP") && items.len() >= 8 {
            if let Some(pdp) = pdp_from_node_and_flag(items.get(6), items.get(7)) {
                out.push(pdp);
            }
            return;
        }
    }

    for child in items {
        collect_pdps(child, out);
    }
}

fn pdp_from_node_and_flag(node: Option<&Ast>, flag: Option<&Ast>) -> Option<PathDataPoint> {
    let node_id = node.and_then(|a| match a {
        Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
        _ => None,
    })?;
    let junction_flag = flag.and_then(|a| match a {
        Ast::Atom(at) => atom_to_number(at).map(|n| n as i32),
        _ => None,
    })?;
    Some(PathDataPoint {
        node_id,
        junction_flag,
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
        if nums.len() >= 2 {
            let node_id = nums[nums.len() - 2] as u32;
            let junction_flag = nums[nums.len() - 1] as i32;
            out.push(PathDataPoint {
                node_id,
                junction_flag,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn parse_track_pdp_native_pat() {
        let text = r#"
( TrPathNode
    ( TrPathName "Test" )
    ( TrackPDPs 2
        ( TrackPDP -6079 14925 -961.337 28.558 -71.912 42 0 )
        ( TrackPDP -6080 14925 998.528 28.558 306.463 99 1 )
    )
)"#;
        let ast = parse_from_first_paren(text).expect("parse pat");
        let path = PathFile::from_ast(&ast).expect("path");
        assert_eq!(path.pdps.len(), 2);
        assert_eq!(path.pdps[0].node_id, 42);
        assert_eq!(path.pdps[0].junction_flag, 0);
        assert_eq!(path.pdps[1].node_id, 99);
        assert_eq!(path.pdps[1].junction_flag, 1);
        assert_eq!(path.start_node(), Some(42));
        assert_eq!(path.end_node(), Some(99));
    }
}
