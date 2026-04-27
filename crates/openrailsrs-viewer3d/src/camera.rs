//! Free 3D cameras: orbit around a focus point and FPS-style fly.
//!
//! The same entity carries [`OrbitState`] and [`FlyState`] components. The
//! active mode is selected by the [`CameraMode`] resource (toggled with `F1`
//! for orbit and `F2` for fly). When switching modes, state is synced so the
//! viewport doesn't jump.
//!
//! Math helpers ([`orbit_position`], [`fly_translation_delta`]) are pure
//! functions and unit-tested without spinning up Bevy.

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

// ── Tunables ───────────────────────────────────────────────────────────────

/// Maximum allowed |pitch| for orbit/fly cameras (rad). Just under π/2 to
/// avoid gimbal flip when looking straight up/down.
pub const MAX_PITCH: f32 = 1.5;

/// Sensitivity (rad per pixel) for orbit rotate (right mouse drag).
const ORBIT_ROTATE_SENSITIVITY: f32 = 0.005;

/// Sensitivity (world units per pixel × distance) for orbit pan (middle drag).
const ORBIT_PAN_SENSITIVITY: f32 = 0.0015;

/// Per-notch zoom factor for the orbit camera (1 + step). 0.1 = 10 % per tick.
const ORBIT_ZOOM_STEP: f32 = 0.1;

/// Min / max distance for the orbit camera (m).
const ORBIT_MIN_DISTANCE: f32 = 1.0;
const ORBIT_MAX_DISTANCE: f32 = 500.0;

/// Sensitivity (rad per pixel) for fly mouselook.
const FLY_LOOK_SENSITIVITY: f32 = 0.002;

/// Default fly speed (m/s). Multiplied by 4 with Shift, divided by 4 with
/// Ctrl, so effective range ≈ 2.5 .. 40 m/s.
const FLY_BASE_SPEED: f32 = 10.0;

// ── Components / resources ────────────────────────────────────────────────

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    #[default]
    Orbit,
    Fly,
}

/// State for the orbit camera: spherical coordinates around a focus point.
#[derive(Component, Clone, Copy, Debug)]
pub struct OrbitState {
    pub focus: Vec3,
    /// Yaw in radians (rotation around world Y).
    pub yaw: f32,
    /// Pitch in radians, clamped to ±[`MAX_PITCH`].
    pub pitch: f32,
    /// Distance from `focus` to the camera (m), clamped to
    /// [`ORBIT_MIN_DISTANCE`]..[`ORBIT_MAX_DISTANCE`].
    pub distance: f32,
}

impl Default for OrbitState {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: 0.7,
            pitch: 0.6,
            distance: 50.0,
        }
    }
}

/// State for the fly (FPS) camera.
#[derive(Component, Clone, Copy, Debug)]
pub struct FlyState {
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for FlyState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

// ── Pure math (unit-tested) ───────────────────────────────────────────────

/// World position of the camera given a focus point and spherical
/// coordinates. With `yaw = 0`, `pitch = 0` the camera sits at
/// `focus + (0, 0, distance)`. Positive yaw rotates around +Y; positive
/// pitch lifts the camera over the focus.
pub fn orbit_position(focus: Vec3, yaw: f32, pitch: f32, distance: f32) -> Vec3 {
    let cp = pitch.cos();
    let offset = Vec3::new(
        distance * cp * yaw.sin(),
        distance * pitch.sin(),
        distance * cp * yaw.cos(),
    );
    focus + offset
}

/// Clamp pitch to ±[`MAX_PITCH`].
#[inline]
pub fn clamp_pitch(pitch: f32) -> f32 {
    pitch.clamp(-MAX_PITCH, MAX_PITCH)
}

/// Clamp the orbit distance.
#[inline]
pub fn clamp_distance(distance: f32) -> f32 {
    distance.clamp(ORBIT_MIN_DISTANCE, ORBIT_MAX_DISTANCE)
}

/// Direction vector for a camera looking with the given yaw/pitch.
/// `yaw = 0`, `pitch = 0` looks toward `-Z`.
pub fn fly_forward(yaw: f32, pitch: f32) -> Vec3 {
    let cp = pitch.cos();
    Vec3::new(-cp * yaw.sin(), pitch.sin(), -cp * yaw.cos()).normalize_or_zero()
}

/// Per-frame translation delta (in world space) for the fly camera.
///
/// `axes` is `(right, up_local, forward)` in [-1, 1]; `right` and `forward`
/// move along the camera's horizontal plane, `up_local` is along world +Y.
pub fn fly_translation_delta(yaw: f32, axes: Vec3, speed: f32, dt: f32) -> Vec3 {
    let forward_h = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right_h = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let up = Vec3::Y;
    (forward_h * axes.z + right_h * axes.x + up * axes.y) * speed * dt
}

// ── Systems (Bevy) ────────────────────────────────────────────────────────

pub fn spawn_camera(mut commands: Commands) {
    let orbit = OrbitState::default();
    let pos = orbit_position(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);
    let transform = Transform::from_translation(pos).looking_at(orbit.focus, Vec3::Y);

    commands.spawn((
        Camera3d::default(),
        transform,
        orbit,
        FlyState::default(),
        // In Bevy 0.18 `AmbientLight` is a per-camera component; attaching
        // it here makes the scene legible without an extra fill light.
        AmbientLight {
            color: Color::srgb(1.0, 1.0, 1.0),
            brightness: 200.0,
            ..default()
        },
        Name::new("viewer-camera"),
    ));
}

pub fn toggle_mode_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<CameraMode>,
    mut query: Query<(&Transform, &mut OrbitState, &mut FlyState)>,
) {
    let want_orbit = keys.just_pressed(KeyCode::F1);
    let want_fly = keys.just_pressed(KeyCode::F2);
    if !want_orbit && !want_fly {
        return;
    }

    let new_mode = if want_orbit {
        CameraMode::Orbit
    } else {
        CameraMode::Fly
    };
    if new_mode == *mode {
        return;
    }

    if let Ok((transform, mut orbit, mut fly)) = query.single_mut() {
        match new_mode {
            CameraMode::Fly => {
                let fwd = transform.forward().as_vec3();
                fly.pitch = clamp_pitch(fwd.y.asin());
                fly.yaw = (-fwd.x).atan2(-fwd.z);
            }
            CameraMode::Orbit => {
                let fwd = transform.forward().as_vec3();
                orbit.focus = transform.translation + fwd * orbit.distance;
                orbit.pitch = clamp_pitch(fwd.y.asin());
                orbit.yaw = (-fwd.x).atan2(-fwd.z);
            }
        }
    }

    *mode = new_mode;
}

pub fn orbit_camera_system(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut query: Query<(&mut Transform, &mut OrbitState)>,
) {
    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        motion.clear();
        wheel.clear();
        return;
    };

    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }

    let mut scroll = 0.0_f32;
    for ev in wheel.read() {
        scroll += ev.y;
    }

    let mut changed = false;

    if mouse_buttons.pressed(MouseButton::Right) && delta != Vec2::ZERO {
        orbit.yaw -= delta.x * ORBIT_ROTATE_SENSITIVITY;
        orbit.pitch = clamp_pitch(orbit.pitch - delta.y * ORBIT_ROTATE_SENSITIVITY);
        changed = true;
    }

    if mouse_buttons.pressed(MouseButton::Middle) && delta != Vec2::ZERO {
        let right = transform.right().as_vec3();
        let up = transform.up().as_vec3();
        let scale = orbit.distance * ORBIT_PAN_SENSITIVITY;
        orbit.focus += -right * (delta.x * scale) + up * (delta.y * scale);
        changed = true;
    }

    if scroll != 0.0 {
        orbit.distance = clamp_distance(orbit.distance * (1.0 - scroll * ORBIT_ZOOM_STEP));
        changed = true;
    }

    if changed {
        let pos = orbit_position(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);
        *transform = Transform::from_translation(pos).looking_at(orbit.focus, Vec3::Y);
    }
}

pub fn fly_camera_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut query: Query<(&mut Transform, &mut FlyState)>,
) {
    let Ok((mut transform, mut fly)) = query.single_mut() else {
        motion.clear();
        return;
    };

    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }

    if mouse_buttons.pressed(MouseButton::Right) && delta != Vec2::ZERO {
        fly.yaw -= delta.x * FLY_LOOK_SENSITIVITY;
        fly.pitch = clamp_pitch(fly.pitch - delta.y * FLY_LOOK_SENSITIVITY);
    }

    let axes = read_fly_axes(&keys);
    let mut speed = FLY_BASE_SPEED;
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        speed *= 4.0;
    }
    if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        speed *= 0.25;
    }

    if axes != Vec3::ZERO {
        transform.translation += fly_translation_delta(fly.yaw, axes, speed, time.delta_secs());
    }

    transform.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
}

fn read_fly_axes(keys: &ButtonInput<KeyCode>) -> Vec3 {
    let mut axes = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        axes.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        axes.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        axes.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        axes.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) {
        axes.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyQ) {
        axes.y -= 1.0;
    }
    axes
}

// ── Run conditions ────────────────────────────────────────────────────────

pub fn in_orbit_mode(mode: Res<CameraMode>) -> bool {
    *mode == CameraMode::Orbit
}

pub fn in_fly_mode(mode: Res<CameraMode>) -> bool {
    *mode == CameraMode::Fly
}

/// While in fly mode, hide the cursor and confine it to the window during
/// right-button mouselook; restore defaults in orbit mode or when the
/// button is released.
pub fn update_primary_window_cursor(
    mode: Res<CameraMode>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut q: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = q.single_mut() else {
        return;
    };
    match *mode {
        CameraMode::Orbit => {
            cursor.visible = true;
            cursor.grab_mode = CursorGrabMode::None;
        }
        CameraMode::Fly => {
            if mouse_buttons.pressed(MouseButton::Right) {
                cursor.visible = false;
                cursor.grab_mode = CursorGrabMode::Confined;
            } else {
                cursor.visible = true;
                cursor.grab_mode = CursorGrabMode::None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn vec3_close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).length() < eps
    }

    #[test]
    fn orbit_position_yaw_zero_pitch_zero_sits_on_plus_z() {
        let pos = orbit_position(Vec3::ZERO, 0.0, 0.0, 10.0);
        assert!(vec3_close(pos, Vec3::new(0.0, 0.0, 10.0), 1e-5));
    }

    #[test]
    fn orbit_position_pitch_pi_over_two_sits_above() {
        let pos = orbit_position(Vec3::new(1.0, 2.0, 3.0), 0.0, FRAC_PI_2, 5.0);
        assert!(vec3_close(pos, Vec3::new(1.0, 7.0, 3.0), 1e-5));
    }

    #[test]
    fn orbit_position_yaw_pi_over_two_sits_on_plus_x() {
        let pos = orbit_position(Vec3::ZERO, FRAC_PI_2, 0.0, 4.0);
        assert!(vec3_close(pos, Vec3::new(4.0, 0.0, 0.0), 1e-5));
    }

    #[test]
    fn orbit_position_respects_focus_offset() {
        let focus = Vec3::new(10.0, -5.0, 2.5);
        let pos = orbit_position(focus, 0.0, 0.0, 0.0);
        assert!(vec3_close(pos, focus, 1e-5));
    }

    #[test]
    fn clamp_pitch_inside_range_unchanged() {
        assert!((clamp_pitch(0.0) - 0.0).abs() < 1e-6);
        assert!((clamp_pitch(1.0) - 1.0).abs() < 1e-6);
        assert!((clamp_pitch(-1.0) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_pitch_clamps_to_max_pitch() {
        assert!((clamp_pitch(10.0) - MAX_PITCH).abs() < 1e-6);
        assert!((clamp_pitch(-10.0) + MAX_PITCH).abs() < 1e-6);
    }

    #[test]
    fn clamp_distance_clamps_to_bounds() {
        assert!((clamp_distance(0.0) - ORBIT_MIN_DISTANCE).abs() < 1e-6);
        assert!((clamp_distance(1e9) - ORBIT_MAX_DISTANCE).abs() < 1e-6);
        assert!((clamp_distance(50.0) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fly_forward_yaw_zero_pitch_zero_points_to_minus_z() {
        let fwd = fly_forward(0.0, 0.0);
        assert!(vec3_close(fwd, Vec3::new(0.0, 0.0, -1.0), 1e-5));
    }

    #[test]
    fn fly_forward_yaw_pi_over_two_points_to_minus_x() {
        let fwd = fly_forward(FRAC_PI_2, 0.0);
        assert!(vec3_close(fwd, Vec3::new(-1.0, 0.0, 0.0), 1e-5));
    }

    #[test]
    fn fly_translation_delta_forward_w_at_yaw_zero_moves_minus_z() {
        let d = fly_translation_delta(0.0, Vec3::new(0.0, 0.0, 1.0), 10.0, 0.5);
        assert!(vec3_close(d, Vec3::new(0.0, 0.0, -5.0), 1e-5));
    }

    #[test]
    fn fly_translation_delta_strafe_d_at_yaw_zero_moves_plus_x() {
        let d = fly_translation_delta(0.0, Vec3::new(1.0, 0.0, 0.0), 10.0, 0.5);
        assert!(vec3_close(d, Vec3::new(5.0, 0.0, 0.0), 1e-5));
    }

    #[test]
    fn fly_translation_delta_up_axis_is_world_y() {
        let d = fly_translation_delta(1.234, Vec3::new(0.0, 1.0, 0.0), 10.0, 0.5);
        assert!(vec3_close(d, Vec3::new(0.0, 5.0, 0.0), 1e-5));
    }

    #[test]
    fn fly_translation_delta_zero_axes_is_zero() {
        let d = fly_translation_delta(0.7, Vec3::ZERO, 100.0, 1.0);
        assert!(vec3_close(d, Vec3::ZERO, 1e-5));
    }

    #[test]
    fn fly_translation_delta_yaw_pi_over_two_forward_moves_minus_x() {
        let d = fly_translation_delta(FRAC_PI_2, Vec3::new(0.0, 0.0, 1.0), 8.0, 1.0);
        assert!(vec3_close(d, Vec3::new(-8.0, 0.0, 0.0), 1e-5));
    }
}
