//! Parser for MSTS Track Database (`.tdb`) files.
//!
//! A `.tdb` file contains a `TrackDB` section with a `TrackNodes` list.
//! Each node is one of:
//! - `TrEndNode`      — dead-end or route terminus.
//! - `TrJunctionNode` — switch/points; references two connecting node IDs via `TrPins`.
//! - `TrVectorNode`   — a straight or curved track section with `TrVectorSections`.
//!
//! The parser is intentionally lenient: unknown sub-fields are silently ignored so
//! that real-world MSTS route files (which contain many extra metadata fields) work
//! without errors.

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::parser::parse_from_first_paren;
use crate::units::kmh_to_mps;

use super::atom_to_number;

/// One node in the MSTS Track Database.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackDbNode {
    /// 1-based sequential ID as stored in the `.tdb` file.
    pub id: u32,
    pub kind: TrackNodeKind,
}

/// Type and payload of a track database node.
#[derive(Clone, Debug, PartialEq)]
pub enum TrackNodeKind {
    /// Dead-end or route entry/exit point.
    End,
    /// Switch (points).  `pin1` / `pin2` are the two diverging node IDs.
    Junction { pin1: u32, pin2: u32 },
    /// A track section (vector).  `pins` are the two connecting node IDs.
    Vector {
        length_m: f64,
        speed_limit_mps: f64,
        pins: (u32, u32),
    },
}

/// Parsed representation of a `.tdb` file.
#[derive(Clone, Debug, Default)]
pub struct TrackDbFile {
    pub nodes: Vec<TrackDbNode>,
}

impl TrackDbFile {
    /// Parse from a pre-built AST (e.g. produced by `parse_from_first_paren`).
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let mut nodes: Vec<TrackDbNode> = Vec::new();
        collect_nodes(ast, &mut nodes);
        Ok(Self { nodes })
    }

    /// Convenience: read and parse a `.tdb` file from disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let text =
            std::fs::read_to_string(path.as_ref()).map_err(|e| FormatError::UnexpectedToken {
                offset: 0,
                message: format!("failed to read {}: {e}", path.as_ref().display()),
            })?;
        let ast = parse_from_first_paren(&text)?;
        Self::from_ast(&ast)
    }
}

/// Recursively walk the AST looking for `(TrackNode <id> ...)` lists.
fn collect_nodes(ast: &Ast, out: &mut Vec<TrackDbNode>) {
    let Ast::List(items) = ast else { return };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("TrackNode") && items.len() >= 3 {
            if let Some(id) = parse_u32(&items[1]) {
                if let Some(kind) = parse_node_kind(&items[2..]) {
                    out.push(TrackDbNode { id, kind });
                    return; // don't recurse into already-parsed node
                }
            }
        }
    }

    for child in items {
        collect_nodes(child, out);
    }
}

/// Determine the kind of a track node from its inner S-expressions.
fn parse_node_kind(body: &[Ast]) -> Option<TrackNodeKind> {
    for item in body {
        let Ast::List(sub) = item else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };

        if tag.eq_ignore_ascii_case("TrEndNode") {
            return Some(TrackNodeKind::End);
        }

        if tag.eq_ignore_ascii_case("TrJunctionNode") {
            let (pin1, pin2) = parse_junction_pins(sub);
            return Some(TrackNodeKind::Junction { pin1, pin2 });
        }

        if tag.eq_ignore_ascii_case("TrVectorNode") {
            let length_m = parse_vector_length(sub);
            let speed_limit_mps = parse_vector_speed(sub);
            let (pin1, pin2) = parse_tr_pins(sub);
            return Some(TrackNodeKind::Vector {
                length_m,
                speed_limit_mps,
                pins: (pin1, pin2),
            });
        }
    }
    None
}

/// Extract `TrPins` from a `TrJunctionNode`; returns `(0, 0)` on failure.
fn parse_junction_pins(junction: &[Ast]) -> (u32, u32) {
    parse_tr_pins(junction)
}

/// Extract `(TrPins <count> <pin1> <pin2>)` from a node body.
fn parse_tr_pins(body: &[Ast]) -> (u32, u32) {
    for item in body {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("TrPins") && sub.len() >= 4 {
                let pin1 = parse_u32(&sub[2]).unwrap_or(0);
                let pin2 = parse_u32(&sub[3]).unwrap_or(0);
                return (pin1, pin2);
            }
        }
    }
    (0, 0)
}

/// Sum the lengths of all `TrVectorSection` entries inside a `TrVectorNode`.
fn parse_vector_length(vector_node: &[Ast]) -> f64 {
    let mut total = 0.0_f64;
    for item in vector_node {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("TrVectorSections") {
                for section in sub.iter().skip(1) {
                    if let Ast::List(sec_items) = section {
                        if let Some(Ast::Atom(Atom::Symbol(sec_tag))) = sec_items.first() {
                            if sec_tag.eq_ignore_ascii_case("TrVectorSection") {
                                // Layout: (TrVectorSection <shape_idx> <section_idx> <tile_x> <tile_z> <x> <y> <z> <ay> <length> ...)
                                // Index 8 = length_m (0-based from tag).
                                if let Some(len) = sec_items.get(9).and_then(|a| match a {
                                    Ast::Atom(at) => atom_to_number(at),
                                    _ => None,
                                }) {
                                    total += len;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Fallback: if no section data, use a nominal 500 m so the edge is usable.
    if total <= 0.0 { 500.0 } else { total }
}

/// Extract speed limit (km/h → m/s) from `(SpeedMpS ...)` or `(MaxVelocity ...)` inside a vector node.
fn parse_vector_speed(vector_node: &[Ast]) -> f64 {
    for item in vector_node {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            // SpeedMpS is already in m/s in MSTS files.
            if tag.eq_ignore_ascii_case("SpeedMpS") {
                if let Some(v) = sub.get(1).and_then(|a| match a {
                    Ast::Atom(at) => atom_to_number(at),
                    _ => None,
                }) {
                    return v;
                }
            }
            // Some older files use MaxVelocity in km/h.
            if tag.eq_ignore_ascii_case("MaxVelocity") {
                if let Some(v) = sub.get(1).and_then(|a| match a {
                    Ast::Atom(at) => atom_to_number(at),
                    _ => None,
                }) {
                    return kmh_to_mps(v);
                }
            }
        }
    }
    // Default: 80 km/h if no speed information is present.
    kmh_to_mps(80.0)
}

fn parse_u32(ast: &Ast) -> Option<u32> {
    match ast {
        Ast::Atom(Atom::Integer(i)) if *i >= 0 => Some(*i as u32),
        Ast::Atom(Atom::Number(n)) if *n >= 0.0 => Some(*n as u32),
        _ => None,
    }
}
