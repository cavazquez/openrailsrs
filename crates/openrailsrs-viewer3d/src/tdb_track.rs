//! Continuous procedural track from the MSTS `.tdb` vector graph (Phase 3).
//!
//! Branches are walked via `TrPins` (end → vector → junction → …).  Each consecutive
//! pair of section anchors along a branch becomes one rail pair.  Drawn without
//! sleepers in `--track-dev`.

use bevy::prelude::*;
use openrailsrs_formats::{
    TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind,
    TrackProceduralLink, TrackVectorGeometry,
};
use std::collections::HashSet;

use crate::dyntrack::{
    MSTS_DEFAULT_SECTION_LENGTH_M, MSTS_STANDARD_HALF_GAUGE_M, ProceduralTrackSegment,
    ProceduralTrackStyle, spawn_procedural_track_batch,
};
use crate::launch::{
    TRACK_DEV_MAX_BRANCHES, TRACK_DEV_MAX_SEGMENTS, ViewerSceneryMode, track_dev_render_enabled,
    track_dev_tdb_radius_m,
};
use crate::shapes::RouteAssets;
use crate::track::TrackScene;
use crate::track_audit::run_track_dev_audit;
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};

/// One straight chord between consecutive vector-section anchors (Bevy/MSTS world).
#[derive(Clone, Copy, Debug)]
pub struct TdbChord {
    pub node_id: u32,
    pub section_index: usize,
    pub shape_idx: u32,
    pub start_world: Vec3,
    pub end_world: Vec3,
}

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

/// Collect `.tdb` chords within `radius_m` of `focus` (for rendering and audit).
pub fn collect_tdb_chords(tdb: &TrackDbFile, focus: &RouteFocus, radius_m: f32) -> Vec<TdbChord> {
    if tdb.nodes.len() <= crate::launch::TRACK_DEV_BRANCH_WALK_MAX_NODES {
        let branch_chords = collect_tdb_chords_from_branches(tdb, focus, radius_m);
        if !branch_chords.is_empty() {
            return branch_chords;
        }
    }
    collect_tdb_chords_per_vector(tdb, focus, radius_m)
}

/// Walk `TrPins` from end nodes near `focus` and emit chords along continuous branches.
fn collect_tdb_chords_from_branches(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
) -> Vec<TdbChord> {
    let mut branches = Vec::new();
    for node in &tdb.nodes {
        if !matches!(node.kind, TrackNodeKind::End) {
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
            .flat_map(|branch| branch_to_chords(tdb, &branch, focus, radius_m))
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
        TrackNodeKind::Vector { sections, .. } => sections
            .iter()
            .filter(|s| s.shape_idx != 0)
            .any(|s| focus.horizontal_distance(section_world_vec3(*s)) <= radius_m),
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
) -> Vec<TdbChord> {
    let mut points: Vec<AnchorPoint> = Vec::new();
    for step in branch {
        let Some(node) = tdb.node_by_id(step.vector_id) else {
            continue;
        };
        for anchor in vector_oriented_anchors(node, step.entry_pin) {
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

fn vector_oriented_anchors(node: &TrackDbNode, entry_pin: usize) -> Vec<AnchorPoint> {
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

    let mut out: Vec<AnchorPoint> = ordered
        .iter()
        .map(|(idx, section)| AnchorPoint {
            world: section_world_vec3(*section),
            node_id: node.id,
            section_index: *idx,
            shape_idx: section.shape_idx,
        })
        .collect();

    if ordered.len() == 1 {
        if let Some(end) =
            single_section_end_world(ordered[0].1, *geometry, *node_length_m, entry_pin != 0)
        {
            if out[0].world.distance(end) > 0.5 {
                out.push(AnchorPoint {
                    world: end,
                    node_id: node.id,
                    section_index: ordered[0].0,
                    shape_idx: ordered[0].1.shape_idx,
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
) -> Option<Vec3> {
    let start = section_world_vec3(section);
    if let Some(geom) = geometry {
        let end_pt = point_world_vec3(geom.end);
        if chord_heading_and_length(start, end_pt).is_some() {
            return Some(end_pt);
        }
    }
    let heading = section.heading_deg()?;
    let len = single_section_length(node_length_m, section.shape_idx);
    let h = if reversed { heading + 180.0 } else { heading };
    Some(end_from_heading(start, h, len))
}

/// Per-vector chord collection (fallback when branch walk yields nothing).
fn collect_tdb_chords_per_vector(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
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

        if sections.len() == 1 {
            if let Some(chord) = single_section_chord(
                node.id,
                sections[0],
                *geometry,
                *node_length_m,
                focus,
                radius_m,
            ) {
                out.push(chord);
            }
            continue;
        }

        for (section_index, section) in sections.iter().enumerate() {
            if section_index + 1 >= sections.len() {
                break;
            }
            let start = section_world_vec3(*section);
            let end = section_world_vec3(sections[section_index + 1]);
            if focus.horizontal_distance(start) > radius_m
                && focus.horizontal_distance(end) > radius_m
            {
                continue;
            }
            if chord_heading_and_length(start, end).is_none() {
                continue;
            }
            out.push(TdbChord {
                node_id: node.id,
                section_index,
                shape_idx: section.shape_idx,
                start_world: start,
                end_world: end,
            });
        }
    }
    out
}

/// Build procedural segments for vector nodes within `radius_m` of `focus`.
pub fn tdb_procedural_segments_near(
    tdb: &TrackDbFile,
    tsection: &TSectionCatalog,
    scene: &TrackScene,
    focus: &RouteFocus,
    radius_m: f32,
) -> Vec<ProceduralTrackSegment> {
    collect_tdb_chords(tdb, focus, radius_m)
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

fn single_section_chord(
    node_id: u32,
    section: TrVectorSectionRecord,
    geometry: Option<TrackVectorGeometry>,
    node_length_m: f64,
    focus: &RouteFocus,
    radius_m: f32,
) -> Option<TdbChord> {
    let start = section_world_vec3(section);
    if focus.horizontal_distance(start) > radius_m {
        return None;
    }
    let end = if let Some(geom) = geometry {
        let end_pt = point_world_vec3(geom.end);
        if chord_heading_and_length(start, end_pt).is_some() {
            end_pt
        } else if let Some(h) = section.heading_deg() {
            let len = single_section_length(node_length_m, section.shape_idx);
            end_from_heading(start, h, len)
        } else {
            return None;
        }
    } else if let Some(h) = section.heading_deg() {
        let len = single_section_length(node_length_m, section.shape_idx);
        end_from_heading(start, h, len)
    } else {
        return None;
    };
    Some(TdbChord {
        node_id,
        section_index: 0,
        shape_idx: section.shape_idx,
        start_world: start,
        end_world: end,
    })
}

fn end_from_heading(start: Vec3, heading_deg: f64, length_m: f32) -> Vec3 {
    let yaw = heading_deg.to_radians() as f32;
    start + Vec3::new(yaw.sin() * length_m, 0.0, yaw.cos() * length_m)
}

fn single_section_length(node_length_m: f64, _shape_idx: u32) -> f32 {
    if node_length_m > 0.5 {
        return node_length_m as f32;
    }
    MSTS_DEFAULT_SECTION_LENGTH_M
}

fn straight_segment_from_tsection_link(
    position: Vec3,
    rotation: Quat,
    length_m: f32,
    link: Option<&TrackProceduralLink>,
) -> ProceduralTrackSegment {
    let half_gauge = link
        .map(|l| l.dims.half_gauge_m as f32)
        .or(Some(MSTS_STANDARD_HALF_GAUGE_M));

    ProceduralTrackSegment {
        position,
        rotation,
        length_m: Some(length_m),
        half_gauge_m: half_gauge,
        curve_radius_m: None,
        curve_angle_deg: None,
    }
}

fn section_world_vec3(section: TrVectorSectionRecord) -> Vec3 {
    point_world_vec3(section.start)
}

fn point_world_vec3(point: openrailsrs_formats::TrackVectorPoint) -> Vec3 {
    // Must match `world_anchor_position` / `.w` / train focus — not `graph_z` (+Z import).
    let (x, y, z) = point.bevy_position();
    Vec3::new(x, y, z)
}

fn chord_heading_and_length(from: Vec3, to: Vec3) -> Option<(f64, f32)> {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    let len = (dx * dx + dz * dz).sqrt();
    if len < 0.5 {
        return None;
    }
    Some((f64::from(dx).atan2(f64::from(dz)).to_degrees(), len))
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
) {
    if !mode.is_track_dev() {
        return;
    }
    if !track_dev_render_enabled() {
        return;
    }
    let Some(tdb) = assets.track_db() else {
        viewer_log!("openrailsrs-viewer3d: tdb-graph — no .tdb loaded");
        return;
    };
    let radius_m = track_dev_tdb_radius_m();
    viewer_log!(
        "openrailsrs-viewer3d: tdb-graph — collecting chords within {:.0}m…",
        radius_m
    );
    let chords = collect_tdb_chords(tdb, &focus, radius_m);
    viewer_log!(
        "openrailsrs-viewer3d: tdb-graph — {} chord(s), running audit…",
        chords.len()
    );
    run_track_dev_audit(tdb, &scene, &focus, *offset, radius_m, &chords);
    let mut segments =
        tdb_procedural_segments_near(tdb, assets.tsection(), &scene, &focus, radius_m);
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
        &scene.bounds,
        "tdb-graph",
        ProceduralTrackStyle::RailsOnly,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
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
        TrVectorSectionRecord {
            shape_idx,
            aux_shape_idx: 0,
            start: point_at(x, z),
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
        let world = section_world_vec3(sections[0]);
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
    fn branch_walk_links_vectors_through_junction() {
        let tdb = TrackDbFile::from_path(fixtures_tdb("native_msts.tdb")).expect("tdb");
        let vector = tdb.node_by_id(2).expect("vector 2");
        let TrackNodeKind::Vector { sections, .. } = &vector.kind else {
            panic!("vector");
        };
        let world = section_world_vec3(sections[0]);
        let focus = RouteFocus {
            center: world,
            height_origin: world.y,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 500.0);
        assert!(
            !chords.is_empty(),
            "branch walk should emit at least one chord for native fixture"
        );
        assert!(
            tdb.pins_connect(2, 3),
            "vector 2 should connect to junction 3 via TrPins"
        );
    }
}
