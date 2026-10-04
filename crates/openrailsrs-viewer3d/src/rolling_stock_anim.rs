//! Exterior rolling-stock part animation (#40 / #69 / #81): wheels, bogies, doors/panto.
//!
//! Meshes stay rest-baked (same pattern as WORLD #34). Drivers update each part's
//! local `Transform` without moving the car body.
//!
//! Bogie yaw (#69) samples track heading at the car pivot and at the bogie's
//! longitudinal offset (TDB via [`vehicle_position_yaw_on_graph_edge`], graph fallback).
//! Door/panto keys (#81) follow [`RollingStockExteriorState`] (env debug overrides).
//!
//! Car bodies follow per-vehicle TDB/graph chainage (#128), not a rigid bar on the lead.

use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    ShapeAnimBinding, animation_pose_matrices, world_baked_anim_transform,
};
use openrailsrs_formats::ShapeFile;
use openrailsrs_or_shader::coordinates::static_hierarchy_chain_transform;
use openrailsrs_sim::RollingStockExteriorState;
use std::{collections::HashMap, sync::Arc};

use crate::floating_origin::{FloatingOrigin, view_position};
use crate::live::{LiveDrive, LiveTrainMarker};
use crate::shapes::{RouteAssets, vehicle_authored_frame_transform};
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::track_position::{
    TrackPositionResolver, advance_along_graph, vehicle_pose_on_graph_edge,
    vehicle_position_yaw_on_graph_edge,
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

/// Wheel rotation driven by signed presentation distance (not wall-clock time).
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

/// Path offset of a consist car relative to the train head (#69 / #128).
#[derive(Component, Clone, Debug)]
pub struct TrainCarTrackOffset {
    /// Metres along the path from the consist head (negative = behind).
    pub offset_m: f32,
    /// Replay track index; live drive ignores this (always primary).
    pub track_index: usize,
    /// `.con` Flip — applied in the authored vehicle frame (#130 / #128).
    pub flipped: bool,
}

/// World pose for a car at `path_offset_m` from the consist head (#128).
pub fn car_world_pose_at_head_offset(
    graph: &openrailsrs_track::TrackGraph,
    live: Option<&LiveDrive>,
    head_edge: &str,
    head_pos: f64,
    path_offset_m: f64,
    flipped: bool,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    route_offset: Vec3,
    focus: &RouteFocus,
    terrain: Option<&TerrainElevation>,
    origin: &FloatingOrigin,
) -> Option<Transform> {
    let (edge_id, pos) = if let Some(live) = live {
        live.visual_position_at_head_offset(path_offset_m)?
    } else {
        advance_along_graph(graph, head_edge, head_pos, path_offset_m)?
    };
    let (world_pos, world_rot) = vehicle_pose_on_graph_edge(
        graph,
        &edge_id,
        pos,
        resolver,
        scene,
        route_offset,
        focus,
        terrain,
    )?;
    let track =
        Transform::from_translation(view_position(world_pos, origin)).with_rotation(world_rot);
    let authored = vehicle_authored_frame_transform(0.0, flipped);
    Some(track * authored)
}

/// Local child transform so `parent * local ≈ car_world` (#128).
pub fn car_local_from_parent_and_world(parent: &Transform, car_world: &Transform) -> Transform {
    let parent_m = parent.to_matrix();
    let car_m = car_world.to_matrix();
    Transform::from_matrix(parent_m.inverse() * car_m)
}

/// Apply a local bone rotation to rest-baked mesh vertices without moving the pivot.
///
/// Rolling-stock part meshes already contain their complete rest hierarchy. A raw
/// entity rotation would rotate them around the car origin and detach wheels/bogies.
fn baked_part_local_rotation(
    shape: &ShapeFile,
    matrix_idx: usize,
    local_rotation: Quat,
) -> Transform {
    let rest = static_hierarchy_chain_transform(shape, matrix_idx);
    rest * Transform::from_rotation(local_rotation)
        * Transform::from_matrix(rest.to_matrix().inverse())
}

/// Update consist car bodies to individual track chainage (#128).
///
/// Runs after the lead marker pose is written; children keep authored mesh frame + Flip.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_consist_car_track_poses(
    live: Option<Res<LiveDrive>>,
    replay: Option<Res<ReplayState>>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
    resolver_cache: Res<crate::track_position::TrackPositionResolverCache>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    terrain: Option<Res<TerrainElevation>>,
    origin: Res<FloatingOrigin>,
    parents: Query<&Transform, Or<(With<LiveTrainMarker>, With<TrainMarker>)>>,
    // Disjoint from `parents`: lead may be LiveTrainMarker without TrainMarker (Bevy B0001).
    mut cars: Query<
        (&TrainCarTrackOffset, &ChildOf, &mut Transform),
        (Without<TrainMarker>, Without<LiveTrainMarker>),
    >,
) {
    let live_ref = live.as_deref();
    let replay_ref = replay.as_deref();
    let tdb_resolver = assets
        .track_db()
        .map(|tdb| resolver_cache.resolver(tdb, Some(assets.tsection())));
    let terrain_ref = terrain.as_deref();

    for (car, child_of, mut tf) in &mut cars {
        let Ok(parent_tf) = parents.get(child_of.parent()) else {
            continue;
        };
        let Some((head_edge, head_pos)) =
            head_graph_position(live_ref, replay_ref, car.track_index)
        else {
            continue;
        };
        let Some(car_world) = car_world_pose_at_head_offset(
            &scene.graph,
            live_ref,
            &head_edge,
            head_pos,
            f64::from(car.offset_m),
            car.flipped,
            tdb_resolver.as_ref(),
            &scene,
            offset.delta,
            &focus,
            terrain_ref,
            &origin,
        ) else {
            continue;
        };
        *tf = car_local_from_parent_and_world(parent_tf, &car_world);
    }
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

/// Normalized [0, 1] target from live exterior state (no env override).
pub fn frac_for_kind(kind: RollingStockPartKind, exterior: &RollingStockExteriorState) -> f32 {
    match kind {
        RollingStockPartKind::Door if exterior.door.anim_open_target() => 1.0,
        RollingStockPartKind::Door => 0.0,
        RollingStockPartKind::Pantograph if exterior.pantograph_command_up => 1.0,
        RollingStockPartKind::Pantograph => 0.0,
        _ => 0.0,
    }
}

/// Map normalized fraction to shape animation key.
pub fn key_from_frac(frac: f32, frame_count: f32) -> f32 {
    let frac = frac.clamp(0.0, 1.0);
    if frame_count > 0.0 {
        frac * (frame_count - 1.0).max(0.0)
    } else {
        frac
    }
}

/// Env debug overrides sim; otherwise use exterior (or 0 if no live session).
pub fn resolve_keyed_frac(
    kind: RollingStockPartKind,
    exterior: Option<&RollingStockExteriorState>,
) -> f32 {
    let env_name = match kind {
        RollingStockPartKind::Door => Some("OPENRAILSRS_DEBUG_DOOR_KEY"),
        RollingStockPartKind::Pantograph => Some("OPENRAILSRS_DEBUG_PANTO_KEY"),
        _ => None,
    };
    if let Some(name) = env_name
        && let Some(frac) = env_key_frac(name)
    {
        return frac;
    }
    exterior.map(|ext| frac_for_kind(kind, ext)).unwrap_or(0.0)
}

fn stub_key_for_kind(kind: RollingStockPartKind, shape: &ShapeFile) -> f32 {
    let frame_count = shape
        .animations
        .first()
        .map(|a| a.frame_count as f32)
        .unwrap_or(0.0);
    key_from_frac(resolve_keyed_frac(kind, None), frame_count)
}

/// Build anim components for one exterior part, if the matrix name is animated.
#[allow(clippy::type_complexity)]
pub fn part_anim_bundle(
    shape: &Arc<ShapeFile>,
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
        shape: Arc::clone(shape),
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
    shape: &Arc<ShapeFile>,
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

fn wheel_angle(distance_m: f64, radius_m: f32, flipped: bool) -> f32 {
    let direction = if flipped { -1.0 } else { 1.0 };
    ((distance_m * direction / f64::from(radius_m.max(0.15))).rem_euclid(std::f64::consts::TAU))
        as f32
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
        return live.visual_position_at_head_offset(0.0);
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
    car_offset_m: f64,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    route_offset: Vec3,
    focus: &RouteFocus,
    terrain: Option<&TerrainElevation>,
) -> Option<f32> {
    let (edge_id, pos) = if let Some(live) = live {
        live.visual_position_for_car(path_offset_m, car_offset_m)?
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
    live: Option<Res<LiveDrive>>,
    replay: Option<Res<ReplayState>>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
    resolver_cache: Res<crate::track_position::TrackPositionResolverCache>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    terrain: Option<Res<TerrainElevation>>,
    mut wheels: Query<
        (
            &mut TrainWheelAnim,
            Ref<ShapeAnimBinding>,
            &mut Transform,
            &ChildOf,
        ),
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
        (&mut TrainKeyedAnim, Ref<ShapeAnimBinding>, &mut Transform),
        (
            With<TrainExteriorAnimPart>,
            Without<TrainWheelAnim>,
            Without<TrainBogieAnim>,
        ),
    >,
) {
    let live_ref = live.as_deref();
    let replay_ref = replay.as_deref();
    let exterior = live_ref.map(|l| &l.session.exterior);

    for (mut wheel, binding, mut tf, parent) in &mut wheels {
        let car = cars.get(parent.parent()).ok();
        let track_index = car.map_or(0, |car| car.track_index);
        let distance = live_ref
            .map(|live| live.visual_car_distance_m(car.map_or(0.0, |car| f64::from(car.offset_m))))
            .or_else(|| replay_ref.and_then(|replay| replay.wheel_distance_m(track_index)))
            .unwrap_or(0.0);
        let angle = wheel_angle(distance, wheel.radius_m, car.is_some_and(|car| car.flipped));
        if wheel.angle_rad == angle && !wheel.is_added() && !binding.is_changed() {
            continue;
        }
        wheel.angle_rad = angle;
        // Rotate about the authored axle while retaining its baked pivot.
        let rot = Quat::from_rotation_x(-wheel.angle_rad);
        let next = baked_part_local_rotation(&binding.shape, wheel.matrix_idx, rot);
        if next.translation.is_finite() && next.rotation.is_finite() {
            tf.set_if_neq(next);
        }
    }

    let terrain_ref = terrain.as_deref();
    let tdb_resolver = assets
        .track_db()
        .map(|tdb| resolver_cache.resolver(tdb, Some(assets.tsection())));
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
        let bogie_path =
            car_path + f64::from(bogie.long_offset_m) * if car_off.flipped { -1.0 } else { 1.0 };
        let Some(car_yaw) = sample_yaw_at_path_offset(
            &scene.graph,
            live_ref,
            &head_edge,
            head_pos,
            car_path,
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

        let rel = bogie_relative_yaw(car_yaw, bogie_yaw);
        let next =
            baked_part_local_rotation(&binding.shape, bogie.matrix_idx, Quat::from_rotation_y(rel));
        if next.rotation.is_finite() {
            tf.set_if_neq(next);
        }
    }

    let mut pose_cache = HashMap::new();
    for (mut keyed_anim, binding, mut tf) in &mut keyed {
        let frac = resolve_keyed_frac(keyed_anim.kind, exterior);
        let key = key_from_frac(frac, binding.frame_count);
        if keyed_anim.key == key && !keyed_anim.is_added() && !binding.is_changed() {
            continue;
        }
        keyed_anim.key = key;
        if binding.frame_count > 0.0 && !binding.shape.animations.is_empty() {
            let shape_id = Arc::as_ptr(&binding.shape) as usize;
            let pose = pose_cache
                .entry((shape_id, key.to_bits()))
                .or_insert_with(|| animation_pose_matrices(&binding.shape, key));
            let next = world_baked_anim_transform(
                Transform::IDENTITY,
                &binding.shape,
                keyed_anim.matrix_idx,
                pose,
            );
            if next.translation.is_finite() && next.rotation.is_finite() {
                tf.set_if_neq(next);
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
    fn animated_parts_share_the_authored_shape_instead_of_copying_mesh_data() {
        let shape = Arc::new(shape_with_named_matrix("WHEELS1"));
        let first = part_anim_bundle(&shape, 0, 0.5).unwrap().2;
        let second = part_anim_bundle(&shape, 0, 0.5).unwrap().2;
        assert!(Arc::ptr_eq(&first.shape, &second.shape));
        assert!(Arc::ptr_eq(&first.shape, &shape));
    }

    #[test]
    fn part_anim_bundle_selects_wheel() {
        let shape = shape_with_named_matrix("WHEELS1");
        let bundle = part_anim_bundle(&Arc::new(shape.clone()), 0, 0.5).expect("wheel");
        assert_eq!(bundle.1, RollingStockPartKind::Wheel);
        assert!(bundle.3.is_some());
        assert!(bundle.4.is_none());
    }

    fn spawn_test_wheel(app: &mut App, track: usize, flipped: bool) -> Entity {
        let car = app
            .world_mut()
            .spawn(TrainCarTrackOffset {
                offset_m: 0.0,
                track_index: track,
                flipped,
            })
            .id();
        let shape = shape_with_named_matrix("WHEELS1");
        let (marker, _, binding, wheel, _, _) =
            part_anim_bundle(&Arc::new(shape.clone()), 0, 0.5).unwrap();
        app.world_mut()
            .spawn((
                marker,
                binding,
                wheel.unwrap(),
                Transform::IDENTITY,
                ChildOf(car),
            ))
            .id()
    }

    #[test]
    fn live_wheels_follow_body_distance_at_accelerated_time_pause_and_reset() {
        let mut app = crate::test_harness::minimal_app();
        let mut live =
            LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path()).unwrap();
        live.session.speed_mul = 4.0;
        live.session.state.velocity_mps = 10.0;
        crate::test_harness::insert_live_bundle(&mut app, live);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(100),
        ));
        app.add_systems(Update, update_rolling_stock_part_anim);
        let wheel = spawn_test_wheel(&mut app, 0, false);
        let flipped = spawn_test_wheel(&mut app, 0, true);
        app.update();
        app.world_mut()
            .resource_mut::<LiveDrive>()
            .session
            .step_realtime(0.1, |_| {});
        app.update();
        let live = app.world().resource::<LiveDrive>();
        let distance = live.visual_distance_m();
        assert!(
            distance > 2.0,
            "accelerated presentation should actually move"
        );
        let (edge, body_position) = live.visual_position_at_head_offset(0.0).unwrap();
        assert_eq!(edge, live.session.state.path_edges[0]);
        assert!((body_position - live.session.start_chainage_m - distance).abs() < 1e-6);
        let normal = app.world().get::<Transform>(wheel).unwrap().rotation;
        let inverse = app.world().get::<Transform>(flipped).unwrap().rotation;
        let expected = Quat::from_rotation_x(-(distance / 0.5) as f32);
        assert!(normal.dot(expected).abs() > 0.99999);
        assert!(normal.inverse().dot(inverse).abs() > 0.99999);
        app.world_mut().resource_mut::<LiveDrive>().paused = true;
        app.update();
        app.update();
        assert_eq!(
            app.world().get::<Transform>(wheel).unwrap().rotation,
            normal,
            "paused wheels must not keep spinning"
        );
        app.world_mut().resource_mut::<LiveDrive>().reset().unwrap();
        app.update();
        assert_eq!(
            *app.world().get::<Transform>(wheel).unwrap(),
            Transform::IDENTITY
        );
        assert_eq!(
            *app.world().get::<Transform>(flipped).unwrap(),
            Transform::IDENTITY
        );
    }

    #[test]
    fn replay_wheels_use_each_trains_distance_and_seek_with_the_body() {
        use crate::train::TrainTrack;
        let make_track = |distance: f64| TrainTrack {
            label: "test".into(),
            color: Color::WHITE,
            rows: vec![
                CsvRow {
                    time_s: 0.0,
                    velocity_mps: 10.0,
                    edge_id: "e1".into(),
                    pos_on_edge_m: 20.0,
                    odometer_m: Some(0.0),
                },
                CsvRow {
                    time_s: 1.0,
                    velocity_mps: 10.0,
                    edge_id: "e1".into(),
                    pos_on_edge_m: 20.0 + distance,
                    odometer_m: Some(distance),
                },
            ],
        };
        let mut app = crate::test_harness::minimal_app();
        crate::test_harness::insert_replay_bundle(
            &mut app,
            TrackScene::from_graph(elbow_graph()),
            ReplayState::new("test".into(), vec![make_track(1.25), make_track(-0.75)]),
        );
        app.add_systems(Update, update_rolling_stock_part_anim);
        let first = spawn_test_wheel(&mut app, 0, false);
        let second = spawn_test_wheel(&mut app, 1, false);
        app.update();
        {
            let mut replay = app.world_mut().resource_mut::<ReplayState>();
            replay.t_sim = 1.0;
            replay.speed = 16.0;
        }
        app.update();
        let rotation = |app: &App, entity| app.world().get::<Transform>(entity).unwrap().rotation;
        assert!(rotation(&app, first).dot(Quat::from_rotation_x(-2.5)).abs() > 0.99999);
        assert!(rotation(&app, second).dot(Quat::from_rotation_x(1.5)).abs() > 0.99999);
        let before = rotation(&app, first);
        app.world_mut().resource_mut::<ReplayState>().paused = true;
        app.update();
        app.update();
        assert_eq!(rotation(&app, first), before);
        app.world_mut().resource_mut::<ReplayState>().t_sim = 0.0;
        app.update();
        assert_eq!(rotation(&app, first), Quat::IDENTITY);
        assert_eq!(rotation(&app, second), Quat::IDENTITY);
        assert!(wheel_angle(10_000_000.0, 0.5, false) < std::f32::consts::TAU);
    }

    #[test]
    fn flipped_bogie_samples_the_opposite_end_of_the_car_on_a_curve() {
        use crate::train::TrainTrack;
        let mut app = crate::test_harness::minimal_app();
        let replay = ReplayState::new(
            "test".into(),
            vec![TrainTrack {
                label: "test".into(),
                color: Color::WHITE,
                rows: vec![CsvRow {
                    time_s: 0.0,
                    velocity_mps: 0.0,
                    edge_id: "e1".into(),
                    pos_on_edge_m: 85.0,
                    odometer_m: None,
                }],
            }],
        );
        crate::test_harness::insert_replay_bundle(
            &mut app,
            TrackScene::from_graph(elbow_graph()),
            replay,
        );
        app.add_systems(Update, update_rolling_stock_part_anim);
        let mut entities = Vec::new();
        for flipped in [false, true] {
            let car = app
                .world_mut()
                .spawn(TrainCarTrackOffset {
                    offset_m: 0.0,
                    track_index: 0,
                    flipped,
                })
                .id();
            let shape = shape_with_named_matrix("BOGIE1");
            let (marker, _, binding, _, mut bogie, _) =
                part_anim_bundle(&Arc::new(shape.clone()), 0, 0.5).unwrap();
            bogie.as_mut().unwrap().long_offset_m = 35.0;
            entities.push(
                app.world_mut()
                    .spawn((
                        marker,
                        binding,
                        bogie.unwrap(),
                        Transform::IDENTITY,
                        ChildOf(car),
                    ))
                    .id(),
            );
        }
        app.update();
        assert!(
            app.world()
                .get::<Transform>(entities[0])
                .unwrap()
                .rotation
                .dot(Quat::IDENTITY)
                .abs()
                < 0.999
        );
        assert_eq!(
            app.world().get::<Transform>(entities[1]).unwrap().rotation,
            Quat::IDENTITY
        );
    }

    #[test]
    fn rest_baked_wheel_rotation_keeps_authored_pivot_fixed() {
        let mut shape = shape_with_named_matrix("WHEELS1");
        shape.matrices[0].matrix.rows[3] = [3.0, 2.0, 7.0];
        let rest = static_hierarchy_chain_transform(&shape, 0);
        let pivot = rest.translation;
        let delta = baked_part_local_rotation(&shape, 0, Quat::from_rotation_x(0.8));

        assert!(
            delta.transform_point(pivot).distance(pivot) < 1e-4,
            "wheel animation moved its authored pivot: {pivot:?} -> {:?}",
            delta.transform_point(pivot)
        );
        assert!(
            delta.rotation.dot(Quat::IDENTITY).abs() < 0.999,
            "wheel must still rotate around the retained pivot"
        );
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
    fn three_cars_on_elbow_are_not_colinear_in_world() {
        // #128: per-car chainage → lead and rear have distinct yaw / non-colinear positions.
        let g = elbow_graph();
        let scene = TrackScene::from_graph(g.clone());
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let origin = FloatingOrigin::default();
        // Head just past the elbow on e2; followers still on e1 → distinct yaw.
        let offsets = [0.0_f64, -40.0, -80.0];
        let mut poses = Vec::new();
        for &off in &offsets {
            let pose = car_world_pose_at_head_offset(
                &g,
                None,
                "e2",
                20.0,
                off,
                false,
                None,
                &scene,
                Vec3::ZERO,
                &focus,
                None,
                &origin,
            )
            .expect("car pose");
            poses.push(pose);
        }
        let yaw = |t: &Transform| t.rotation.to_euler(EulerRot::YXZ).0;
        assert!(
            (yaw(&poses[0]) - yaw(&poses[2])).abs() > 0.2,
            "lead vs rear yaw should differ on elbow, got {} vs {}",
            yaw(&poses[0]),
            yaw(&poses[2])
        );
        let v01 = (poses[1].translation - poses[0].translation).normalize_or_zero();
        let v12 = (poses[2].translation - poses[1].translation).normalize_or_zero();
        let colinear = v01.dot(v12).abs();
        assert!(
            colinear < 0.98,
            "three cars on a curve must not stay colinear (dot={colinear})"
        );
        let parent = poses[0];
        let local = car_local_from_parent_and_world(&parent, &poses[2]);
        let rebuilt = parent * local;
        assert!(rebuilt.translation.distance(poses[2].translation) < 1e-3);
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
        let bundle = part_anim_bundle(&Arc::new(shape.clone()), 0, 0.5).expect("door");
        assert_eq!(bundle.1, RollingStockPartKind::Door);
        let keyed = bundle.5.expect("keyed");
        assert_eq!(keyed.matrix_idx, 0);
        assert!(keyed.key.is_finite());
    }

    #[test]
    fn door_state_maps_to_key() {
        use openrailsrs_sim::DoorState;
        let mut ext = RollingStockExteriorState::new();
        assert_eq!(
            key_from_frac(frac_for_kind(RollingStockPartKind::Door, &ext), 11.0),
            0.0
        );
        ext.set_door(DoorState::Opening);
        assert_eq!(
            key_from_frac(frac_for_kind(RollingStockPartKind::Door, &ext), 11.0),
            10.0
        );
        ext.set_door(DoorState::Open);
        assert_eq!(
            key_from_frac(frac_for_kind(RollingStockPartKind::Door, &ext), 11.0),
            10.0
        );
        ext.set_door(DoorState::Closing);
        assert_eq!(
            key_from_frac(frac_for_kind(RollingStockPartKind::Door, &ext), 11.0),
            0.0
        );
    }

    #[test]
    fn panto_command_maps_to_key() {
        let mut ext = RollingStockExteriorState::new();
        assert_eq!(
            key_from_frac(frac_for_kind(RollingStockPartKind::Pantograph, &ext), 5.0),
            0.0
        );
        ext.set_pantograph_up(true);
        assert_eq!(
            key_from_frac(frac_for_kind(RollingStockPartKind::Pantograph, &ext), 5.0),
            4.0
        );
    }

    #[test]
    fn resolve_keyed_frac_uses_exterior_without_env() {
        use openrailsrs_sim::DoorState;
        let mut ext = RollingStockExteriorState::new();
        ext.set_door(DoorState::Open);
        assert_eq!(
            resolve_keyed_frac(RollingStockPartKind::Door, Some(&ext)),
            1.0
        );
        assert_eq!(resolve_keyed_frac(RollingStockPartKind::Door, None), 0.0);
    }
}
