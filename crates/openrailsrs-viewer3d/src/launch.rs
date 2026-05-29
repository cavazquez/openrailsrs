//! Launch-time options (set from `main` before the viewer plugin runs).

use bevy::prelude::*;

/// Options chosen at process start (live vs replay, etc.).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ViewerLaunchOpts {
    pub live: bool,
}

/// Cap flat ground size in live mode so shadow/fill passes stay cheap on huge routes.
pub const LIVE_GROUND_HALF_MAX_M: f32 = 2000.0;

/// Shape LOD distance for the player consist in live drive.
///
/// The camera is often close to the consist in live mode; using a far-distance
/// LOD can expose simplified/interior geometry instead of the exterior shell.
pub const LIVE_TRAIN_LOD_DISTANCE_M: f32 = 25.0;
