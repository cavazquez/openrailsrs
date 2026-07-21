//! Continuous procedural track from the MSTS `.tdb` vector graph (Phase 3).
//!
//! Geometry SSOT: [`openrailsrs_bevy_scenery::spawn::tdb_track`].
//! This module adapts [`RouteFocus`] / floating origin / stream / audits.

use std::collections::HashSet;

use bevy::prelude::*;
use openrailsrs_formats::{TSectionCatalog, TrackDbFile};

use crate::floating_origin::{FloatingOrigin, view_translation};
use crate::launch::{
    RunCorridorPath, TRACK_DEV_BRANCH_WALK_MAX_NODES, TRACK_DEV_MAX_BRANCHES,
    TRACK_DEV_MAX_SEGMENTS, ViewerSceneryMode, run_corridor_ahead_m, run_corridor_behind_m,
    tdb_radius_for_mode, tdb_stream_radius_m, track_dev_render_enabled,
};
use crate::live::LiveDrive;
use crate::shapes::RouteAssets;
use crate::track::TrackScene;
use crate::track_audit::run_track_dev_audit;
use crate::view_window::ViewWindow;
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};
use openrailsrs_bevy_scenery::spawn::dyntrack::ProceduralTrackSegment;
use openrailsrs_bevy_scenery::spawn::dyntrack::{
    ProceduralTrackStyle, arc_local_frame, spawn_procedural_track_batch,
    spawn_procedural_track_single,
};
use openrailsrs_bevy_scenery::spawn::tdb_track::{
    ChordCollectLimits, FocusQuery, collect_tdb_chords as collect_tdb_chords_ssot,
    collect_tdb_path_segments as collect_tdb_path_segments_ssot,
};
pub use openrailsrs_bevy_scenery::spawn::tdb_track::{
    TDB_JUNCTION_BRIDGE_SECTION, TdbChord, inter_node_junction_gap_m, nearest_oriented_anchor,
    section_world_vec3, vector_oriented_anchors,
};

fn viewer_collect_limits() -> ChordCollectLimits {
    ChordCollectLimits {
        branch_walk_max_nodes: TRACK_DEV_BRANCH_WALK_MAX_NODES,
        max_branches: TRACK_DEV_MAX_BRANCHES,
    }
}

fn focus_query(focus: &RouteFocus, radius_m: f32) -> FocusQuery {
    FocusQuery::new(focus.center, radius_m)
}

pub fn collect_tdb_chords(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
) -> Vec<TdbChord> {
    collect_tdb_chords_ssot(
        tdb,
        &focus_query(focus, radius_m),
        tsection,
        viewer_collect_limits(),
    )
}

/// Full TSection path segments for `--track-dev` / `--run-corridor` mesh (arcs + multi-link shapes).
pub fn collect_tdb_path_segments(
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
) -> Vec<ProceduralTrackSegment> {
    collect_tdb_path_segments_ssot(
        tdb,
        &focus_query(focus, radius_m),
        tsection,
        viewer_collect_limits(),
    )
}

/// Build procedural segments for vector nodes within `radius_m` of `focus`.
///
/// Preserves TDB absolute Y via [`RouteFocus::to_render_surface`] (#65). Terrain
/// sampling is only a fallback when the section pose has no finite Y.
pub fn tdb_procedural_segments_near(
    tdb: &TrackDbFile,
    tsection: &TSectionCatalog,
    scene: &TrackScene,
    focus: &RouteFocus,
    radius_m: f32,
) -> Vec<ProceduralTrackSegment> {
    collect_tdb_path_segments(tdb, focus, radius_m, Some(tsection))
        .into_iter()
        .map(|mut seg| {
            let world = preserve_tdb_world_y(seg.position, scene);
            seg.position = focus.to_render_surface(world);
            seg
        })
        .collect()
}

/// Keep TDB elevation; fall back to terrain only when Y is missing/non-finite.
fn preserve_tdb_world_y(mut world: Vec3, scene: &TrackScene) -> Vec3 {
    if !world.y.is_finite() {
        world.y = crate::terrain::ground_y_at(None, world.x, world.z, scene);
    }
    world
}

fn procedural_segment_end_world(seg: &ProceduralTrackSegment) -> Vec3 {
    if let (Some(r), Some(a)) = (seg.curve_radius_m, seg.curve_angle_deg) {
        if r.abs() > 1e-6 && a.abs() > 1e-6 {
            let (local, _) = arc_local_frame(r, a, 1.0);
            return seg.position + seg.rotation * local;
        }
    }
    let len = seg.length_m.unwrap_or(0.0);
    seg.position + seg.rotation * Vec3::new(0.0, 0.0, len)
}

/// MSTS world endpoints for a spawned TDB segment (mobile stream despawn).
#[derive(Component, Clone, Copy, Debug)]
pub struct TdbTrackSegment {
    pub start_msts: Vec3,
    pub end_msts: Vec3,
}

type SegmentKey = (i32, i32, i32, i32, i32, i32);

fn quant_m(v: f32) -> i32 {
    (v * 10.0).round() as i32
}

fn segment_key(start: Vec3, end: Vec3) -> SegmentKey {
    let a = (quant_m(start.x), quant_m(start.y), quant_m(start.z));
    let b = (quant_m(end.x), quant_m(end.y), quant_m(end.z));
    if a <= b {
        (a.0, a.1, a.2, b.0, b.1, b.2)
    } else {
        (b.0, b.1, b.2, a.0, a.1, a.2)
    }
}

fn segment_end_msts(seg: &ProceduralTrackSegment, focus: &RouteFocus) -> Vec3 {
    let render_end = procedural_segment_end_world(seg);
    Vec3::new(
        render_end.x + focus.center.x,
        render_end.y + focus.height_origin,
        render_end.z + focus.center.z,
    )
}

fn segment_start_msts(seg: &ProceduralTrackSegment, focus: &RouteFocus) -> Vec3 {
    Vec3::new(
        seg.position.x + focus.center.x,
        seg.position.y + focus.height_origin,
        seg.position.z + focus.center.z,
    )
}

const TDB_STREAM_CENTER_DELTA_M: f32 = 40.0;
const TDB_STREAM_DESPAWN_HYSTERESIS_M: f32 = 24.0;

#[derive(Resource, Default)]
pub struct TdbTrackStream {
    spawned: HashSet<SegmentKey>,
    last_center: Option<Vec3>,
    frames_since_tick: u32,
    rail_material: Option<Handle<StandardMaterial>>,
    audit_done: bool,
}

pub fn tdb_stream_active(live: Option<Res<LiveDrive>>, mode: Res<ViewerSceneryMode>) -> bool {
    live.is_some() && mode.draws_tdb_track() && (!mode.is_track_dev() || track_dev_render_enabled())
}

pub fn tdb_startup_spawn_active(
    live: Option<Res<LiveDrive>>,
    mode: Res<ViewerSceneryMode>,
) -> bool {
    live.is_none() && mode.draws_tdb_track() && (!mode.is_track_dev() || track_dev_render_enabled())
}

fn segment_passes_corridor_filter(
    corridor: &RunCorridorPath,
    mode: ViewerSceneryMode,
    center: Vec3,
    start_msts: Vec3,
    end_msts: Vec3,
) -> bool {
    if !mode.is_run_corridor() || !corridor.active() {
        return true;
    }
    corridor.contains_segment_near(
        center,
        start_msts,
        end_msts,
        run_corridor_ahead_m(),
        run_corridor_behind_m(),
    )
}

/// Mobile TDB spawn/despawn around [`ViewWindow`] in live mode.
#[allow(clippy::too_many_arguments)]
pub fn tdb_track_stream_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    scene: Res<TrackScene>,
    mode: Res<ViewerSceneryMode>,
    corridor: Res<RunCorridorPath>,
    window: Res<ViewWindow>,
    origin: Res<FloatingOrigin>,
    mut stream: ResMut<TdbTrackStream>,
    segments_query: Query<(Entity, &TdbTrackSegment)>,
) {
    let Some(tdb) = assets.track_db() else {
        return;
    };
    let radius_m = tdb_stream_radius_m(*mode);
    let center = window.center_world;
    stream.frames_since_tick += 1;
    let moved = stream
        .last_center
        .map(|last| {
            Vec2::new(last.x - center.x, last.z - center.z).length() > TDB_STREAM_CENTER_DELTA_M
        })
        .unwrap_or(true);
    if !moved && stream.frames_since_tick % 30 != 0 {
        return;
    }
    stream.last_center = Some(center);
    stream.frames_since_tick = 0;

    let focus_at = window.route_focus_at_center(focus.height_origin);
    let despawn_radius = radius_m + TDB_STREAM_DESPAWN_HYSTERESIS_M;

    for (entity, seg) in segments_query.iter() {
        let d0 = window.horizontal_distance_world(seg.start_msts);
        let d1 = window.horizontal_distance_world(seg.end_msts);
        if d0 > despawn_radius && d1 > despawn_radius {
            let key = segment_key(seg.start_msts, seg.end_msts);
            stream.spawned.remove(&key);
            commands.entity(entity).despawn();
        }
    }

    if std::env::var_os("OPENRAILSRS_TRACK_AUDIT").is_some() && !stream.audit_done {
        let chords = collect_tdb_chords(tdb, &focus_at, radius_m, Some(assets.tsection()));
        run_track_dev_audit(
            tdb,
            &scene,
            &focus_at,
            RouteWorldOffset::default(),
            radius_m,
            &chords,
            None,
            Some(assets.tsection()),
        );
        stream.audit_done = true;
    }

    let mut raw: Vec<_> =
        collect_tdb_path_segments(tdb, &focus_at, radius_m, Some(assets.tsection()))
            .into_iter()
            .map(|mut seg| {
                let world = preserve_tdb_world_y(seg.position, &scene);
                let start_msts = world;
                let render = focus.to_render_surface(world);
                seg.position = view_translation(render, &origin);
                let end_msts = segment_end_msts(
                    &ProceduralTrackSegment {
                        position: render,
                        ..seg
                    },
                    &focus,
                );
                (seg, start_msts, end_msts)
            })
            .filter(|(_, start, end)| {
                segment_passes_corridor_filter(&corridor, *mode, center, *start, *end)
            })
            .collect();

    if raw.len() > TRACK_DEV_MAX_SEGMENTS {
        raw.truncate(TRACK_DEV_MAX_SEGMENTS);
    }

    if stream.rail_material.is_none() {
        stream.rail_material = Some(materials.add(StandardMaterial {
            base_color: Color::srgb(0.35, 0.38, 0.42),
            emissive: LinearRgba::new(0.15, 0.16, 0.18, 1.0),
            perceptual_roughness: 0.35,
            metallic: 0.75,
            ..default()
        }));
    }
    let rail_material = stream.rail_material.clone().unwrap();

    let mut spawned_now = 0usize;
    for (seg, start_msts, end_msts) in raw {
        let key = segment_key(start_msts, end_msts);
        if stream.spawned.contains(&key) {
            continue;
        }
        let entity = spawn_procedural_track_single(
            &mut commands,
            &mut meshes,
            &mut materials,
            seg,
            "tdb-graph",
            ProceduralTrackStyle::RailsOnly,
            &rail_material,
        );
        commands.entity(entity).insert(TdbTrackSegment {
            start_msts,
            end_msts,
        });
        stream.spawned.insert(key);
        spawned_now += 1;
    }
    if spawned_now > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: tdb-stream — +{spawned_now} segment(s) within {:.0}m of train",
            radius_m
        );
    }
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
    let chords = collect_tdb_chords(tdb, &focus, radius_m, Some(assets.tsection()));
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
        Some(assets.tsection()),
    );
    let mut segments: Vec<_> =
        collect_tdb_path_segments(tdb, &focus, radius_m, Some(assets.tsection()))
            .into_iter()
            .map(|mut seg| {
                let world = preserve_tdb_world_y(seg.position, &scene);
                seg.position = focus.to_render_surface(world);
                seg
            })
            .collect();
    if mode.is_run_corridor() && corridor.active() {
        let before = segments.len();
        let clip_center = focus.center;
        segments.retain(|seg| {
            let start = segment_start_msts(seg, &focus);
            let end = segment_end_msts(seg, &focus);
            corridor.contains_segment_near(
                clip_center,
                start,
                end,
                run_corridor_ahead_m(),
                run_corridor_behind_m(),
            )
        });
        viewer_log!(
            "openrailsrs-viewer3d: run_corridor — corridor filter {} → {} segment(s), width {:.0}m",
            before,
            segments.len(),
            corridor.half_width_m * 2.0
        );
    }
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
    use bevy::math::EulerRot;
    use openrailsrs_formats::{
        TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind,
        TrackVectorGeometry, TrackVectorPoint,
    };
    use std::path::PathBuf;

    fn fixtures_tdb(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../openrailsrs-msts/tests/fixtures/{name}"))
    }

    fn section_at(x: f64, z: f64, shape_idx: u32) -> TrVectorSectionRecord {
        section_at_y(x, 0.0, z, shape_idx)
    }

    fn section_at_y(x: f64, y: f64, z: f64, shape_idx: u32) -> TrVectorSectionRecord {
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y,
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
        assert_eq!(
            segments.len(),
            2,
            "terminal section skipped; two active spans"
        );
        let first_yaw = segments[0].rotation.to_euler(EulerRot::YXZ).0.to_degrees();
        let second_yaw = segments[1].rotation.to_euler(EulerRot::YXZ).0.to_degrees();
        assert!(
            (first_yaw - second_yaw).abs() > 45.0,
            "path must turn: first={first_yaw} second={second_yaw}"
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
            "terminal section at destination is skipped when already reached"
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
        let TrackNodeKind::Vector {
            length_m,
            sections,
            geometry,
            ..
        } = &vector.kind
        else {
            panic!("vector");
        };
        let node_length_m = *length_m;
        let has_geometry = geometry.is_some();
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
            "large-route path should emit chords near focus (length_m={node_length_m}, geometry={has_geometry})",
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
        let near_a = nearest_oriented_anchor(v_a, pin_a, hint, None).expect("near a");
        let near_b = nearest_oriented_anchor(v_b, pin_b, hint, None).expect("near b");
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
            inter_node_junction_gap_m(&tdb, 16839, 16931, None).unwrap_or(-1.0)
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
        let gap = inter_node_junction_gap_m(&tdb, 16655, 16683, None).expect("gap");
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
        let gap = inter_node_junction_gap_m(&tdb, 17401, 17835, None).expect("gap");
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
        let gap = inter_node_junction_gap_m(&tdb, 16099, 17902, None).expect("gap");
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
        let gap = inter_node_junction_gap_m(&tdb, 17522, 17703, None).expect("gap");
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
                .procedural_dims(s.shape_index)
                .map(|d| d.length_m)
                .unwrap_or(0.0);
            eprintln!(
                "  sec[{i}] shape={} dist_obj={dist:.1}m tsection_len={len:.1}m heading={:?}",
                s.shape_index,
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
        let gap = inter_node_junction_gap_m(&tdb, 17386, 17634, None).expect("gap");
        eprintln!("V17386↔V17634 via J17635 junction face gap: {gap:.1}m");
        assert!(
            gap < 20.0,
            "short-vector fallback should clamp this outlier near switch: {gap}m"
        );
    }

    #[test]
    fn minimal_tdb_shape_zero_yields_chords_with_section_zero_catalog() {
        use openrailsrs_formats::typed::{TrackSectionDef, TrackShapeDef, TrackShapePath};

        let tdb = TrackDbFile::from_path(fixtures_tdb("minimal.tdb")).expect("tdb");
        let mut cat = TSectionCatalog::default();
        cat.sections.insert(
            0,
            TrackSectionDef {
                gauge_m: 1.435,
                length_m: 1000.0,
                curve_radius_m: None,
                curve_angle_deg: None,
                skew_deg: None,
            },
        );
        cat.shapes.insert(
            0,
            TrackShapeDef {
                file_name: "sec0.s".into(),
                road_shape: false,
                paths: vec![TrackShapePath {
                    offset: [0.0, 0.0, 0.0],
                    angle_deg: 0.0,
                    num_sections: 1,
                    section_indices: vec![0],
                }],
                main_route: Some(0),
                clearance_dist_m: None,
            },
        );
        let focus = RouteFocus {
            center: Vec3::new(500.0, 0.0, 0.0),
            height_origin: 0.0,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 2000.0, Some(&cat));
        assert!(
            !chords.is_empty(),
            "minimal.tdb shape_idx=0 should produce chords when tsection section 0 exists"
        );
        assert!(
            chords
                .iter()
                .any(|c| (c.start_world.distance(c.end_world) - 1000.0).abs() < 5.0),
            "expected ~1000m span from tsection section 0"
        );
    }

    #[test]
    fn path_spans_chain_with_zero_intra_node_gap() {
        use openrailsrs_formats::typed::{TrackSectionDef, TrackShapeDef, TrackShapePath};

        let mut cat = TSectionCatalog::default();
        for (id, len) in [(1, 100.0_f64), (5005, 0.0)] {
            cat.sections.insert(
                id,
                if id == 5005 {
                    TrackSectionDef {
                        gauge_m: 1.435,
                        length_m: 0.0,
                        curve_radius_m: Some(500.0),
                        curve_angle_deg: Some(-5.0),
                        skew_deg: None,
                    }
                } else {
                    TrackSectionDef {
                        gauge_m: 1.435,
                        length_m: len,
                        curve_radius_m: None,
                        curve_angle_deg: None,
                        skew_deg: None,
                    }
                },
            );
        }
        cat.shapes.insert(
            1,
            TrackShapeDef {
                file_name: "s1.s".into(),
                road_shape: false,
                paths: vec![TrackShapePath {
                    offset: [0.0, 0.0, 0.0],
                    angle_deg: 0.0,
                    num_sections: 1,
                    section_indices: vec![1],
                }],
                main_route: Some(0),
                clearance_dist_m: None,
            },
        );
        cat.shapes.insert(
            2,
            TrackShapeDef {
                file_name: "s2.s".into(),
                road_shape: false,
                paths: vec![TrackShapePath {
                    offset: [0.0, 0.0, 0.0],
                    angle_deg: 0.0,
                    num_sections: 1,
                    section_indices: vec![1],
                }],
                main_route: Some(0),
                clearance_dist_m: None,
            },
        );

        let s0 = section_at(0.0, 0.0, 1);
        let s1 = section_at(0.0, 100.0, 2);
        let s2 = section_at(0.0, 200.0, 2);
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
            center: Vec3::new(50.0, 0.0, 0.0),
            height_origin: 0.0,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 500.0, Some(&cat));
        assert!(chords.len() >= 2);
        let mut by_node: std::collections::HashMap<u32, Vec<_>> = std::collections::HashMap::new();
        for c in &chords {
            by_node.entry(c.node_id).or_default().push(c);
        }
        for mut node_chords in by_node.into_values() {
            if node_chords.len() < 2 {
                continue;
            }
            node_chords.sort_by_key(|c| (c.section_index, c.span_index));
            for pair in node_chords.windows(2) {
                let gap = pair[0].end_world.distance(pair[1].start_world);
                assert!(
                    gap < 0.5,
                    "intra-node chain gap should be near zero, got {gap}m"
                );
            }
        }
    }

    #[test]
    fn segment_key_stable_and_order_independent() {
        let a = Vec3::new(1.0, 0.0, 2.0);
        let b = Vec3::new(3.0, 0.0, 4.0);
        assert_eq!(super::segment_key(a, b), super::segment_key(b, a));
        assert_ne!(
            super::segment_key(a, b),
            super::segment_key(a, a + Vec3::new(50.0, 0.0, 0.0))
        );
    }

    #[test]
    fn birmingham_like_placement_keeps_tdb_y_not_terrain() {
        // Rail MSL ≈ 35.8; terrain/height_origin ≈ 28.5. Must not flatten to ground_y_at (0.3).
        let section = section_at_y(0.0, 35.7818, 0.0, 1);
        let mut cat = TSectionCatalog::default();
        cat.sections.insert(
            1,
            openrailsrs_formats::typed::TrackSectionDef {
                gauge_m: 1.435,
                length_m: 100.0,
                curve_radius_m: None,
                curve_angle_deg: None,
                skew_deg: None,
            },
        );
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
        let focus = RouteFocus {
            center: Vec3::new(0.0, 35.7818, 0.0),
            height_origin: 28.5,
        };
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let segments = tdb_procedural_segments_near(&tdb, &cat, &scene, &focus, 500.0);
        assert!(!segments.is_empty());
        let expected_render_y = 35.7818 - 28.5;
        assert!(
            (segments[0].position.y - expected_render_y).abs() < 0.05,
            "expected render Y≈{expected_render_y} (TDB−terrain), got {}",
            segments[0].position.y
        );
        // Old bug flattened to ground_y_at≈0.3 → render Y ≈ 0.3−28.5.
        assert!(
            segments[0].position.y > 5.0,
            "rail must sit above terrain plane, got {}",
            segments[0].position.y
        );
    }

    #[test]
    fn view_window_120m_collects_limited_segments() {
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
        let near = collect_tdb_path_segments(&tdb, &focus, 120.0, None);
        let far = collect_tdb_path_segments(&tdb, &focus, 5000.0, None);
        assert!(!near.is_empty());
        assert!(near.len() <= far.len());
    }
}
