//! Free 3D cameras: orbit around a focus point and FPS-style fly.
//!
//! The same entity carries [`OrbitState`] and [`FlyState`] components. The
//! active mode is selected by the [`CameraMode`] resource (toggled with `F1`
//! for orbit and `F2` for fly). When switching modes, state is synced so the
//! viewport doesn't jump.
//!
//! Math helpers ([`orbit_position`], [`fly_translation_delta`]) are pure
//! functions and unit-tested without spinning up Bevy.

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::cab_render::camera_layers_outdoor;
use crate::launch::ViewerLaunchOpts;

/// Ambient fill for live drive / terrain routes (MSTS `.ace` albedos are often very dark).
pub const LIVE_OUTDOOR_AMBIENT: f32 = 15000.0;

/// Effective ambient fill (lux), overridable via `OPENRAILSRS_AMBIENT` for A/B tuning.
/// Open Rails lights its world fairly flat; a higher sky fill keeps sun-shadowed
/// faces from crushing to black under the physical sun.
pub fn live_outdoor_ambient() -> f32 {
    std::env::var("OPENRAILSRS_AMBIENT")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| *v >= 0.0)
        .unwrap_or(LIVE_OUTDOOR_AMBIENT)
}

/// Tonemapper for the live outdoor world. Open Rails tone-maps its HDR sun-lit scene;
/// `Tonemapping::None` clips highlights and crushes shadows. A neutral filmic curve
/// (TonyMcMapface) reframes the HDR for a more OR-like look. Override at runtime with
/// `OPENRAILSRS_TONEMAP=none|tony|agx|aces|reinhard|blender` to A/B compare.
pub fn live_tonemapping() -> Tonemapping {
    parse_tonemapping(std::env::var("OPENRAILSRS_TONEMAP").ok().as_deref())
}

/// Pure parser for [`live_tonemapping`] (unit-tested without env state).
fn parse_tonemapping(value: Option<&str>) -> Tonemapping {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("none") => Tonemapping::None,
        Some("agx") => Tonemapping::AgX,
        Some("aces" | "aces_fitted") => Tonemapping::AcesFitted,
        Some("reinhard") => Tonemapping::ReinhardLuminance,
        Some("blender" | "filmic" | "blender_filmic") => Tonemapping::BlenderFilmic,
        Some("boring" | "somewhat_boring") => Tonemapping::SomewhatBoringDisplayTransform,
        // Default (unset, empty, or "tony"/"tonymcmapface"): neutral filmic LUT.
        _ => Tonemapping::TonyMcMapface,
    }
}

// ── Tunables ───────────────────────────────────────────────────────────────

/// Maximum allowed |pitch| for orbit/fly cameras (rad). Just under π/2 to
/// avoid gimbal flip when looking straight up/down.
pub const MAX_PITCH: f32 = 1.5;

/// Sensitivity (rad per pixel) for orbit rotate (left mouse drag).
const ORBIT_ROTATE_SENSITIVITY: f32 = 0.005;

/// Fraction of orbit distance per pixel when right-dragging vertically (dolly zoom).
const ORBIT_RMB_DOLLY_SENSITIVITY: f32 = 0.004;

/// Sensitivity (world units per pixel × distance) for orbit pan (middle drag).
const ORBIT_PAN_SENSITIVITY: f32 = 0.0015;

/// Per-notch zoom factor for the orbit camera (1 + step). 0.1 = 10 % per tick.
const ORBIT_ZOOM_STEP: f32 = 0.1;

/// Keyboard pan speed as a fraction of orbit distance per second.
const ORBIT_KEY_PAN_SPEED: f32 = 0.85;

/// Min distance for the orbit camera (m).
const ORBIT_MIN_DISTANCE: f32 = 8.0;

/// Default max distance before a route-specific limit is applied at startup.
const ORBIT_DEFAULT_MAX_DISTANCE: f32 = 8_000.0;

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

/// Distance moved along the current view direction per mouse-wheel line in fly mode.
const FLY_WHEEL_DOLLY_STEP_M: f32 = 8.0;

/// Orbit focus lerp speed when following the train (1/s).
const FOLLOW_LERP_SPEED: f32 = 8.0;

/// Fixed pitch (rad) for chase camera behind the train.
pub(crate) const CHASE_PITCH: f32 = 0.5;

/// Minimum orbit distance while following (avoids clipping into the marker).
pub(crate) const FOLLOW_MIN_DISTANCE: f32 = 5.0;

/// Target orbit distance for chase follow in live mode (m).
pub const LIVE_CHASE_DISTANCE: f32 = 120.0;

/// Driver eye height above track (m); overridden per consist when spawned.
pub const DRIVER_EYE_HEIGHT_M: f32 = 2.4;

/// Metres behind the train head (into the cab) for the eye position.
pub const DRIVER_CAB_BACK_M: f32 = 2.8;

/// Slight downward pitch for driver view (rad).
pub const DRIVER_LOOK_PITCH: f32 = -0.04;

/// FOV in driver view (degrees) — Open Rails default `ViewingFOV` is often ~60.
pub const DRIVER_FOV_DEG_DEFAULT: f32 = 60.0;

/// Resolve driver cab FOV: CLI `--cab-fov`, then `OPENRAILSRS_CAB_FOV`, else [`DRIVER_FOV_DEG_DEFAULT`].
pub fn driver_cab_fov_deg(opts: &ViewerLaunchOpts) -> f32 {
    opts.cab_fov_deg
        .or_else(|| {
            std::env::var("OPENRAILSRS_CAB_FOV")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .filter(|f| *f >= 40.0 && *f <= 90.0)
        .unwrap_or(DRIVER_FOV_DEG_DEFAULT)
}

/// FOV in driver view (degrees).
pub const DRIVER_FOV_DEG: f32 = DRIVER_FOV_DEG_DEFAULT;

/// Near clip in driver view (m) — Open Rails `InsideThreeDimCamera.NearPlane` = 0.1.
pub const DRIVER_NEAR_CLIP_M: f32 = 0.1;

/// Mouse-look sensitivity in cab (rad / pixel).
pub const DRIVER_LOOK_SENSITIVITY: f32 = 0.004;

/// Max head-turn left/right in cab (rad).
pub const DRIVER_LOOK_YAW_MAX: f32 = 0.85;

/// Max look up/down relative to default cab pitch (rad).
pub const DRIVER_LOOK_PITCH_MAX: f32 = 0.55;
pub const DRIVER_LOOK_PITCH_MIN: f32 = -0.65;

/// Yaw offset so the camera −Z axis aligns with MSTS shape +Z (train-local +X after
/// [`crate::shapes::msts_shape_to_train_rotation`]).
pub const DRIVER_CAB_FORWARD_YAW_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;

/// Per-consist cab eye placement (set when the live train spawns).
#[derive(Resource, Clone, Copy, Debug)]
pub struct LiveDriverCab {
    /// Metres behind the train head along the consist axis (fallback when no ORTS head pos).
    pub back_m: f32,
    pub height_m: f32,
    /// Eyepoint in train-local metres from `ORTS3DCabHeadPos` (Open Rails 3D cab).
    pub head_pos_train: Option<Vec3>,
    /// ORTS eyepoint in **lead vehicle local** space (same as `PULLMAN_GR.s` mesh coords).
    pub head_lead_local: Option<Vec3>,
    /// Look pitch (rad) from `StartDirection` or default.
    pub look_pitch: f32,
    /// Cab `.s` placement on the train root (lead vehicle origin, unit scale).
    pub interior_placement: Transform,
    /// Raw `ORTS3DCabHeadPos` in MSTS shape metres (for camera-attached cab).
    pub head_msts: Option<Vec3>,
}

/// Extra yaw/pitch applied with the mouse while in driver view (head stays fixed).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct DriverLookOffset {
    pub yaw: f32,
    pub pitch: f32,
}

impl DriverLookOffset {
    pub fn apply_drag(&mut self, delta: Vec2) {
        self.yaw = (self.yaw - delta.x * DRIVER_LOOK_SENSITIVITY)
            .clamp(-DRIVER_LOOK_YAW_MAX, DRIVER_LOOK_YAW_MAX);
        self.pitch = (self.pitch - delta.y * DRIVER_LOOK_SENSITIVITY)
            .clamp(DRIVER_LOOK_PITCH_MIN, DRIVER_LOOK_PITCH_MAX);
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Default for LiveDriverCab {
    fn default() -> Self {
        Self {
            back_m: DRIVER_CAB_BACK_M,
            height_m: DRIVER_EYE_HEIGHT_M,
            head_pos_train: None,
            head_lead_local: None,
            look_pitch: DRIVER_LOOK_PITCH,
            interior_placement: Transform {
                rotation: crate::shapes::msts_shape_to_train_rotation(),
                ..default()
            },
            head_msts: None,
        }
    }
}

// ── Components / resources ────────────────────────────────────────────────

/// Which replay track the follow camera tracks (cycle with `[` / `]` or Shift+T).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CameraFollowTarget {
    pub track_index: usize,
}

impl CameraFollowTarget {
    pub fn clamp_to(&mut self, count: usize) {
        if count == 0 || self.track_index >= count {
            self.track_index = 0;
        }
    }

    pub fn cycle_next(&mut self, count: usize) {
        if count > 0 {
            self.track_index = (self.track_index + 1) % count;
        }
    }

    pub fn cycle_prev(&mut self, count: usize) {
        if count > 0 {
            self.track_index = (self.track_index + count - 1) % count;
        }
    }
}

/// Train-tracking camera behaviour (cycle with `T` during replay).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraFollowMode {
    #[default]
    Off,
    OrbitFollow,
    ChaseCam,
    /// First-person view from the locomotive cab (live mode).
    DriverCam,
}

impl CameraFollowMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::OrbitFollow,
            Self::OrbitFollow => Self::ChaseCam,
            Self::ChaseCam => Self::DriverCam,
            Self::DriverCam => Self::Off,
        }
    }

    pub fn hud_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::OrbitFollow => "orbit",
            Self::ChaseCam => "chase",
            Self::DriverCam => "driver",
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

/// Upper distance for user-driven orbit zoom (scroll / RMB dolly), independent of route framing.
#[inline]
pub fn orbit_user_zoom_max() -> f32 {
    ORBIT_ABSOLUTE_MAX_DISTANCE
}
/// Apply a vertical mouse delta as an orbit dolly factor (`>1` zoom out, `<1` zoom in).
pub fn orbit_rmb_dolly_factor(delta_y: f32) -> f32 {
    (1.0 + delta_y * ORBIT_RMB_DOLLY_SENSITIVITY).clamp(0.05, 20.0)
}

/// Scale orbit distance from mouse-wheel lines (`scroll` > 0 = zoom in).
pub fn orbit_wheel_zoom_factor(scroll_lines: f32) -> f32 {
    if std::env::var_os("OPENRAILSRS_INVERT_SCROLL_ZOOM").is_some() {
        1.0 + scroll_lines * ORBIT_ZOOM_STEP
    } else {
        1.0 - scroll_lines * ORBIT_ZOOM_STEP
    }
}

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

pub fn fly_wheel_dolly_delta(yaw: f32, pitch: f32, scroll_lines: f32) -> Vec3 {
    fly_forward(yaw, pitch) * scroll_lines * FLY_WHEEL_DOLLY_STEP_M
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
    let target_focus = Vec3::new(
        train.translation.x,
        train.translation.y,
        train.translation.z,
    );
    let focus = lerp_follow_focus(orbit.focus, target_focus, dt);
    let mut yaw = orbit.yaw;
    let mut pitch = orbit.pitch;
    let mut distance = orbit.distance;

    if follow == CameraFollowMode::ChaseCam {
        yaw = lerp_yaw_toward(yaw, chase_yaw_from_train(train.yaw), dt);
        pitch = lerp_yaw_toward(pitch, CHASE_PITCH, dt);
        distance = lerp_yaw_toward(distance, LIVE_CHASE_DISTANCE, dt);
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

/// First-person driver view locked to the train head pose.
pub fn driver_camera_transform(
    train: TrainFollowPose,
    cab: LiveDriverCab,
    look: DriverLookOffset,
) -> Transform {
    driver_camera_transform_at_eye(driver_eye_world(train, cab), train.yaw, cab, look)
}

/// Driver camera from a precomputed world-space eyepoint (fallback without lead [`GlobalTransform`]).
pub fn driver_camera_transform_at_eye(
    eye: Vec3,
    train_yaw: f32,
    cab: LiveDriverCab,
    look: DriverLookOffset,
) -> Transform {
    let user = driver_user_look_quat(look);
    let base = driver_cab_base_pitch(cab);
    Transform {
        translation: eye,
        rotation: Quat::from_rotation_y(train_yaw) * user * base,
        scale: Vec3::ONE,
    }
}

/// Fallback eyepoint when the lead vehicle [`GlobalTransform`] is not available yet.
pub fn driver_eye_world(train: TrainFollowPose, cab: LiveDriverCab) -> Vec3 {
    let rot = Quat::from_rotation_y(train.yaw);
    if let Some(head) = cab.head_pos_train {
        train.translation + rot * head
    } else {
        let forward = rot * Vec3::X;
        train.translation - forward * cab.back_m + Vec3::Y * cab.height_m
    }
}

/// ORTS eyepoint in world space via the lead vehicle hierarchy (preferred in live mode).
pub fn driver_eye_from_lead(lead_global: &GlobalTransform, cab: LiveDriverCab) -> Option<Vec3> {
    cab.head_lead_local
        .map(|local| lead_global.transform_point(local))
}

/// User mouse-look as a quaternion in head-local space (YXZ).
pub fn driver_user_look_quat(look: DriverLookOffset) -> Quat {
    Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0)
}

/// Default cab downward pitch from `.eng` / `StartDirection`.
pub fn driver_cab_base_pitch(cab: LiveDriverCab) -> Quat {
    Quat::from_rotation_x(cab.look_pitch)
}

/// Inverse local rotation for camera-attached cab mesh (world cab stays fixed while the head turns).
pub fn driver_cab_counter_look(look: DriverLookOffset, cab: LiveDriverCab) -> Quat {
    let user = driver_user_look_quat(look);
    let base = driver_cab_base_pitch(cab);
    base.inverse() * user.inverse() * base
}

/// Driver camera aligned to the lead vehicle (cab shape space + `look_pitch`).
pub fn driver_camera_transform_from_lead(
    lead_global: &GlobalTransform,
    cab: LiveDriverCab,
    look: DriverLookOffset,
) -> Transform {
    let eye = cab
        .head_lead_local
        .map(|local| lead_global.transform_point(local))
        .unwrap_or_else(|| lead_global.translation());
    let user = driver_user_look_quat(look);
    let base = driver_cab_base_pitch(cab);
    Transform {
        translation: eye,
        rotation: lead_global.rotation() * user * base,
        scale: Vec3::ONE,
    }
}

// ── Systems (Bevy) ────────────────────────────────────────────────────────

/// Optional debug/capture override of the initial orbit (yaw/pitch/distance, radians/m)
/// via `OPENRAILSRS_CAM_YAW` / `_PITCH` / `_DIST`. No effect when unset.
pub fn orbit_state_with_env_overrides(mut orbit: OrbitState) -> OrbitState {
    let env_f32 = |key: &str| {
        std::env::var(key)
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
    };
    if let Some(yaw) = env_f32("OPENRAILSRS_CAM_YAW") {
        orbit.yaw = yaw;
    }
    if let Some(pitch) = env_f32("OPENRAILSRS_CAM_PITCH") {
        orbit.pitch = clamp_pitch(pitch);
    }
    if let Some(dist) = env_f32("OPENRAILSRS_CAM_DIST") {
        orbit.distance = clamp_distance(dist);
    }
    orbit
}

pub fn spawn_camera(mut commands: Commands, opts: Res<ViewerLaunchOpts>) {
    let orbit = orbit_state_with_env_overrides(OrbitState::default());
    let transform =
        camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);

    let (ambient_brightness, exposure, tonemapping) = if opts.live {
        (
            live_outdoor_ambient(),
            Exposure::SUNLIGHT,
            live_tonemapping(),
        )
    } else {
        (0.15, Exposure::BLENDER, Tonemapping::None)
    };

    commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        transform,
        orbit,
        FlyState::default(),
        Msaa::Off,
        AmbientLight {
            color: Color::srgb(0.85, 0.9, 1.0),
            brightness: ambient_brightness,
            ..default()
        },
        exposure,
        tonemapping,
        // Atmospheric fog on by default; `F` toggles via `toggle_distance_fog` (#39).
        crate::sky::camera_distance_fog(),
        camera_layers_outdoor(),
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

fn enter_driver_cam(
    follow: &mut CameraFollowMode,
    mode: &mut CameraMode,
    look: &mut DriverLookOffset,
) {
    *follow = CameraFollowMode::DriverCam;
    *mode = CameraMode::Orbit;
    look.reset();
}

pub fn cycle_follow_mode(
    keys: Res<ButtonInput<KeyCode>>,
    replay: Option<Res<crate::train::ReplayState>>,
    live: Option<Res<crate::live::LiveDrive>>,
    mut mode: ResMut<CameraMode>,
    mut follow: ResMut<CameraFollowMode>,
    mut look: ResMut<DriverLookOffset>,
    mut target: ResMut<CameraFollowTarget>,
) {
    let replay_active = replay.as_ref().is_some_and(|r| r.is_active());
    let live_active = live.is_some();
    if !replay_active && !live_active {
        return;
    }
    let count = if live_active {
        1
    } else {
        replay.as_ref().map(|r| r.tracks.len()).unwrap_or(0)
    };
    target.clamp_to(count);

    if keys.just_pressed(KeyCode::KeyV) && !shift_held(&keys) {
        if *follow == CameraFollowMode::DriverCam {
            *follow = CameraFollowMode::ChaseCam;
            look.reset();
        } else {
            enter_driver_cam(&mut follow, &mut mode, &mut look);
        }
        return;
    }

    if live_active {
        if keys.just_pressed(KeyCode::KeyT) && !shift_held(&keys) {
            let next = follow.cycle();
            *follow = next;
            if next == CameraFollowMode::DriverCam {
                *mode = CameraMode::Orbit;
            }
            look.reset();
        }
        return;
    }

    if keys.just_pressed(KeyCode::BracketLeft)
        || (keys.just_pressed(KeyCode::KeyT) && shift_held(&keys))
    {
        target.cycle_next(count);
        return;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        target.cycle_prev(count);
        return;
    }
    if keys.just_pressed(KeyCode::KeyT) && !shift_held(&keys) {
        let next = follow.cycle();
        *follow = next;
        if next == CameraFollowMode::DriverCam {
            *mode = CameraMode::Orbit;
        }
        look.reset();
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn follow_train_camera(
    time: Res<Time>,
    mode: Res<CameraMode>,
    follow: Res<CameraFollowMode>,
    target: Res<CameraFollowTarget>,
    replay: Option<Res<crate::train::ReplayState>>,
    live: Option<Res<crate::live::LiveDrive>>,
    cab: Option<Res<LiveDriverCab>>,
    look: Option<Res<DriverLookOffset>>,
    train_query: Query<
        (&Transform, Option<&crate::train::TrainMarker>),
        (Without<OrbitState>, Without<crate::live::LiveTrainMarker>),
    >,
    live_train: Query<&Transform, (With<crate::live::LiveTrainMarker>, Without<OrbitState>)>,
    lead_car: Query<&GlobalTransform, With<crate::cab_view::CabLeadVehicle>>,
    mut orbit_query: Query<
        (&mut Transform, &mut OrbitState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) {
    if *follow == CameraFollowMode::Off {
        return;
    }
    let replay_active = replay.as_ref().is_some_and(|r| r.is_active());
    let live_active = live.is_some();
    if !replay_active && !live_active {
        return;
    }
    if *mode != CameraMode::Orbit && *follow != CameraFollowMode::DriverCam {
        return;
    }

    let train_tf = if live_active {
        live_train.iter().next()
    } else {
        train_query
            .iter()
            .find(|(_, marker)| marker.is_some_and(|m| m.track_index == target.track_index))
            .map(|(tf, _)| tf)
    };
    let Some(train_tf) = train_tf else {
        return;
    };

    let Ok((mut transform, mut orbit)) = orbit_query.single_mut() else {
        return;
    };

    let dt = time.delta_secs();
    let train_pose = TrainFollowPose {
        translation: train_tf.translation,
        yaw: yaw_from_transform(train_tf),
    };

    if *follow == CameraFollowMode::DriverCam {
        orbit.focus = lerp_follow_focus(orbit.focus, train_pose.translation, dt);
        orbit.yaw = train_pose.yaw;
        let cab = cab.as_deref().copied().unwrap_or_default();
        let look = look.as_deref().copied().unwrap_or_default();
        *transform = lead_car
            .iter()
            .next()
            .map(|lead| driver_camera_transform_from_lead(lead, cab, look))
            .unwrap_or_else(|| driver_camera_transform(train_pose, cab, look));
        return;
    }

    let update = apply_orbit_follow(*orbit, *follow, train_pose, dt);
    orbit.focus = update.focus;
    orbit.yaw = update.yaw;
    orbit.pitch = update.pitch;
    orbit.distance = update.distance;
    *transform = camera_transform_from_orbit(update);
}

fn shift_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

fn read_orbit_pan_axes(keys: &ButtonInput<KeyCode>, live_mode: bool) -> Vec3 {
    let mut axes = Vec3::ZERO;
    if live_mode {
        // In live mode W/S are not used for throttle; I/K pan forward/back instead.
        if keys.pressed(KeyCode::KeyI) {
            axes.z += 1.0;
        }
        if keys.pressed(KeyCode::KeyK) {
            axes.z -= 1.0;
        }
    } else {
        if keys.pressed(KeyCode::KeyW) {
            axes.z += 1.0;
        }
        if keys.pressed(KeyCode::KeyS) {
            axes.z -= 1.0;
        }
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

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn orbit_camera_system(
    time: Res<Time>,
    mode: Res<CameraMode>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    keys: Res<ButtonInput<KeyCode>>,
    mut follow: ResMut<CameraFollowMode>,
    mut look: ResMut<DriverLookOffset>,
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

    if *follow == CameraFollowMode::DriverCam {
        let mut delta = Vec2::ZERO;
        for ev in motion.read() {
            delta += ev.delta;
        }
        wheel.clear();
        if keys.just_pressed(KeyCode::Home) {
            look.reset();
        }
        let dragging =
            mouse_buttons.pressed(MouseButton::Left) || mouse_buttons.pressed(MouseButton::Right);
        if dragging && delta != Vec2::ZERO {
            look.apply_drag(delta);
        }
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
    let mouse_dragging = mouse_buttons.pressed(MouseButton::Left)
        || mouse_buttons.pressed(MouseButton::Right)
        || mouse_buttons.pressed(MouseButton::Middle);
    let drag_rotate_lmb = mouse_buttons.pressed(MouseButton::Left) && !shift;
    let drag_rmb = mouse_buttons.pressed(MouseButton::Right) && !shift;
    let drag_pan = mouse_buttons.pressed(MouseButton::Middle)
        || (shift
            && (mouse_buttons.pressed(MouseButton::Left)
                || mouse_buttons.pressed(MouseButton::Right)));

    if drag_rotate_lmb && delta != Vec2::ZERO {
        *follow = CameraFollowMode::Off;
        orbit.yaw -= delta.x * ORBIT_ROTATE_SENSITIVITY;
        orbit.pitch = clamp_pitch(orbit.pitch - delta.y * ORBIT_ROTATE_SENSITIVITY);
        changed = true;
    }

    if drag_rmb && delta != Vec2::ZERO {
        *follow = CameraFollowMode::Off;
        if delta.x.abs() >= delta.y.abs() {
            orbit.yaw -= delta.x * ORBIT_ROTATE_SENSITIVITY;
        } else {
            orbit.distance = clamp_distance_to_limit(
                orbit.distance * orbit_rmb_dolly_factor(delta.y),
                orbit_user_zoom_max(),
            );
        }
        changed = true;
    }

    if drag_pan && delta != Vec2::ZERO {
        *follow = CameraFollowMode::Off;
        pan_orbit_focus(&mut orbit, &transform, delta);
        changed = true;
    }

    let pan_axes = read_orbit_pan_axes(&keys, opts.live);
    if pan_axes != Vec3::ZERO {
        *follow = CameraFollowMode::Off;
        keyboard_pan_orbit_focus(&mut orbit, pan_axes, time.delta_secs());
        changed = true;
    }

    let zoom_keys = if opts.live {
        keys.pressed(KeyCode::BracketLeft) || keys.pressed(KeyCode::Comma)
    } else {
        keys.pressed(KeyCode::BracketLeft)
    };
    let zoom_out_keys = if opts.live {
        keys.pressed(KeyCode::BracketRight) || keys.pressed(KeyCode::Period)
    } else {
        keys.pressed(KeyCode::BracketRight)
    };
    if zoom_out_keys {
        *follow = CameraFollowMode::Off;
        orbit.distance = clamp_distance_to_limit(
            orbit.distance * 1.08_f32.powf(time.delta_secs() * 10.0),
            orbit_user_zoom_max(),
        );
        changed = true;
    }
    if zoom_keys {
        *follow = CameraFollowMode::Off;
        orbit.distance = clamp_distance_to_limit(
            orbit.distance / 1.08_f32.powf(time.delta_secs() * 10.0),
            orbit_user_zoom_max(),
        );
        changed = true;
    }

    if scroll != 0.0 && !mouse_dragging {
        // Scroll zoom uses the absolute cap so the user can always zoom further out.
        // When actively following in ChaseCam the lerp would override the distance every
        // frame, so switch to OrbitFollow (focus still tracks the train, distance is free).
        if *follow == CameraFollowMode::ChaseCam {
            *follow = CameraFollowMode::OrbitFollow;
        }
        orbit.distance = clamp_distance_to_limit(
            orbit.distance * orbit_wheel_zoom_factor(scroll),
            orbit_user_zoom_max(),
        );
        changed = true;
    }

    if changed {
        *transform =
            camera_transform_from_orbit_state(orbit.focus, orbit.yaw, orbit.pitch, orbit.distance);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn fly_camera_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    follow: Res<CameraFollowMode>,
    replay: Option<Res<crate::train::ReplayState>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut query: Query<
        (&mut Transform, &mut FlyState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) {
    if *follow == CameraFollowMode::DriverCam {
        motion.clear();
        wheel.clear();
        return;
    }
    let Ok((mut transform, mut fly)) = query.single_mut() else {
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
    if scroll != 0.0 {
        transform.translation += fly_wheel_dolly_delta(fly.yaw, fly.pitch, scroll);
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

pub fn follow_train_camera_active(
    mode: Res<CameraMode>,
    follow: Res<CameraFollowMode>,
    replay: Option<Res<crate::train::ReplayState>>,
    live: Option<Res<crate::live::LiveDrive>>,
) -> bool {
    if *follow == CameraFollowMode::Off {
        return false;
    }
    let sim_active = live.is_some() || replay.as_ref().is_some_and(|r| r.is_active());
    if !sim_active {
        return false;
    }
    *follow == CameraFollowMode::DriverCam || *mode == CameraMode::Orbit
}

pub fn fly_camera_allowed(follow: Res<CameraFollowMode>) -> bool {
    *follow != CameraFollowMode::DriverCam
}

/// Widen FOV, tighten near clip, and tune exposure/ambient per camera mode.
pub fn update_driver_camera_fov(
    opts: Res<ViewerLaunchOpts>,
    follow: Res<CameraFollowMode>,
    mut query: Query<
        (
            &mut Projection,
            &mut AmbientLight,
            &mut Tonemapping,
            &mut Exposure,
        ),
        With<Camera3d>,
    >,
) {
    let Ok((mut projection, mut ambient, mut tonemapping, mut exposure)) = query.single_mut()
    else {
        return;
    };
    let Projection::Perspective(persp) = &mut *projection else {
        return;
    };
    if *follow == CameraFollowMode::DriverCam {
        persp.fov = driver_cab_fov_deg(&opts).to_radians();
        persp.near = DRIVER_NEAR_CLIP_M;
        ambient.brightness = 350.0;
        ambient.color = Color::srgb(0.95, 0.94, 0.92);
        *tonemapping = Tonemapping::None;
        *exposure = Exposure::BLENDER;
    } else if opts.live {
        persp.fov = std::f32::consts::FRAC_PI_4;
        persp.near = 0.1;
        ambient.brightness = live_outdoor_ambient();
        ambient.color = Color::srgb(0.85, 0.9, 1.0);
        *tonemapping = live_tonemapping();
        *exposure = Exposure::SUNLIGHT;
    } else {
        persp.fov = std::f32::consts::FRAC_PI_4;
        persp.near = 0.1;
        ambient.brightness = 0.15;
        ambient.color = Color::srgb(1.0, 1.0, 1.0);
        *tonemapping = Tonemapping::default();
        *exposure = Exposure::BLENDER;
    }
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
    use bevy::input::touch::TouchPhase;
    use std::f32::consts::FRAC_PI_2;

    fn vec3_close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).length() < eps
    }

    #[test]
    fn parse_tonemapping_defaults_to_tony() {
        assert_eq!(parse_tonemapping(None), Tonemapping::TonyMcMapface);
        assert_eq!(parse_tonemapping(Some("")), Tonemapping::TonyMcMapface);
        assert_eq!(parse_tonemapping(Some("tony")), Tonemapping::TonyMcMapface);
    }

    #[test]
    fn parse_tonemapping_recognizes_named_curves() {
        assert_eq!(parse_tonemapping(Some("none")), Tonemapping::None);
        assert_eq!(parse_tonemapping(Some("AgX")), Tonemapping::AgX);
        assert_eq!(parse_tonemapping(Some(" aces ")), Tonemapping::AcesFitted);
        assert_eq!(
            parse_tonemapping(Some("reinhard")),
            Tonemapping::ReinhardLuminance
        );
        assert_eq!(
            parse_tonemapping(Some("blender")),
            Tonemapping::BlenderFilmic
        );
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
    fn orbit_wheel_zoom_factor_scroll_up_zooms_in() {
        assert!(orbit_wheel_zoom_factor(1.0) < 1.0);
        assert!(orbit_wheel_zoom_factor(-1.0) > 1.0);
    }

    #[test]
    fn orbit_rmb_dolly_factor_zooms_in_on_drag_up() {
        assert!(orbit_rmb_dolly_factor(-100.0) < 1.0);
        assert!(orbit_rmb_dolly_factor(100.0) > 1.0);
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
    fn fly_wheel_dolly_moves_along_view_direction() {
        let d = fly_wheel_dolly_delta(0.0, 0.0, 2.0);
        assert!(vec3_close(d, Vec3::new(0.0, 0.0, -16.0), 1e-5));
    }

    #[test]
    fn pixel_wheel_scroll_is_normalized_to_lines() {
        let ev = MouseWheel {
            unit: MouseScrollUnit::Pixel,
            x: 0.0,
            y: 250.0,
            window: Entity::PLACEHOLDER,
            phase: TouchPhase::Moved,
        };
        assert!((wheel_scroll_lines(&ev) - 2.5).abs() < 1e-6);
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
        assert_eq!(
            CameraFollowMode::ChaseCam.cycle(),
            CameraFollowMode::DriverCam
        );
        assert_eq!(CameraFollowMode::DriverCam.cycle(), CameraFollowMode::Off);
    }

    #[test]
    fn camera_follow_mode_hud_labels() {
        assert_eq!(CameraFollowMode::Off.hud_label(), "off");
        assert_eq!(CameraFollowMode::OrbitFollow.hud_label(), "orbit");
        assert_eq!(CameraFollowMode::ChaseCam.hud_label(), "chase");
        assert_eq!(CameraFollowMode::DriverCam.hud_label(), "driver");
    }

    #[test]
    fn driver_camera_transform_places_eye_in_cab() {
        let tf = driver_camera_transform(
            TrainFollowPose {
                translation: Vec3::new(10.0, 0.0, 20.0),
                yaw: 0.0,
            },
            LiveDriverCab {
                back_m: 3.0,
                height_m: 2.5,
                head_pos_train: None,
                look_pitch: DRIVER_LOOK_PITCH,
                ..Default::default()
            },
            DriverLookOffset::default(),
        );
        assert!((tf.translation.x - 7.0).abs() < 1e-5);
        assert!((tf.translation.y - 2.5).abs() < 1e-5);
        assert!((tf.translation.z - 20.0).abs() < 1e-5);
    }

    #[test]
    fn driver_camera_uses_orts_head_pos_when_set() {
        let head_train = Vec3::new(1.7, 3.63, 0.8);
        let tf = driver_camera_transform(
            TrainFollowPose {
                translation: Vec3::new(100.0, 0.0, 50.0),
                yaw: 0.0,
            },
            LiveDriverCab {
                back_m: 3.0,
                height_m: 2.5,
                head_pos_train: Some(head_train),
                look_pitch: -15.0_f32.to_radians(),
                ..Default::default()
            },
            DriverLookOffset::default(),
        );
        assert!((tf.translation.x - 101.7).abs() < 1e-4);
        assert!((tf.translation.y - 3.63).abs() < 1e-4);
        assert!((tf.translation.z - 50.8).abs() < 1e-4);
    }

    #[test]
    fn driver_eye_from_lead_uses_head_lead_local() {
        let head_lead = crate::shapes::msts_shape_vec3_to_bevy(Vec3::new(-0.8, 2.875, 8.60));
        let placement = Transform {
            translation: Vec3::new(2.0, -0.5, 0.0),
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        };
        let train = Transform::from_translation(Vec3::new(100.0, 5.0, 200.0))
            .with_rotation(Quat::from_rotation_y(0.25));
        let lead_global = GlobalTransform::from(train.mul_transform(placement));
        let cab = LiveDriverCab {
            head_lead_local: Some(head_lead),
            ..Default::default()
        };
        let eye = driver_eye_from_lead(&lead_global, cab).expect("head_lead_local");
        let expected = lead_global.transform_point(head_lead);
        assert!((eye - expected).length() < 1e-4);
    }

    #[test]
    fn driver_eye_fixed_when_look_offset_changes() {
        let head_lead = crate::shapes::msts_shape_vec3_to_bevy(Vec3::new(-0.8, 2.875, 8.60));
        let lead_global = GlobalTransform::from(Transform {
            translation: Vec3::new(-487.0, 39.0, -1768.0),
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        });
        let cab = LiveDriverCab {
            head_lead_local: Some(head_lead),
            look_pitch: -15.0_f32.to_radians(),
            ..Default::default()
        };
        let a = driver_camera_transform_from_lead(&lead_global, cab, DriverLookOffset::default());
        let b = driver_camera_transform_from_lead(
            &lead_global,
            cab,
            DriverLookOffset {
                yaw: 0.5,
                pitch: 0.25,
            },
        );
        assert!(
            (a.translation - b.translation).length() < 1e-4,
            "eye must stay at ORTS: {:?} vs {:?}",
            a.translation,
            b.translation
        );
    }

    #[test]
    fn driver_look_offset_changes_forward() {
        let placement = Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        };
        let lead_global = GlobalTransform::from(placement);
        let cab = LiveDriverCab {
            look_pitch: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let base =
            driver_camera_transform_from_lead(&lead_global, cab, DriverLookOffset::default());
        let turned = driver_camera_transform_from_lead(
            &lead_global,
            cab,
            DriverLookOffset {
                yaw: 0.4,
                pitch: 0.0,
            },
        );
        let base_fwd = base.forward().as_vec3();
        let turned_fwd = turned.forward().as_vec3();
        assert!(
            base_fwd.dot(turned_fwd) < 0.99,
            "yaw offset should rotate view: {base_fwd:?} vs {turned_fwd:?}"
        );
    }

    #[test]
    fn driver_camera_looks_along_cab_forward() {
        let train_yaw = 0.35;
        let placement = Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        };
        let train = Transform::from_rotation(Quat::from_rotation_y(train_yaw));
        let lead_global = GlobalTransform::from(train.mul_transform(placement));
        let tf = driver_camera_transform_from_lead(
            &lead_global,
            LiveDriverCab {
                look_pitch: 0.0,
                head_lead_local: Some(Vec3::ZERO),
                ..Default::default()
            },
            DriverLookOffset::default(),
        );
        let cam_fwd = tf.forward().as_vec3();
        let cab_fwd = lead_global.rotation().mul_vec3(Vec3::NEG_Z);
        let cam_h = Vec3::new(cam_fwd.x, 0.0, cam_fwd.z).normalize();
        let cab_h = Vec3::new(cab_fwd.x, 0.0, cab_fwd.z).normalize();
        assert!(cam_h.dot(cab_h) > 0.99, "cam={cam_h:?} cab={cab_h:?}");
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
        assert!(update.focus.y > 0.0 && update.focus.y < 5.0);
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
            distance: 0.5, // well below FOLLOW_MIN_DISTANCE
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
    fn camera_follow_target_cycles() {
        let mut target = CameraFollowTarget::default();
        target.cycle_next(3);
        assert_eq!(target.track_index, 1);
        target.cycle_prev(3);
        assert_eq!(target.track_index, 0);
        target.clamp_to(1);
        target.track_index = 5;
        target.clamp_to(1);
        assert_eq!(target.track_index, 0);
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
