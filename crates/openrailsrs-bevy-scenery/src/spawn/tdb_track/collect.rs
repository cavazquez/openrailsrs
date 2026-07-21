//! Branch walking, per-vector chords, junction bridges, and path segments.
//!
//! Apps inject [`FocusQuery`] `{ center, radius_m }` instead of hardcoding `RouteFocus`.

use bevy::prelude::*;
use openrailsrs_formats::{
    TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind,
    TrackVectorGeometry, TrackVectorPoint,
};
use std::collections::{HashMap, HashSet};

use crate::spawn::dyntrack::ProceduralTrackSegment;
use crate::spawn::tdb_track::geometry::{
    TDB_JUNCTION_BRIDGE_SECTION, TdbChord, chord_heading_and_length, point_world_vec3,
    procedural_segment_from_span, section_is_drawable, section_path_spans,
    section_single_curve_metadata, section_world_vec3, single_section_end_world,
    vector_junction_face_world,
};
use crate::spawn::tdb_track::focus::{ChordCollectLimits, FocusQuery};

#[derive(Clone)]
struct BranchVectorStep {
    vector_id: u32,
    entry_pin: usize,
}

/// Oriented section endpoint used for junction face / audit diagnostics.
#[derive(Clone, Copy, Debug)]
pub struct AnchorPoint {
    pub world: Vec3,
    pub node_id: u32,
    pub section_index: usize,
    pub shape_idx: u32,
}

/// If a vector has no plausible anchor near a connected junction, snap its face to junction UiD.
const JUNCTION_FACE_FALLBACK_DIST_M: f32 = 60.0;
const SHORT_VECTOR_JUNCTION_FACE_FALLBACK_DIST_M: f32 = 30.0;
/// Gap between junction-facing endpoints of two connected vectors (metres, XZ).
pub fn inter_node_junction_gap_m(
    tdb: &TrackDbFile,
    a: u32,
    b: u32,
    tsection: Option<&TSectionCatalog>,
) -> Option<f32> {
    let nodes_by_id: HashMap<u32, &TrackDbNode> = tdb.nodes.iter().map(|n| (n.id, n)).collect();
    let (side_a, _, side_b) = facing_junction_endpoints(a, b, &nodes_by_id, tsection)?;
    let dx = side_a.x - side_b.x;
    let dz = side_a.z - side_b.z;
    Some((dx * dx + dz * dz).sqrt())
}
pub fn collect_tdb_chords(
    tdb: &TrackDbFile,
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
    limits: ChordCollectLimits,
) -> Vec<TdbChord> {
    if tdb.nodes.len() <= limits.branch_walk_max_nodes {
        let branch_chords = collect_tdb_chords_from_branches(tdb, focus, tsection, limits);
        if !branch_chords.is_empty() {
            return branch_chords;
        }
    }
    let per_vector = collect_tdb_chords_per_vector(tdb, focus, tsection);
    let bridges = collect_junction_bridge_chords(tdb, focus, &per_vector, tsection);
    dedupe_chords(per_vector.into_iter().chain(bridges).collect())
}

/// Full TSection path segments for `--track-dev` / `--run-corridor` mesh (arcs + multi-link shapes).
pub fn collect_tdb_path_segments(
    tdb: &TrackDbFile,
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
    limits: ChordCollectLimits,
) -> Vec<ProceduralTrackSegment> {
    if tdb.nodes.len() <= limits.branch_walk_max_nodes {
        let branch = collect_path_segments_from_branches(tdb, focus, tsection, limits);
        if !branch.is_empty() {
            return branch;
        }
    }
    collect_path_segments_per_vector(tdb, focus, tsection)
}

/// Walk `TrPins` from end nodes near `focus` and emit chords along continuous branches.
fn collect_tdb_chords_from_branches(
    tdb: &TrackDbFile,
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
    limits: ChordCollectLimits,
) -> Vec<TdbChord> {
    let mut branches = Vec::new();
    for node in &tdb.nodes {
        if !matches!(node.kind, TrackNodeKind::End) {
            continue;
        }
        if !node
            .pin_refs
            .iter()
            .any(|pin| node_reaches_focus(tdb, pin.node_id, focus, 4, tsection))
        {
            continue;
        }
        for pin in &node.pin_refs {
            if branches.len() >= limits.max_branches {
                break;
            }
            if !node_reaches_focus(tdb, pin.node_id, focus, 4, tsection) {
                continue;
            }
            let mut path = Vec::new();
            let mut visited = HashSet::new();
            walk_track_branch(
                tdb,
                pin.node_id,
                node.id,
                &mut path,
                &mut branches,
                &mut visited,
            );
        }
        if branches.len() >= limits.max_branches {
            break;
        }
    }
    dedupe_chords(
        branches
            .into_iter()
            .flat_map(|branch| branch_to_chords(tdb, &branch, focus, tsection))
            .collect(),
    )
}

/// True if `node_id` or a pin neighbour within `hops` steps has a vector section near `focus`.
fn node_reaches_focus(
    tdb: &TrackDbFile,
    node_id: u32,
    focus: &FocusQuery,
    hops: u8,
    tsection: Option<&TSectionCatalog>,
) -> bool {
    if hops == 0 {
        return false;
    }
    let Some(node) = tdb.node_by_id(node_id) else {
        return false;
    };
    match &node.kind {
        TrackNodeKind::Vector { sections, .. } => sections.iter().any(|s| {
            section_is_drawable(s, tsection)
                && focus.horizontal_distance(section_world_vec3(*s, Some(focus.center))) <= focus.radius_m
        }),
        _ => node.pin_refs.iter().any(|pin| {
            node_reaches_focus(
                tdb,
                pin.node_id,
                focus,
                hops.saturating_sub(1),
                tsection,
            )
        }),
    }
}

fn dedupe_chords(chords: Vec<TdbChord>) -> Vec<TdbChord> {
    let mut seen = HashSet::new();
    chords
        .into_iter()
        .filter(|c| {
            let key = (
                (c.start_world.x * 2.0).round() as i32,
                (c.start_world.z * 2.0).round() as i32,
                (c.end_world.x * 2.0).round() as i32,
                (c.end_world.z * 2.0).round() as i32,
            );
            seen.insert(key)
        })
        .collect()
}

fn walk_track_branch(
    tdb: &TrackDbFile,
    node_id: u32,
    came_from: u32,
    path: &mut Vec<BranchVectorStep>,
    branches: &mut Vec<Vec<BranchVectorStep>>,
    visited: &mut HashSet<(u32, u32)>,
) {
    if !visited.insert((came_from, node_id)) {
        if !path.is_empty() {
            branches.push(path.clone());
        }
        return;
    }

    let Some(node) = tdb.node_by_id(node_id) else {
        return;
    };

    match &node.kind {
        TrackNodeKind::End => {
            if !path.is_empty() {
                branches.push(path.clone());
            }
        }
        TrackNodeKind::Junction { .. } => {
            let mut continued = false;
            for pin in &node.pin_refs {
                if pin.node_id == came_from {
                    continue;
                }
                continued = true;
                walk_track_branch(tdb, pin.node_id, node_id, path, branches, visited);
            }
            if !continued && !path.is_empty() {
                branches.push(path.clone());
            }
        }
        TrackNodeKind::Vector { .. } => {
            let Some(entry_pin) = node.pin_refs.iter().position(|p| p.node_id == came_from) else {
                return;
            };
            path.push(BranchVectorStep {
                vector_id: node_id,
                entry_pin,
            });
            let exit_pin = if entry_pin == 0 { 1 } else { 0 };
            if let Some(exit) = node.pin_refs.get(exit_pin) {
                walk_track_branch(tdb, exit.node_id, node_id, path, branches, visited);
            } else if !path.is_empty() {
                branches.push(path.clone());
            }
            path.pop();
        }
    }

    visited.remove(&(came_from, node_id));
}

fn branch_to_chords(
    tdb: &TrackDbFile,
    branch: &[BranchVectorStep],
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    let mut out = Vec::new();
    let mut chain_hint = Some(focus.center);
    for step in branch {
        let Some(node) = tdb.node_by_id(step.vector_id) else {
            continue;
        };
        let TrackNodeKind::Vector {
            length_m: node_length_m,
            sections,
            geometry,
            ..
        } = &node.kind
        else {
            continue;
        };
        let section_count = sections
            .iter()
            .filter(|s| section_is_drawable(s, tsection))
            .count()
            .max(1);
        let mut drawable: Vec<(usize, TrVectorSectionRecord)> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| section_is_drawable(s, tsection))
            .map(|(i, s)| (i, *s))
            .collect();
        if drawable.is_empty() {
            continue;
        }
        if step.entry_pin != 0 {
            drawable.reverse();
        }
        for (i, (orig_idx, section)) in drawable.iter().enumerate() {
            let start = section_world_vec3(*section, chain_hint);
            let end = if let Some((_, next)) = drawable.get(i + 1) {
                section_world_vec3(*next, Some(start))
            } else {
                single_section_end_world(
                    *section,
                    geometry.as_ref().copied(),
                    *node_length_m,
                    step.entry_pin != 0,
                    chain_hint,
                    tsection,
                    section_count,
                )
                .unwrap_or(start)
            };
            if drawable.len() > 1
                && i + 1 == drawable.len()
                && chain_hint.is_some_and(|h| h.distance(start) < 0.5)
            {
                continue;
            }
            if (focus.horizontal_distance(start) > focus.radius_m
                && focus.horizontal_distance(end) > focus.radius_m)
                || chord_heading_and_length(start, end).is_none()
            {
                chain_hint = Some(end);
                continue;
            }
            let (curve_radius_m, curve_angle_deg) = section_single_curve_metadata(
                *section,
                tsection,
                chain_hint,
                *node_length_m,
                section_count,
            );
            out.push(TdbChord {
                node_id: node.id,
                section_index: *orig_idx,
                span_index: 0,
                shape_idx: section.shape_index,
                start_world: start,
                end_world: end,
                curve_radius_m,
                curve_angle_deg,
            });
            chain_hint = Some(end);
        }
    }
    out
}

fn collect_path_segments_from_branches(
    tdb: &TrackDbFile,
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
    limits: ChordCollectLimits,
) -> Vec<ProceduralTrackSegment> {
    let mut branches = Vec::new();
    for node in &tdb.nodes {
        if !matches!(node.kind, TrackNodeKind::End) {
            continue;
        }
        if !node
            .pin_refs
            .iter()
            .any(|pin| node_reaches_focus(tdb, pin.node_id, focus, 4, tsection))
        {
            continue;
        }
        for pin in &node.pin_refs {
            if branches.len() >= limits.max_branches {
                break;
            }
            if !node_reaches_focus(tdb, pin.node_id, focus, 4, tsection) {
                continue;
            }
            let mut path = Vec::new();
            let mut visited = HashSet::new();
            walk_track_branch(
                tdb,
                pin.node_id,
                node.id,
                &mut path,
                &mut branches,
                &mut visited,
            );
        }
        if branches.len() >= limits.max_branches {
            break;
        }
    }
    branches
        .into_iter()
        .flat_map(|branch| branch_to_path_segments(tdb, &branch, focus, tsection))
        .collect()
}

fn branch_to_path_segments(
    tdb: &TrackDbFile,
    branch: &[BranchVectorStep],
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
) -> Vec<ProceduralTrackSegment> {
    let mut out = Vec::new();
    let mut chain_hint = Some(focus.center);
    for step in branch {
        let Some(node) = tdb.node_by_id(step.vector_id) else {
            continue;
        };
        let TrackNodeKind::Vector {
            length_m: node_length_m,
            sections,
            ..
        } = &node.kind
        else {
            continue;
        };
        let section_count = sections
            .iter()
            .filter(|s| section_is_drawable(s, tsection))
            .count()
            .max(1);
        let mut drawable: Vec<(usize, TrVectorSectionRecord)> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| section_is_drawable(s, tsection))
            .map(|(i, s)| (i, *s))
            .collect();
        if drawable.is_empty() {
            continue;
        }
        if step.entry_pin != 0 {
            drawable.reverse();
        }
        for (i, (_, section)) in drawable.iter().enumerate() {
            if drawable.len() > 1 && i + 1 == drawable.len() {
                let start = section_world_vec3(*section, chain_hint);
                if chain_hint.is_some_and(|h| h.distance(start) < 0.5) {
                    continue;
                }
            }
            let next_anchor = drawable
                .get(i + 1)
                .map(|(_, s)| section_world_vec3(*s, chain_hint));
            let spans = section_path_spans(
                *section,
                tsection,
                chain_hint,
                *node_length_m,
                section_count,
                next_anchor,
            );
            for span in &spans {
                if focus.horizontal_distance(span.start_world) > focus.radius_m
                    && focus.horizontal_distance(span.end_world) > focus.radius_m
                {
                    continue;
                }
                if chord_heading_and_length(span.start_world, span.end_world).is_some()
                    || span.is_curved()
                {
                    out.push(procedural_segment_from_span(*span));
                }
            }
            if let Some(last) = spans.last() {
                chain_hint = Some(last.end_world);
            }
        }
    }
    out
}

/// Section start/end anchors along a vector, oriented by `entry_pin`.
pub fn vector_oriented_anchors(
    node: &TrackDbNode,
    entry_pin: usize,
    near_hint: Option<Vec3>,
    tsection: Option<&TSectionCatalog>,
) -> Vec<AnchorPoint> {
    let TrackNodeKind::Vector {
        length_m: node_length_m,
        sections,
        ..
    } = &node.kind
    else {
        return Vec::new();
    };
    let section_count = sections
        .iter()
        .filter(|s| section_is_drawable(s, tsection))
        .count()
        .max(1);

    let sections: Vec<(usize, TrVectorSectionRecord)> = sections
        .iter()
        .enumerate()
        .filter(|(_, s)| section_is_drawable(s, tsection))
        .map(|(i, s)| (i, *s))
        .collect();
    if sections.is_empty() {
        return Vec::new();
    }

    let ordered: Vec<(usize, TrVectorSectionRecord)> = if entry_pin == 0 {
        sections
    } else {
        sections.into_iter().rev().collect()
    };

    let mut out: Vec<AnchorPoint> = Vec::new();
    let mut chain_hint = near_hint;
    for (i, (idx, section)) in ordered.iter().enumerate() {
        let next_anchor = ordered
            .get(i + 1)
            .map(|(_, s)| section_world_vec3(*s, chain_hint));
        let spans = section_path_spans(
            *section,
            tsection,
            chain_hint,
            *node_length_m,
            section_count,
            next_anchor,
        );
        if spans.is_empty() {
            continue;
        }
        let start = spans.first().unwrap().start_world;
        let end = spans.last().unwrap().end_world;
        if out
            .last()
            .is_none_or(|last| last.world.distance(start) > 0.25)
        {
            out.push(AnchorPoint {
                world: start,
                node_id: node.id,
                section_index: *idx,
                shape_idx: section.shape_index,
            });
        }
        out.push(AnchorPoint {
            world: end,
            node_id: node.id,
            section_index: *idx,
            shape_idx: section.shape_index,
        });
        chain_hint = Some(end);
    }
    out
}

/// Spawn merged procedural track along the `.tdb` vector graph (`--track-dev`).
fn collect_tdb_chords_per_vector(
    tdb: &TrackDbFile,
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    let mut out = Vec::new();
    for node in &tdb.nodes {
        let TrackNodeKind::Vector {
            sections, geometry, ..
        } = &node.kind
        else {
            continue;
        };
        let sections: Vec<(usize, TrVectorSectionRecord)> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| section_is_drawable(s, tsection))
            .map(|(i, s)| (i, *s))
            .collect();
        if sections.is_empty() {
            continue;
        }
        out.extend(collect_vector_section_chords(
            node.id, &sections, focus, tsection, tdb, node, *geometry,
        ));
    }
    out
}

/// One audit chord per drawable section (anchor → next anchor; curve metadata when single arc).
#[allow(clippy::too_many_arguments)]
fn collect_vector_section_chords(
    node_id: u32,
    sections: &[(usize, TrVectorSectionRecord)],
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
    tdb: &TrackDbFile,
    node: &TrackDbNode,
    geometry: Option<TrackVectorGeometry>,
) -> Vec<TdbChord> {
    let TrackNodeKind::Vector {
        length_m: node_length_m,
        ..
    } = &node.kind
    else {
        return Vec::new();
    };
    let section_records: Vec<TrVectorSectionRecord> = sections.iter().map(|(_, s)| *s).collect();
    let mut chain_hint = Some(vector_chain_seed(node, &section_records, tdb, focus));
    let section_count = sections.len().max(1);
    let mut out = Vec::new();
    for (i, (orig_idx, section)) in sections.iter().enumerate() {
        let start = section_world_vec3(*section, chain_hint);
        let end = if let Some((_, next)) = sections.get(i + 1) {
            section_world_vec3(*next, Some(start))
        } else {
            single_section_end_world(
                *section,
                geometry,
                *node_length_m,
                false,
                chain_hint,
                tsection,
                section_count,
            )
            .unwrap_or(start)
        };
        if sections.len() > 1
            && i + 1 == sections.len()
            && chain_hint.is_some_and(|h| h.distance(start) < 0.5)
        {
            continue;
        }
        if (focus.horizontal_distance(start) > focus.radius_m
            && focus.horizontal_distance(end) > focus.radius_m)
            || chord_heading_and_length(start, end).is_none()
        {
            chain_hint = Some(end);
            continue;
        }
        let (curve_radius_m, curve_angle_deg) = section_single_curve_metadata(
            *section,
            tsection,
            chain_hint,
            *node_length_m,
            section_count,
        );
        out.push(TdbChord {
            node_id,
            section_index: *orig_idx,
            span_index: 0,
            shape_idx: section.shape_index,
            start_world: start,
            end_world: end,
            curve_radius_m,
            curve_angle_deg,
        });
        chain_hint = Some(end);
    }
    out
}

fn collect_path_segments_per_vector(
    tdb: &TrackDbFile,
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
) -> Vec<ProceduralTrackSegment> {
    let mut out = Vec::new();
    for node in &tdb.nodes {
        let TrackNodeKind::Vector {
            length_m: node_length_m,
            sections,
            geometry,
            ..
        } = &node.kind
        else {
            continue;
        };
        let sections: Vec<(usize, TrVectorSectionRecord)> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| section_is_drawable(s, tsection))
            .map(|(i, s)| (i, *s))
            .collect();
        if sections.is_empty() {
            continue;
        }
        out.extend(collect_vector_path_segments(
            &sections,
            focus,
            tsection,
            *node_length_m,
            section_count_for_sections(&sections, tsection),
            geometry.as_ref().copied(),
            node,
            tdb,
        ));
    }
    out
}

fn section_count_for_sections(
    sections: &[(usize, TrVectorSectionRecord)],
    _tsection: Option<&TSectionCatalog>,
) -> usize {
    sections.len().max(1)
}

#[allow(clippy::too_many_arguments)]
fn collect_vector_path_segments(
    sections: &[(usize, TrVectorSectionRecord)],
    focus: &FocusQuery,
    tsection: Option<&TSectionCatalog>,
    node_length_m: f64,
    section_count: usize,
    geometry: Option<TrackVectorGeometry>,
    node: &TrackDbNode,
    tdb: &TrackDbFile,
) -> Vec<ProceduralTrackSegment> {
    let section_records: Vec<TrVectorSectionRecord> = sections.iter().map(|(_, s)| *s).collect();
    let mut chain_hint = Some(vector_chain_seed(node, &section_records, tdb, focus));
    let mut out = Vec::new();
    for (i, (_, section)) in sections.iter().enumerate() {
        if sections.len() > 1
            && i + 1 == sections.len()
            && chain_hint
                .is_some_and(|h| h.distance(section_world_vec3(*section, chain_hint)) < 0.5)
        {
            continue;
        }
        let next_anchor = if i + 1 < sections.len() {
            sections
                .get(i + 1)
                .map(|(_, s)| section_world_vec3(*s, chain_hint))
        } else if let Some(geom) = geometry {
            let header = (section.header_tile_x, section.header_tile_z);
            Some(point_world_vec3(geom.end, header, chain_hint))
        } else {
            None
        };
        let spans = section_path_spans(
            *section,
            tsection,
            chain_hint,
            node_length_m,
            section_count,
            next_anchor,
        );
        for span in &spans {
            if focus.horizontal_distance(span.start_world) > focus.radius_m
                && focus.horizontal_distance(span.end_world) > focus.radius_m
            {
                continue;
            }
            if chord_heading_and_length(span.start_world, span.end_world).is_some()
                || span.is_curved()
            {
                out.push(procedural_segment_from_span(*span));
            }
        }
        if let Some(last) = spans.last() {
            chain_hint = Some(last.end_world);
        }
    }
    out
}

/// Best hint to rebase the first section anchor: route focus or a connected junction/end.
fn vector_chain_seed(
    node: &TrackDbNode,
    sections: &[TrVectorSectionRecord],
    tdb: &TrackDbFile,
    focus: &FocusQuery,
) -> Vec3 {
    let mut hints = vec![focus.center];
    for pin in &node.pin_refs {
        let Some(neighbor) = tdb.node_by_id(pin.node_id) else {
            continue;
        };
        if matches!(
            neighbor.kind,
            TrackNodeKind::Junction { .. } | TrackNodeKind::End
        ) {
            if let Some(p) = node_world_position(neighbor) {
                hints.push(p);
            }
        }
    }
    hints
        .into_iter()
        .min_by(|a, b| {
            let best_dist = |hint: Vec3| {
                sections
                    .iter()
                    .map(|s| {
                        let p = section_world_vec3(*s, Some(hint));
                        focus.horizontal_distance(p)
                    })
                    .fold(f32::INFINITY, f32::min)
            };
            best_dist(*a)
                .partial_cmp(&best_dist(*b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(focus.center)
}

/// Short chords across TrPin junctions between vectors already in `per_vector` (large routes).
fn collect_junction_bridge_chords(
    tdb: &TrackDbFile,
    _focus: &FocusQuery,
    per_vector: &[TdbChord],
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    let vector_ids: HashSet<u32> = per_vector.iter().map(|c| c.node_id).collect();
    if vector_ids.is_empty() {
        return Vec::new();
    }
    let nodes_by_id: HashMap<u32, &TrackDbNode> = tdb.nodes.iter().map(|n| (n.id, n)).collect();
    let mut seen_pairs = HashSet::new();
    let mut out = Vec::new();

    for &a in &vector_ids {
        for b in connected_vector_neighbors(a, &vector_ids, &nodes_by_id) {
            let pair = if a < b { (a, b) } else { (b, a) };
            if !seen_pairs.insert(pair) {
                continue;
            }
            let Some((side_a, anchor_a, side_b)) =
                facing_junction_endpoints(a, b, &nodes_by_id, tsection)
            else {
                continue;
            };
            if side_a.distance(side_b) < 0.25 {
                continue;
            }
            if chord_heading_and_length(side_a, side_b).is_none() {
                continue;
            }
            out.push(TdbChord {
                node_id: a,
                section_index: TDB_JUNCTION_BRIDGE_SECTION,
                span_index: 0,
                shape_idx: anchor_a.shape_idx,
                start_world: side_a,
                end_world: side_b,
                curve_radius_m: None,
                curve_angle_deg: None,
            });
        }
    }
    out
}

fn connected_vector_neighbors(
    vector_id: u32,
    vector_ids: &HashSet<u32>,
    nodes_by_id: &HashMap<u32, &TrackDbNode>,
) -> Vec<u32> {
    let Some(node) = nodes_by_id.get(&vector_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pin in &node.pin_refs {
        if pin.node_id != vector_id && vector_ids.contains(&pin.node_id) {
            out.push(pin.node_id);
        }
        let Some(pin_node) = nodes_by_id.get(&pin.node_id) else {
            continue;
        };
        for next in &pin_node.pin_refs {
            if next.node_id != vector_id && vector_ids.contains(&next.node_id) {
                out.push(next.node_id);
            }
        }
    }
    out
}

fn facing_junction_endpoints(
    a: u32,
    b: u32,
    nodes_by_id: &HashMap<u32, &TrackDbNode>,
    tsection: Option<&TSectionCatalog>,
) -> Option<(Vec3, AnchorPoint, Vec3)> {
    let node_a = nodes_by_id.get(&a)?;
    let node_b = nodes_by_id.get(&b)?;

    if let Some(pin_a) = node_a.pin_refs.iter().position(|p| p.node_id == b) {
        let pin_b = node_b.pin_refs.iter().position(|p| p.node_id == a)?;
        let hint = direct_link_hint(node_a, pin_a, node_b, pin_b, tsection);
        let anchor_a = nearest_oriented_anchor(node_a, pin_a, hint, tsection)?;
        let anchor_b = nearest_oriented_anchor(node_b, pin_b, hint, tsection)?;
        return Some((anchor_a.world, anchor_a, anchor_b.world));
    }

    for (pin_a_idx, pin_a) in node_a.pin_refs.iter().enumerate() {
        let Some(mid) = nodes_by_id.get(&pin_a.node_id) else {
            continue;
        };
        if !matches!(mid.kind, TrackNodeKind::Junction { .. }) {
            continue;
        }
        if !node_b.pin_refs.iter().any(|p| p.node_id == pin_a.node_id) {
            continue;
        }
        let pin_b_idx = node_b
            .pin_refs
            .iter()
            .position(|p| p.node_id == pin_a.node_id)?;
        let hint = junction_link_hint(node_a, pin_a_idx, node_b, pin_b_idx, mid, tsection)?;
        let junction_point = mid.position?;
        let anchor_a =
            nearest_junction_face_anchor(node_a, pin_a_idx, junction_point, hint, tsection)?;
        let anchor_b =
            nearest_junction_face_anchor(node_b, pin_b_idx, junction_point, hint, tsection)?;
        return Some((anchor_a.world, anchor_a, anchor_b.world));
    }
    None
}

fn junction_link_hint(
    node_a: &TrackDbNode,
    pin_a: usize,
    node_b: &TrackDbNode,
    pin_b: usize,
    junction: &TrackDbNode,
    tsection: Option<&TSectionCatalog>,
) -> Option<Vec3> {
    if let Some(j) = node_world_position(junction) {
        return Some(j);
    }
    let a0 = vector_oriented_anchors(node_a, pin_a, None, tsection)
        .into_iter()
        .next()
        .map(|a| a.world);
    let b0 = vector_oriented_anchors(node_b, pin_b, None, tsection)
        .into_iter()
        .next()
        .map(|a| a.world);
    match (a0, b0) {
        (Some(wa), Some(wb)) => Some((wa + wb) * 0.5),
        (Some(wa), None) => Some(wa),
        (None, Some(wb)) => Some(wb),
        (None, None) => None,
    }
}

fn direct_link_hint(
    node_a: &TrackDbNode,
    pin_a: usize,
    node_b: &TrackDbNode,
    pin_b: usize,
    tsection: Option<&TSectionCatalog>,
) -> Vec3 {
    let mut pts = Vec::new();
    if let Some(a) = vector_oriented_anchors(node_a, pin_a, None, tsection)
        .into_iter()
        .next()
    {
        pts.push(a.world);
    }
    if let Some(b) = vector_oriented_anchors(node_b, pin_b, None, tsection)
        .into_iter()
        .next()
    {
        pts.push(b.world);
    }
    if pts.is_empty() {
        Vec3::ZERO
    } else {
        pts.iter().copied().sum::<Vec3>() / pts.len() as f32
    }
}

fn node_world_position(node: &TrackDbNode) -> Option<Vec3> {
    node.position.map(|p| {
        let (x, y, z) = p.bevy_position();
        Vec3::new(x, y, z)
    })
}

/// Closest oriented anchor on `node` to `near`.
pub fn nearest_oriented_anchor(
    node: &TrackDbNode,
    entry_pin: usize,
    near: Vec3,
    tsection: Option<&TSectionCatalog>,
) -> Option<AnchorPoint> {
    vector_oriented_anchors(node, entry_pin, Some(near), tsection)
        .into_iter()
        .min_by(|a, b| {
            a.world
                .distance(near)
                .partial_cmp(&b.world.distance(near))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn nearest_junction_face_anchor(
    node: &TrackDbNode,
    entry_pin: usize,
    junction_point: TrackVectorPoint,
    hint: Vec3,
    tsection: Option<&TSectionCatalog>,
) -> Option<AnchorPoint> {
    let TrackNodeKind::Vector {
        length_m: node_length_m,
        sections,
        ..
    } = &node.kind
    else {
        return nearest_oriented_anchor(node, entry_pin, hint, tsection);
    };
    let (jx, _, jz) = junction_point.bevy_position();
    let junction_hint = Vec3::new(jx, hint.y, jz);
    let _section_count = sections
        .iter()
        .filter(|s| section_is_drawable(s, tsection))
        .count()
        .max(1);
    if let Some(face) =
        vector_junction_face_world(sections, entry_pin, tsection, junction_hint, *node_length_m)
    {
        let shape_idx = sections
            .iter()
            .find(|s| section_is_drawable(s, tsection))
            .map(|s| s.shape_index)
            .unwrap_or(0);
        return Some(AnchorPoint {
            world: face,
            node_id: node.id,
            section_index: 0,
            shape_idx,
        });
    }
    let fallback_dist = if sections.len() <= 2 {
        SHORT_VECTOR_JUNCTION_FACE_FALLBACK_DIST_M
    } else {
        JUNCTION_FACE_FALLBACK_DIST_M
    };
    let near_hint = Some(junction_hint);
    let ref_tile = Some(junction_point);
    let mut best: Option<AnchorPoint> = None;
    let mut best_dist = f32::INFINITY;
    for anchor in vector_oriented_anchors(node, entry_pin, near_hint, tsection) {
        let section = sections.get(anchor.section_index).copied();
        let mut worlds: Vec<Vec3> = vec![anchor.world];
        if let Some(section) = section {
            worlds.extend(
                section
                    .bevy_position_candidates(ref_tile)
                    .into_iter()
                    .map(|(x, y, z)| Vec3::new(x, y, z)),
            );
        }
        for world in worlds {
            let dist = world.distance(junction_hint);
            if dist < best_dist {
                best_dist = dist;
                best = Some(AnchorPoint {
                    world,
                    node_id: anchor.node_id,
                    section_index: anchor.section_index,
                    shape_idx: anchor.shape_idx,
                });
            }
        }
    }
    if let Some(anchor) = best {
        if best_dist <= fallback_dist {
            return Some(anchor);
        }
        return Some(AnchorPoint {
            world: junction_hint,
            node_id: anchor.node_id,
            section_index: anchor.section_index,
            shape_idx: anchor.shape_idx,
        });
    }
    nearest_oriented_anchor(node, entry_pin, hint, tsection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TrackVectorPoint;

    fn section_at(x: f64, z: f64, shape_idx: u32) -> TrVectorSectionRecord {
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 0.0,
            z,
        };
        TrVectorSectionRecord {
            section_index: shape_idx,
            shape_index: 0,
            header_tile_x: start.tile_x,
            header_tile_z: start.tile_z,
            start,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
        }
    }

    #[test]
    fn focus_query_filters_distant_chords() {
        let section = section_at(0.0, 0.0, 1);
        let tdb = TrackDbFile {
            nodes: vec![TrackDbNode {
                id: 1,
                position: None,
                pin_refs: Vec::new(),
                kind: TrackNodeKind::Vector {
                    length_m: 100.0,
                    speed_limit_mps: 0.0,
                    pins: (0, 0),
                    item_ids: Vec::new(),
                    sections: vec![section],
                    geometry: None,
                },
            }],
            items: Vec::new(),
        };
        let near = FocusQuery::new(Vec3::ZERO, 200.0);
        let far = FocusQuery::new(Vec3::new(10_000.0, 0.0, 10_000.0), 50.0);
        let near_chords =
            collect_tdb_chords(&tdb, &near, None, ChordCollectLimits::PER_VECTOR_ONLY);
        let far_chords = collect_tdb_chords(&tdb, &far, None, ChordCollectLimits::PER_VECTOR_ONLY);
        assert!(!near_chords.is_empty());
        assert!(far_chords.is_empty());
        assert_eq!(
            crate::spawn::tdb_track::tdb_chord_geometry_hash(&near_chords),
            crate::spawn::tdb_track::tdb_chord_geometry_hash(&near_chords)
        );
    }
}

