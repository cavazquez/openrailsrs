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
        /// `TrItemId`s referenced by this vector node via `TrItemRefs`.
        item_ids: Vec<u32>,
    },
}

/// Initial signal aspect parsed from a `SignalItem` (defaults to `Stop` for safety).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SignalAspectKind {
    #[default]
    Stop,
    Caution,
    Clear,
}

impl SignalAspectKind {
    /// Lowercase string used in the openrailsrs `track.toml` `[[signals]]` schema.
    pub fn as_toml_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Caution => "caution",
            Self::Clear => "clear",
        }
    }
}

/// Kind/payload of a `TrItem` parsed from `TrItemTable`.
#[derive(Clone, Debug, PartialEq)]
pub enum TrItemKind {
    /// `SignalItem`: a wayside signal with an initial aspect.
    Signal { aspect_initial: SignalAspectKind },
    /// `SoundSourceItem`: an ambient sound region anchored to a track segment.
    /// `sms_file` holds the referenced `.sms` (or `.wav`) filename when present.
    SoundSource { sms_file: Option<String> },
    /// Any other `TrItem` kind (siding, platform, level crossing, etc.).
    Other,
}

/// One entry of `TrItemTable`.
#[derive(Clone, Debug, PartialEq)]
pub struct TrItem {
    /// `TrItemId` (1-based, unique inside the `.tdb`).
    pub id: u32,
    pub kind: TrItemKind,
    /// Distance in metres from the start of the parent vector node (`TrItemSData`).
    pub distance_m: f64,
}

/// Parsed representation of a `.tdb` file.
#[derive(Clone, Debug, Default)]
pub struct TrackDbFile {
    pub nodes: Vec<TrackDbNode>,
    /// Items declared in `TrItemTable` (signals, sidings, platforms, etc.).
    pub items: Vec<TrItem>,
}

impl TrackDbFile {
    /// Parse from a pre-built AST (e.g. produced by `parse_from_first_paren`).
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let mut nodes: Vec<TrackDbNode> = Vec::new();
        collect_nodes(ast, &mut nodes);
        let mut items: Vec<TrItem> = Vec::new();
        collect_items(ast, &mut items);
        Ok(Self { nodes, items })
    }

    /// Convenience: read and parse a `.tdb` file from disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let text = crate::encoding::read_msts_file_to_string(path.as_ref())?;
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
            let item_ids = parse_tr_item_refs(sub);
            return Some(TrackNodeKind::Vector {
                length_m,
                speed_limit_mps,
                pins: (pin1, pin2),
                item_ids,
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

/// Extract `(TrItemRefs <count> (TrItemId <id>) ...)` from a vector node body.
///
/// The MSTS schema lists each referenced item as either `(TrItemId <id>)` or
/// the legacy `(TrItemRef <id>)`; both spellings are accepted.
fn parse_tr_item_refs(vector_node: &[Ast]) -> Vec<u32> {
    let mut out = Vec::new();
    for item in vector_node {
        let Ast::List(sub) = item else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("TrItemRefs") {
            continue;
        }
        // The first child is the count atom (e.g. `1`); we skip it implicitly
        // because only `(TrItemId|TrItemRef <n>)` lists yield ids.
        for ref_item in sub.iter().skip(1) {
            let Ast::List(ref_sub) = ref_item else {
                continue;
            };
            let Some(Ast::Atom(Atom::Symbol(ref_tag))) = ref_sub.first() else {
                continue;
            };
            if ref_tag.eq_ignore_ascii_case("TrItemId") || ref_tag.eq_ignore_ascii_case("TrItemRef")
            {
                if let Some(id) = ref_sub.get(1).and_then(parse_u32) {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// Walk the AST looking for the `TrItemTable` section and collect every entry.
fn collect_items(ast: &Ast, out: &mut Vec<TrItem>) {
    let Ast::List(items) = ast else { return };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("TrItemTable") {
            for entry in items.iter().skip(1) {
                if let Ast::List(_) = entry {
                    if let Some(item) = parse_tr_item(entry) {
                        out.push(item);
                    }
                }
            }
            return;
        }
    }

    for child in items {
        collect_items(child, out);
    }
}

/// Parse a single `(<KindItem> ...)` list inside `TrItemTable`.
fn parse_tr_item(ast: &Ast) -> Option<TrItem> {
    let Ast::List(items) = ast else { return None };
    let Some(Ast::Atom(Atom::Symbol(tag))) = items.first() else {
        return None;
    };

    let id = find_tr_item_id(items)?;
    let distance_m = find_tr_item_distance(items);

    let kind = if tag.eq_ignore_ascii_case("SignalItem") {
        TrItemKind::Signal {
            aspect_initial: parse_signal_aspect(items),
        }
    } else if tag.eq_ignore_ascii_case("SoundSourceItem")
        || tag.eq_ignore_ascii_case("SoundRegionItem")
    {
        TrItemKind::SoundSource {
            sms_file: parse_sound_source_file(items),
        }
    } else {
        TrItemKind::Other
    };

    Some(TrItem {
        id,
        kind,
        distance_m,
    })
}

fn find_tr_item_id(item: &[Ast]) -> Option<u32> {
    for child in item {
        let Ast::List(sub) = child else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if tag.eq_ignore_ascii_case("TrItemId") {
            return sub.get(1).and_then(parse_u32);
        }
    }
    None
}

/// `(TrItemSData <distance_m> <flags>)` — first numeric child is the distance.
fn find_tr_item_distance(item: &[Ast]) -> f64 {
    for child in item {
        let Ast::List(sub) = child else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if tag.eq_ignore_ascii_case("TrItemSData") {
            if let Some(v) = sub.get(1).and_then(|a| match a {
                Ast::Atom(at) => atom_to_number(at),
                _ => None,
            }) {
                return v;
            }
        }
    }
    0.0
}

/// Look for a `.sms`/`.wav` filename anywhere inside a `SoundSourceItem`.
///
/// MSTS variants spell the field as `SoundSourceFile`, `FileName`, or simply
/// embed the filename as a bare string atom.  We accept any of those by
/// scanning the list recursively for the first string atom that has a
/// recognised audio extension.
fn parse_sound_source_file(item: &[Ast]) -> Option<String> {
    for child in item {
        if let Some(name) = find_audio_filename(child) {
            return Some(name);
        }
    }
    None
}

fn find_audio_filename(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(Atom::String(s)) | Ast::Atom(Atom::Symbol(s)) => {
            let lower = s.to_ascii_lowercase();
            if lower.ends_with(".sms") || lower.ends_with(".wav") {
                Some(s.clone())
            } else {
                None
            }
        }
        Ast::List(items) => items.iter().find_map(find_audio_filename),
        _ => None,
    }
}

/// Initial aspect heuristic: `(SignalFlags ...)` or `(InitialAspect ...)` may
/// hint at the starting aspect; if absent, we default to `Stop` (most
/// restrictive, safe baseline for imported routes).
fn parse_signal_aspect(item: &[Ast]) -> SignalAspectKind {
    for child in item {
        let Ast::List(sub) = child else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if tag.eq_ignore_ascii_case("InitialAspect") || tag.eq_ignore_ascii_case("SignalAspect") {
            if let Some(Ast::Atom(at)) = sub.get(1) {
                let s = match at {
                    Atom::Symbol(s) | Atom::String(s) => s.to_ascii_lowercase(),
                    _ => continue,
                };
                return match s.as_str() {
                    "clear" | "green" | "proceed" => SignalAspectKind::Clear,
                    "caution" | "yellow" | "approach" => SignalAspectKind::Caution,
                    _ => SignalAspectKind::Stop,
                };
            }
        }
    }
    SignalAspectKind::Stop
}
