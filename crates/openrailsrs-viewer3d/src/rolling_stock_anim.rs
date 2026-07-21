//! Exterior rolling-stock part animation (#40 / #69): wheels, bogies, door/panto stubs.
//!
//! Meshes stay rest-baked (same pattern as WORLD #34). Drivers update each part's
//! local `Transform` without moving the car body.
//!
//! Bogie yaw (#69) samples track heading at the car pivot and at the bogie's
//! longitudinal offset (TDB via [`vehicle_position_yaw_on_graph_edge`], graph fallback).

use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    ShapeAnimBinding, animation_pose_matrices, world_baked_anim_transform,
};
use openrailsrs_formats::ShapeFile;

use crate::live::LiveDrive;
use crate::shapes::RouteAssets;
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::track_position::{
    TrackPositionResolver, advance_along_graph, vehicle_position_yaw_on_graph_edge,
};
use crate::train::{CsvRow, ReplayState, TrainMarker};
use crate::world::{RouteFocus, RouteWorldOffset};

/// Default wheel radius when shape bounds are unavailable (metres).
pub const DEFAULT_WHEEL_RADIUS_M: f32 = 0.46;
/// Max |relative yaw| applied to a bogie (radians).
pub const BOGIE_YAW_CLAMP: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollingStockPartKind {
    Wheel,
    Bogie,
    Door,
    Pantograph,
    Other,
}

/// Classify an MSTS matrix name (OR `MSTSWagonViewer` conventions).
pub fn classify_matrix_name(name: &str) -> RollingStockPartKind {
    let n = name.trim().to_ascii_uppercase();
    if n.starts_with("WHEEL") {
        return RollingStockPartKind::Wheel;
    }
    if n == "BOGIE" || n.starts_with("BOGIE") || n.starts_with("BOGEY") {
        return RollingStockPartKind::Bogie;
    }
    if n.starts_with("DOOR") {
        return RollingStockPartKind::Door;
    }
    if n.starts_with("PANTO") || n.contains("PANTOGRAPH") {
        return RollingStockPartKind::Pantograph;
    }
    RollingStockPartKind::Other
}

/// Wheel rotation driven by train speed (not shape keyframes).
#[derive(Component, Clone, Debug)]
pub struct TrainWheelAnim {
    pub matrix_idx: usize,
    pub radius_m: f32,
    pub angle_rad: f32,
}

/// Bogie yaw relative to the car body (track samples at ±longitudinal offset).
#[derive(Component, Clone, Debug)]
pub struct TrainBogieAnim {
    pub matrix_idx: usize,
    /// Longitudinal offset in shape space (MSTS matrix Z, metres). After
    /// `msts_shape_to_train_rotation` this is along train +X (forward).
    pub long_offset_m: f32,
}

/// Path offset of a consist car relative to the train head (#69).
#[derive(Component, Clone, Debug)]
pub struct TrainCarTrackOffset {
    /// Metres along the path from the consist head (negative = behind).
    pub offset_m: f32,
    /// Replay track index; live drive ignores this (always primary).
    pub track_index: usize,
}

/// Door / pantograph stub driven by a scalar key (shape anim or debug env).
#[derive(Component, Clone, Debug)]
pub struct TrainKeyedAnim {
    pub matrix_idx: usize,
    pub kind: RollingStockPartKind,
    /// Animation key in `[0, frame_count)` or normalized fraction when no anim.
    pub key: f32,
}

/// Marker: this part is exterior rolling-stock anim (skip cab interior).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TrainExteriorAnimPart;

/// Resolve matrix index for a prim_state (WORLD/train shared helper).
pub fn matrix_idx_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> usize {
    shape
        .prim_states
        .get(prim_state_idx.max(0) as usize)
        .and_then(|ps| shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize))
        .map(|vs| vs.matrix_idx.max(0) as usize)
        .unwrap_or(0)
}

pub fn matrix_name(shape: &ShapeFile, matrix_idx: usize) -> &str {
    shape
        .matrices
        .get(matrix_idx)
        .map(|m| m.name.as_str())
        .unwrap_or("")
}

fn env_key_frac(name: &str) -> Option<f32> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 1.0))
}

fn stub_key_for_kind(kind: RollingStockPartKind, shape: &ShapeFile) -> f32 {
    let frac = match kind {
        RollingStockPartKind::Door => env_key_frac("OPENRAILSRS_DEBUG_DOOR_KEY").unwrap_or(0.0),
        RollingStockPartKind::Pantograph => {
            env_key_frac("OPENRAILSRS_DEBUG_PANTO_KEY").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    let frame_count = shape
        .animations
        .first()
        .map(|a| a.frame_count as f32)
        .unwrap_or(0.0);
    if frame_count > 0.0 {
        frac * (frame_count - 1.0).max(0.0)
    } else {
        frac
    }
}

/// Build anim components for one exterior part, if the matrix name is animated.
#[allow(clippy::type_complexity)]
pub fn part_anim_bundle(
    shape: &ShapeFile,
    prim_state_idx: i32,
    radius_m: f32,
) -> Option<(
    TrainExteriorAnimPart,
    RollingStockPartKind,
    ShapeAnimBinding,
    Option<TrainWheelAnim>,
    Option<TrainBogieAnim>,
    Option<TrainKeyedAnim>,
)> {
    let matrix_idx = matrix_idx_for_prim_state(shape, prim_state_idx);
    let kind = classify_matrix_name(matrix_name(shape, matrix_idx));
    if kind == RollingStockPartKind::Other {
        return None;
    }
    let binding = ShapeAnimBinding {
        shape: shape.clone(),
        matrix_idx,
        speed: 0.0,
        frame_count: shape
            .animations
            .first()
            .map(|a| a.frame_count as f32)
            .unwrap_or(0.0),
        placement: Transform::IDENTITY,
        baked_rest_mesh: true,
    };
    let wheel = (kind == RollingStockPartKind::Wheel).then_some(TrainWheelAnim {
        matrix_idx,
        radius_m: radius_m.max(0.15),
        angle_rad: 0.0,
    });
    let bogie = (kind == RollingStockPartKind::Bogie).then(|| {
        let long_offset_m = shape
            .matrices
            .get(matrix_idx)
            .map(|m| m.matrix.rows[3][2] as f32)
            .unwrap_or(0.0);
        TrainBogieAnim {
            matrix_idx,
            long_offset_m,
        }
    });
    let keyed = matches!(
        kind,
        RollingStockPartKind::Door | RollingStockPartKind::Pantograph
    )
    .then(|| TrainKeyedAnim {
        matrix_idx,
        kind,
        key: stub_key_for_kind(kind, shape),
    });
    Some((TrainExteriorAnimPart, kind, binding, wheel, bogie, keyed))
}

/// Insert anim components on a freshly spawned exterior part entity.
pub fn insert_part_anim(
    entity: &mut EntityCommands,
    shape: &ShapeFile,
    prim_state_idx: i32,
    radius_m: f32,
) {
    let Some((marker, _kind, binding, wheel, bogie, keyed)) =
        part_anim_bundle(shape, prim_state_idx, radius_m)
    else {
        return;
    };
    entity.insert((marker, binding));
    if let Some(w) = wheel {
        entity.insert(w);
    }
    if let Some(b) = bogie {
        entity.insert(b);
    }
    if let Some(k) = keyed {
        entity.insert(k);
    }
}

fn train_speed_mps(live: Option<&LiveDrive>, replay: Option<&ReplayState>) -> f32 {
    if let Some(live) = live {
        return live.session.velocity_mps() as f32;
    }
    if let Some(replay) = replay.filter(|r| r.is_active()) {
        if let Some(track) = replay.tracks.first() {
            // Nearest row by time for visual wheel speed.
            let mut best = 0.0f32;
            let mut best_dt = f64::MAX;
            for row in &track.rows {
                let dt = (row.time_s - replay.t_sim).abs();
                if dt < best_dt {
                    best_dt = dt;
                    best = row.velocity_mps as f32;
                }
            }
            return best;
        }
    }
    0.0
}

fn wrap_angle(a: f32) -> f32 {
    let mut x = a;
    while x > std::f32::consts::PI {
        x -= std::f32::consts::TAU;
    }
    while x < -std::f32::consts::PI {
        x += std::f32::consts::TAU;
    }
    x
}

/// Relative bogie yaw from track headings at car pivot and bogie sample (#69).
pub fn bogie_relative_yaw(car_yaw: f32, bogie_track_yaw: f32) -> f32 {
    wrap_angle(bogie_track_yaw - car_yaw).clamp(-BOGIE_YAW_CLAMP, BOGIE_YAW_CLAMP)
}

fn csv_row_at(rows: &[CsvRow], t: f64) -> Option<&CsvRow> {
    if rows.is_empty() {
        return None;
    }
    let idx = rows
        .partition_point(|r| r.time_s <= t)
        .saturating_sub(1)
        .min(rows.len() - 1);
    Some(&rows[idx])
}

/// Head `(edge_id, pos_on_edge_m)` for a consist car (live path or replay CSV).
fn head_graph_position(
    live: Option<&LiveDrive>,
    replay: Option<&ReplayState>,
    track_index: usize,
) -> Option<(String, f64)> {
    if let Some(live) = live {
        let edge = live.session.current_edge_id()?.to_string();
        return Some((edge, live.session.pos_on_edge_m()));
    }
    let replay = replay.filter(|r| r.is_active())?;
    let track = replay.tracks.get(track_index)?;
    let row = csv_row_at(&track.rows, replay.t_sim)?;
    if row.edge_id.trim().is_empty() {
        return None;
    }
    Some((row.edge_id.clone(), row.pos_on_edge_m))
}

#[allow(clippy::too_many_arguments)]
fn sample_yaw_at_path_offset(
    graph: &openrailsrs_track::TrackGraph,
    live: Option<&LiveDrive>,
    head_edge: &str,
    head_pos: f64,
    path_offset_m: f64,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    route_offset: Vec3,
    focus: &RouteFocus,
    terrain: Option<&TerrainElevation>,
) -> Option<f32> {
    let (edge_id, pos) = if let Some(live) = live {
        live.session.position_at_head_offset(path_offset_m)?
    } else {
        advance_along_graph(graph, head_edge, head_pos, path_offset_m)?
    };
    vehicle_position_yaw_on_graph_edge(
        graph,
        &edge_id,
        pos,
        resolver,
        scene,
        route_offset,
        focus,
        terrain,
    )
    .map(|(_, yaw)| yaw)
}

/// Advance wheel / bogie / keyed exterior parts each frame (#40 / #69).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_rolling_stock_part_anim(
    time: Res<Time>,
    live: Option<Res<LiveDrive>>,
    replay: Option<Res<ReplayState>>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    terrain: Option<Res<TerrainElevation>>,
    mut wheels: Query<
        (&mut TrainWheelAnim, &ShapeAnimBinding, &mut Transform),
        With<TrainExteriorAnimPart>,
    >,
    mut bogies: Query<
        (&TrainBogieAnim, &ShapeAnimBinding, &mut Transform, &ChildOf),
        (With<TrainExteriorAnimPart>, Without<TrainWheelAnim>),
    >,
    cars: Query<&TrainCarTrackOffset, Without<TrainExteriorAnimPart>>,
    train_markers: Query<&TrainMarker>,
    car_parents: Query<&ChildOf, Without<TrainExteriorAnimPart>>,
    mut keyed: Query<
        (&TrainKeyedAnim, &ShapeAnimBinding, &mut Transform),
        (
            With<TrainExteriorAnimPart>,
            Without<TrainWheelAnim>,
            Without<TrainBogieAnim>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let live_ref = live.as_deref();
    let replay_ref = replay.as_deref();
    let speed = train_speed_mps(live_ref, replay_ref);

    for (mut wheel, binding, mut tf) in &mut wheels {
        let r = wheel.radius_m.max(0.15);
        wheel.angle_rad += (speed / r) * dt;
        // Bevy: +X lateral; negative angle so +Z forward motion rolls "forward".
        let rot = Quat::from_rotation_x(-wheel.angle_rad);
        let next = Transform {
            translation: Vec3::ZERO,
            rotation: rot,
            scale: Vec3::ONE,
        };
        if next.translation.is_finite() && next.rotation.is_finite() {
            *tf = next;
        }
        let _ = binding; // wheel uses speed, not shape keys
    }

    let terrain_ref = terrain.as_deref();
    let tdb_resolver = assets
        .track_db()
        .map(|tdb| TrackPositionResolver::from_track_scene(tdb, Some(assets.tsection()), &scene));
    let resolver_ref = tdb_resolver.as_ref();

    for (bogie, binding, mut tf, child_of) in &mut bogies {
        let Ok(car_off) = cars.get(child_of.parent()) else {
            // No path offset on parent (e.g. fallback cube) — leave bogie straight.
            *tf = Transform::IDENTITY;
            let _ = binding;
            continue;
        };
        let track_index = car_parents
            .get(child_of.parent())
            .ok()
            .and_then(|p| train_markers.get(p.parent()).ok())
            .map(|m| m.track_index)
            .unwrap_or(car_off.track_index);

        let Some((head_edge, head_pos)) = head_graph_position(live_ref, replay_ref, track_index)
        else {
            *tf = Transform::IDENTITY;
            let _ = binding;
            continue;
        };

        let car_path = f64::from(car_off.offset_m);
        let bogie_path = car_path + f64::from(bogie.long_offset_m);
        let Some(car_yaw) = sample_yaw_at_path_offset(
            &scene.graph,
            live_ref,
            &head_edge,
            head_pos,
            car_path,
            resolver_ref,
            &scene,
            offset.delta,
            &focus,
            terrain_ref,
        ) else {
            *tf = Transform::IDENTITY;
            let _ = binding;
            continue;
        };
        let Some(bogie_yaw) = sample_yaw_at_path_offset(
            &scene.graph,
            live_ref,
            &head_edge,
            head_pos,
            bogie_path,
            resolver_ref,
            &scene,
            offset.delta,
            &focus,
            terrain_ref,
        ) else {
            *tf = Transform::IDENTITY;
            let _ = binding;
            continue;
        };

        let rel = bogie_relative_yaw(car_yaw, bogie_yaw);
        let next = Transform {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_y(rel),
            scale: Vec3::ONE,
        };
        if next.rotation.is_finite() {
            *tf = next;
        }
        let _ = binding;
    }

    for (keyed_anim, binding, mut tf) in &mut keyed {
        let key = keyed_anim.key;
        if binding.frame_count > 0.0 && !binding.shape.animations.is_empty() {
            let pose = animation_pose_matrices(&binding.shape, key);
            let next = world_baked_anim_transform(
                Transform::IDENTITY,
                &binding.shape,
                keyed_anim.matrix_idx,
                &pose,
            );
            if next.translation.is_finite() && next.rotation.is_finite() {
                *tf = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_formats::{Matrix43, NamedMatrix, PrimState, VtxState};
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

    use crate::track_position::advance_along_graph;

    fn identity_matrix() -> Matrix43 {
        Matrix43 {
            rows: [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
            ],
        }
    }

    #[test]
    fn classify_matrix_names() {
        assert_eq!(classify_matrix_name("WHEELS1"), RollingStockPartKind::Wheel);
        assert_eq!(classify_matrix_name("WHEEL"), RollingStockPartKind::Wheel);
        assert_eq!(classify_matrix_name("BOGIE2"), RollingStockPartKind::Bogie);
        assert_eq!(classify_matrix_name("bogie"), RollingStockPartKind::Bogie);
        assert_eq!(
            classify_matrix_name("DOOR_LEFT"),
            RollingStockPartKind::Door
        );
        assert_eq!(
            classify_matrix_name("PANTOGRAPH1"),
            RollingStockPartKind::Pantograph
        );
        assert_eq!(
            classify_matrix_name("PANTO_FRONT"),
            RollingStockPartKind::Pantograph
        );
        assert_eq!(classify_matrix_name("MAIN"), RollingStockPartKind::Other);
    }

    fn shape_with_named_matrix(name: &str) -> ShapeFile {
        let mut shape = ShapeFile::default();
        shape.matrices.push(NamedMatrix {
            name: name.into(),
            matrix: identity_matrix(),
        });
        shape.vtx_states.push(VtxState {
            flags: 0,
            matrix_idx: 0,
            light_mat_idx: -5,
            light_cfg_idx: 0,
        });
        shape.prim_states.push(PrimState {
            name: None,
            flags: 0,
            shader_idx: 0,
            texture_idx: -1,
            tex_indices: vec![],
            vertex_state_idx: 0,
            z_bias: None,
            alpha_test_mode: -1,
            z_buf_mode: -1,
        });
        shape
    }

    #[test]
    fn part_anim_bundle_selects_wheel() {
        let shape = shape_with_named_matrix("WHEELS1");
        let bundle = part_anim_bundle(&shape, 0, 0.5).expect("wheel");
        assert_eq!(bundle.1, RollingStockPartKind::Wheel);
        assert!(bundle.3.is_some());
        assert!(bundle.4.is_none());
    }

    #[test]
    fn wheel_angle_increases_with_speed_body_untouched() {
        let mut wheel = TrainWheelAnim {
            matrix_idx: 0,
            radius_m: 0.5,
            angle_rad: 0.0,
        };
        let body = Transform::from_xyz(10.0, 0.0, 3.0);
        let speed = 10.0f32;
        let dt = 0.1f32;
        wheel.angle_rad += (speed / wheel.radius_m) * dt;
        assert!((wheel.angle_rad - 2.0).abs() < 1e-4);
        // Body transform is independent of wheel angle.
        assert!((body.translation.x - 10.0).abs() < 1e-6);
    }

    #[test]
    fn bogie_relative_yaw_zero_on_matching_headings() {
        let rel = bogie_relative_yaw(1.2, 1.2);
        assert!(rel.abs() < 1e-5);
    }

    #[test]
    fn bogie_relative_yaw_nonzero_on_curve_and_clamped() {
        let rel = bogie_relative_yaw(0.0, 0.2);
        assert!(rel > 0.05, "expected non-zero relative yaw, got {rel}");
        let big = bogie_relative_yaw(0.0, 1.5);
        assert!((big.abs() - BOGIE_YAW_CLAMP).abs() < 1e-5);
    }

    #[test]
    fn bogie_yaw_clamp_finite() {
        let rel = bogie_relative_yaw(0.0, 0.5);
        assert!(rel.is_finite());
        assert!(rel.abs() <= BOGIE_YAW_CLAMP);
    }

    /// L-shaped graph: e1 along +X, e2 along +Z — yaw changes at the corner.
    fn elbow_graph() -> TrackGraph {
        let mut g = TrackGraph::new();
        for (id, x_m, y_m) in [("a", 0.0, 0.0), ("b", 100.0, 0.0), ("c", 100.0, 100.0)] {
            g.insert_node(Node {
                id: NodeId(id.into()),
                kind: NodeKind::Plain,
                x_m,
                y_m,
            })
            .unwrap();
        }
        g.insert_edge(Edge {
            id: EdgeId("e1".into()),
            from: NodeId("a".into()),
            to: NodeId("b".into()),
            length_m: 100.0,
            speed_limit_mps: 20.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g.insert_edge(Edge {
            id: EdgeId("e2".into()),
            from: NodeId("b".into()),
            to: NodeId("c".into()),
            length_m: 100.0,
            speed_limit_mps: 20.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g
    }

    #[test]
    fn advance_along_graph_crosses_elbow() {
        let g = elbow_graph();
        let (eid, pos) = advance_along_graph(&g, "e1", 90.0, 20.0).expect("advance");
        assert_eq!(eid, "e2");
        assert!((pos - 10.0).abs() < 1e-6);
        let (back_e, back_p) = advance_along_graph(&g, "e2", 10.0, -20.0).expect("back");
        assert_eq!(back_e, "e1");
        assert!((back_p - 90.0).abs() < 1e-6);
    }

    #[test]
    fn track_yaw_differs_across_elbow_for_bogie_sample() {
        let g = elbow_graph();
        let scene = TrackScene::from_graph(g.clone());
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let (_, yaw_car) = vehicle_position_yaw_on_graph_edge(
            &g,
            "e1",
            50.0,
            None,
            &scene,
            Vec3::ZERO,
            &focus,
            None,
        )
        .expect("car yaw");
        let (e_bogie, p_bogie) = advance_along_graph(&g, "e1", 95.0, 10.0).expect("bogie pos");
        assert_eq!(e_bogie, "e2");
        let (_, yaw_bogie) = vehicle_position_yaw_on_graph_edge(
            &g,
            &e_bogie,
            p_bogie,
            None,
            &scene,
            Vec3::ZERO,
            &focus,
            None,
        )
        .expect("bogie yaw");
        let rel = bogie_relative_yaw(yaw_car, yaw_bogie);
        assert!(
            rel.abs() > 0.05,
            "curve sample should steer bogie, car={yaw_car} bogie={yaw_bogie} rel={rel}"
        );
        // Straight: same edge, same heading → ~0.
        let (_, yaw_a) = vehicle_position_yaw_on_graph_edge(
            &g,
            "e1",
            40.0,
            None,
            &scene,
            Vec3::ZERO,
            &focus,
            None,
        )
        .unwrap();
        let (_, yaw_b) = vehicle_position_yaw_on_graph_edge(
            &g,
            "e1",
            60.0,
            None,
            &scene,
            Vec3::ZERO,
            &focus,
            None,
        )
        .unwrap();
        assert!(bogie_relative_yaw(yaw_a, yaw_b).abs() < 1e-4);
    }

    #[test]
    fn keyed_stub_matrix_idx_stable() {
        let shape = shape_with_named_matrix("DOOR_LEFT");
        let bundle = part_anim_bundle(&shape, 0, 0.5).expect("door");
        assert_eq!(bundle.1, RollingStockPartKind::Door);
        let keyed = bundle.5.expect("keyed");
        assert_eq!(keyed.matrix_idx, 0);
        assert!(keyed.key.is_finite());
    }
}
