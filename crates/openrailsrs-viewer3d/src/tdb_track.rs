//! Continuous procedural track from the MSTS `.tdb` vector graph (Phase 3).
//!
//! Branches are walked via `TrPins` (end → vector → junction → …).  Each consecutive
//! pair of section anchors along a branch becomes one rail pair.  Drawn without
//! sleepers in `--track-dev`.

use bevy::prelude::*;
use openrailsrs_formats::{
    TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind,
    TrackVectorGeometry, TrackVectorPoint,
};
use std::collections::{HashMap, HashSet};

use crate::dyntrack::{ProceduralTrackStyle, spawn_procedural_track_batch};
use openrailsrs_bevy_scenery::spawn::dyntrack::ProceduralTrackSegment;
use openrailsrs_bevy_scenery::spawn::tdb_track::{
    chord_heading_and_length, end_from_heading, point_world_vec3, section_shape_length_m,
    section_world_vec3, straight_segment_from_tsection_link,
};

use crate::launch::{
    RunCorridorPath, TRACK_DEV_BRANCH_WALK_MAX_NODES, TRACK_DEV_MAX_BRANCHES,
    TRACK_DEV_MAX_SEGMENTS, ViewerSceneryMode, tdb_radius_for_mode, track_dev_render_enabled,
};
use crate::shapes::RouteAssets;
use crate::track::TrackScene;
use crate::track_audit::run_track_dev_audit;
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};
pub use openrailsrs_bevy_scenery::spawn::tdb_track::{TDB_JUNCTION_BRIDGE_SECTION, TdbChord};

#[derive(Clone, Copy, Debug)]
struct BranchVectorStep {
    vector_id: u32,
    entry_pin: usize,
}

#[derive(Clone, Copy, Debug)]
struct AnchorPoint {
    world: Vec3,
    node_id: u32,
    section_index: usize,
    shape_idx: u32,
}

/// If a vector has no plausible anchor near a connected junction, snap its face to junction UiD.
const JUNCTION_FACE_FALLBACK_DIST_M: f32 = 60.0;
const SHORT_VECTOR_JUNCTION_FACE_FALLBACK_DIST_M: f32 = 30.0;
/// Gap between junction-facing endpoints of two connected vectors (metres, XZ).
pub fn inter_node_junction_gap_m(tdb: &TrackDbFile, a: u32, b: u32) -> Option<f32> {
    let nodes_by_id: HashMap<u32, &TrackDbNode> = tdb.nodes.iter().map(|n| (n.id, n)).collect();
    let (side_a, _, side_b) = facing_junction_endpoints(a, b, &nodes_by_id)?;
    let dx = side_a.x - side_b.x;
    let dz = side_a.z - side_b.z;
    Some((dx * dx + dz * dz).sqrt())
}
pub fn collect_tdb_chords(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    if tdb.nodes.len() <= TRACK_DEV_BRANCH_WALK_MAX_NODES {
        let branch_chords = collect_tdb_chords_from_branches(tdb, focus, radius_m, tsection);
        if !branch_chords.is_empty() {
            return branch_chords;
        }
    }
    let per_vector = collect_tdb_chords_per_vector(tdb, focus, radius_m, tsection);
    let bridges = collect_junction_bridge_chords(tdb, focus, radius_m, &per_vector);
    dedupe_chords(per_vector.into_iter().chain(bridges).collect())
}

/// Walk `TrPins` from end nodes near `focus` and emit chords along continuous branches.
fn collect_tdb_chords_from_branches(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    let mut branches = Vec::new();
    for node in &tdb.nodes {
        if !matches!(node.kind, TrackNodeKind::End) {
            continue;
        }
        if !node
            .pin_refs
            .iter()
            .any(|pin| node_reaches_focus(tdb, pin.node_id, focus, radius_m, 4))
        {
            continue;
        }
        for pin in &node.pin_refs {
            if branches.len() >= TRACK_DEV_MAX_BRANCHES {
                viewer_log!(
                    "openrailsrs-viewer3d: tdb-graph — branch walk cap ({TRACK_DEV_MAX_BRANCHES}) reached"
                );
                break;
            }
            if !node_reaches_focus(tdb, pin.node_id, focus, radius_m, 4) {
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
        if branches.len() >= TRACK_DEV_MAX_BRANCHES {
            break;
        }
    }
    dedupe_chords(
        branches
            .into_iter()
            .flat_map(|branch| branch_to_chords(tdb, &branch, focus, radius_m, tsection))
            .collect(),
    )
}

/// True if `node_id` or a pin neighbour within `hops` steps has a vector section near `focus`.
fn node_reaches_focus(
    tdb: &TrackDbFile,
    node_id: u32,
    focus: &RouteFocus,
    radius_m: f32,
    hops: u8,
) -> bool {
    if hops == 0 {
        return false;
    }
    let Some(node) = tdb.node_by_id(node_id) else {
        return false;
    };
    match &node.kind {
        TrackNodeKind::Vector { sections, .. } => {
            sections.iter().filter(|s| s.shape_idx != 0).any(|s| {
                focus.horizontal_distance(section_world_vec3(*s, Some(focus.center))) <= radius_m
            })
        }
        _ => node.pin_refs.iter().any(|pin| {
            node_reaches_focus(tdb, pin.node_id, focus, radius_m, hops.saturating_sub(1))
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
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    let mut points: Vec<AnchorPoint> = Vec::new();
    for step in branch {
        let Some(node) = tdb.node_by_id(step.vector_id) else {
            continue;
        };
        for anchor in vector_oriented_anchors(node, step.entry_pin, Some(focus.center), tsection) {
            if points
                .last()
                .is_some_and(|last| last.world.distance(anchor.world) < 0.25)
            {
                continue;
            }
            points.push(anchor);
        }
    }

    let mut chords = Vec::new();
    for pair in points.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if focus.horizontal_distance(start.world) > radius_m
            && focus.horizontal_distance(end.world) > radius_m
        {
            continue;
        }
        if chord_heading_and_length(start.world, end.world).is_none() {
            continue;
        }
        chords.push(TdbChord {
            node_id: start.node_id,
            section_index: start.section_index,
            shape_idx: start.shape_idx,
            start_world: start.world,
            end_world: end.world,
        });
    }
    chords
}

fn vector_oriented_anchors(
    node: &TrackDbNode,
    entry_pin: usize,
    near_hint: Option<Vec3>,
    tsection: Option<&TSectionCatalog>,
) -> Vec<AnchorPoint> {
    let TrackNodeKind::Vector {
        sections,
        geometry,
        length_m: node_length_m,
        ..
    } = &node.kind
    else {
        return Vec::new();
    };

    let sections: Vec<_> = sections
        .iter()
        .copied()
        .filter(|s| s.shape_idx != 0)
        .collect();
    if sections.is_empty() {
        return Vec::new();
    }

    let forward: Vec<(usize, TrVectorSectionRecord)> = sections.into_iter().enumerate().collect();
    let ordered: Vec<(usize, TrVectorSectionRecord)> = if entry_pin == 0 {
        forward
    } else {
        forward.into_iter().rev().collect()
    };

    let mut out: Vec<AnchorPoint> = Vec::new();
    let mut chain_hint = near_hint;
    for (idx, section) in &ordered {
        let world = section_world_vec3(*section, chain_hint);
        out.push(AnchorPoint {
            world,
            node_id: node.id,
            section_index: *idx,
            shape_idx: section.shape_idx,
        });
        chain_hint = Some(world);
    }

    if let Some((idx, section)) = ordered.last().copied() {
        let section_count = ordered.len();
        if let Some(exit) = single_section_end_world(
            section,
            *geometry,
            *node_length_m,
            entry_pin != 0,
            chain_hint,
            tsection,
            section_count,
        ) {
            if out
                .last()
                .is_none_or(|last| last.world.distance(exit) > 0.5)
            {
                out.push(AnchorPoint {
                    world: exit,
                    node_id: node.id,
                    section_index: idx,
                    shape_idx: section.shape_idx,
                });
            }
        }
    }
    out
}

fn single_section_end_world(
    section: TrVectorSectionRecord,
    geometry: Option<TrackVectorGeometry>,
    node_length_m: f64,
    reversed: bool,
    near_hint: Option<Vec3>,
    tsection: Option<&TSectionCatalog>,
    section_count: usize,
) -> Option<Vec3> {
    let start = section_world_vec3(section, near_hint);
    if let Some(geom) = geometry {
        let header = (section.header_tile_x, section.header_tile_z);
        let end_pt = point_world_vec3(geom.end, header, near_hint);
        if chord_heading_and_length(start, end_pt).is_some() {
            return Some(end_pt);
        }
    }
    let heading = section.heading_deg()?;
    let len = section_shape_length_m(tsection, section.shape_idx, node_length_m, section_count);
    let h = if reversed { heading + 180.0 } else { heading };
    Some(end_from_heading(start, h, len))
}

/// Spawn merged procedural track along the `.tdb` vector graph (`--track-dev`).
fn collect_tdb_chords_per_vector(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
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
        let sections: Vec<_> = sections
            .iter()
            .copied()
            .filter(|s| s.shape_idx != 0)
            .collect();
        if sections.is_empty() {
            continue;
        }
        out.extend(collect_vector_section_chords(
            node,
            &sections,
            *geometry,
            *node_length_m,
            focus,
            radius_m,
            tsection,
            tdb,
        ));
    }
    out
}

/// One chord per `TrVectorSection` (start → section span end).
#[allow(clippy::too_many_arguments)]
fn collect_vector_section_chords(
    node: &TrackDbNode,
    sections: &[TrVectorSectionRecord],
    geometry: Option<TrackVectorGeometry>,
    node_length_m: f64,
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
    tdb: &TrackDbFile,
) -> Vec<TdbChord> {
    let node_id = node.id;
    let mut chain_hint = Some(vector_chain_seed(node, sections, tdb, focus));
    let mut out = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        let start = section_world_vec3(*section, chain_hint);
        let end = if section_index + 1 < sections.len() {
            section_world_vec3(sections[section_index + 1], Some(start))
        } else if let Some(exit) = single_section_end_world(
            *section,
            geometry,
            node_length_m,
            false,
            Some(start),
            tsection,
            sections.len(),
        ) {
            exit
        } else {
            continue;
        };
        chain_hint = Some(end);
        if focus.horizontal_distance(start) > radius_m && focus.horizontal_distance(end) > radius_m
        {
            continue;
        }
        if chord_heading_and_length(start, end).is_none() {
            continue;
        }
        out.push(TdbChord {
            node_id,
            section_index,
            shape_idx: section.shape_idx,
            start_world: start,
            end_world: end,
        });
    }
    out
}

/// Best hint to rebase the first section anchor: route focus or a connected junction/end.
fn vector_chain_seed(
    node: &TrackDbNode,
    sections: &[TrVectorSectionRecord],
    tdb: &TrackDbFile,
    focus: &RouteFocus,
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
    _focus: &RouteFocus,
    _radius_m: f32,
    per_vector: &[TdbChord],
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
            let Some((side_a, anchor_a, side_b)) = facing_junction_endpoints(a, b, &nodes_by_id)
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
                shape_idx: anchor_a.shape_idx,
                start_world: side_a,
                end_world: side_b,
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
) -> Option<(Vec3, AnchorPoint, Vec3)> {
    let node_a = nodes_by_id.get(&a)?;
    let node_b = nodes_by_id.get(&b)?;

    if let Some(pin_a) = node_a.pin_refs.iter().position(|p| p.node_id == b) {
        let pin_b = node_b.pin_refs.iter().position(|p| p.node_id == a)?;
        let hint = direct_link_hint(node_a, pin_a, node_b, pin_b);
        let anchor_a = nearest_oriented_anchor(node_a, pin_a, hint)?;
        let anchor_b = nearest_oriented_anchor(node_b, pin_b, hint)?;
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
        let hint = junction_link_hint(node_a, pin_a_idx, node_b, pin_b_idx, mid)?;
        let junction_point = mid.position?;
        let anchor_a = nearest_junction_face_anchor(node_a, pin_a_idx, junction_point, hint)?;
        let anchor_b = nearest_junction_face_anchor(node_b, pin_b_idx, junction_point, hint)?;
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
) -> Option<Vec3> {
    if let Some(j) = node_world_position(junction) {
        return Some(j);
    }
    let a0 = vector_oriented_anchors(node_a, pin_a, None, None)
        .into_iter()
        .next()
        .map(|a| a.world);
    let b0 = vector_oriented_anchors(node_b, pin_b, None, None)
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
) -> Vec3 {
    let mut pts = Vec::new();
    if let Some(a) = vector_oriented_anchors(node_a, pin_a, None, None)
        .into_iter()
        .next()
    {
        pts.push(a.world);
    }
    if let Some(b) = vector_oriented_anchors(node_b, pin_b, None, None)
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

fn nearest_oriented_anchor(
    node: &TrackDbNode,
    entry_pin: usize,
    near: Vec3,
) -> Option<AnchorPoint> {
    vector_oriented_anchors(node, entry_pin, Some(near), None)
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
) -> Option<AnchorPoint> {
    let TrackNodeKind::Vector { sections, .. } = &node.kind else {
        return nearest_oriented_anchor(node, entry_pin, hint);
    };
    let fallback_dist = if sections.len() <= 2 {
        SHORT_VECTOR_JUNCTION_FACE_FALLBACK_DIST_M
    } else {
        JUNCTION_FACE_FALLBACK_DIST_M
    };
    let (jx, _, jz) = junction_point.bevy_position();
    let near_hint = Some(Vec3::new(jx, hint.y, jz));
    let ref_tile = Some(junction_point);
    let mut best: Option<AnchorPoint> = None;
    let mut best_dist = f32::INFINITY;
    for anchor in vector_oriented_anchors(node, entry_pin, near_hint, None) {
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
            let dist = world.distance(hint);
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
        // Some single-section MSTS vectors have only far section anchors; use junction face.
        return Some(AnchorPoint {
            world: hint,
            node_id: anchor.node_id,
            section_index: anchor.section_index,
            shape_idx: anchor.shape_idx,
        });
    }
    nearest_oriented_anchor(node, entry_pin, hint)
}

/// Build procedural segments for vector nodes within `radius_m` of `focus`.
pub fn tdb_procedural_segments_near(
    tdb: &TrackDbFile,
    tsection: &TSectionCatalog,
    scene: &TrackScene,
    focus: &RouteFocus,
    radius_m: f32,
) -> Vec<ProceduralTrackSegment> {
    collect_tdb_chords(tdb, focus, radius_m, Some(tsection))
        .into_iter()
        .filter_map(|chord| chord_to_segment(chord, tsection, scene, focus))
        .collect()
}

fn chord_to_segment(
    chord: TdbChord,
    tsection: &TSectionCatalog,
    scene: &TrackScene,
    focus: &RouteFocus,
) -> Option<ProceduralTrackSegment> {
    let (heading_deg, length_m) = chord_heading_and_length(chord.start_world, chord.end_world)?;
    // Bevy/MSTS world (matches `RouteFocus::center` from world anchor). Same space as the train.
    let mut world = chord.start_world;
    world.y = crate::terrain::ground_y_at(None, world.x, world.z, scene);
    let render_pos = focus.to_render_surface(world);
    let rotation = Quat::from_rotation_y(heading_deg.to_radians() as f32);
    let link = tsection
        .procedural_links_primary_path(chord.shape_idx)
        .into_iter()
        .next();
    Some(straight_segment_from_tsection_link(
        render_pos,
        rotation,
        length_m,
        link.as_ref(),
    ))
}

/// Spawn merged procedural track along the `.tdb` vector graph (`--track-dev`).
#[allow(clippy::too_many_arguments)]
pub fn spawn_tdb_graph_track(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    offset: Res<RouteWorldOffset>,
    scene: Res<TrackScene>,
    mode: Res<ViewerSceneryMode>,
    corridor: Res<RunCorridorPath>,
) {
    if !mode.draws_tdb_track() {
        return;
    }
    if mode.is_track_dev() && !track_dev_render_enabled() {
        return;
    }
    let Some(tdb) = assets.track_db() else {
        viewer_log!("openrailsrs-viewer3d: tdb-graph — no .tdb loaded");
        return;
    };
    let radius_m = tdb_radius_for_mode(*mode);
    viewer_log!(
        "openrailsrs-viewer3d: tdb-graph — collecting chords within {:.0}m ({:?})…",
        radius_m,
        *mode
    );
    let mut chords = collect_tdb_chords(tdb, &focus, radius_m, Some(assets.tsection()));
    if mode.is_run_corridor() && corridor.active() {
        let before = chords.len();
        chords.retain(|chord| corridor.contains_segment(chord.start_world, chord.end_world));
        viewer_log!(
            "openrailsrs-viewer3d: run_corridor — corridor filter {} → {} chord(s), width {:.0}m",
            before,
            chords.len(),
            corridor.half_width_m * 2.0
        );
    }
    viewer_log!(
        "openrailsrs-viewer3d: tdb-graph — {} chord(s), running audit…",
        chords.len()
    );
    let audit_route_dir = mode.is_track_dev().then_some(assets.route_dir.as_path());
    run_track_dev_audit(
        tdb,
        &scene,
        &focus,
        *offset,
        radius_m,
        &chords,
        audit_route_dir,
    );
    let mut segments: Vec<_> = chords
        .into_iter()
        .filter_map(|chord| chord_to_segment(chord, assets.tsection(), &scene, &focus))
        .collect();
    if segments.is_empty() {
        viewer_log!(
            "openrailsrs-viewer3d: tdb-graph — no vector sections within {:.0}m",
            radius_m
        );
        return;
    }
    let total = segments.len();
    if total > TRACK_DEV_MAX_SEGMENTS {
        viewer_log!(
            "openrailsrs-viewer3d: tdb-graph — capping segments {total} → {TRACK_DEV_MAX_SEGMENTS} (raise OPENRAILSRS_TRACK_DEV_RADIUS_M or TRACK_DEV_MAX_SEGMENTS to debug)"
        );
        segments.truncate(TRACK_DEV_MAX_SEGMENTS);
    }
    viewer_log!(
        "openrailsrs-viewer3d: tdb-graph — {} tramo(s) encadenados .tdb vía TrPins within {:.0}m (2 rieles c/u, sin durmientes)",
        segments.len(),
        radius_m
    );
    spawn_procedural_track_batch(
        &mut commands,
        &mut meshes,
        &mut materials,
        &segments,
        "tdb-graph",
        ProceduralTrackStyle::RailsOnly,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::TRACK_DEV_BRANCH_WALK_MAX_NODES;
    use openrailsrs_formats::{TrackDbFile, TrackNodeKind, TrackVectorPoint};
    use std::path::PathBuf;

    fn fixtures_tdb(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../openrailsrs-msts/tests/fixtures/{name}"))
    }

    fn point_at(x: f64, z: f64) -> TrackVectorPoint {
        TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 0.0,
            z,
        }
    }

    fn section_at(x: f64, z: f64, shape_idx: u32) -> TrVectorSectionRecord {
        let start = point_at(x, z);
        TrVectorSectionRecord {
            shape_idx,
            aux_shape_idx: 0,
            header_tile_x: start.tile_x,
            header_tile_z: start.tile_z,
            start,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
        }
    }

    #[test]
    fn native_tdb_single_section_yields_one_segment() {
        let tdb = TrackDbFile::from_path(fixtures_tdb("native_msts.tdb")).expect("tdb");
        let vector = tdb
            .nodes
            .iter()
            .find(|n| matches!(n.kind, TrackNodeKind::Vector { .. }))
            .expect("vector");
        let TrackNodeKind::Vector { sections, .. } = &vector.kind else {
            panic!("vector");
        };
        let world = section_world_vec3(sections[0], None);
        let focus = RouteFocus {
            center: world,
            height_origin: world.y,
        };
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let segments =
            tdb_procedural_segments_near(&tdb, &TSectionCatalog::default(), &scene, &focus, 500.0);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn chained_sections_use_per_chord_heading_not_global_geometry() {
        let s0 = section_at(0.0, 0.0, 1);
        let s1 = section_at(100.0, 0.0, 1);
        let s2 = section_at(100.0, 100.0, 1);
        let node = openrailsrs_formats::TrackDbNode {
            id: 1,
            position: None,
            pin_refs: Vec::new(),
            kind: TrackNodeKind::Vector {
                length_m: 200.0,
                speed_limit_mps: 0.0,
                pins: (0, 0),
                item_ids: Vec::new(),
                sections: vec![s0, s1, s2],
                geometry: Some(TrackVectorGeometry {
                    start: s0.start,
                    end: s2.start,
                }),
            },
        };
        let tdb = TrackDbFile {
            nodes: vec![node],
            items: Vec::new(),
        };
        let focus = RouteFocus {
            center: Vec3::new(50.0, 0.0, 50.0),
            height_origin: 0.0,
        };
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let segments =
            tdb_procedural_segments_near(&tdb, &TSectionCatalog::default(), &scene, &focus, 500.0);
        assert_eq!(segments.len(), 2, "N sections => N-1 chained segments");
        let first_yaw = segments[0].rotation.to_euler(EulerRot::YXZ).0.to_degrees();
        let second_yaw = segments[1].rotation.to_euler(EulerRot::YXZ).0.to_degrees();
        assert!(
            (first_yaw - second_yaw).abs() > 45.0,
            "chords must turn: first={first_yaw} second={second_yaw}"
        );
        assert!((segments[0].length_m.unwrap() - 100.0).abs() < 1.0);
        assert!((segments[1].length_m.unwrap() - 100.0).abs() < 1.0);
    }

    #[test]
    fn chained_rebase_keeps_consecutive_chord_endpoints_aligned() {
        let s0 = section_at(0.0, 0.0, 1);
        let s1 = section_at(100.0, 0.0, 2);
        let s2 = section_at(200.0, 0.0, 3);
        let tdb = TrackDbFile {
            nodes: vec![TrackDbNode {
                id: 2,
                position: None,
                pin_refs: Vec::new(),
                kind: TrackNodeKind::Vector {
                    length_m: 200.0,
                    speed_limit_mps: 0.0,
                    pins: (1, 3),
                    item_ids: Vec::new(),
                    sections: vec![s0, s1, s2],
                    geometry: None,
                },
            }],
            items: Vec::new(),
        };
        let focus = RouteFocus {
            center: Vec3::new(100.0, 0.0, 0.0),
            height_origin: 0.0,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 500.0, None);
        assert_eq!(
            chords.len(),
            2,
            "last section has no geometry end — two chords"
        );
        for pair in chords.windows(2) {
            let gap = pair[0].end_world.distance(pair[1].start_world);
            assert!(
                gap < 0.01,
                "chained rebase should align endpoints: gap={gap}m"
            );
        }
    }

    #[test]
    fn branch_walk_links_vectors_through_junction() {
        let tdb = TrackDbFile::from_path(fixtures_tdb("native_msts.tdb")).expect("tdb");
        let vector = tdb.node_by_id(2).expect("vector 2");
        let TrackNodeKind::Vector { sections, .. } = &vector.kind else {
            panic!("vector");
        };
        let world = section_world_vec3(sections[0], None);
        let focus = RouteFocus {
            center: world,
            height_origin: world.y,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 500.0, None);
        assert!(
            !chords.is_empty(),
            "branch walk should emit at least one chord for native fixture"
        );
        assert!(
            tdb.pins_connect(2, 3),
            "vector 2 should connect to junction 3 via TrPins"
        );
    }

    #[test]
    fn large_route_uses_junction_bridges_not_global_branch_walk() {
        let mut tdb = TrackDbFile::from_path(fixtures_tdb("native_msts.tdb")).expect("tdb");
        let vector = tdb.node_by_id(2).expect("vector 2");
        let TrackNodeKind::Vector { sections, .. } = &vector.kind else {
            panic!("vector");
        };
        let world = section_world_vec3(sections[0], None);
        let focus = RouteFocus {
            center: world,
            height_origin: world.y,
        };
        for i in 0..801 {
            tdb.nodes.push(TrackDbNode {
                id: 10_000 + i as u32,
                position: None,
                pin_refs: Vec::new(),
                kind: TrackNodeKind::End,
            });
        }
        assert!(tdb.nodes.len() > TRACK_DEV_BRANCH_WALK_MAX_NODES);
        let chords = collect_tdb_chords(&tdb, &focus, 500.0, None);
        assert!(
            !chords.is_empty(),
            "large-route path should emit chords near focus"
        );
    }

    /// Requires Chiltern `.tdb` in `OPENRAILSRS_MSTS_CONTENT`.
    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_j16831_vector_pair_gap() {
        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let j = tdb.node_by_id(16831).expect("junction");
        let v_a = tdb.node_by_id(16839).expect("vector a");
        let v_b = tdb.node_by_id(16931).expect("vector b");
        let j_pos = j.position.expect("junction UiD");
        let (jx, jy, jz) = j_pos.bevy_position();
        let hint = Vec3::new(jx, jy, jz);
        let pin_a = v_a
            .pin_refs
            .iter()
            .position(|p| p.node_id == 16831)
            .expect("pin a");
        let pin_b = v_b
            .pin_refs
            .iter()
            .position(|p| p.node_id == 16831)
            .expect("pin b");
        let near_hint = Some(hint);
        let anchors_a = vector_oriented_anchors(v_a, pin_a, near_hint, None);
        let anchors_b = vector_oriented_anchors(v_b, pin_b, near_hint, None);
        for (i, a) in anchors_a.iter().enumerate() {
            eprintln!(
                "V16839 anchor {i}: ({:.1},{:.1},{:.1}) dist_j={:.1}m",
                a.world.x,
                a.world.y,
                a.world.z,
                a.world.distance(hint)
            );
        }
        for (i, a) in anchors_b.iter().enumerate() {
            eprintln!(
                "V16931 anchor {i}: ({:.1},{:.1},{:.1}) dist_j={:.1}m",
                a.world.x,
                a.world.y,
                a.world.z,
                a.world.distance(hint)
            );
        }
        let near_a = nearest_oriented_anchor(v_a, pin_a, hint).expect("near a");
        let near_b = nearest_oriented_anchor(v_b, pin_b, hint).expect("near b");
        eprintln!(
            "nearest faces: A=({:.1},{:.1},{:.1}) B=({:.1},{:.1},{:.1}) gap={:.1}m",
            near_a.world.x,
            near_a.world.y,
            near_a.world.z,
            near_b.world.x,
            near_b.world.y,
            near_b.world.z,
            near_a.world.distance(near_b.world)
        );
        let focus = RouteFocus {
            center: hint,
            height_origin: hint.y,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 1500.0, None);
        let ids: std::collections::HashSet<_> = chords.iter().map(|c| c.node_id).collect();
        eprintln!(
            "chords near J: total={} has16839={} has16931={}",
            chords.len(),
            ids.contains(&16839),
            ids.contains(&16931)
        );
        eprintln!(
            "junction gap: {:.1}m",
            inter_node_junction_gap_m(&tdb, 16839, 16931).unwrap_or(-1.0)
        );
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_j16660_tile_boundary_pair() {
        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let gap = inter_node_junction_gap_m(&tdb, 16655, 16683).expect("gap");
        eprintln!("V16655↔V16683 via J16660 junction face gap: {gap:.1}m");
        assert!(
            gap < 15.0,
            "tile-boundary pair should be a few metres, not ~4072m: {gap}m"
        );
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_j17530_tile_x_boundary_pair() {
        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let gap = inter_node_junction_gap_m(&tdb, 17401, 17835).expect("gap");
        eprintln!("V17401↔V17835 via J17530 junction face gap: {gap:.1}m");
        assert!(
            gap < 20.0,
            "tile-x boundary pair should be ~14m, not ~2018m: {gap}m"
        );
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_j16102_single_section_far_anchor_pair() {
        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let gap = inter_node_junction_gap_m(&tdb, 16099, 17902).expect("gap");
        eprintln!("V16099↔V17902 via J16102 junction face gap: {gap:.1}m");
        assert!(
            gap < 20.0,
            "single-section far-anchor pair should snap to junction face: {gap}m"
        );
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_j17705_pair_gap() {
        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let j = tdb.node_by_id(17705).expect("junction");
        let va = tdb.node_by_id(17522).expect("vector a");
        let vb = tdb.node_by_id(17703).expect("vector b");
        let jp = j.position.expect("junction UiD");
        let (jx, jy, jz) = jp.bevy_position();
        let jv = Vec3::new(jx, jy, jz);
        let pa = va
            .pin_refs
            .iter()
            .position(|p| p.node_id == 17705)
            .expect("pin a");
        let pb = vb
            .pin_refs
            .iter()
            .position(|p| p.node_id == 17705)
            .expect("pin b");
        let near_hint = Some(jv);
        for (i, a) in vector_oriented_anchors(va, pa, near_hint, None)
            .iter()
            .enumerate()
        {
            eprintln!(
                "V17522 anchor {i}: ({:.1},{:.1},{:.1}) dist_j={:.1}m",
                a.world.x,
                a.world.y,
                a.world.z,
                a.world.distance(jv)
            );
        }
        for (i, a) in vector_oriented_anchors(vb, pb, near_hint, None)
            .iter()
            .enumerate()
        {
            eprintln!(
                "V17703 anchor {i}: ({:.1},{:.1},{:.1}) dist_j={:.1}m",
                a.world.x,
                a.world.y,
                a.world.z,
                a.world.distance(jv)
            );
        }
        let gap = inter_node_junction_gap_m(&tdb, 17522, 17703).expect("gap");
        eprintln!("V17522↔V17703 via J17705 junction face gap: {gap:.1}m");
        assert!(
            gap < 20.0,
            "junction-face fallback should clamp this outlier near switch: {gap}m"
        );
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_v16357_trackobj_gap() {
        use crate::track::point_segment_distance_xz;

        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let tsection = TSectionCatalog::load_for_route(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern"),
        )
        .expect("tsection");
        // TrackObj world pos from audit (already bevy global)
        let trackobj = Vec3::new(12451561.0, 28.5422, 30565716.0);
        let focus = RouteFocus {
            center: trackobj,
            height_origin: trackobj.y,
        };
        let v = tdb.node_by_id(16357).expect("V16357");
        let TrackNodeKind::Vector {
            sections,
            geometry: _,
            length_m,
            ..
        } = &v.kind
        else {
            panic!("not vector");
        };
        eprintln!(
            "V16357: {} section(s), node_length={length_m:.1}m",
            sections.len()
        );
        for (i, s) in sections.iter().enumerate() {
            let (x, _y, z) = s.bevy_position_nearest_to(
                trackobj.x,
                trackobj.z,
                Some((s.header_tile_x, s.header_tile_z)),
            );
            let dist = (trackobj.x - x).hypot(trackobj.z - z);
            let len = tsection
                .procedural_dims(s.shape_idx)
                .map(|d| d.length_m)
                .unwrap_or(0.0);
            eprintln!(
                "  sec[{i}] shape={} dist_obj={dist:.1}m tsection_len={len:.1}m heading={:?}",
                s.shape_idx,
                s.heading_deg()
            );
        }
        let chords = collect_tdb_chords(&tdb, &focus, 500.0, Some(&tsection));
        let v_chords: Vec<_> = chords.iter().filter(|c| c.node_id == 16357).collect();
        eprintln!("chords on V16357: {}", v_chords.len());
        for c in &v_chords {
            let d = point_segment_distance_xz(
                trackobj.x,
                trackobj.z,
                c.start_world.x,
                c.start_world.z,
                c.end_world.x,
                c.end_world.z,
            );
            eprintln!(
                "  chord sec={} shape={} len={:.1}m dist_obj={d:.1}m",
                c.section_index,
                c.shape_idx,
                c.start_world.distance(c.end_world)
            );
        }
        let best = chords
            .iter()
            .map(|c| {
                (
                    point_segment_distance_xz(
                        trackobj.x,
                        trackobj.z,
                        c.start_world.x,
                        c.start_world.z,
                        c.end_world.x,
                        c.end_world.z,
                    ),
                    c.node_id,
                )
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        eprintln!("best chord: {best:?}");
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn diagnose_j17635_short_vector_pair_gap() {
        let tdb_path = std::path::PathBuf::from(
            std::env::var("OPENRAILSRS_MSTS_CONTENT").expect("OPENRAILSRS_MSTS_CONTENT")
                + "/Chiltern/ROUTES/Chiltern/Chiltern.tdb",
        );
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let gap = inter_node_junction_gap_m(&tdb, 17386, 17634).expect("gap");
        eprintln!("V17386↔V17634 via J17635 junction face gap: {gap:.1}m");
        assert!(
            gap < 20.0,
            "short-vector fallback should clamp this outlier near switch: {gap}m"
        );
    }
}
