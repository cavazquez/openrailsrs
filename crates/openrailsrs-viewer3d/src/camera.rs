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
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::cab_render::camera_layers_outdoor;
use crate::launch::ViewerLaunchOpts;
use crate::viewer_log;

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

/// Fixed pitch (rad) for the close chase camera above the leading vehicles.
pub(crate) const CHASE_PITCH: f32 = 0.24;

/// Minimum orbit distance while following (avoids clipping into the marker).
pub(crate) const FOLLOW_MIN_DISTANCE: f32 = 5.0;

/// Target orbit distance for the close live chase camera (m).
pub const LIVE_CHASE_DISTANCE: f32 = 28.0;

/// Put the chase focus beyond the leading vehicle, leaving the camera above
/// the first two cars while it looks forward over the train.
const LIVE_CHASE_LOOK_AHEAD_M: f32 = 12.0;

/// Driver eye height above track (m); overridden per consist when spawned.
pub const DRIVER_EYE_HEIGHT_M: f32 = 2.4;

/// Metres behind the train head (into the cab) for the eye position.
pub const DRIVER_CAB_BACK_M: f32 = 2.8;

/// Slight downward pitch for driver view (rad).
pub const DRIVER_LOOK_PITCH: f32 = -0.04;

/// Vertical FOV in driver view (degrees).
///
/// Open Rails declares `ViewingFOV` as `Default(45)`: MSTS' 60° horizontal
/// field on a 4:3 display is approximately 45° vertically.
pub const DRIVER_FOV_DEG_DEFAULT: f32 = 45.0;

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

/// Max head-turn left/right in cab (rad) — fallback when no authored limit.
pub const DRIVER_LOOK_YAW_MAX: f32 = 0.85;

/// Max look up/down relative to default cab pitch (rad) — fallback.
pub const DRIVER_LOOK_PITCH_MAX: f32 = 0.55;
pub const DRIVER_LOOK_PITCH_MIN: f32 = -0.65;

/// Open Rails `InsideThreeDimCamera` does not clamp to `.eng` `RotationLimit`;
/// use a soft vertical clamper (~±85°) and free yaw for 3D cab mouse look.
pub const DRIVER_CAM_FREE_YAW_MAX: f32 = std::f32::consts::PI;
pub const DRIVER_CAM_FREE_PITCH_MAX: f32 = 1.48;

/// Yaw offset so the camera −Z axis aligns with train-local +X.
///
/// This is only needed by the fallback camera built from the train root.  A camera
/// built from the lead vehicle already lives in converted MSTS shape space, where
/// its neutral −Z axis points through the cab windshield.
pub const DRIVER_CAB_FORWARD_YAW_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;

/// Which eyepoint is active in driver view (3D cab viewpoint vs HeadOut).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DriverViewSlot {
    #[default]
    CabViewpoint,
    HeadOut,
}

/// One selectable driver eyepoint (cab viewpoint or HeadOut).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriverEyepoint {
    pub head_msts: Vec3,
    pub look_pitch: f32,
    pub look_yaw: f32,
    pub pitch_limit: Option<f32>,
    pub yaw_limit: Option<f32>,
    pub slot: DriverViewSlot,
}

/// Per-consist cab eye placement (set when the live train spawns).
#[derive(Resource, Clone, Debug)]
pub struct LiveDriverCab {
    /// Metres behind the train head along the consist axis (fallback when no ORTS head pos).
    pub back_m: f32,
    pub height_m: f32,
    /// Eyepoint in train-local metres from `ORTS3DCabHeadPos` (Open Rails 3D cab).
    pub head_pos_train: Option<Vec3>,
    /// ORTS eyepoint in **lead vehicle local** space (same as `PULLMAN_GR.s` mesh coords).
    pub head_lead_local: Option<Vec3>,
    /// Look pitch (rad) from `StartDirection.X` or default.
    pub look_pitch: f32,
    /// Look yaw (rad) from `StartDirection.Y` (rear cab ≈ ±π).
    pub look_yaw: f32,
    /// Mouse-look pitch clamp (±rad) from `RotationLimit.X`, else default.
    pub pitch_limit: f32,
    /// Mouse-look yaw clamp (±rad) from `RotationLimit.Y`, else default.
    pub yaw_limit: f32,
    /// Cab `.s` placement on the train root (lead vehicle authored origin, unit scale).
    pub interior_placement: Transform,
    /// Raw `ORTS3DCabHeadPos` in MSTS shape metres (for camera-attached cab).
    pub head_msts: Option<Vec3>,
    /// All cab + HeadOut eyepoints for cycling (Shift+V in driver cam).
    pub eyepoints: Vec<DriverEyepoint>,
    pub eyepoint_index: usize,
}

/// Extra yaw/pitch applied with the mouse while in driver view (head stays fixed).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct DriverLookOffset {
    pub yaw: f32,
    pub pitch: f32,
}

impl DriverLookOffset {
    pub fn apply_drag(&mut self, delta: Vec2) {
        self.apply_drag_clamped(delta, DRIVER_LOOK_YAW_MAX, DRIVER_LOOK_PITCH_MAX);
    }

    pub fn apply_drag_clamped(&mut self, delta: Vec2, yaw_limit: f32, pitch_limit: f32) {
        let yaw_max = yaw_limit.abs().max(1e-3);
        let pitch_max = pitch_limit.abs().max(1e-3);
        self.yaw = (self.yaw - delta.x * DRIVER_LOOK_SENSITIVITY).clamp(-yaw_max, yaw_max);
        self.pitch = (self.pitch - delta.y * DRIVER_LOOK_SENSITIVITY).clamp(-pitch_max, pitch_max);
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
            look_yaw: 0.0,
            pitch_limit: DRIVER_LOOK_PITCH_MAX,
            yaw_limit: DRIVER_LOOK_YAW_MAX,
            interior_placement: Transform {
                rotation: crate::shapes::msts_shape_to_train_rotation(),
                ..default()
            },
            head_msts: None,
            eyepoints: Vec::new(),
            eyepoint_index: 0,
        }
    }
}

impl LiveDriverCab {
    /// Apply an ORTS eyepoint (cab or HeadOut) onto the active camera fields.
    pub fn apply_eyepoint(&mut self, eye: DriverEyepoint, placement: Transform) {
        let head_bevy = crate::shapes::msts_shape_vec3_to_bevy(eye.head_msts);
        self.head_msts = Some(eye.head_msts);
        self.head_lead_local = Some(head_bevy);
        self.head_pos_train = Some(placement.transform_point(head_bevy));
        self.look_pitch = eye.look_pitch;
        self.look_yaw = eye.look_yaw;
        self.pitch_limit = eye.pitch_limit.unwrap_or(DRIVER_LOOK_PITCH_MAX);
        self.yaw_limit = eye.yaw_limit.unwrap_or(DRIVER_LOOK_YAW_MAX);
    }

    pub fn cycle_eyepoint(&mut self) -> bool {
        if self.eyepoints.len() <= 1 {
            return false;
        }
        self.eyepoint_index = (self.eyepoint_index + 1) % self.eyepoints.len();
        let eye = self.eyepoints[self.eyepoint_index];
        let placement = self.interior_placement;
        self.apply_eyepoint(eye, placement);
        true
    }

    pub fn active_slot(&self) -> DriverViewSlot {
        self.eyepoints
            .get(self.eyepoint_index)
            .map(|e| e.slot)
            .unwrap_or(DriverViewSlot::CabViewpoint)
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

/// Like Open Rails `Use3DCab`: key **1** enters 3D cab when true, 2D when false.
/// Toggled with **Alt+1** (`CameraToggleThreeDimensionalCab`).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefer3dCab(pub bool);

impl Default for Prefer3dCab {
    fn default() -> Self {
        Self(true)
    }
}

/// Train-tracking camera behaviour (cycle with `T` during replay).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraFollowMode {
    #[default]
    Off,
    OrbitFollow,
    ChaseCam,
    /// First-person view from the locomotive 3D cab (live mode).
    DriverCam,
    /// Open Rails 2D `Cab` view: CVF ACE background + control sprites (#152).
    Cab2d,
    /// Open Rails camera 5 — passenger seat; **5** cycles cars with `Inside`.
    PassengerCam,
}

impl CameraFollowMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::OrbitFollow,
            Self::OrbitFollow => Self::ChaseCam,
            Self::ChaseCam => Self::DriverCam,
            Self::DriverCam => Self::Off,
            // Cab2d left via T-cycle / outside cams (OR: 1 stays in cab).
            Self::Cab2d => Self::Off,
            Self::PassengerCam => Self::Off,
        }
    }

    pub fn hud_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::OrbitFollow => "orbit",
            Self::ChaseCam => "chase",
            Self::DriverCam => "driver",
            Self::Cab2d => "cab2d",
            Self::PassengerCam => "passenger",
        }
    }

    /// True for the 2D CVF cab panel (not the 3D CABVIEW3D mesh).
    pub fn is_cab2d(self) -> bool {
        matches!(self, Self::Cab2d)
    }

    pub fn is_passenger(self) -> bool {
        matches!(self, Self::PassengerCam)
    }
}

/// Active passenger seat (OR camera 5).
#[derive(Resource, Clone, Debug)]
pub struct PassengerCamState {
    /// Slot among cars that have ≥1 passenger viewpoint.
    pub car_slot: usize,
    /// Viewpoint within the current car.
    pub view_index: usize,
    /// Consist car index (`LiveTrainCar.index`).
    pub consist_car: usize,
    pub head_msts: Vec3,
    pub look_pitch: f32,
    pub look_yaw: f32,
    pub pitch_limit: f32,
    pub yaw_limit: f32,
}

impl Default for PassengerCamState {
    fn default() -> Self {
        Self {
            car_slot: 0,
            view_index: 0,
            consist_car: 0,
            head_msts: Vec3::ZERO,
            look_pitch: 0.0,
            look_yaw: 0.0,
            pitch_limit: 30f32.to_radians(),
            yaw_limit: 70f32.to_radians(),
        }
    }
}

impl PassengerCamState {
    pub fn apply_viewpoint(&mut self, vp: &PassengerViewpointAuthored, consist_car: usize) {
        self.consist_car = consist_car;
        self.head_msts = vp.head_msts;
        self.look_pitch = vp.look_pitch;
        self.look_yaw = vp.look_yaw;
        self.pitch_limit = vp.pitch_limit.unwrap_or(30f32.to_radians());
        self.yaw_limit = vp.yaw_limit.unwrap_or(70f32.to_radians());
    }
}

/// One authored passenger seat from `.wag` / `.eng` `Inside`.
#[derive(Clone, Debug, PartialEq)]
pub struct PassengerViewpointAuthored {
    pub head_msts: Vec3,
    pub look_pitch: f32,
    pub look_yaw: f32,
    pub pitch_limit: Option<f32>,
    pub yaw_limit: Option<f32>,
}

/// Per-consist-car passenger viewpoints (empty vec = no seat on that car).
#[derive(Resource, Clone, Debug, Default)]
pub struct PassengerSeatCatalog {
    pub by_car: Vec<Vec<PassengerViewpointAuthored>>,
}

impl PassengerSeatCatalog {
    /// `(consist_car_index, viewpoints)` for cars that have seats.
    pub fn seat_cars(&self) -> Vec<(usize, &Vec<PassengerViewpointAuthored>)> {
        self.by_car
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
            .collect()
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

/// Orbit yaw for a chase camera behind a train whose longitudinal axis is local `+X`.
///
/// [`orbit_position`] uses `yaw=0` for an offset on `+Z`, while rolling stock uses
/// `Transform::rotation * Vec3::X` as travel.  Therefore the camera's behind-train
/// offset is the train root yaw minus 90°, not the opposite of Bevy's local `-Z`.
#[inline]
pub fn chase_yaw_from_train(train_yaw: f32) -> f32 {
    train_yaw - std::f32::consts::FRAC_PI_2
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

/// Close chase target from the first/last car origins after path placement.
///
/// This intentionally derives the longitudinal axis from actual car positions:
/// a nearest-TDB fallback can have a different yaw convention than the train root.
fn consist_chase_pose(head: Vec3, tail: Vec3) -> Option<(TrainFollowPose, f32)> {
    let travel = head - tail;
    let planar = Vec2::new(travel.x, travel.z);
    let origin_span_m = planar.length();
    if origin_span_m < 1.0 {
        return None;
    }
    // For Quat::from_rotation_y(yaw), local +X maps to (cos(yaw), -sin(yaw)).
    let yaw = (-planar.y).atan2(planar.x);
    let forward = travel / origin_span_m;
    Some((
        TrainFollowPose {
            translation: head + forward * LIVE_CHASE_LOOK_AHEAD_M,
            yaw,
        },
        LIVE_CHASE_DISTANCE,
    ))
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
    chase_distance_m: f32,
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
        distance = lerp_yaw_toward(distance, chase_distance_m, dt);
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
    cab: &LiveDriverCab,
    look: DriverLookOffset,
) -> Transform {
    driver_camera_transform_at_eye(driver_eye_world(train, cab), train.yaw, cab, look)
}

/// Driver camera from a precomputed world-space eyepoint (fallback without lead [`GlobalTransform`]).
pub fn driver_camera_transform_at_eye(
    eye: Vec3,
    train_yaw: f32,
    cab: &LiveDriverCab,
    look: DriverLookOffset,
) -> Transform {
    Transform {
        translation: eye,
        rotation: Quat::from_rotation_y(train_yaw) * driver_train_look_quat(look, cab),
        scale: Vec3::ONE,
    }
}

/// Fallback eyepoint when the lead vehicle [`GlobalTransform`] is not available yet.
pub fn driver_eye_world(train: TrainFollowPose, cab: &LiveDriverCab) -> Vec3 {
    let rot = Quat::from_rotation_y(train.yaw);
    if let Some(head) = cab.head_pos_train {
        train.translation + rot * head
    } else {
        let forward = rot * Vec3::X;
        train.translation - forward * cab.back_m + Vec3::Y * cab.height_m
    }
}

/// ORTS eyepoint in world space via the lead vehicle hierarchy (preferred in live mode).
pub fn driver_eye_from_lead(lead_global: &GlobalTransform, cab: &LiveDriverCab) -> Option<Vec3> {
    cab.head_lead_local
        .map(|local| lead_global.transform_point(local))
}

/// User mouse-look as a quaternion in head-local space (YXZ).
pub fn driver_user_look_quat(look: DriverLookOffset) -> Quat {
    Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0)
}

/// CVF `Direction` / MSTS `StartDirection` → Bevy look offset.
///
/// X = pitch degrees (positive = look down) → negated Bevy pitch.
/// Y = yaw degrees (rear cab ≈ ±180).
pub fn msts_direction_to_look_offset(direction_deg: [f64; 3]) -> DriverLookOffset {
    DriverLookOffset {
        pitch: -(direction_deg[0] as f32).to_radians(),
        yaw: (direction_deg[1] as f32).to_radians(),
    }
}

/// Viewpoint delta from a CVF `CabView.Direction` (degrees).
///
/// `StartDirection` lives on [`LiveDriverCab`] (`look_yaw` / `look_pitch`) and is applied by
/// [`driver_camera_transform_from_lead`] / [`cab_view_orientation_quat`] — this helper only
/// returns the CVF viewpoint offset so Cab2d and DriverCam compose the same way (#169).
pub fn cab_view_look_offset(
    _cab: &LiveDriverCab,
    view_direction_deg: [f64; 3],
) -> DriverLookOffset {
    msts_direction_to_look_offset(view_direction_deg)
}

/// Cab orientation in converted MSTS shape space: `StartDirection` + CVF `Direction`.
///
/// Bevy camera −Z is already the neutral windshield direction in this space, so
/// no shape→train axis correction belongs here.
pub fn cab_view_orientation_quat(cab: &LiveDriverCab, view_direction_deg: [f64; 3]) -> Quat {
    let view = cab_view_look_offset(cab, view_direction_deg);
    Quat::from_euler(
        EulerRot::YXZ,
        cab.look_yaw + view.yaw,
        cab.look_pitch + view.pitch,
        0.0,
    )
}

/// Default cab orientation from `.eng` `StartDirection` (yaw + pitch).
pub fn driver_cab_base_orientation(cab: &LiveDriverCab) -> Quat {
    cab_view_orientation_quat(cab, [0.0; 3])
}

/// Cab base look + mouse offset in one YXZ euler (FPS-style, shape-local).
pub fn driver_combined_look_quat(look: DriverLookOffset, cab: &LiveDriverCab) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        cab.look_yaw + look.yaw,
        cab.look_pitch + look.pitch,
        0.0,
    )
}

/// Cab look relative to the train root when no lead-vehicle transform is available.
fn driver_train_look_quat(look: DriverLookOffset, cab: &LiveDriverCab) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        cab.look_yaw + DRIVER_CAB_FORWARD_YAW_OFFSET + look.yaw,
        cab.look_pitch + look.pitch,
        0.0,
    )
}

/// Default cab downward pitch from `.eng` / `StartDirection` (compat helper).
pub fn driver_cab_base_pitch(cab: &LiveDriverCab) -> Quat {
    Quat::from_rotation_x(cab.look_pitch)
}

/// Inverse local rotation for camera-attached cab mesh (world cab stays fixed while the head turns).
pub fn driver_cab_counter_look(look: DriverLookOffset, cab: &LiveDriverCab) -> Quat {
    let combined = driver_combined_look_quat(look, cab);
    let base = driver_cab_base_orientation(cab);
    combined.inverse() * base
}

/// Driver camera aligned to the lead vehicle (converted MSTS cab shape space).
pub fn driver_camera_transform_from_lead(
    lead_global: &GlobalTransform,
    cab: &LiveDriverCab,
    look: DriverLookOffset,
) -> Transform {
    let eye = cab
        .head_lead_local
        .map(|local| lead_global.transform_point(local))
        .unwrap_or_else(|| lead_global.translation());
    Transform {
        translation: eye,
        rotation: lead_global.rotation() * driver_combined_look_quat(look, cab),
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

/// Capture / golden override for driver mouse-look (`OPENRAILSRS_LOOK_YAW` / `_PITCH`, radians).
pub fn driver_look_with_env_overrides(mut look: DriverLookOffset) -> DriverLookOffset {
    let env_f32 = |key: &str| {
        std::env::var(key)
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
    };
    if let Some(yaw) = env_f32("OPENRAILSRS_LOOK_YAW") {
        look.yaw = yaw.clamp(-DRIVER_LOOK_YAW_MAX, DRIVER_LOOK_YAW_MAX);
    }
    if let Some(pitch) = env_f32("OPENRAILSRS_LOOK_PITCH") {
        look.pitch = pitch.clamp(-DRIVER_LOOK_PITCH_MAX, DRIVER_LOOK_PITCH_MAX);
    }
    look
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
        // A camera has no render mesh; keep the non-caster intent explicit so
        // diagnostics cannot mistake this entity for shadow geometry.
        NotShadowCaster,
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

fn enter_cab2d_cam(
    follow: &mut CameraFollowMode,
    mode: &mut CameraMode,
    look: &mut DriverLookOffset,
    overlay: Option<&mut crate::cab_cvf_overlay::CabCvfOverlayState>,
) {
    *follow = CameraFollowMode::Cab2d;
    *mode = CameraMode::Orbit;
    look.reset();
    if let Some(o) = overlay {
        o.view_index = 0;
    }
}

fn enter_preferred_cab(
    prefer_3d: bool,
    follow: &mut CameraFollowMode,
    mode: &mut CameraMode,
    look: &mut DriverLookOffset,
    overlay: Option<&mut crate::cab_cvf_overlay::CabCvfOverlayState>,
) {
    if prefer_3d {
        enter_driver_cam(follow, mode, look);
    } else {
        enter_cab2d_cam(follow, mode, look, overlay);
    }
}

fn alt_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight)
}

fn ctrl_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
}

fn handle_passenger_camera_key(
    keys: &ButtonInput<KeyCode>,
    follow: &mut CameraFollowMode,
    mode: &mut CameraMode,
    look: &mut DriverLookOffset,
    passenger: Option<&mut PassengerCamState>,
    catalog: Option<&PassengerSeatCatalog>,
) {
    let Some(catalog) = catalog else {
        return;
    };
    let Some(pass) = passenger else {
        return;
    };
    let seats = catalog.seat_cars();
    if seats.is_empty() {
        viewer_log!("openrailsrs-viewer3d: passenger cam — no Inside viewpoints");
        return;
    }

    let ctrl = ctrl_held(keys);
    let shift = shift_held(keys);
    let alt = alt_held(keys);

    // Ctrl+Shift+5 — cycle viewpoints within the current car
    if ctrl && shift && !alt && *follow == CameraFollowMode::PassengerCam {
        let Some(&(consist_car, views)) = seats.get(pass.car_slot % seats.len()) else {
            return;
        };
        if views.len() <= 1 {
            return;
        }
        pass.view_index = (pass.view_index + 1) % views.len();
        if let Some(vp) = views.get(pass.view_index) {
            pass.apply_viewpoint(vp, consist_car);
            look.reset();
            viewer_log!(
                "openrailsrs-viewer3d: passenger viewpoint {}/{} car={}",
                pass.view_index + 1,
                views.len(),
                consist_car
            );
        }
        return;
    }

    if alt || ctrl {
        return;
    }

    // 5 — enter or advance to next car with seats (OR OnActivate sameCamera)
    if *follow == CameraFollowMode::PassengerCam {
        pass.car_slot = (pass.car_slot + 1) % seats.len();
        pass.view_index = 0;
    } else {
        pass.car_slot = 0;
        pass.view_index = 0;
    }
    let Some(&(consist_car, views)) = seats.get(pass.car_slot) else {
        return;
    };
    if let Some(vp) = views.first() {
        pass.apply_viewpoint(vp, consist_car);
        *follow = CameraFollowMode::PassengerCam;
        *mode = CameraMode::Orbit;
        look.reset();
        viewer_log!(
            "openrailsrs-viewer3d: passenger cam car {}/{} (consist #{})",
            pass.car_slot + 1,
            seats.len(),
            consist_car
        );
    }
}

/// Open Rails camera keys (InputSettings defaults):
/// **1** cab · **Alt+1** toggle 2D/3D · **Ctrl+Shift+1** 3D eyepoint ·
/// **2** outside front · **3** outside rear · **5** passenger · **8** free/fly.
pub fn cycle_follow_mode(
    keys: Res<ButtonInput<KeyCode>>,
    replay: Option<Res<crate::train::ReplayState>>,
    live: Option<Res<crate::live::LiveDrive>>,
    mut mode: ResMut<CameraMode>,
    mut follow: ResMut<CameraFollowMode>,
    mut look: ResMut<DriverLookOffset>,
    mut target: ResMut<CameraFollowTarget>,
    mut prefer_3d: ResMut<Prefer3dCab>,
    mut driver_cab: Option<ResMut<LiveDriverCab>>,
    mut overlay: Option<ResMut<crate::cab_cvf_overlay::CabCvfOverlayState>>,
    mut passenger: Option<ResMut<PassengerCamState>>,
    catalog: Option<Res<PassengerSeatCatalog>>,
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

    let digit1 = keys.just_pressed(KeyCode::Digit1) || keys.just_pressed(KeyCode::Numpad1);
    if digit1 {
        let alt = alt_held(&keys);
        let ctrl = ctrl_held(&keys);
        let shift = shift_held(&keys);

        // Ctrl+Shift+1 — CameraChange3DCabViewPoint
        if ctrl && shift && !alt {
            if *follow == CameraFollowMode::DriverCam {
                if let Some(ref mut cab) = driver_cab {
                    if cab.cycle_eyepoint() {
                        look.reset();
                        viewer_log!(
                            "openrailsrs-viewer3d: driver eyepoint {}/{} ({:?})",
                            cab.eyepoint_index + 1,
                            cab.eyepoints.len(),
                            cab.active_slot()
                        );
                    }
                }
            }
            return;
        }

        // Alt+1 — CameraToggleThreeDimensionalCab
        if alt && !ctrl {
            prefer_3d.0 = !prefer_3d.0;
            enter_preferred_cab(
                prefer_3d.0,
                &mut follow,
                &mut mode,
                &mut look,
                overlay.as_deref_mut(),
            );
            viewer_log!(
                "openrailsrs-viewer3d: cab toggle → {}",
                if prefer_3d.0 { "3D" } else { "2D" }
            );
            return;
        }

        // 1 — Camera Cab (enter preferred 2D/3D; stays in cab if already there)
        if !alt && !ctrl {
            enter_preferred_cab(
                prefer_3d.0,
                &mut follow,
                &mut mode,
                &mut look,
                overlay.as_deref_mut(),
            );
            return;
        }
    }

    // 2 — Camera Outside Front
    if keys.just_pressed(KeyCode::Digit2) || keys.just_pressed(KeyCode::Numpad2) {
        *follow = CameraFollowMode::ChaseCam;
        *mode = CameraMode::Orbit;
        look.reset();
        return;
    }

    // 3 — Camera Outside Rear (orbit follow as external alternate)
    if keys.just_pressed(KeyCode::Digit3) || keys.just_pressed(KeyCode::Numpad3) {
        *follow = CameraFollowMode::OrbitFollow;
        *mode = CameraMode::Orbit;
        look.reset();
        return;
    }

    // 5 — Passenger camera (cycle cars with Inside viewpoints)
    let digit5 = keys.just_pressed(KeyCode::Digit5) || keys.just_pressed(KeyCode::Numpad5);
    if digit5 {
        handle_passenger_camera_key(
            &keys,
            &mut follow,
            &mut mode,
            &mut look,
            passenger.as_deref_mut(),
            catalog.as_deref(),
        );
        return;
    }

    // 8 — Camera Free
    if keys.just_pressed(KeyCode::Digit8) || keys.just_pressed(KeyCode::Numpad8) {
        *follow = CameraFollowMode::Off;
        *mode = CameraMode::Fly;
        look.reset();
        return;
    }

    if live_active {
        if keys.just_pressed(KeyCode::KeyT) && !shift_held(&keys) {
            let next = follow.cycle();
            *follow = next;
            if next == CameraFollowMode::DriverCam || next.is_cab2d() {
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
    live_framing: Option<Res<crate::live::LiveTrainCameraFrame>>,
    cab: Option<Res<LiveDriverCab>>,
    look: Option<Res<DriverLookOffset>>,
    overlay: Option<Res<crate::cab_cvf_overlay::CabCvfOverlayState>>,
    passenger: Option<Res<PassengerCamState>>,
    train_query: Query<
        (&Transform, Option<&crate::train::TrainMarker>),
        (
            Without<OrbitState>,
            Without<crate::live::LiveTrainMarker>,
            Without<crate::live::LiveTrainCar>,
        ),
    >,
    live_train: Query<
        &Transform,
        (
            With<crate::live::LiveTrainMarker>,
            Without<OrbitState>,
            Without<crate::live::LiveTrainCar>,
        ),
    >,
    lead_car: Query<&GlobalTransform, With<crate::cab_view::CabLeadVehicle>>,
    passenger_cars: Query<
        (&Transform, &GlobalTransform, &crate::live::LiveTrainCar),
        (
            Without<crate::live::LiveTrainMarker>,
            Without<crate::train::TrainMarker>,
            Without<OrbitState>,
            Without<Camera3d>,
        ),
    >,
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
    if *mode != CameraMode::Orbit
        && *follow != CameraFollowMode::DriverCam
        && !follow.is_cab2d()
        && !follow.is_passenger()
    {
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
    let train_root_pose = TrainFollowPose {
        translation: train_tf.translation,
        yaw: yaw_from_transform(train_tf),
    };

    // Cab2d: same eyepoint as 3D cab so ACE window alpha shows the forward world.
    // Compose `.eng` StartDirection + CVF Direction (same forward as DriverCam, #169).
    if follow.is_cab2d() {
        orbit.focus = lerp_follow_focus(orbit.focus, train_root_pose.translation, dt);
        orbit.yaw = train_root_pose.yaw;
        let default_cab = LiveDriverCab::default();
        let cab = cab.as_deref().unwrap_or(&default_cab);
        let dir = overlay
            .as_ref()
            .map(|o| o.view_direction_deg)
            .unwrap_or([0.0; 3]);
        let look = cab_view_look_offset(cab, dir);
        *transform = lead_car
            .iter()
            .next()
            .map(|lead| driver_camera_transform_from_lead(lead, cab, look))
            .unwrap_or_else(|| driver_camera_transform(train_root_pose, cab, look));
        return;
    }

    if *follow == CameraFollowMode::DriverCam {
        orbit.focus = lerp_follow_focus(orbit.focus, train_root_pose.translation, dt);
        orbit.yaw = train_root_pose.yaw;
        let default_cab = LiveDriverCab::default();
        let cab = cab.as_deref().unwrap_or(&default_cab);
        let look = look.as_deref().copied().unwrap_or_default();
        *transform = lead_car
            .iter()
            .next()
            .map(|lead| driver_camera_transform_from_lead(lead, cab, look))
            .unwrap_or_else(|| driver_camera_transform(train_root_pose, cab, look));
        return;
    }

    if follow.is_passenger() {
        orbit.focus = lerp_follow_focus(orbit.focus, train_root_pose.translation, dt);
        orbit.yaw = train_root_pose.yaw;
        let Some(pass) = passenger.as_deref() else {
            return;
        };
        let look = look.as_deref().copied().unwrap_or_default();
        let mut seat_cab = LiveDriverCab {
            look_pitch: pass.look_pitch,
            look_yaw: pass.look_yaw,
            head_lead_local: Some(crate::shapes::msts_shape_vec3_to_bevy(pass.head_msts)),
            head_msts: Some(pass.head_msts),
            ..Default::default()
        };
        seat_cab.head_pos_train = seat_cab.head_lead_local;
        let car_gt = passenger_cars
            .iter()
            .find(|(_, _, car)| car.index == pass.consist_car)
            .map(|(_, gt, _)| *gt);
        if let Some(gt) = car_gt {
            *transform = driver_camera_transform_from_lead(&gt, &seat_cab, look);
        } else {
            *transform = driver_camera_transform(train_root_pose, &seat_cab, look);
        }
        return;
    }

    let frame = live_framing.as_deref().copied().unwrap_or_default();
    let live_car_pose = if live_active {
        let mut first: Option<(usize, Vec3)> = None;
        let mut last: Option<(usize, Vec3)> = None;
        for (local, _, car) in &passenger_cars {
            let world = train_tf.mul_transform(*local).translation;
            if first.is_none_or(|(index, _)| car.index < index) {
                first = Some((car.index, world));
            }
            if last.is_none_or(|(index, _)| car.index > index) {
                last = Some((car.index, world));
            }
        }
        first
            .zip(last)
            .and_then(|((_, head), (_, tail))| consist_chase_pose(head, tail))
    } else {
        None
    };
    let (train_pose, chase_distance_m) = live_car_pose.unwrap_or_else(|| {
        let train_focus = if live_active {
            frame.focus_from_train(train_tf)
        } else {
            train_root_pose.translation
        };
        (
            TrainFollowPose {
                translation: train_focus,
                yaw: train_root_pose.yaw,
            },
            if live_active {
                frame.chase_distance_m
            } else {
                LIVE_CHASE_DISTANCE
            },
        )
    });
    let update = apply_orbit_follow(*orbit, *follow, train_pose, chase_distance_m, dt);
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
        // OR: A/D=throttle, W/S=reverser — pan with I/J/K/L instead.
        if keys.pressed(KeyCode::KeyI) {
            axes.z += 1.0;
        }
        if keys.pressed(KeyCode::KeyK) {
            axes.z -= 1.0;
        }
        if keys.pressed(KeyCode::KeyL) {
            axes.x += 1.0;
        }
        if keys.pressed(KeyCode::KeyJ) {
            axes.x -= 1.0;
        }
    } else {
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
    passenger: Option<Res<PassengerCamState>>,
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

    if follow.is_cab2d() {
        // Fixed ACE views (←/→); look comes from CVF Direction in follow_train_camera.
        motion.clear();
        wheel.clear();
        return;
    }

    if *follow == CameraFollowMode::DriverCam || follow.is_passenger() {
        let mut delta = Vec2::ZERO;
        for ev in motion.read() {
            delta += ev.delta;
        }
        wheel.clear();
        if keys.just_pressed(KeyCode::Home) {
            look.reset();
        }
        // Look via mouse (OR RMB). Arrows stay throttle aliases in live.
        let (yaw_lim, pitch_lim) = if *follow == CameraFollowMode::DriverCam {
            // OR InsideThreeDimCamera ignores eng RotationLimit for 3D cab.
            (DRIVER_CAM_FREE_YAW_MAX, DRIVER_CAM_FREE_PITCH_MAX)
        } else {
            passenger
                .as_ref()
                .map(|p| (p.yaw_limit, p.pitch_limit))
                .unwrap_or((70f32.to_radians(), 30f32.to_radians()))
        };
        look.yaw = look.yaw.clamp(-yaw_lim, yaw_lim);
        look.pitch = look.pitch.clamp(-pitch_lim, pitch_lim);
        // OR RotateByMouse: right mouse button only (LMB reserved for cab controls).
        if mouse_buttons.pressed(MouseButton::Right) && delta != Vec2::ZERO {
            look.apply_drag_clamped(delta, yaw_lim, pitch_lim);
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
    live: Option<Res<crate::live::LiveDrive>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut query: Query<
        (&mut Transform, &mut FlyState),
        (With<Camera3d>, Without<crate::train::TrainMarker>),
    >,
) {
    if *follow == CameraFollowMode::DriverCam || follow.is_cab2d() || follow.is_passenger() {
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

    // OR: Space = horn in live — never use it as fly-up while driving.
    let axes = read_fly_axes(&keys, replay.as_deref(), live.is_some());
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

fn read_fly_axes(
    keys: &ButtonInput<KeyCode>,
    replay: Option<&crate::train::ReplayState>,
    live_blocks_space: bool,
) -> Vec3 {
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
    if keys.pressed(KeyCode::Space) && !replay_blocks_space(replay) && !live_blocks_space {
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
    *follow == CameraFollowMode::DriverCam
        || follow.is_cab2d()
        || follow.is_passenger()
        || *mode == CameraMode::Orbit
}

pub fn fly_camera_allowed(follow: Res<CameraFollowMode>) -> bool {
    *follow != CameraFollowMode::DriverCam && !follow.is_cab2d() && !follow.is_passenger()
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
    if *follow == CameraFollowMode::DriverCam || follow.is_cab2d() || follow.is_passenger() {
        // Cab2d needs the forward world through ACE window alpha.
        // Passenger uses the same near clip as InsideThreeDimCamera.
        persp.fov = driver_cab_fov_deg(&opts).to_radians();
        persp.near = DRIVER_NEAR_CLIP_M;
        ambient.brightness = 350.0;
        ambient.color = Color::srgb(0.95, 0.94, 0.92);
        *tonemapping = Tonemapping::None;
        *exposure = Exposure::BLENDER;
        // Do not toggle `Msaa` here: Bevy 0.19 crashes when the main pass switches to
        // sample count 4 while cached `opaque_mesh_pipeline` (world/StandardMaterial)
        // is still specialized for sample count 1. Keep camera `Msaa::Off` for the session.
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
    use bevy::ecs::system::RunSystemOnce;
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
    fn viewer_camera_has_no_mesh_and_is_explicitly_not_a_shadow_caster() {
        let mut app = App::new();
        app.insert_resource(ViewerLaunchOpts::default());
        app.world_mut().run_system_once(spawn_camera).unwrap();

        let world = app.world_mut();
        let mut cameras =
            world.query_filtered::<(Option<&Mesh3d>, Option<&NotShadowCaster>), With<Camera3d>>();
        let (mesh, no_shadow) = cameras.single(world).expect("viewer camera");
        assert!(mesh.is_none());
        assert!(no_shadow.is_some());
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
    fn chase_yaw_from_train_uses_local_x_travel_axis() {
        let yaw = chase_yaw_from_train(0.0);
        assert!((yaw + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        let camera_offset = orbit_position(Vec3::ZERO, yaw, 0.0, 10.0);
        assert!(
            camera_offset.x < -9.99 && camera_offset.z.abs() < 1e-4,
            "train yaw 0 travels +X, so chase camera must sit on -X: {camera_offset:?}"
        );
    }

    #[test]
    fn consist_chase_pose_uses_placed_head_and_tail() {
        let head = Vec3::new(81.0, 0.0, -4.0);
        let tail = Vec3::new(84.0, 0.0, 74.0);
        let (pose, distance) = consist_chase_pose(head, tail).expect("consist pose");
        let forward = (head - tail).normalize();
        assert!(((pose.translation - head).dot(forward) - 12.0).abs() < 1e-4);
        assert!((distance - LIVE_CHASE_DISTANCE).abs() < 1e-4);
        let camera = orbit_position(
            pose.translation,
            chase_yaw_from_train(pose.yaw),
            CHASE_PITCH,
            distance,
        );
        let behind_head_m = (camera - head).xz().dot((tail - head).xz().normalize());
        let toward_tail = (tail - head).xz().normalize();
        assert!(
            (camera - pose.translation)
                .xz()
                .normalize()
                .dot(toward_tail)
                > 0.99,
            "camera must sit behind the placed head: {camera:?}"
        );
        assert!(
            behind_head_m > 14.0 && behind_head_m < 16.0 && camera.y > 6.0,
            "camera should be close and above the leading cars: {camera:?}"
        );
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
        assert_eq!(CameraFollowMode::Cab2d.cycle(), CameraFollowMode::Off);
        assert_eq!(
            CameraFollowMode::PassengerCam.cycle(),
            CameraFollowMode::Off
        );
    }

    #[test]
    fn camera_follow_mode_hud_labels() {
        assert_eq!(CameraFollowMode::Off.hud_label(), "off");
        assert_eq!(CameraFollowMode::OrbitFollow.hud_label(), "orbit");
        assert_eq!(CameraFollowMode::ChaseCam.hud_label(), "chase");
        assert_eq!(CameraFollowMode::DriverCam.hud_label(), "driver");
        assert_eq!(CameraFollowMode::Cab2d.hud_label(), "cab2d");
        assert_eq!(CameraFollowMode::PassengerCam.hud_label(), "passenger");
        assert!(CameraFollowMode::Cab2d.is_cab2d());
        assert!(!CameraFollowMode::DriverCam.is_cab2d());
        assert!(CameraFollowMode::PassengerCam.is_passenger());
    }

    #[test]
    fn msts_direction_matches_start_direction_pitch_sign() {
        let from_dir = msts_direction_to_look_offset([10.0, 0.0, 0.0]);
        let cab = LiveDriverCab {
            look_pitch: -(10f32).to_radians(),
            look_yaw: 0.0,
            ..Default::default()
        };
        assert!((from_dir.pitch - cab.look_pitch).abs() < 1e-5);
        assert!(
            from_dir.pitch < 0.0,
            "positive MSTS X = look down = negative Bevy pitch"
        );
    }

    #[test]
    fn driver_camera_transform_places_eye_in_cab() {
        let cab = LiveDriverCab {
            back_m: 3.0,
            height_m: 2.5,
            head_pos_train: None,
            look_pitch: DRIVER_LOOK_PITCH,
            ..Default::default()
        };
        let tf = driver_camera_transform(
            TrainFollowPose {
                translation: Vec3::new(10.0, 0.0, 20.0),
                yaw: 0.0,
            },
            &cab,
            DriverLookOffset::default(),
        );
        assert!((tf.translation.x - 7.0).abs() < 1e-5);
        assert!((tf.translation.y - 2.5).abs() < 1e-5);
        assert!((tf.translation.z - 20.0).abs() < 1e-5);
    }

    #[test]
    fn driver_camera_uses_orts_head_pos_when_set() {
        let head_train = Vec3::new(1.7, 3.63, 0.8);
        let cab = LiveDriverCab {
            back_m: 3.0,
            height_m: 2.5,
            head_pos_train: Some(head_train),
            look_pitch: -15.0_f32.to_radians(),
            ..Default::default()
        };
        let tf = driver_camera_transform(
            TrainFollowPose {
                translation: Vec3::new(100.0, 0.0, 50.0),
                yaw: 0.0,
            },
            &cab,
            DriverLookOffset::default(),
        );
        assert!((tf.translation.x - 101.7).abs() < 1e-4);
        assert!((tf.translation.y - 3.63).abs() < 1e-4);
        assert!((tf.translation.z - 50.8).abs() < 1e-4);
    }

    #[test]
    fn driver_root_fallback_looks_along_train_local_x() {
        let yaw = 0.37;
        let cab = LiveDriverCab {
            look_pitch: 0.0,
            look_yaw: 0.0,
            ..Default::default()
        };
        let tf = driver_camera_transform_at_eye(Vec3::ZERO, yaw, &cab, DriverLookOffset::default());
        let expected = Quat::from_rotation_y(yaw).mul_vec3(Vec3::X);
        assert!(
            tf.forward().as_vec3().dot(expected) > 0.99,
            "train-root fallback must retain its −90° axis conversion"
        );
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
        let eye = driver_eye_from_lead(&lead_global, &cab).expect("head_lead_local");
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
        let a = driver_camera_transform_from_lead(&lead_global, &cab, DriverLookOffset::default());
        let b = driver_camera_transform_from_lead(
            &lead_global,
            &cab,
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
            driver_camera_transform_from_lead(&lead_global, &cab, DriverLookOffset::default());
        let turned = driver_camera_transform_from_lead(
            &lead_global,
            &cab,
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
    fn driver_look_pitch_changes_vertical_forward() {
        // Regression: user * base made pitch roll around train +X after −π/2 cab yaw.
        let lead_global = GlobalTransform::from(Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        });
        let cab = LiveDriverCab {
            look_pitch: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let level =
            driver_camera_transform_from_lead(&lead_global, &cab, DriverLookOffset::default());
        let pitched = driver_camera_transform_from_lead(
            &lead_global,
            &cab,
            DriverLookOffset {
                yaw: 0.0,
                pitch: 0.35,
            },
        );
        let level_y = level.forward().as_vec3().y;
        let pitched_y = pitched.forward().as_vec3().y;
        assert!(
            (pitched_y - level_y).abs() > 0.2,
            "pitch must tilt view up/down, not roll: level_y={level_y} pitched_y={pitched_y}"
        );
    }

    #[test]
    fn driver_camera_applies_start_direction_yaw() {
        let lead_global = GlobalTransform::from(Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        });
        let front = LiveDriverCab {
            look_pitch: 0.0,
            look_yaw: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let rear = LiveDriverCab {
            look_pitch: 0.0,
            look_yaw: std::f32::consts::PI,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let front_fwd =
            driver_camera_transform_from_lead(&lead_global, &front, DriverLookOffset::default())
                .forward()
                .as_vec3();
        let rear_fwd =
            driver_camera_transform_from_lead(&lead_global, &rear, DriverLookOffset::default())
                .forward()
                .as_vec3();
        let cab_forward = lead_global.rotation().mul_vec3(Vec3::NEG_Z);
        let travel_h = Vec3::new(cab_forward.x, 0.0, cab_forward.z).normalize();
        let front_h = Vec3::new(front_fwd.x, 0.0, front_fwd.z).normalize();
        let rear_h = Vec3::new(rear_fwd.x, 0.0, rear_fwd.z).normalize();
        assert!(
            front_h.dot(travel_h) > 0.99,
            "front StartDirection.Y=0 should look through cab-local -Z: {front_h:?} vs {travel_h:?}"
        );
        assert!(
            rear_h.dot(-travel_h) > 0.99,
            "rear StartDirection.Y=180 should look opposite travel: {rear_h:?} vs {travel_h:?}"
        );
        assert!(
            front_fwd.dot(rear_fwd) < -0.9,
            "rear StartDirection.Y=180 should look opposite: {front_fwd:?} vs {rear_fwd:?}"
        );
    }

    #[test]
    fn cab2d_and_driver_share_forward_for_frontal_view() {
        let train_yaw = 0.4;
        let placement = Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        };
        let train = Transform::from_rotation(Quat::from_rotation_y(train_yaw));
        let lead_global = GlobalTransform::from(train.mul_transform(placement));
        let cab = LiveDriverCab {
            look_pitch: -15f32.to_radians(),
            look_yaw: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        // Cab2d frontal CVF Direction (0,0,0) + StartDirection — same as DriverCam idle.
        let look_2d = cab_view_look_offset(&cab, [0.0; 3]);
        let fwd_2d = driver_camera_transform_from_lead(&lead_global, &cab, look_2d)
            .forward()
            .as_vec3();
        let fwd_3d =
            driver_camera_transform_from_lead(&lead_global, &cab, DriverLookOffset::default())
                .forward()
                .as_vec3();
        assert!(
            fwd_2d.dot(fwd_3d) > 0.99,
            "2D/3D frontal forwards must match: 2d={fwd_2d:?} 3d={fwd_3d:?}"
        );
        let cab_forward = lead_global.rotation().mul_vec3(Vec3::NEG_Z);
        let h2 = Vec3::new(fwd_2d.x, 0.0, fwd_2d.z).normalize();
        let ht = Vec3::new(cab_forward.x, 0.0, cab_forward.z).normalize();
        assert!(
            h2.dot(ht) > 0.99,
            "both should face the windshield: {h2:?} vs {ht:?}"
        );
    }

    #[test]
    fn cab_view_rear_direction_inverts_once() {
        let lead_global = GlobalTransform::from(Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        });
        let cab = LiveDriverCab {
            look_pitch: 0.0,
            look_yaw: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let front = driver_camera_transform_from_lead(
            &lead_global,
            &cab,
            cab_view_look_offset(&cab, [0.0; 3]),
        )
        .forward()
        .as_vec3();
        let rear = driver_camera_transform_from_lead(
            &lead_global,
            &cab,
            cab_view_look_offset(&cab, [0.0, 180.0, 0.0]),
        )
        .forward()
        .as_vec3();
        let fh = Vec3::new(front.x, 0.0, front.z).normalize();
        let rh = Vec3::new(rear.x, 0.0, rear.z).normalize();
        assert!(
            fh.dot(rh) < -0.99,
            "rear CVF Direction flips once: {fh:?} {rh:?}"
        );
    }

    #[test]
    fn cab_view_respects_flipped_lead_frame() {
        let placement = crate::shapes::vehicle_authored_frame_transform(0.0, true);
        let train = Transform::from_rotation(Quat::from_rotation_y(0.25));
        let lead_global = GlobalTransform::from(train.mul_transform(placement));
        let cab = LiveDriverCab {
            look_pitch: 0.0,
            look_yaw: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let look = cab_view_look_offset(&cab, [0.0; 3]);
        let fwd = driver_camera_transform_from_lead(&lead_global, &cab, look)
            .forward()
            .as_vec3();
        let travel = lead_global.rotation().mul_vec3(Vec3::NEG_Z);
        let fh = Vec3::new(fwd.x, 0.0, fwd.z).normalize();
        let th = Vec3::new(travel.x, 0.0, travel.z).normalize();
        assert!(
            fh.dot(th) > 0.99,
            "Flip lead frame: camera still looks along cab-local -Z: {fh:?} vs {th:?}"
        );
    }

    #[test]
    fn driver_look_respects_rotation_limit() {
        let mut look = DriverLookOffset::default();
        look.apply_drag_clamped(
            Vec2::new(10_000.0, 0.0),
            10f32.to_radians(),
            5f32.to_radians(),
        );
        assert!((look.yaw.abs() - 10f32.to_radians()).abs() < 1e-3);
        look.apply_drag_clamped(
            Vec2::new(0.0, 10_000.0),
            10f32.to_radians(),
            5f32.to_radians(),
        );
        assert!((look.pitch.abs() - 5f32.to_radians()).abs() < 1e-3);
    }

    #[test]
    fn cycle_eyepoint_advances_head_and_slot() {
        let placement = crate::shapes::vehicle_authored_frame_transform(0.0, false);
        let mut cab = LiveDriverCab {
            interior_placement: placement,
            eyepoints: vec![
                DriverEyepoint {
                    head_msts: Vec3::new(-0.8, 2.875, 8.60),
                    look_pitch: -15f32.to_radians(),
                    look_yaw: 0.0,
                    pitch_limit: Some(10f32.to_radians()),
                    yaw_limit: Some(90f32.to_radians()),
                    slot: DriverViewSlot::CabViewpoint,
                },
                DriverEyepoint {
                    head_msts: Vec3::new(1.2, 2.5, 7.0),
                    look_pitch: 0.0,
                    look_yaw: 0.0,
                    pitch_limit: None,
                    yaw_limit: None,
                    slot: DriverViewSlot::HeadOut,
                },
            ],
            ..Default::default()
        };
        cab.apply_eyepoint(cab.eyepoints[0], placement);
        assert_eq!(cab.active_slot(), DriverViewSlot::CabViewpoint);
        assert!(cab.cycle_eyepoint());
        assert_eq!(cab.eyepoint_index, 1);
        assert_eq!(cab.active_slot(), DriverViewSlot::HeadOut);
        assert_eq!(cab.head_msts, Some(Vec3::new(1.2, 2.5, 7.0)));
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
        let cab = LiveDriverCab {
            look_pitch: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let tf = driver_camera_transform_from_lead(&lead_global, &cab, DriverLookOffset::default());
        let cam_fwd = tf.forward().as_vec3();
        // Converted MSTS cab forward is lead-local −Z.
        let travel_fwd = lead_global.rotation().mul_vec3(Vec3::NEG_Z);
        let cam_h = Vec3::new(cam_fwd.x, 0.0, cam_fwd.z).normalize();
        let travel_h = Vec3::new(travel_fwd.x, 0.0, travel_fwd.z).normalize();
        assert!(
            cam_h.dot(travel_h) > 0.99,
            "cam={cam_h:?} travel={travel_h:?}"
        );
    }

    #[test]
    fn driver_camera_aligns_with_cab_windshield_forward() {
        let train_yaw = -0.7;
        let placement = Transform {
            rotation: crate::shapes::msts_shape_to_train_rotation(),
            ..default()
        };
        let train = Transform::from_rotation(Quat::from_rotation_y(train_yaw));
        let lead_global = GlobalTransform::from(train.mul_transform(placement));
        let cab = LiveDriverCab {
            look_pitch: -15f32.to_radians(),
            look_yaw: 0.0,
            head_lead_local: Some(Vec3::ZERO),
            ..Default::default()
        };
        let cam_fwd =
            driver_camera_transform_from_lead(&lead_global, &cab, DriverLookOffset::default())
                .forward()
                .as_vec3();
        // Lead frame already includes shape→train; do not apply that basis twice.
        let travel = lead_global.rotation().mul_vec3(Vec3::NEG_Z);
        let cam_h = Vec3::new(cam_fwd.x, 0.0, cam_fwd.z).normalize();
        let travel_h = Vec3::new(travel.x, 0.0, travel.z).normalize();
        assert!(
            cam_h.dot(travel_h) > 0.99,
            "cam={cam_h:?} travel={travel_h:?}"
        );
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
        let update = apply_orbit_follow(
            orbit,
            CameraFollowMode::OrbitFollow,
            train,
            LIVE_CHASE_DISTANCE,
            0.05,
        );
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
        let update = apply_orbit_follow(
            orbit,
            CameraFollowMode::ChaseCam,
            train,
            LIVE_CHASE_DISTANCE,
            0.5,
        );
        assert!(
            update.yaw < 0.0,
            "local +X travel puts the chase camera toward -X"
        );
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
        let update = apply_orbit_follow(
            orbit,
            CameraFollowMode::OrbitFollow,
            train,
            LIVE_CHASE_DISTANCE,
            0.016,
        );
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
        let axes = read_fly_axes(&keys, None, false);
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
        let axes = read_fly_axes(&keys, Some(&replay), false);
        assert_eq!(axes.y, 0.0);
    }

    #[test]
    fn read_fly_axes_space_blocked_during_live() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Space);
        let axes = read_fly_axes(&keys, None, true);
        assert_eq!(axes.y, 0.0);
    }
}
