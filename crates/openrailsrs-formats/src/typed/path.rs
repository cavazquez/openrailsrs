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
        let ast = parse_from_first_paren(&text)?;
        Self::from_ast(&ast)
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
            let node_id = items.get(1).and_then(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                _ => None,
            });
            let jf = items.get(2).and_then(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as i32),
                _ => None,
            });
            if let (Some(nid), Some(jflag)) = (node_id, jf) {
                out.push(PathDataPoint {
                    node_id: nid,
                    junction_flag: jflag,
                });
                return;
            }
        }
    }

    for child in items {
        collect_pdps(child, out);
    }
}
