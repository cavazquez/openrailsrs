//! Launch-time options (set from `main` before the viewer plugin runs).

use bevy::prelude::*;

/// What to draw from `.w` / terrain (full route vs track-focused views).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewerSceneryMode {
    #[default]
    Full,
    /// Grafo `track.toml`, vía procedural continua desde `.tdb`; sin terreno ni shapes.
    TrackDev,
    /// Minimal playable view: train + `.tdb` procedural route corridor, no scenery/world tiles.
    RunCorridor,
    /// Laboratorio de validación: UN solo tile (el del anchor) con capas opt-in
    /// (`OPENRAILSRS_TILE_LAB_LAYERS`), cámara fija sobre el centro del tile y
    /// sin streaming. Para validar un elemento a la vez contra Open Rails.
    TileLab,
}

impl ViewerSceneryMode {
    pub fn is_track_dev(self) -> bool {
        matches!(self, Self::TrackDev)
    }

    pub fn is_run_corridor(self) -> bool {
        matches!(self, Self::RunCorridor)
    }

    pub fn is_tile_lab(self) -> bool {
        matches!(self, Self::TileLab)
    }

    pub fn is_track_focused(self) -> bool {
        matches!(self, Self::TrackDev | Self::RunCorridor)
    }

    pub fn draws_tdb_track(self) -> bool {
        matches!(self, Self::TrackDev | Self::RunCorridor)
    }
}

/// Capas habilitadas en `--tile-lab` (todas opt-in salvo terreno).
#[derive(Clone, Copy, Debug)]
pub struct TileLabLayers {
    pub terrain: bool,
    pub track: bool,
    pub world: bool,
    pub train: bool,
}

impl Default for TileLabLayers {
    fn default() -> Self {
        Self {
            terrain: true,
            track: false,
            world: false,
            train: false,
        }
    }
}

impl TileLabLayers {
    pub fn label(&self) -> String {
        let mut on = Vec::new();
        if self.terrain {
            on.push("terrain");
        }
        if self.track {
            on.push("track");
        }
        if self.world {
            on.push("world");
        }
        if self.train {
            on.push("train");
        }
        if on.is_empty() {
            "(ninguna)".to_string()
        } else {
            on.join(",")
        }
    }
}

/// Parse `OPENRAILSRS_TILE_LAB_LAYERS` (lista separada por comas de
/// `terrain,track,world,train`; `all` habilita todo). Default: solo `terrain`.
pub fn tile_lab_layers() -> TileLabLayers {
    let Ok(raw) = std::env::var("OPENRAILSRS_TILE_LAB_LAYERS") else {
        return TileLabLayers::default();
    };
    let mut layers = TileLabLayers {
        terrain: false,
        track: false,
        world: false,
        train: false,
    };
    for token in raw.split(',').map(|t| t.trim().to_ascii_lowercase()) {
        match token.as_str() {
            "terrain" => layers.terrain = true,
            "track" => layers.track = true,
            "world" => layers.world = true,
            "train" => layers.train = true,
            "all" => {
                layers = TileLabLayers {
                    terrain: true,
                    track: true,
                    world: true,
                    train: true,
                }
            }
            "" => {}
            other => {
                crate::viewer_log!(
                    "openrailsrs-viewer3d: tile-lab — capa desconocida \"{other}\" (válidas: terrain,track,world,train,all)"
                );
            }
        }
    }
    layers
}

/// Distancia orbital inicial en `--tile-lab` (encuadra el tile de 2048 m).
pub const TILE_LAB_ORBIT_DISTANCE_M: f32 = 2_600.0;
/// Zoom máximo en `--tile-lab`.
pub const TILE_LAB_ORBIT_MAX_M: f32 = 8_000.0;

pub fn full_scenery_active(mode: Res<ViewerSceneryMode>) -> bool {
    !mode.is_track_focused()
}

/// Sky dome for full routes and run_corridor (driver cab windows need a backdrop).
pub fn sky_dome_active(mode: Res<ViewerSceneryMode>) -> bool {
    matches!(
        *mode,
        ViewerSceneryMode::Full | ViewerSceneryMode::RunCorridor
    )
}

pub fn track_dev_active(mode: Res<ViewerSceneryMode>) -> bool {
    mode.is_track_focused()
}

/// Options chosen at process start (live vs replay, etc.).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ViewerLaunchOpts {
    pub live: bool,
    /// Driver cab FOV override (degrees), from `--cab-fov`.
    pub cab_fov_deg: Option<f32>,
}

/// Cap flat ground in live mode and track-dev debug (route bbox can be 100+ km).
pub const LIVE_GROUND_HALF_MAX_M: f32 = 2000.0;
pub const TRACK_DEV_GROUND_HALF_M: f32 = 600.0;

/// Default cull radius for `.tdb` procedural track in `--track-dev` (metres).
pub const TRACK_DEV_TDB_RADIUS_M: f32 = 1500.0;
/// Default cull radius for `.tdb` procedural track in `--run-corridor` (metres).
pub const RUN_CORRIDOR_TDB_RADIUS_M: f32 = 3000.0;
/// Default corridor half-width around the scenario graph path in `--run-corridor`.
pub const RUN_CORRIDOR_HALF_WIDTH_M: f32 = 120.0;

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

pub fn run_corridor_tdb_radius_m() -> f32 {
    std::env::var("OPENRAILSRS_RUN_CORRIDOR_RADIUS_M")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|r| *r > 50.0 && *r <= 20_000.0)
        .unwrap_or(RUN_CORRIDOR_TDB_RADIUS_M)
}

pub fn run_corridor_half_width_m() -> f32 {
    std::env::var("OPENRAILSRS_RUN_CORRIDOR_WIDTH_M")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .map(|w| w * 0.5)
        .filter(|half| *half >= 5.0 && *half <= 1000.0)
        .unwrap_or(RUN_CORRIDOR_HALF_WIDTH_M)
}

pub fn tdb_radius_for_mode(mode: ViewerSceneryMode) -> f32 {
    if mode.is_run_corridor() {
        run_corridor_tdb_radius_m()
    } else {
        track_dev_tdb_radius_m()
    }
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

/// Scenario graph path used to keep `--run-corridor` visually focused on the driven route.
#[derive(Resource, Clone, Debug)]
pub struct RunCorridorPath {
    pub points_world: Vec<Vec3>,
    pub half_width_m: f32,
}

impl Default for RunCorridorPath {
    fn default() -> Self {
        Self {
            points_world: Vec::new(),
            half_width_m: RUN_CORRIDOR_HALF_WIDTH_M,
        }
    }
}

impl RunCorridorPath {
    pub fn active(&self) -> bool {
        self.points_world.len() >= 2
    }

    pub fn contains_segment(&self, start: Vec3, end: Vec3) -> bool {
        if !self.active() {
            return true;
        }
        let mid = (start + end) * 0.5;
        let half = self.half_width_m;
        self.distance_to_polyline_xz(start) <= half
            || self.distance_to_polyline_xz(mid) <= half
            || self.distance_to_polyline_xz(end) <= half
    }

    fn distance_to_polyline_xz(&self, point: Vec3) -> f32 {
        self.points_world
            .windows(2)
            .map(|pair| point_segment_distance_xz(point, pair[0], pair[1]))
            .fold(f32::INFINITY, f32::min)
    }
}

fn point_segment_distance_xz(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    let p = Vec2::new(point.x, point.z);
    let a = Vec2::new(a.x, a.z);
    let b = Vec2::new(b.x, b.z);
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Full TrPins branch walk only on small `.tdb` files (large routes OOM if walked globally).
pub const TRACK_DEV_BRANCH_WALK_MAX_NODES: usize = 800;

/// Shape LOD distance for the player consist in live drive.
///
/// The camera is often close to the consist in live mode; using a far-distance
/// LOD can expose simplified/interior geometry instead of the exterior shell.
pub const LIVE_TRAIN_LOD_DISTANCE_M: f32 = 25.0;
