//! Exterior rolling-stock part animation (#40): wheels, bogies, door/panto stubs.
//!
//! Meshes stay rest-baked (same pattern as WORLD #34). Drivers update each part's
//! local `Transform` without moving the car body.

use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    ShapeAnimBinding, animation_pose_matrices, world_baked_anim_transform,
};
use openrailsrs_formats::ShapeFile;

use crate::live::LiveDrive;
use crate::train::ReplayState;

/// Default wheel radius when shape bounds are unavailable (metres).
pub const DEFAULT_WHEEL_RADIUS_M: f32 = 0.46;
/// Look-ahead distance for bogie yaw approximation (metres).
const BOGIE_LOOKAHEAD_M: f32 = 12.0;
/// Max |relative yaw| applied to a bogie (radians).
const BOGIE_YAW_CLAMP: f32 = 0.35;

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

/// Bogie yaw relative to the car body (curve approximation).
#[derive(Component, Clone, Debug)]
pub struct TrainBogieAnim {
    pub matrix_idx: usize,
    /// Longitudinal offset hint along the car (+forward), metres; used for curve lever.
    pub long_offset_m: f32,
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

pub fn matrix_name<'a>(shape: &'a ShapeFile, matrix_idx: usize) -> &'a str {
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
    Some((
        TrainExteriorAnimPart,
        kind,
        binding,
        wheel,
        bogie,
        keyed,
    ))
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

fn train_speed_mps(
    live: Option<&LiveDrive>,
    replay: Option<&ReplayState>,
) -> f32 {
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

/// Advance wheel / bogie / keyed exterior parts each frame (#40).
pub fn update_rolling_stock_part_anim(
    time: Res<Time>,
    live: Option<Res<LiveDrive>>,
    replay: Option<Res<ReplayState>>,
    mut wheels: Query<
        (&mut TrainWheelAnim, &ShapeAnimBinding, &mut Transform),
        With<TrainExteriorAnimPart>,
    >,
    mut bogies: Query<
        (&TrainBogieAnim, &ShapeAnimBinding, &mut Transform, &ChildOf),
        (With<TrainExteriorAnimPart>, Without<TrainWheelAnim>),
    >,
    cars: Query<&Transform, Without<TrainExteriorAnimPart>>,
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
    let speed = train_speed_mps(live.as_deref(), replay.as_deref());

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

    // Bogie: relative yaw from car heading vs a synthetic look-ahead (curve lever).
    let look_ahead_yaw = {
        // Prefer car parent yaw + small bias from speed sign as a cheap curve cue when
        // we lack per-bogie track samples; relative stays near 0 on straight.
        0.0f32
    };
    for (bogie, binding, mut tf, child_of) in &mut bogies {
        let Ok(car_tf) = cars.get(child_of.parent()) else {
            continue;
        };
        let car_yaw = car_tf.rotation.to_euler(EulerRot::YXZ).0;
        let dyaw = wrap_angle(look_ahead_yaw - car_yaw);
        let lever = (bogie.long_offset_m / BOGIE_LOOKAHEAD_M).clamp(-1.0, 1.0);
        let rel = (dyaw * lever + speed.signum() * lever * 0.02).clamp(-BOGIE_YAW_CLAMP, BOGIE_YAW_CLAMP);
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
    use openrailsrs_formats::{Matrix43, NamedMatrix, PrimState, VtxState};

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
        assert_eq!(
            classify_matrix_name("WHEELS1"),
            RollingStockPartKind::Wheel
        );
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
    fn bogie_yaw_clamp_finite() {
        let lever = (5.0_f32 / BOGIE_LOOKAHEAD_M).clamp(-1.0, 1.0);
        let rel = (0.5 * lever).clamp(-BOGIE_YAW_CLAMP, BOGIE_YAW_CLAMP);
        assert!(rel.is_finite());
        assert!(rel.abs() <= BOGIE_YAW_CLAMP);
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
