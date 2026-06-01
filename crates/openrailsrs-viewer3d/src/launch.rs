//! Launch-time options (set from `main` before the viewer plugin runs).

use bevy::prelude::*;

/// What to draw from `.w` / terrain (full route vs track-focused debug).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewerSceneryMode {
    #[default]
    Full,
    /// Grafo `track.toml`, vía procedural continua desde `.tdb`; sin terreno ni shapes.
    TrackDev,
}

impl ViewerSceneryMode {
    pub fn is_track_dev(self) -> bool {
        matches!(self, Self::TrackDev)
    }
}

pub fn full_scenery_active(mode: Res<ViewerSceneryMode>) -> bool {
    !mode.is_track_dev()
}

pub fn track_dev_active(mode: Res<ViewerSceneryMode>) -> bool {
    mode.is_track_dev()
}

/// Options chosen at process start (live vs replay, etc.).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ViewerLaunchOpts {
    pub live: bool,
}

/// Cap flat ground in live mode and track-dev debug (route bbox can be 100+ km).
pub const LIVE_GROUND_HALF_MAX_M: f32 = 2000.0;
pub const TRACK_DEV_GROUND_HALF_M: f32 = 600.0;

/// Default cull radius for `.tdb` procedural track in `--track-dev` (metres).
pub const TRACK_DEV_TDB_RADIUS_M: f32 = 1500.0;

/// Max rail segments spawned in `--track-dev` (avoids OOM on large routes).
pub const TRACK_DEV_MAX_SEGMENTS: usize = 4000;

/// Max branch walks started from `.tdb` end nodes near the focus.
pub const TRACK_DEV_MAX_BRANCHES: usize = 512;

/// Cull radius for `.tdb` chords/segments; override with `OPENRAILSRS_TRACK_DEV_RADIUS_M`.
pub fn track_dev_tdb_radius_m() -> f32 {
    std::env::var("OPENRAILSRS_TRACK_DEV_RADIUS_M")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|r| *r > 50.0 && *r <= 8000.0)
        .unwrap_or(TRACK_DEV_TDB_RADIUS_M)
}

/// Initial orbit distance in `--track-dev` (metres); avoids framing the whole route bbox.
pub const TRACK_DEV_ORBIT_DISTANCE_M: f32 = 100.0;

/// Max orbit zoom in `--track-dev` (full-route bbox can reach 500 km otherwise).
pub const TRACK_DEV_ORBIT_MAX_M: f32 = 2_000.0;

/// When false (default), `--track-dev` runs audit only — no rail meshes (saves RAM).
pub fn track_dev_render_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_TRACK_DEV_RENDER").is_some_and(|v| {
        let s = v.to_string_lossy();
        !s.eq_ignore_ascii_case("0") && s != "false"
    })
}

/// Full TrPins branch walk only on small `.tdb` files (large routes OOM if walked globally).
pub const TRACK_DEV_BRANCH_WALK_MAX_NODES: usize = 800;

/// Shape LOD distance for the player consist in live drive.
///
/// The camera is often close to the consist in live mode; using a far-distance
/// LOD can expose simplified/interior geometry instead of the exterior shell.
pub const LIVE_TRAIN_LOD_DISTANCE_M: f32 = 25.0;
