//! Launch-time options (set from `main` before the viewer plugin runs).

use bevy::prelude::*;

/// Options chosen at process start (live vs replay, etc.).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ViewerLaunchOpts {
    pub live: bool,
}

/// Cap flat ground size in live mode so shadow/fill passes stay cheap on huge routes.
pub const LIVE_GROUND_HALF_MAX_M: f32 = 2000.0;

/// Shape LOD distance for the player consist in live drive (coarser than max detail).
pub const LIVE_TRAIN_LOD_DISTANCE_M: f32 = 600.0;
