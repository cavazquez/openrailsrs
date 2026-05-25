//! Free 3D cameras: orbit around a focus point and FPS-style fly.
//!
//! The same entity carries [`OrbitState`] and [`FlyState`] components. The
//! active mode is selected by the [`CameraMode`] resource (toggled with `F1`
//! for orbit and `F2` for fly). When switching modes, state is synced so the
//! viewport doesn't jump.
//!
//! Math helpers ([`orbit_position`], [`fly_translation_delta`]) are pure
//! functions and unit-tested without spinning up Bevy.

use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
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

/// Keyboard pan speed as a fraction of orbit distance per second.
const ORBIT_KEY_PAN_SPEED: f32 = 0.85;

/// Min distance for the orbit camera (m).
const ORBIT_MIN_DISTANCE: f32 = 1.0;

/// Default max distance before a route-specific limit is applied at startup.
const ORBIT_DEFAULT_MAX_DISTANCE: f32 = 500.0;

/// Upper cap for orbit zoom-out on very large imported routes (m).
const ORBIT_ABSOLUTE_MAX_DISTANCE: f32 = 500_000.0;

/// Scene-specific orbit distance limit (updated when framing a loaded route).
#[derive(Resource, Clone, Copy, Debug)]
pub struct OrbitDistanceLimit {
    pub max: f32,
}

impl Default for OrbitDistanceLimit {
    fn default() -> Self {
        Self {
            max: ORBIT_DEFAULT_MAX_DISTANCE,
        }
    }
}

/// Sensitivity (rad per pixel) for fly mouselook.
const FLY_LOOK_SENSITIVITY: f32 = 0.002;

/// Default fly speed (m/s). Multiplied by 4 with Shift, divided by 4 with
/// Ctrl, so effective range ≈ 2.5 .. 40 m/s.
const FLY_BASE_SPEED: f32 = 10.0;

/// Orbit focus lerp speed when following the train (1/s).
const FOLLOW_LERP_SPEED: f32 = 8.0;

/// Fixed pitch (rad) for chase camera behind the train.
const CHASE_PITCH: f32 = 0.5;

/// Minimum orbit distance while following (avoids clipping into the marker).
const FOLLOW_MIN_DISTANCE: f32 = 80.0;

// ── Components / resources ────────────────────────────────────────────────

/// Train-tracking camera behaviour (cycle with `T` during replay).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraFollowMode {
    #[default]
    Off,
    OrbitFollow,
    ChaseCam,
}

impl CameraFollowMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::OrbitFollow,
            Self::OrbitFollow => Self::ChaseCam,
            Self::ChaseCam => Self::Off,
        }
    }

    pub fn hud_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::OrbitFollow => "orbit",
            Self::ChaseCam => "chase",
        }
    }
}

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

/// Clamp the orbit distance using the default sandbox limit.
#[inline]
pub fn clamp_distance(distance: f32) -> f32 {
    clamp_distance_to_limit(distance, ORBIT_DEFAULT_MAX_DISTANCE)
}

/// Clamp `distance` to `[ORBIT_MIN_DISTANCE, max]`.
#[inline]
pub fn clamp_distance_to_limit(distance: f32, max: f32) -> f32 {
    let max = max.clamp(ORBIT_MIN_DISTANCE, ORBIT_ABSOLUTE_MAX_DISTANCE);
    distance.clamp(ORBIT_MIN_DISTANCE, max)
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

/// Lerp orbit focus toward a target world position (exponential smoothing).
pub fn lerp_follow_focus(focus: Vec3, target: Vec3, dt: f32) -> Vec3 {
    let t = (FOLLOW_LERP_SPEED * dt).clamp(0.0, 1.0);
    focus.lerp(target, t)
}

/// Shortest-path lerp between two yaw angles (radians).
pub fn lerp_yaw_toward(current: f32, target: f32, dt: f32) -> f32 {
    let t = (FOLLOW_LERP_SPEED * dt).clamp(0.0, 1.0);
    let delta = (target - current).rem_euclid(std::f32::consts::TAU);
    let delta = if delta > std::f32::consts::PI {
        delta - std::f32::consts::TAU
    } else {
        delta
    };
    current + delta * t
}

/// Yaw for a chase camera sitting behind the train (looks toward +travel).
#[inline]
pub fn chase_yaw_from_train(train_yaw: f32) -> f32 {
    train_yaw + std::f32::consts::PI
}

/// Extract yaw (rotation around Y) from a train marker transform.
pub fn yaw_from_transform(transform: &Transform) -> f32 {
    let fwd = transform.forward();
    (-fwd.x).atan2(-fwd.z)
}

/// Train pose inputs for [`apply_orbit_follow`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrainFollowPose {
    pub translation: Vec3,
    pub yaw: f32,
}

/// Orbit camera state after one follow update (before building the view transform).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitFollowUpdate {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

/// Apply orbit/chase follow smoothing for one frame (pure, unit-tested).
pub fn apply_orbit_follow(
    orbit: OrbitState,
    follow: CameraFollowMode,
    train: TrainFollowPose,
    dt: f32,
) -> OrbitFollowUpdate {
    let target_focus = Vec3::new(train.translation.x, 0.0, train.translation.z);
    let focus = lerp_follow_focus(orbit.focus, target_focus, dt);
    let mut yaw = orbit.yaw;
    let mut pitch = orbit.pitch;
    let mut distance = orbit.distance;

    if follow == CameraFollowMode::ChaseCam {
        yaw = lerp_yaw_toward(yaw, chase_yaw_from_train(train.yaw), dt);
        pitch = lerp_yaw_toward(pitch, CHASE_PITCH, dt);
    }

    if distance < FOLLOW_MIN_DISTANCE {
        distance = FOLLOW_MIN_DISTANCE;
    }

    OrbitFollowUpdate {
        focus,
        yaw,
        pitch,
        distance,
    }
}

/// Build the camera transform from a follow/orbit state snapshot.
pub fn camera_transform_from_orbit(update: OrbitFollowUpdate) -> Transform {
    camera_transform_from_orbit_state(update.focus, update.yaw, update.pitch, update.distance)
}

/// Build the camera transform from raw orbit parameters.
pub fn camera_transform_from_orbit_state(
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
) -> Transform {
    let pos = orbit_position(focus, yaw, pitch, distance);
    Transform::from_translation(pos).looking_at(focus, Vec3::Y)
}

// ── Systems (Bevy) ────────────────────────────────────────────────────────

pub fn spawn_camera(mut commands: Commands) {
    let orbit = OrbitState::default();
    let transform =
        camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);

    commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
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

#[allow(clippy::type_complexity)]
pub fn toggle_mode_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<CameraMode>,
    mut query: Query<
        (&Transform, &mut OrbitState, &mut FlyState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
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

pub fn cycle_follow_mode(
    keys: Res<ButtonInput<KeyCode>>,
    replay: Option<Res<crate::train::ReplayState>>,
    mut follow: ResMut<CameraFollowMode>,
) {
    if !keys.just_pressed(KeyCode::KeyT) {
        return;
    }
    if replay.as_ref().is_some_and(|r| r.is_active()) {
        *follow = follow.cycle();
    }
}

#[allow(clippy::type_complexity)]
pub fn follow_train_camera(
    time: Res<Time>,
    mode: Res<CameraMode>,
    follow: Res<CameraFollowMode>,
    replay: Option<Res<crate::train::ReplayState>>,
    train_query: Query<(&Transform, &crate::train::TrainMarker), Without<OrbitState>>,
    mut orbit_query: Query<
        (&mut Transform, &mut OrbitState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) {
    if *mode != CameraMode::Orbit || *follow == CameraFollowMode::Off {
        return;
    }
    if !replay.as_ref().is_some_and(|r| r.is_active()) {
        return;
    }

    let Some(train_tf) = train_query
        .iter()
        .find(|(_, marker)| marker.track_index == 0)
        .map(|(tf, _)| tf)
    else {
        return;
    };

    let Ok((mut transform, mut orbit)) = orbit_query.single_mut() else {
        return;
    };

    let dt = time.delta_secs();
    let update = apply_orbit_follow(
        *orbit,
        *follow,
        TrainFollowPose {
            translation: train_tf.translation,
            yaw: yaw_from_transform(train_tf),
        },
        dt,
    );
    orbit.focus = update.focus;
    orbit.yaw = update.yaw;
    orbit.pitch = update.pitch;
    orbit.distance = update.distance;
    *transform = camera_transform_from_orbit(update);
}

fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

fn read_orbit_pan_axes(keys: &ButtonInput<KeyCode>) -> Vec3 {
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
    if keys.pressed(KeyCode::KeyE) {
        axes.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyQ) {
        axes.y -= 1.0;
    }
    axes
}

fn pan_orbit_focus(orbit: &mut OrbitState, transform: &Transform, delta: Vec2) {
    let right = transform.right().as_vec3();
    let up = transform.up().as_vec3();
    let scale = orbit.distance * ORBIT_PAN_SENSITIVITY;
    orbit.focus += -right * (delta.x * scale) + up * (delta.y * scale);
}

fn keyboard_pan_orbit_focus(orbit: &mut OrbitState, axes: Vec3, dt: f32) {
    let forward_h = Vec3::new(-orbit.yaw.sin(), 0.0, -orbit.yaw.cos());
    let right_h = Vec3::new(orbit.yaw.cos(), 0.0, -orbit.yaw.sin());
    let mut speed = orbit.distance * ORBIT_KEY_PAN_SPEED * dt;
    if axes.length_squared() > 1.0 {
        speed /= axes.length();
    }
    orbit.focus += forward_h * axes.z * speed + right_h * axes.x * speed + Vec3::Y * axes.y * speed;
}

fn wheel_scroll_lines(ev: &MouseWheel) -> f32 {
    match ev.unit {
        MouseScrollUnit::Line => ev.y,
        MouseScrollUnit::Pixel => ev.y / 100.0,
    }
}

#[allow(clippy::type_complexity)]
pub fn orbit_camera_system(
    time: Res<Time>,
    mode: Res<CameraMode>,
    limit: Res<OrbitDistanceLimit>,
    keys: Res<ButtonInput<KeyCode>>,
    mut follow: ResMut<CameraFollowMode>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut query: Query<
        (&mut Transform, &mut OrbitState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) {
    if *mode != CameraMode::Orbit {
        motion.clear();
        wheel.clear();
        return;
    }

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
        scroll += wheel_scroll_lines(ev);
    }

    let mut changed = false;
    let shift = shift_held(&keys);
    let drag_rotate = (mouse_buttons.pressed(MouseButton::Left)
        || mouse_buttons.pressed(MouseButton::Right))
        && !shift;
    let drag_pan = mouse_buttons.pressed(MouseButton::Middle)
        || (shift
            && (mouse_buttons.pressed(MouseButton::Left)
                || mouse_buttons.pressed(MouseButton::Right)));

    if drag_rotate && delta != Vec2::ZERO {
        orbit.yaw -= delta.x * ORBIT_ROTATE_SENSITIVITY;
        orbit.pitch = clamp_pitch(orbit.pitch - delta.y * ORBIT_ROTATE_SENSITIVITY);
        changed = true;
    }

    if drag_pan && delta != Vec2::ZERO {
        *follow = CameraFollowMode::Off;
        pan_orbit_focus(&mut orbit, &transform, delta);
        changed = true;
    }

    let pan_axes = read_orbit_pan_axes(&keys);
    if pan_axes != Vec3::ZERO {
        *follow = CameraFollowMode::Off;
        keyboard_pan_orbit_focus(&mut orbit, pan_axes, time.delta_secs());
        changed = true;
    }

    if scroll != 0.0 {
        orbit.distance =
            clamp_distance_to_limit(orbit.distance * (1.0 - scroll * ORBIT_ZOOM_STEP), limit.max);
        changed = true;
    }

    if changed {
        *transform =
            camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);
    }
}

#[allow(clippy::type_complexity)]
pub fn fly_camera_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    replay: Option<Res<crate::train::ReplayState>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut query: Query<
        (&mut Transform, &mut FlyState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
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

    let axes = read_fly_axes(&keys, replay.as_deref());
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

fn replay_blocks_space(replay: Option<&crate::train::ReplayState>) -> bool {
    replay.is_some_and(|r| r.is_active())
}

fn read_fly_axes(keys: &ButtonInput<KeyCode>, replay: Option<&crate::train::ReplayState>) -> Vec3 {
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
    if keys.pressed(KeyCode::KeyE) {
        axes.y += 1.0;
    }
    if keys.pressed(KeyCode::Space) && !replay_blocks_space(replay) {
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
    use crate::train::{CsvRow, ReplayState, TrainTrack};
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
        assert!((clamp_distance(1e9) - ORBIT_DEFAULT_MAX_DISTANCE).abs() < 1e-6);
        assert!((clamp_distance(50.0) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn clamp_distance_to_limit_allows_large_routes() {
        let d = clamp_distance_to_limit(120_000.0, 150_000.0);
        assert!((d - 120_000.0).abs() < 1e-3);
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

    #[test]
    fn lerp_follow_focus_moves_toward_target() {
        let focus = Vec3::ZERO;
        let target = Vec3::new(100.0, 0.0, 0.0);
        let next = lerp_follow_focus(focus, target, 0.05);
        assert!(next.x > 0.0 && next.x < 100.0);
    }

    #[test]
    fn chase_yaw_from_train_is_opposite_travel() {
        let yaw = chase_yaw_from_train(0.0);
        assert!((yaw - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn camera_follow_mode_cycles() {
        assert_eq!(CameraFollowMode::Off.cycle(), CameraFollowMode::OrbitFollow);
        assert_eq!(
            CameraFollowMode::OrbitFollow.cycle(),
            CameraFollowMode::ChaseCam
        );
        assert_eq!(CameraFollowMode::ChaseCam.cycle(), CameraFollowMode::Off);
    }

    #[test]
    fn camera_follow_mode_hud_labels() {
        assert_eq!(CameraFollowMode::Off.hud_label(), "off");
        assert_eq!(CameraFollowMode::OrbitFollow.hud_label(), "orbit");
        assert_eq!(CameraFollowMode::ChaseCam.hud_label(), "chase");
    }

    #[test]
    fn lerp_yaw_toward_shortest_path() {
        let current = 0.1;
        let target = std::f32::consts::PI - 0.1;
        let next = lerp_yaw_toward(current, target, 0.05);
        assert!(next > current);
        assert!(next <= target);
    }

    #[test]
    fn yaw_from_transform_matches_forward() {
        let tf = Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let yaw = yaw_from_transform(&tf);
        assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn apply_orbit_follow_moves_focus_toward_train() {
        let orbit = OrbitState::default();
        let train = TrainFollowPose {
            translation: Vec3::new(200.0, 5.0, 100.0),
            yaw: 0.0,
        };
        let update = apply_orbit_follow(orbit, CameraFollowMode::OrbitFollow, train, 0.05);
        assert!(update.focus.x > 0.0 && update.focus.x < 200.0);
        assert_eq!(update.focus.y, 0.0);
        assert!(update.focus.z > 0.0 && update.focus.z < 100.0);
    }

    #[test]
    fn apply_orbit_follow_chase_adjusts_yaw_and_pitch() {
        let orbit = OrbitState {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        };
        let train = TrainFollowPose {
            translation: Vec3::new(50.0, 0.0, 0.0),
            yaw: 0.0,
        };
        let update = apply_orbit_follow(orbit, CameraFollowMode::ChaseCam, train, 0.5);
        assert!(update.yaw > 0.0);
        assert!(update.pitch > 0.0);
    }

    #[test]
    fn apply_orbit_follow_clamps_min_distance() {
        let orbit = OrbitState {
            distance: 10.0,
            ..Default::default()
        };
        let train = TrainFollowPose {
            translation: Vec3::ZERO,
            yaw: 0.0,
        };
        let update = apply_orbit_follow(orbit, CameraFollowMode::OrbitFollow, train, 0.016);
        assert!((update.distance - FOLLOW_MIN_DISTANCE).abs() < 1e-5);
    }

    #[test]
    fn camera_transform_from_orbit_looks_at_focus() {
        let update = OrbitFollowUpdate {
            focus: Vec3::new(10.0, 0.0, 10.0),
            yaw: 0.0,
            pitch: 0.3,
            distance: 50.0,
        };
        let tf = camera_transform_from_orbit(update);
        let fwd = tf.forward().as_vec3();
        let to_focus = (update.focus - tf.translation).normalize();
        assert!((fwd - to_focus).length() < 0.05);
    }

    #[test]
    fn read_fly_axes_wasd_and_qe() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyD);
        keys.press(KeyCode::KeyQ);
        let axes = read_fly_axes(&keys, None);
        assert_eq!(axes, Vec3::new(1.0, -1.0, 1.0));
    }

    #[test]
    fn read_fly_axes_space_blocked_during_replay() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Space);
        let replay = ReplayState::new(
            "x".into(),
            vec![TrainTrack {
                label: "t".into(),
                color: Color::WHITE,
                rows: vec![CsvRow {
                    time_s: 0.0,
                    velocity_mps: 0.0,
                    edge_id: String::new(),
                    pos_on_edge_m: 0.0,
                }],
            }],
        );
        let axes = read_fly_axes(&keys, Some(&replay));
        assert_eq!(axes.y, 0.0);
    }
}
