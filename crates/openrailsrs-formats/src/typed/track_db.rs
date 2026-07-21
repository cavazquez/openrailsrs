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
    /// World-space node location from native `UiD (...)`, when present.
    pub position: Option<TrackVectorPoint>,
    /// All `TrPin` entries on this node (end, junction, and vector).
    pub pin_refs: Vec<TrPinRef>,
    pub kind: TrackNodeKind,
}

/// One connection reference on a junction or vector node (`TrPin` in MSTS).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrPinRef {
    /// Referenced TDB node id (end, junction, or vector).
    pub node_id: u32,
    /// MSTS branch index: 0 = common/stem, 1+ = diverging branches.
    pub branch_index: u8,
}

/// One world-space point from a `TrVectorSection`.
///
/// `tile_x` keeps the MSTS/Open Rails internal sign convention from `.tdb`
/// files (UK routes are commonly negative on X).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackVectorPoint {
    pub tile_x: i32,
    pub tile_z: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Minimal geometry recovered from a vector node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackVectorGeometry {
    pub start: TrackVectorPoint,
    pub end: TrackVectorPoint,
}

/// One `TrVectorSection` entry inside a vector node (native or tagged layout).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrVectorSectionRecord {
    /// `TrackShape` index in `tsection.dat` (native section field 0).
    pub shape_idx: u32,
    /// Secondary native shape/section field when present.
    pub aux_shape_idx: u32,
    /// Native `TrVectorSection` header tile (fields +2/+3); anchor `start` may use an adjacent tile.
    pub header_tile_x: i32,
    pub header_tile_z: i32,
    pub start: TrackVectorPoint,
    /// Native section orientation fields (`AX`, `AY`, `AZ` in Open Rails).
    pub ax: f64,
    pub ay: f64,
    pub az: f64,
}

impl TrVectorSectionRecord {
    /// Best-effort heading in degrees (Y-up) from native `AY` when plausible.
    pub fn heading_deg(&self) -> Option<f64> {
        if self.ay.is_finite()
            && self.ay.abs() > 1e-9
            && self.ay.abs() <= 360.0
            && self.ax.abs() < 45.0
            && self.az.abs() < 45.0
        {
            Some(self.ay)
        } else {
            None
        }
    }

    /// Bevy world positions for this section anchor, including tile-boundary variants.
    pub fn bevy_position_candidates(
        &self,
        ref_tile: Option<TrackVectorPoint>,
    ) -> Vec<(f32, f32, f32)> {
        let mut points = vec![self.start];
        push_point_tile_variants(
            &mut points,
            self.start,
            self.header_tile_x,
            self.header_tile_z,
        );
        if let Some(r) = ref_tile {
            if r.tile_x != self.start.tile_x || r.tile_z != self.start.tile_z {
                push_point_tile_variants(&mut points, self.start, r.tile_x, r.tile_z);
            }
        }
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for p in points {
            let bevy = p.bevy_position();
            let key = (
                (bevy.0 * 4.0).round() as i32,
                (bevy.1 * 4.0).round() as i32,
                (bevy.2 * 4.0).round() as i32,
            );
            if seen.insert(key) {
                out.push(bevy);
            }
        }
        out
    }

    /// Closest Bevy anchor to a reference world XZ, preferring tile-boundary variants near `ref_tile`.
    pub fn bevy_position_nearest_to(
        &self,
        ref_x: f32,
        ref_z: f32,
        ref_tile: Option<(i32, i32)>,
    ) -> (f32, f32, f32) {
        let ref_point = ref_tile.map(|(tile_x, tile_z)| TrackVectorPoint {
            tile_x,
            tile_z,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        let mut best = self.start.bevy_position();
        let mut best_dist = f32::INFINITY;
        for cand in self.bevy_position_candidates(ref_point) {
            let dx = ref_x - cand.0;
            let dz = ref_z - cand.2;
            let dist = dx * dx + dz * dz;
            if dist < best_dist {
                best_dist = dist;
                best = cand;
            }
        }
        best
    }
}

/// MSTS often stores the same junction anchor on an adjacent tile with flipped local X or Z.
fn push_point_tile_variants(
    out: &mut Vec<TrackVectorPoint>,
    base: TrackVectorPoint,
    tile_x: i32,
    tile_z: i32,
) {
    if tile_x == base.tile_x && tile_z == base.tile_z {
        return;
    }
    for (x, z) in [
        (base.x, base.z),
        (base.x, -base.z),
        (-base.x, base.z),
        (-base.x, -base.z),
    ] {
        out.push(TrackVectorPoint {
            tile_x,
            tile_z,
            x,
            y: base.y,
            z,
        });
    }
}

/// Indexed vector section for spatial lookup (TrackObj ↔ TDB alignment).
#[derive(Clone, Debug, PartialEq)]
pub struct IndexedTrVectorSection {
    pub node_id: u32,
    pub record: TrVectorSectionRecord,
}

/// Type and payload of a track database node.
#[derive(Clone, Debug, PartialEq)]
pub enum TrackNodeKind {
    /// Dead-end or route entry/exit point.
    End,
    /// Switch (points).  `pins` lists all `TrPin` entries (common + branches).
    Junction { pins: Vec<TrPinRef> },
    /// A track section (vector).  `pins` are the two connecting node IDs.
    Vector {
        length_m: f64,
        speed_limit_mps: f64,
        pins: (u32, u32),
        /// `TrItemId`s referenced by this vector node via `TrItemRefs`.
        item_ids: Vec<u32>,
        /// Parsed `TrVectorSection` entries (shape index + world anchor).
        sections: Vec<TrVectorSectionRecord>,
        /// Best-effort section geometry for placing imported nodes in MSTS world space.
        geometry: Option<TrackVectorGeometry>,
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
    /// `SpeedPostItem`: wayside speed limit (second field of `SpeedpostTrItemData` is mph).
    SpeedPost { speed_mph: f64 },
    /// Any other `TrItem` kind (siding, platform, level crossing, etc.).
    Other,
}

/// World pose from `TrItemRData ( x y z tileX tileZ )` (TDB/RDB).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrItemWorldPose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub tile_x: i32,
    pub tile_z: i32,
}

impl TrItemWorldPose {
    /// Bevy world position (same convention as [`TrackVectorPoint::bevy_position`]).
    pub fn bevy_position(self) -> (f32, f32, f32) {
        const TILE: f64 = 2048.0;
        (
            (self.tile_x as f64 * TILE + self.x) as f32,
            self.y as f32,
            (-(self.tile_z as f64 * TILE + self.z)) as f32,
        )
    }
}

/// One entry of `TrItemTable`.
#[derive(Clone, Debug, PartialEq)]
pub struct TrItem {
    /// `TrItemId` (1-based, unique inside the `.tdb`).
    pub id: u32,
    pub kind: TrItemKind,
    /// Distance in metres from the start of the parent vector node (`TrItemSData`).
    pub distance_m: f64,
    /// Absolute pose when `TrItemRData` is present (required for RDB CarSpawner endpoints).
    pub world: Option<TrItemWorldPose>,
}

/// Host vector node for a `TrItem` referenced via `TrItemRefs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrItemHost {
    pub vector_id: u32,
}

/// Parsed representation of a `.tdb` file.
#[derive(Clone, Debug, Default)]
pub struct TrackDbFile {
    pub nodes: Vec<TrackDbNode>,
    /// Items declared in `TrItemTable` (signals, sidings, platforms, etc.).
    pub items: Vec<TrItem>,
}

impl TrackVectorPoint {
    /// Graph X in metres (signed internal tile convention, matches route import).
    pub fn graph_x_m(self) -> f64 {
        point_graph_x(self)
    }

    /// Graph Z in metres (Bevy/MSTS world convention, local MSTS Z negated).
    pub fn graph_z_m(self) -> f64 {
        point_graph_z(self)
    }

    /// Bevy world position (`msts_to_bevy`, signed internal tile convention).
    pub fn bevy_position(self) -> (f32, f32, f32) {
        const TILE: f64 = 2048.0;
        (
            (self.tile_x as f64 * TILE + self.x) as f32,
            self.y as f32,
            (-(self.tile_z as f64 * TILE + self.z)) as f32,
        )
    }

    /// Closest Bevy position among tile-boundary variants near `ref_tile` / `header_tile`.
    pub fn bevy_position_nearest_to(
        self,
        ref_x: f32,
        ref_z: f32,
        ref_tile: Option<(i32, i32)>,
        header_tile: Option<(i32, i32)>,
    ) -> (f32, f32, f32) {
        let mut points = vec![self];
        if let Some((tile_x, tile_z)) = header_tile {
            push_point_tile_variants(&mut points, self, tile_x, tile_z);
        }
        if let Some((tile_x, tile_z)) = ref_tile {
            push_point_tile_variants(&mut points, self, tile_x, tile_z);
        }
        let mut best = self.bevy_position();
        let mut best_dist = f32::INFINITY;
        let mut seen = std::collections::HashSet::new();
        for p in points {
            let bevy = p.bevy_position();
            let key = (
                (bevy.0 * 4.0).round() as i32,
                (bevy.1 * 4.0).round() as i32,
                (bevy.2 * 4.0).round() as i32,
            );
            if !seen.insert(key) {
                continue;
            }
            let dx = ref_x - bevy.0;
            let dz = ref_z - bevy.2;
            let dist = dx * dx + dz * dz;
            if dist < best_dist {
                best_dist = dist;
                best = bevy;
            }
        }
        best
    }
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

    /// Merge `SpeedPostItem` entries from a companion `.tit` file (common on real routes).
    pub fn merge_tit_speed_posts(
        &mut self,
        tit_path: impl AsRef<std::path::Path>,
    ) -> Result<(), FormatError> {
        let text = crate::encoding::read_msts_file_to_string(tit_path.as_ref())?;
        let ast = parse_from_first_paren(&text)?;
        let mut tit_items = Vec::new();
        if let Ast::List(root) = &ast {
            // Native `.tit` roots at `( <count> SpeedPostItem ( ... ) ... )` without `TrItemTable`.
            parse_tr_item_table_entries(root, &mut tit_items);
        }
        collect_items(&ast, &mut tit_items);
        for item in tit_items {
            if matches!(item.kind, TrItemKind::SpeedPost { .. }) {
                self.items.retain(|existing| existing.id != item.id);
                self.items.push(item);
            }
        }
        Ok(())
    }

    /// Group vector sections by `tsection.dat` `TrackShape` index.
    pub fn index_vector_sections_by_shape(
        &self,
    ) -> std::collections::HashMap<u32, Vec<IndexedTrVectorSection>> {
        let mut out: std::collections::HashMap<u32, Vec<IndexedTrVectorSection>> =
            std::collections::HashMap::new();
        for node in &self.nodes {
            let TrackNodeKind::Vector { sections, .. } = &node.kind else {
                continue;
            };
            for record in sections {
                if record.shape_idx == 0 {
                    continue;
                }
                out.entry(record.shape_idx)
                    .or_default()
                    .push(IndexedTrVectorSection {
                        node_id: node.id,
                        record: *record,
                    });
            }
        }
        out
    }

    /// Junction nodes (`TrJunctionNode`) in the track graph.
    pub fn junction_nodes(&self) -> impl Iterator<Item = &TrackDbNode> {
        self.nodes
            .iter()
            .filter(|n| matches!(n.kind, TrackNodeKind::Junction { .. }))
    }

    /// Lookup a track node by its 1-based `.tdb` id.
    pub fn node_by_id(&self, id: u32) -> Option<&TrackDbNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Lookup a `TrItemTable` entry by `TrItemId`.
    /// Bevy world position of a `TrItem` that carries `TrItemRData` (RDB CarSpawner endpoints).
    pub fn item_bevy_position(&self, id: u32) -> Option<(f32, f32, f32)> {
        self.item_by_id(id)
            .and_then(|item| item.world.map(|w| w.bevy_position()))
    }

    pub fn item_by_id(&self, id: u32) -> Option<&TrItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Map each `TrItemId` to vector node id(s) that reference it via `TrItemRefs`.
    pub fn index_item_hosts(&self) -> std::collections::HashMap<u32, Vec<u32>> {
        let mut hosts: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
        for node in &self.nodes {
            if let TrackNodeKind::Vector { item_ids, .. } = &node.kind {
                for item_id in item_ids {
                    hosts.entry(*item_id).or_default().push(node.id);
                }
            }
        }
        hosts
    }

    /// Single host vector for a `TrItemId` (TSRE expects exactly one for active items).
    pub fn host_vector_for_item(&self, item_id: u32) -> Option<u32> {
        let hosts_map = self.index_item_hosts();
        let hosts = hosts_map.get(&item_id)?;
        if hosts.len() == 1 {
            Some(hosts[0])
        } else {
            None
        }
    }

    pub fn tr_item_host(&self, item_id: u32) -> Option<TrItemHost> {
        self.host_vector_for_item(item_id)
            .map(|vector_id| TrItemHost { vector_id })
    }

    /// Map MSTS tile indices to vector/junction/end node ids whose anchors lie in that tile.
    pub fn index_nodes_by_tile(&self) -> std::collections::HashMap<(i32, i32), Vec<u32>> {
        let mut out: std::collections::HashMap<(i32, i32), Vec<u32>> =
            std::collections::HashMap::new();
        for node in &self.nodes {
            let mut tiles = Vec::new();
            if let Some(pos) = node.position {
                tiles.push((pos.tile_x, pos.tile_z));
            }
            if let TrackNodeKind::Vector { sections, .. } = &node.kind {
                for section in sections {
                    tiles.push((section.start.tile_x, section.start.tile_z));
                    tiles.push((section.header_tile_x, section.header_tile_z));
                }
            }
            for (tx, tz) in tiles {
                out.entry((tx, tz)).or_default().push(node.id);
            }
        }
        for ids in out.values_mut() {
            ids.sort_unstable();
            ids.dedup();
        }
        out
    }

    /// True when `other_id` is reachable from `node_id` via a single `TrPin` hop.
    pub fn pins_connect(&self, node_id: u32, other_id: u32) -> bool {
        let Some(node) = self.node_by_id(node_id) else {
            return false;
        };
        if node.pin_refs.iter().any(|p| p.node_id == other_id) {
            return true;
        }
        self.node_by_id(other_id)
            .is_some_and(|other| other.pin_refs.iter().any(|p| p.node_id == node_id))
    }
}

/// Recursively walk the AST looking for track nodes in unified or native MSTS layout.
fn collect_nodes(ast: &Ast, out: &mut Vec<TrackDbNode>) {
    let Ast::List(items) = ast else { return };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("TrackNodes") {
            parse_track_nodes_children(track_nodes_body(items), out);
            return;
        }
        if head.eq_ignore_ascii_case("TrackNode") && items.len() >= 3 {
            if let Some(id) = parse_u32(&items[1]) {
                if let Some(kind) = parse_node_kind(&items[2..]) {
                    out.push(TrackDbNode {
                        id,
                        position: parse_node_position(&items[2..]),
                        pin_refs: parse_tr_pins(&items[2..]),
                        kind,
                    });
                    return;
                }
            }
        }
    }

    let mut skip_idx: Option<usize> = None;
    for i in 0..items.len().saturating_sub(1) {
        if let Ast::Atom(Atom::Symbol(tag)) = &items[i] {
            if tag.eq_ignore_ascii_case("TrackNodes") {
                if let Ast::List(body) = &items[i + 1] {
                    parse_track_nodes_children(body, out);
                    skip_idx = Some(i + 1);
                    break;
                }
            }
        }
    }

    for (idx, child) in items.iter().enumerate() {
        if skip_idx == Some(idx) {
            continue;
        }
        collect_nodes(child, out);
    }
}

fn track_nodes_body(items: &[Ast]) -> &[Ast] {
    if items.len() >= 2 {
        if let Ast::List(inner) = &items[1] {
            return inner.as_slice();
        }
    }
    &items[1..]
}

/// Parse children of `(TrackNodes N ...)`, supporting native editor layout where
/// `TrackNode` is an atom followed by a sibling list `( <id> ... )`.
fn parse_track_nodes_children(children: &[Ast], out: &mut Vec<TrackDbNode>) {
    let mut i = 0;
    while i < children.len() {
        if matches!(
            children[i],
            Ast::Atom(Atom::Integer(_)) | Ast::Atom(Atom::Number(_))
        ) {
            i += 1;
            continue;
        }

        if let Ast::Atom(Atom::Symbol(tag)) = &children[i] {
            if tag.eq_ignore_ascii_case("TrackNode") {
                if let Some(Ast::List(body)) = children.get(i + 1) {
                    if let Some(id) = body.first().and_then(parse_u32) {
                        if let Some(kind) = parse_node_kind(&body[1..]) {
                            out.push(TrackDbNode {
                                id,
                                position: parse_node_position(&body[1..]),
                                pin_refs: parse_tr_pins(&body[1..]),
                                kind,
                            });
                            i += 2;
                            continue;
                        }
                    }
                }
            }
        }

        if let Ast::List(sub) = &children[i] {
            if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                if tag.eq_ignore_ascii_case("TrackNode") && sub.len() >= 3 {
                    if let Some(id) = parse_u32(&sub[1]) {
                        if let Some(kind) = parse_node_kind(&sub[2..]) {
                            out.push(TrackDbNode {
                                id,
                                position: parse_node_position(&sub[2..]),
                                pin_refs: parse_tr_pins(&sub[2..]),
                                kind,
                            });
                            i += 1;
                            continue;
                        }
                    }
                }
            }
        }

        i += 1;
    }
}

/// Determine the kind of a track node from its inner S-expressions.
fn parse_node_kind(body: &[Ast]) -> Option<TrackNodeKind> {
    let mut i = 0;
    while i < body.len() {
        match &body[i] {
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrEndNode") => {
                return Some(TrackNodeKind::End);
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrJunctionNode") => {
                let pins = parse_tr_pins(body);
                return Some(TrackNodeKind::Junction { pins });
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrVectorNode") => {
                if let Some(Ast::List(vector_sub)) = body.get(i + 1) {
                    return Some(parse_vector_kind(vector_sub, body));
                }
            }
            Ast::List(sub) => {
                let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
                    i += 1;
                    continue;
                };
                if tag.eq_ignore_ascii_case("TrEndNode") {
                    return Some(TrackNodeKind::End);
                }
                if tag.eq_ignore_ascii_case("TrJunctionNode") {
                    let pins = parse_tr_pins(body);
                    return Some(TrackNodeKind::Junction { pins });
                }
                if tag.eq_ignore_ascii_case("TrVectorNode") {
                    return Some(parse_vector_kind(sub, body));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_vector_kind(vector_sub: &[Ast], node_body: &[Ast]) -> TrackNodeKind {
    let pin_refs = parse_tr_pins(node_body);
    let pin_refs = if pin_refs.is_empty() {
        parse_tr_pins(vector_sub)
    } else {
        pin_refs
    };
    let pins = vector_pin_pair(&pin_refs);
    let sections = parse_vector_section_records(vector_sub);
    TrackNodeKind::Vector {
        length_m: parse_vector_length(vector_sub),
        speed_limit_mps: parse_vector_speed(vector_sub),
        pins,
        item_ids: parse_tr_item_refs(vector_sub),
        sections,
        geometry: parse_vector_geometry(vector_sub),
    }
}

fn parse_node_position(body: &[Ast]) -> Option<TrackVectorPoint> {
    let mut i = 0;
    while i < body.len() {
        match &body[i] {
            Ast::List(sub)
                if sub
                    .first()
                    .and_then(symbol_name)
                    .is_some_and(|s| s.eq_ignore_ascii_case("UiD")) =>
            {
                return parse_uid_point(sub);
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("UiD") => {
                if let Some(Ast::List(sub)) = body.get(i + 1) {
                    return parse_uid_point(sub);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_uid_point(items: &[Ast]) -> Option<TrackVectorPoint> {
    let data_start = if items.first().and_then(symbol_name).is_some() {
        1
    } else {
        0
    };
    Some(TrackVectorPoint {
        tile_x: items.get(data_start + 4).and_then(parse_i32)?,
        tile_z: items.get(data_start + 5).and_then(parse_i32)?,
        x: items.get(data_start + 6).and_then(ast_to_f64)?,
        y: items.get(data_start + 7).and_then(ast_to_f64)?,
        z: items.get(data_start + 8).and_then(ast_to_f64)?,
    })
}

fn vector_pin_pair(pins: &[TrPinRef]) -> (u32, u32) {
    if pins.len() >= 2 {
        (pins[0].node_id, pins[1].node_id)
    } else if pins.len() == 1 {
        (pins[0].node_id, 0)
    } else {
        (0, 0)
    }
}

/// Extract `(TrPins ...)` from a node body; supports flat `(TrPins 2 1 3)` and
/// native nested `(TrPins 1 1 (TrPin 3 0) (TrPin 1 1))`.
fn parse_tr_pins(body: &[Ast]) -> Vec<TrPinRef> {
    for i in 0..body.len() {
        let pins_slice = match (&body[i], body.get(i + 1)) {
            (Ast::List(sub), _) if is_tr_pins_list(sub) => sub.as_slice(),
            (Ast::Atom(Atom::Symbol(tag)), Some(Ast::List(sub)))
                if tag.eq_ignore_ascii_case("TrPins") =>
            {
                sub.as_slice()
            }
            _ => continue,
        };
        let parsed = extract_tr_pin_refs(pins_slice);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    Vec::new()
}

fn is_tr_pins_list(sub: &[Ast]) -> bool {
    sub.first()
        .and_then(|a| match a {
            Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
            _ => None,
        })
        .is_some_and(|s| s.eq_ignore_ascii_case("TrPins"))
}

fn extract_tr_pin_refs(pins: &[Ast]) -> Vec<TrPinRef> {
    let mut out = Vec::new();
    let mut i = 1;
    if pins.len() <= 1 {
        return out;
    }

    let has_nested_tr_pins = pins[i..].iter().any(is_tr_pin_entry);

    if has_nested_tr_pins {
        // Native `(TrPins count flags (TrPin ...) ...)` — skip header numerics.
        let mut header_skipped = 0;
        while i < pins.len() && header_skipped < 2 {
            if parse_u32(&pins[i]).is_some() {
                header_skipped += 1;
                i += 1;
            } else {
                break;
            }
        }
        while i < pins.len() {
            match (&pins[i], pins.get(i + 1)) {
                (Ast::List(pin_sub), _) => {
                    if let Some(Ast::Atom(Atom::Symbol(pin_tag))) = pin_sub.first() {
                        if pin_tag.eq_ignore_ascii_case("TrPin") {
                            if let (Some(id), branch) = (
                                pin_sub.get(1).and_then(parse_u32),
                                pin_sub.get(2).and_then(parse_u32).unwrap_or(0) as u8,
                            ) {
                                out.push(TrPinRef {
                                    node_id: id,
                                    branch_index: branch,
                                });
                            }
                            i += 1;
                            continue;
                        }
                    }
                }
                (Ast::Atom(Atom::Symbol(tag)), Some(Ast::List(args)))
                    if tag.eq_ignore_ascii_case("TrPin") =>
                {
                    if let Some(id) = args.first().and_then(parse_u32) {
                        let branch = args.get(1).and_then(parse_u32).unwrap_or(0) as u8;
                        out.push(TrPinRef {
                            node_id: id,
                            branch_index: branch,
                        });
                    }
                    i += 2;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    } else {
        // Flat `(TrPins count node_id ...)` used by unified test fixtures.
        if parse_u32(&pins[i]).is_some() {
            i += 1;
        }
        while i < pins.len() {
            if let Some(id) = parse_u32(&pins[i]) {
                out.push(TrPinRef {
                    node_id: id,
                    branch_index: 0,
                });
            }
            i += 1;
        }
    }
    out
}

fn is_tr_pin_entry(ast: &Ast) -> bool {
    match ast {
        Ast::List(sub) => sub
            .first()
            .and_then(|a| match a {
                Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
                _ => None,
            })
            .is_some_and(|s| s.eq_ignore_ascii_case("TrPin")),
        Ast::Atom(Atom::Symbol(s)) => s.eq_ignore_ascii_case("TrPin"),
        _ => false,
    }
}

/// Estimate vector length from consecutive `TrVectorSection` world positions.
///
/// Open Rails reads the final three native section fields as `AX`, `AY`, `AZ`,
/// not as length data, so exact per-section lengths require looking up
/// `tsection.dat`.  This parser keeps a local geometric estimate instead.
fn parse_vector_length(vector_node: &[Ast]) -> f64 {
    let tagged_total = parse_tagged_vector_lengths(vector_node);
    if tagged_total > 0.0 {
        return tagged_total;
    }
    let Some(points) = parse_vector_points(vector_node) else {
        return 25.0;
    };
    let total = polyline_length(&points);
    if total <= 0.0 { 25.0 } else { total }
}

fn parse_tagged_vector_lengths(vector_node: &[Ast]) -> f64 {
    let mut total = 0.0;
    for sections in vector_sections_lists(vector_node) {
        for section in sections.iter().skip(1) {
            let Ast::List(sec_items) = section else {
                continue;
            };
            if sec_items
                .first()
                .and_then(symbol_name)
                .is_some_and(|s| s.eq_ignore_ascii_case("TrVectorSection"))
            {
                total += sec_items.get(9).and_then(ast_to_f64).unwrap_or(0.0);
            }
        }
    }
    total
}

fn parse_vector_geometry(vector_node: &[Ast]) -> Option<TrackVectorGeometry> {
    let points = parse_vector_points(vector_node)?;
    let start = *points.first()?;
    let end = *points.last()?;
    Some(TrackVectorGeometry { start, end })
}

fn parse_vector_section_records(vector_node: &[Ast]) -> Vec<TrVectorSectionRecord> {
    let mut out = Vec::new();
    for sections in vector_sections_lists(vector_node) {
        let mut list_records = Vec::new();
        for section in sections.iter().skip(1) {
            if let Ast::List(sec_items) = section {
                if sec_items
                    .first()
                    .and_then(symbol_name)
                    .is_some_and(|s| s.eq_ignore_ascii_case("TrVectorSection"))
                {
                    if let Some(point) = parse_tagged_vector_section_point(sec_items) {
                        list_records.push(TrVectorSectionRecord {
                            shape_idx: sec_items.get(1).and_then(parse_u32).unwrap_or(0),
                            aux_shape_idx: sec_items.get(2).and_then(parse_u32).unwrap_or(0),
                            header_tile_x: point.tile_x,
                            header_tile_z: point.tile_z,
                            start: point,
                            ax: sec_items.get(10).and_then(ast_to_f64).unwrap_or(0.0),
                            ay: sec_items.get(11).and_then(ast_to_f64).unwrap_or(0.0),
                            az: sec_items.get(12).and_then(ast_to_f64).unwrap_or(0.0),
                        });
                    }
                }
            }
        }

        if list_records.is_empty() {
            let (count_idx, data_start) = if sections
                .first()
                .and_then(|a| match a {
                    Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
                    _ => None,
                })
                .is_some_and(|s| s.eq_ignore_ascii_case("TrVectorSections"))
            {
                (1usize, 2usize)
            } else {
                (0usize, 1usize)
            };
            if let Some(count) = sections.get(count_idx).and_then(parse_u32) {
                list_records.extend(parse_native_vector_section_records(
                    sections,
                    data_start,
                    count as usize,
                ));
            }
        }
        out.extend(list_records);
    }
    out
}

fn parse_vector_points(vector_node: &[Ast]) -> Option<Vec<TrackVectorPoint>> {
    let records = parse_vector_section_records(vector_node);
    if records.is_empty() {
        None
    } else {
        Some(records.iter().map(|r| r.start).collect())
    }
}

fn polyline_length(points: &[TrackVectorPoint]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let ax = point_graph_x(a);
            let bx = point_graph_x(b);
            let az = point_graph_z(a);
            let bz = point_graph_z(b);
            ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt()
        })
        .sum()
}

fn parse_tagged_vector_section_point(sec_items: &[Ast]) -> Option<TrackVectorPoint> {
    Some(TrackVectorPoint {
        tile_x: sec_items.get(3).and_then(parse_i32)?,
        tile_z: sec_items.get(4).and_then(parse_i32)?,
        x: sec_items.get(5).and_then(ast_to_f64)?,
        y: sec_items.get(6).and_then(ast_to_f64)?,
        z: sec_items.get(7).and_then(ast_to_f64)?,
    })
}

fn parse_native_vector_section_records(
    items: &[Ast],
    data_start: usize,
    count: usize,
) -> Vec<TrVectorSectionRecord> {
    let mut out = Vec::new();
    let mut i = data_start;
    while i + 15 < items.len() && out.len() < count {
        if let Some(record) = parse_native_section_record(items, i) {
            out.push(record);
            i += 16;
        } else {
            i += 1;
        }
    }
    out
}

fn parse_native_section_record(items: &[Ast], base: usize) -> Option<TrVectorSectionRecord> {
    let shape_a = items.get(base).and_then(parse_i32)?;
    let shape_b = items.get(base + 1).and_then(parse_i32)?;
    let world_tile_x = items.get(base + 2).and_then(parse_i32)?;
    let world_tile_z = items.get(base + 3).and_then(parse_i32)?;
    if !is_plausible_shape_id(shape_a)
        || !is_plausible_shape_id(shape_b)
        || !is_plausible_tile_pair(world_tile_x, world_tile_z)
    {
        return None;
    }

    let point = TrackVectorPoint {
        tile_x: items.get(base + 8).and_then(parse_i32)?,
        tile_z: items.get(base + 9).and_then(parse_i32)?,
        x: items.get(base + 10).and_then(ast_to_f64)?,
        y: items.get(base + 11).and_then(ast_to_f64)?,
        z: items.get(base + 12).and_then(ast_to_f64)?,
    };
    if !is_plausible_world_point(point) {
        return None;
    }

    Some(TrVectorSectionRecord {
        shape_idx: shape_a as u32,
        aux_shape_idx: shape_b as u32,
        header_tile_x: world_tile_x,
        header_tile_z: world_tile_z,
        start: point,
        ax: items.get(base + 13).and_then(ast_to_f64).unwrap_or(0.0),
        ay: items.get(base + 14).and_then(ast_to_f64).unwrap_or(0.0),
        az: items.get(base + 15).and_then(ast_to_f64).unwrap_or(0.0),
    })
}

fn is_plausible_shape_id(id: i32) -> bool {
    (20_000..=500_000).contains(&id)
}

fn is_plausible_tile_pair(tile_x: i32, tile_z: i32) -> bool {
    (1000..=20_000).contains(&tile_x.abs()) && (10_000..=20_000).contains(&tile_z.abs())
}

fn is_plausible_world_point(point: TrackVectorPoint) -> bool {
    is_plausible_tile_pair(point.tile_x, point.tile_z)
        && point.x.is_finite()
        && point.y.is_finite()
        && point.z.is_finite()
        && point.x.abs() <= 2048.0
        && point.y.abs() <= 5000.0
        && point.z.abs() <= 2048.0
}

fn vector_sections_lists(vector_node: &[Ast]) -> Vec<&[Ast]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < vector_node.len() {
        match &vector_node[i] {
            Ast::List(sub)
                if sub
                    .first()
                    .and_then(|a| match a {
                        Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
                        _ => None,
                    })
                    .is_some_and(|s| s.eq_ignore_ascii_case("TrVectorSections")) =>
            {
                out.push(sub.as_slice());
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrVectorSections") => {
                if let Some(Ast::List(sub)) = vector_node.get(i + 1) {
                    out.push(sub.as_slice());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn symbol_name(ast: &Ast) -> Option<&str> {
    match ast {
        Ast::Atom(Atom::Symbol(s)) => Some(s),
        _ => None,
    }
}

fn point_graph_x(point: TrackVectorPoint) -> f64 {
    // Signed internal tile X (Open Rails convention). Using the positive
    // "display" value here would mirror the tile grid east-west and break
    // continuity across tile borders.
    point.tile_x as f64 * 2048.0 + point.x
}

fn point_graph_z(point: TrackVectorPoint) -> f64 {
    // Whole-world Z negation (Open Rails XNA convention); negating only the
    // local part would mirror the tile grid north-south.
    -(point.tile_z as f64 * 2048.0 + point.z)
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

fn parse_i32(ast: &Ast) -> Option<i32> {
    match ast {
        Ast::Atom(Atom::Integer(i)) => Some(*i as i32),
        Ast::Atom(Atom::Number(n)) => Some(*n as i32),
        _ => None,
    }
}

/// Extract `(TrItemRefs <count> (TrItemId <id>) ...)` from a vector node body.
///
/// The MSTS schema lists each referenced item as either `(TrItemId <id>)` or
/// the legacy `(TrItemRef <id>)`; both spellings are accepted.
fn parse_tr_item_refs(vector_node: &[Ast]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < vector_node.len() {
        let refs_slice = match (&vector_node[i], vector_node.get(i + 1)) {
            (Ast::List(sub), _) if is_tr_item_refs_list(sub) => {
                i += 1;
                sub.as_slice()
            }
            (Ast::Atom(Atom::Symbol(tag)), Some(Ast::List(sub)))
                if tag.eq_ignore_ascii_case("TrItemRefs") =>
            {
                i += 2;
                sub.as_slice()
            }
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = if refs_slice
            .first()
            .and_then(|a| match a {
                Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
                _ => None,
            })
            .is_some_and(|s| s.eq_ignore_ascii_case("TrItemRefs"))
        {
            1
        } else {
            0
        };
        if j < refs_slice.len() && parse_u32(&refs_slice[j]).is_some() {
            j += 1;
        }
        while j < refs_slice.len() {
            if let (Ast::Atom(Atom::Symbol(tag)), Some(Ast::List(args))) =
                (&refs_slice[j], refs_slice.get(j + 1))
            {
                if tag.eq_ignore_ascii_case("TrItemRef") || tag.eq_ignore_ascii_case("TrItemId") {
                    if let Some(id) = args.first().and_then(parse_u32) {
                        out.push(id);
                    }
                    j += 2;
                    continue;
                }
            }
            if let Some(id) = parse_tr_item_ref_entry(&refs_slice[j]) {
                out.push(id);
            }
            j += 1;
        }
    }
    out
}

fn is_tr_item_refs_list(sub: &[Ast]) -> bool {
    sub.first()
        .and_then(|a| match a {
            Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
            _ => None,
        })
        .is_some_and(|s| s.eq_ignore_ascii_case("TrItemRefs"))
}

fn parse_tr_item_ref_entry(ref_item: &Ast) -> Option<u32> {
    if let Some(id) = parse_u32(ref_item) {
        return Some(id);
    }
    if let Ast::List(ref_sub) = ref_item {
        if let Some(Ast::Atom(Atom::Symbol(ref_tag))) = ref_sub.first() {
            if ref_tag.eq_ignore_ascii_case("TrItemId") || ref_tag.eq_ignore_ascii_case("TrItemRef")
            {
                return ref_sub.get(1).and_then(parse_u32);
            }
        }
        return ref_sub.first().and_then(parse_u32);
    }
    None
}

fn ast_to_f64(ast: &Ast) -> Option<f64> {
    match ast {
        Ast::Atom(at) => atom_to_number(at),
        _ => None,
    }
}

/// Walk the AST looking for the `TrItemTable` section and collect every entry.
fn collect_items(ast: &Ast, out: &mut Vec<TrItem>) {
    let Ast::List(items) = ast else { return };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("TrItemTable") {
            parse_tr_item_table_entries(&items[1..], out);
            return;
        }
    }

    let mut skip_idx: Option<usize> = None;
    for i in 0..items.len().saturating_sub(1) {
        if let Ast::Atom(Atom::Symbol(tag)) = &items[i] {
            if tag.eq_ignore_ascii_case("TrItemTable") {
                if let Ast::List(body) = &items[i + 1] {
                    parse_tr_item_table_entries(body, out);
                    skip_idx = Some(i + 1);
                    break;
                }
            }
        }
    }

    for (idx, child) in items.iter().enumerate() {
        if skip_idx == Some(idx) {
            continue;
        }
        collect_items(child, out);
    }
}

fn parse_tr_item_table_entries(children: &[Ast], out: &mut Vec<TrItem>) {
    let mut i = 0;
    while i < children.len() {
        if matches!(
            children[i],
            Ast::Atom(Atom::Integer(_)) | Ast::Atom(Atom::Number(_))
        ) {
            i += 1;
            continue;
        }

        if let Ast::Atom(Atom::Symbol(tag)) = &children[i] {
            if is_tr_item_kind_tag(tag) {
                if let Some(Ast::List(body)) = children.get(i + 1) {
                    let mut combined = vec![Ast::Atom(Atom::Symbol(tag.clone()))];
                    combined.extend(body.iter().cloned());
                    if let Some(item) = parse_tr_item(&Ast::List(combined)) {
                        out.push(item);
                    }
                    i += 2;
                    continue;
                }
            }
        }

        if let Some(item) = parse_tr_item(&children[i]) {
            out.push(item);
            i += 1;
            continue;
        }

        i += 1;
    }
}

fn is_tr_item_kind_tag(tag: &str) -> bool {
    tag.ends_with("Item") || tag.eq_ignore_ascii_case("SignalItem")
}

/// `(SpeedpostTrItemData <display> <limit_mph> <heading>)` — OR uses the second value as mph.
fn parse_speed_post_limit_mph(item: &[Ast]) -> f64 {
    let mut i = 0;
    while i < item.len() {
        match &item[i] {
            Ast::List(sub) => {
                if let Some(mph) = speedpost_mph_from_tagged_list(sub) {
                    return mph;
                }
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("SpeedpostTrItemData") => {
                if let Some(mph) = item.get(i + 1).and_then(speedpost_mph_from_values) {
                    return mph;
                }
            }
            _ => {}
        }
        i += 1;
    }
    0.0
}

fn speedpost_mph_from_tagged_list(sub: &[Ast]) -> Option<f64> {
    let Ast::Atom(Atom::Symbol(tag)) = sub.first()? else {
        return None;
    };
    if !tag.eq_ignore_ascii_case("SpeedpostTrItemData") {
        return None;
    }
    sub.get(1).and_then(speedpost_mph_from_values).or_else(|| {
        sub.get(2).and_then(|a| match a {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        })
    })
}

fn speedpost_mph_from_values(ast: &Ast) -> Option<f64> {
    if let Ast::List(values) = ast {
        return values.get(1).and_then(|a| match a {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        });
    }
    match ast {
        Ast::Atom(at) => atom_to_number(at),
        _ => None,
    }
}

fn parse_tr_item_scalar(ast: &Ast) -> Option<u32> {
    if let Some(v) = parse_u32(ast) {
        return Some(v);
    }
    if let Ast::List(inner) = ast {
        return inner.first().and_then(parse_u32);
    }
    None
}

fn scalar_from_ast(ast: &Ast) -> Option<f64> {
    match ast {
        Ast::Atom(at) => atom_to_number(at),
        Ast::List(inner) => inner.first().and_then(|x| match x {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        }),
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
    } else if tag.eq_ignore_ascii_case("SpeedPostItem") {
        TrItemKind::SpeedPost {
            speed_mph: parse_speed_post_limit_mph(items),
        }
    } else {
        TrItemKind::Other
    };

    Some(TrItem {
        id,
        kind,
        distance_m,
        world: find_tr_item_world_pose(items),
    })
}

/// `( TrItemRData x y z tileX tileZ )`.
fn find_tr_item_world_pose(item: &[Ast]) -> Option<TrItemWorldPose> {
    let mut i = 0;
    while i < item.len() {
        match &item[i] {
            Ast::List(sub) => {
                let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
                    i += 1;
                    continue;
                };
                if tag.eq_ignore_ascii_case("TrItemRData") {
                    let nums: Vec<f64> = sub
                        .iter()
                        .skip(1)
                        .filter_map(|a| match a {
                            Ast::Atom(at) => atom_to_number(at),
                            _ => None,
                        })
                        .collect();
                    if nums.len() >= 5 {
                        return Some(TrItemWorldPose {
                            x: nums[0],
                            y: nums[1],
                            z: nums[2],
                            tile_x: nums[3] as i32,
                            tile_z: nums[4] as i32,
                        });
                    }
                }
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrItemRData") => {
                // JINX: `TrItemRData ( x y z tileX tileZ )` → Symbol + List(args).
                let nums: Vec<f64> = match item.get(i + 1) {
                    Some(Ast::List(inner)) => inner
                        .iter()
                        .filter_map(|a| match a {
                            Ast::Atom(at) => atom_to_number(at),
                            _ => None,
                        })
                        .collect(),
                    _ => item[i + 1..]
                        .iter()
                        .take(5)
                        .filter_map(|a| match a {
                            Ast::Atom(at) => atom_to_number(at),
                            _ => None,
                        })
                        .collect(),
                };
                if nums.len() >= 5 {
                    return Some(TrItemWorldPose {
                        x: nums[0],
                        y: nums[1],
                        z: nums[2],
                        tile_x: nums[3] as i32,
                        tile_z: nums[4] as i32,
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_tr_item_id(item: &[Ast]) -> Option<u32> {
    let mut i = 0;
    while i < item.len() {
        match &item[i] {
            Ast::List(sub) => {
                let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
                    i += 1;
                    continue;
                };
                if tag.eq_ignore_ascii_case("TrItemId") {
                    return sub.get(1).and_then(parse_tr_item_scalar);
                }
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrItemId") => {
                if let Some(v) = item.get(i + 1).and_then(parse_tr_item_scalar) {
                    return Some(v);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `(TrItemSData <distance_m> <flags>)` — first numeric child is the distance.
fn find_tr_item_distance(item: &[Ast]) -> f64 {
    let mut i = 0;
    while i < item.len() {
        match &item[i] {
            Ast::List(sub) => {
                let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
                    i += 1;
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
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("TrItemSData") => {
                if let Some(v) = item.get(i + 1).and_then(scalar_from_ast) {
                    return v;
                }
            }
            _ => {}
        }
        i += 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../openrailsrs-msts/tests/fixtures")
    }

    #[test]
    fn index_item_hosts_maps_tritem_to_vector() {
        let tdb =
            TrackDbFile::from_path(fixtures_dir().join("with_signals/route.tdb")).expect("tdb");
        let hosts = tdb.index_item_hosts();
        assert_eq!(hosts.get(&1), Some(&vec![2]));
        assert_eq!(tdb.host_vector_for_item(1), Some(2));
        assert_eq!(tdb.tr_item_host(1).map(|h| h.vector_id), Some(2));
        let item = tdb.item_by_id(1).expect("item 1");
        assert!(matches!(item.kind, TrItemKind::Signal { .. }));
    }

    #[test]
    fn parse_unified_minimal_tdb() {
        let tdb = TrackDbFile::from_path(fixtures_dir().join("minimal.tdb")).expect("minimal");
        assert_eq!(tdb.nodes.len(), 3);
        assert!(matches!(tdb.nodes[0].kind, TrackNodeKind::End));
    }

    #[test]
    fn bevy_position_candidates_rebase_tile_z_boundary() {
        let junction = TrackVectorPoint {
            tile_x: -6081,
            tile_z: 14926,
            x: 18.488,
            y: 28.5577,
            z: -1016.008,
        };
        let section = TrVectorSectionRecord {
            shape_idx: 38700,
            aux_shape_idx: 38669,
            header_tile_x: -6081,
            header_tile_z: 14926,
            start: TrackVectorPoint {
                tile_x: -6081,
                tile_z: 14925,
                x: 6.833565,
                y: 28.5577,
                z: 1018.938,
            },
            ax: 0.0,
            ay: 0.7287974,
            az: 0.0,
        };
        let candidates = section.bevy_position_candidates(Some(junction));
        let (jx, jy, jz) = junction.bevy_position();
        let j = (jx, jy, jz);
        let best = candidates
            .iter()
            .map(|(x, _y, z)| {
                let dx = x - j.0;
                let dz = z - j.2;
                (dx * dx + dz * dz).sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            best < 15.0,
            "tile-z boundary rebase should land near junction UiD, best={best}m"
        );
    }

    #[test]
    fn bevy_position_candidates_rebase_tile_x_boundary() {
        let junction = TrackVectorPoint {
            tile_x: -6079,
            tile_z: 14925,
            x: -1018.98,
            y: 28.5577,
            z: 273.3258,
        };
        let section = TrVectorSectionRecord {
            shape_idx: 38701,
            aux_shape_idx: 38668,
            header_tile_x: -6079,
            header_tile_z: 14925,
            start: TrackVectorPoint {
                tile_x: -6080,
                tile_z: 14925,
                x: 1018.18,
                y: 28.5577,
                z: 287.168,
            },
            ax: 0.0,
            ay: 2.409297,
            az: 0.0,
        };
        let candidates = section.bevy_position_candidates(Some(junction));
        let (jx, _jy, jz) = junction.bevy_position();
        let best = candidates
            .iter()
            .map(|(x, _y, z)| {
                let dx = x - jx;
                let dz = z - jz;
                (dx * dx + dz * dz).sqrt()
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            best < 20.0,
            "tile-x boundary rebase should land near junction UiD, best={best}m"
        );
    }

    #[test]
    fn parse_native_msts_tdb() {
        let tdb = TrackDbFile::from_path(fixtures_dir().join("native_msts.tdb")).expect("native");
        assert_eq!(tdb.nodes.len(), 4, "expected 4 track nodes");
        let end1 = tdb.nodes.iter().find(|n| n.id == 1).expect("node 1");
        assert_eq!(end1.pin_refs.len(), 1);
        assert_eq!(end1.pin_refs[0].node_id, 2);
        let vectors: Vec<_> = tdb
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, TrackNodeKind::Vector { .. }))
            .collect();
        assert_eq!(vectors.len(), 1);
        if let TrackNodeKind::Vector {
            length_m,
            pins,
            geometry,
            sections,
            ..
        } = &vectors[0].kind
        {
            assert!(
                (*length_m - 25.0).abs() < 1e-6,
                "single-section fallback length should be used until tsection.dat is consulted, got {length_m}"
            );
            assert_eq!(*pins, (3, 1));
            assert!(geometry.is_some(), "native section geometry should parse");
            assert_eq!(sections.len(), 1);
            assert_eq!(sections[0].shape_idx, 38507);
            assert!((sections[0].ay - 2.91349).abs() < 1e-4);
            assert_eq!(sections[0].heading_deg(), Some(2.91349));
        }
        let junction = tdb
            .nodes
            .iter()
            .find(|n| matches!(n.kind, TrackNodeKind::Junction { .. }))
            .expect("junction");
        if let TrackNodeKind::Junction { pins } = &junction.kind {
            assert_eq!(pins.len(), 3);
            assert_eq!(pins[0].node_id, 4);
            assert_eq!(pins[0].branch_index, 0);
        }
        assert_eq!(tdb.items.len(), 1);
        let index = tdb.index_vector_sections_by_shape();
        let entries = index.get(&38507).expect("shape 38507");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].node_id, 2);
    }

    #[test]
    fn parse_speed_post_item_from_route_tdb() {
        let tdb =
            TrackDbFile::from_path(fixtures_dir().join("with_events/route.tdb")).expect("events");
        let post = tdb
            .items
            .iter()
            .find(|i| i.id == 3)
            .expect("speed post item 3");
        assert!(
            matches!(post.kind, TrItemKind::SpeedPost { speed_mph } if (speed_mph - 50.0).abs() < 1e-6)
        );
        let vector = tdb.nodes.iter().find(|n| n.id == 2).expect("vector node 2");
        if let TrackNodeKind::Vector { item_ids, .. } = &vector.kind {
            assert!(item_ids.contains(&3));
        } else {
            panic!("node 2 not vector");
        }
    }

    #[test]
    fn parse_native_tit_style_speed_post() {
        use crate::parser::parse_from_first_paren;
        let src = r#"( 1
  SpeedPostItem
  (
    TrItemId ( 2706 )
    TrItemSData ( 6.229199 0 )
    SpeedpostTrItemData ( 898 50 0.9293867 )
  )
)"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let mut tdb = TrackDbFile::default();
        if let Ast::List(root) = &ast {
            parse_tr_item_table_entries(root, &mut tdb.items);
        }
        let post = tdb.items.iter().find(|i| i.id == 2706).expect("2706");
        assert!(
            matches!(post.kind, TrItemKind::SpeedPost { speed_mph } if (speed_mph - 50.0).abs() < 1e-6)
        );
    }
}
