//! Launch-time options (set from `main` before the viewer plugin runs).

use std::sync::OnceLock;

use crate::coordinates::MSTS_TILE_SIZE_M;
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

    pub fn loads_msts_scenery(self) -> bool {
        !self.is_track_focused() || (self.is_run_corridor() && run_corridor_scenery_enabled())
    }

    pub fn draws_tdb_track(self) -> bool {
        matches!(self, Self::TrackDev | Self::RunCorridor)
    }
}

/// When set, `--run-corridor --live` also loads WORLD/terreno/shapes (estación, marquesina).
/// Vía sigue siendo procedural `.tdb` en ventana móvil.
pub fn run_corridor_scenery_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_RUN_CORRIDOR_SCENERY").is_some_and(|v| {
        let s = v.to_string_lossy();
        !matches!(s.as_ref(), "0" | "false" | "off")
    })
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
    mode.loads_msts_scenery()
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

/// Default viewing distance (metres) — ≈ one MSTS tile (2048 m), Open Rails–style.
///
/// Historical alias [`VIEW_RADIUS_M`] kept for callers; prefer
/// [`VIEWING_DISTANCE_M`] / [`view_radius_m`].
pub const VIEWING_DISTANCE_M: f32 = 2000.0;
/// Backward-compatible name for [`VIEWING_DISTANCE_M`].
pub const VIEW_RADIUS_M: f32 = VIEWING_DISTANCE_M;

/// Extra metres beyond the load radius before a tile is unloaded (hysteresis).
/// Half a tile keeps neighbours active while crossing a boundary.
pub const VIEW_UNLOAD_HYSTERESIS_M: f32 = 1024.0;

/// Allowed viewing-distance range (CLI / config / env).
pub const VIEWING_DISTANCE_MIN_M: f32 = 50.0;
pub const VIEWING_DISTANCE_MAX_M: f32 = 16_000.0;

/// Default cull radius for `.tdb` procedural track in `--track-dev` (metres).
pub const TRACK_DEV_TDB_RADIUS_M: f32 = 1500.0;
/// Default cull radius for `.tdb` procedural track in `--run-corridor` (metres).
pub const RUN_CORRIDOR_TDB_RADIUS_M: f32 = 150.0;
/// Default corridor half-width around the scenario graph path in `--run-corridor`.
pub const RUN_CORRIDOR_HALF_WIDTH_M: f32 = 120.0;
/// Longitudinal window ahead of train in `--run-corridor` (metres).
pub const RUN_CORRIDOR_AHEAD_M: f32 = 80.0;
/// Longitudinal window behind train in `--run-corridor` (metres).
pub const RUN_CORRIDOR_BEHIND_M: f32 = 40.0;

static VIEWING_DISTANCE_OVERRIDE: OnceLock<f32> = OnceLock::new();

/// Pin the process-wide viewing distance (CLI / scenario config). Call once at startup.
pub fn set_viewing_distance_m(meters: f32) {
    let clamped = meters.clamp(VIEWING_DISTANCE_MIN_M, VIEWING_DISTANCE_MAX_M);
    let _ = VIEWING_DISTANCE_OVERRIDE.set(clamped);
}

/// Resolve viewing distance: CLI/config override → env → default.
///
/// Env: `OPENRAILSRS_VIEW_RADIUS_M` or legacy `OPENRAILSRS_VISIBLE_RADIUS_M`.
pub fn view_radius_m() -> f32 {
    VIEWING_DISTANCE_OVERRIDE
        .get()
        .copied()
        .or_else(|| parse_radius_env("OPENRAILSRS_VIEW_RADIUS_M"))
        .or_else(|| parse_radius_env("OPENRAILSRS_VISIBLE_RADIUS_M"))
        .unwrap_or(VIEWING_DISTANCE_M)
}

/// Max tile-centre distance for stream/load (viewing distance + one tile).
///
/// Matches `world_tile_stream_system` / terrain discovery: a tile whose centre is
/// just outside the viewing distance can still contain visible content.
pub fn view_load_radius_m() -> f32 {
    view_radius_m() + MSTS_TILE_SIZE_M as f32
}

/// Radius used to keep WORLD objects from loaded tiles (tile ring, not fine cull).
pub fn scenery_content_radius_m() -> f32 {
    view_load_radius_m()
}

/// Unload threshold for streamed tiles.
///
/// Must stay **≥** [`view_load_radius_m`] so stream/unload cannot thrash
/// (load at ~radius+tile, unload at a smaller radius).
pub fn view_unload_radius_m() -> f32 {
    view_load_radius_m() + VIEW_UNLOAD_HYSTERESIS_M
}

/// Approximate tile ring radius for logs (`ceil(distance / 2048)`).
pub fn viewing_distance_tile_ring() -> i32 {
    let tile = MSTS_TILE_SIZE_M as f32;
    (view_radius_m() / tile).ceil().max(1.0) as i32
}

fn parse_radius_env(key: &str) -> Option<f32> {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|r| *r >= VIEWING_DISTANCE_MIN_M && *r <= VIEWING_DISTANCE_MAX_M)
}

/// Clamp a user-supplied viewing distance; `None` if out of range / non-finite.
pub fn clamp_viewing_distance_m(meters: f32) -> Option<f32> {
    if meters.is_finite() && (VIEWING_DISTANCE_MIN_M..=VIEWING_DISTANCE_MAX_M).contains(&meters) {
        Some(meters)
    } else {
        None
    }
}

pub fn run_corridor_ahead_m() -> f32 {
    std::env::var("OPENRAILSRS_RUN_CORRIDOR_AHEAD_M")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v| *v >= 10.0 && *v <= 2000.0)
        .unwrap_or(RUN_CORRIDOR_AHEAD_M)
}

pub fn run_corridor_behind_m() -> f32 {
    std::env::var("OPENRAILSRS_RUN_CORRIDOR_BEHIND_M")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v| *v >= 10.0 && *v <= 2000.0)
        .unwrap_or(RUN_CORRIDOR_BEHIND_M)
}

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

/// TDB collect radius for live mobile window (min of view radius and mode cap).
pub fn tdb_stream_radius_m(mode: ViewerSceneryMode) -> f32 {
    view_radius_m().min(tdb_radius_for_mode(mode))
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

    /// Lateral corridor filter plus longitudinal window around `center` on the scenario path.
    pub fn contains_segment_near(
        &self,
        center: Vec3,
        start: Vec3,
        end: Vec3,
        ahead_m: f32,
        behind_m: f32,
    ) -> bool {
        if !self.active() {
            return true;
        }
        if !self.contains_segment(start, end) {
            return false;
        }
        let s_train = self.project_arclength_xz(center);
        let s_mid = self.project_arclength_xz((start + end) * 0.5);
        s_mid >= s_train - behind_m && s_mid <= s_train + ahead_m
    }

    /// Arclength (m) along the polyline to the closest point to `point` (XZ).
    pub fn project_arclength_xz(&self, point: Vec3) -> f32 {
        if self.points_world.len() < 2 {
            return 0.0;
        }
        let mut cum = 0.0f32;
        let mut best_dist = f32::INFINITY;
        let mut best_s = 0.0f32;
        for pair in self.points_world.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let seg_len = Vec2::new(b.x - a.x, b.z - a.z).length();
            let (dist, t) = point_segment_distance_t_xz(point, a, b);
            if dist < best_dist {
                best_dist = dist;
                best_s = cum + t * seg_len;
            }
            cum += seg_len;
        }
        best_s
    }

    fn distance_to_polyline_xz(&self, point: Vec3) -> f32 {
        self.points_world
            .windows(2)
            .map(|pair| point_segment_distance_xz(point, pair[0], pair[1]))
            .fold(f32::INFINITY, f32::min)
    }
}

fn point_segment_distance_xz(point: Vec3, a: Vec3, b: Vec3) -> f32 {
    point_segment_distance_t_xz(point, a, b).0
}

fn point_segment_distance_t_xz(point: Vec3, a: Vec3, b: Vec3) -> (f32, f32) {
    let p = Vec2::new(point.x, point.z);
    let a = Vec2::new(a.x, a.z);
    let b = Vec2::new(b.x, b.z);
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return (p.distance(a), 0.0);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p.distance(a + ab * t), t)
}

/// Full TrPins branch walk only on small `.tdb` files (large routes OOM if walked globally).
pub const TRACK_DEV_BRANCH_WALK_MAX_NODES: usize = 800;

/// Shape LOD distance for the player consist in live drive.
///
/// The camera is often close to the consist in live mode; using a far-distance
/// LOD can expose simplified/interior geometry instead of the exterior shell.
pub const LIVE_TRAIN_LOD_DISTANCE_M: f32 = 25.0;

#[cfg(test)]
mod viewing_distance_tests {
    use super::*;

    #[test]
    fn default_viewing_distance_is_one_tile_scale() {
        assert!((VIEWING_DISTANCE_M - 2000.0).abs() < 0.1);
        assert!(VIEWING_DISTANCE_M < MSTS_TILE_SIZE_M as f32 + 1.0);
        let ring = ((VIEWING_DISTANCE_M / MSTS_TILE_SIZE_M as f32).ceil() as i32).max(1);
        assert_eq!(ring, 1);
        const {
            assert!(VIEW_UNLOAD_HYSTERESIS_M > 0.0);
        }
        assert!(scenery_content_radius_m() >= VIEWING_DISTANCE_M);
    }

    #[test]
    fn clamp_viewing_distance_rejects_out_of_range() {
        assert_eq!(clamp_viewing_distance_m(2000.0), Some(2000.0));
        assert_eq!(clamp_viewing_distance_m(49.0), None);
        assert_eq!(clamp_viewing_distance_m(20_000.0), None);
    }

    #[test]
    fn unload_radius_covers_stream_load_plus_hysteresis() {
        const {
            assert!(VIEW_UNLOAD_HYSTERESIS_M >= 512.0);
        }
        let stream_max = VIEWING_DISTANCE_M + MSTS_TILE_SIZE_M as f32;
        let unload = stream_max + VIEW_UNLOAD_HYSTERESIS_M;
        // Must not thrash: unload ≥ stream inclusion distance.
        assert!(unload >= stream_max);
        assert!(
            (view_load_radius_m() - stream_max).abs() < 0.1
                || view_radius_m() != VIEWING_DISTANCE_M
        );
    }
}

#[cfg(test)]
mod corridor_tests {
    use super::*;

    #[test]
    fn corridor_longitudinal_clip_excludes_far_segment() {
        let path = RunCorridorPath {
            points_world: vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1000.0, 0.0, 0.0)],
            half_width_m: 120.0,
        };
        let train = Vec3::new(100.0, 0.0, 0.0);
        let near = (Vec3::new(150.0, 0.0, 0.0), Vec3::new(170.0, 0.0, 0.0));
        let far = (Vec3::new(600.0, 0.0, 0.0), Vec3::new(620.0, 0.0, 0.0));
        assert!(path.contains_segment_near(
            train,
            near.0,
            near.1,
            RUN_CORRIDOR_AHEAD_M,
            RUN_CORRIDOR_BEHIND_M
        ));
        assert!(!path.contains_segment_near(
            train,
            far.0,
            far.1,
            RUN_CORRIDOR_AHEAD_M,
            RUN_CORRIDOR_BEHIND_M
        ));
    }

    #[test]
    fn snapped_corridor_path_has_points_on_fixture_tdb() {
        use crate::track_position::build_snapped_corridor_path;
        use openrailsrs_formats::TrackDbFile;

        let tdb_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-formats/tests/fixtures/native_msts.tdb");
        if !tdb_path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&tdb_path).expect("tdb");
        let scenario_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/scenario.toml");
        if !scenario_path.is_file() {
            return;
        }
        let scenario = openrailsrs_scenarios::load_scenario(&scenario_path).expect("scenario");
        let graph = openrailsrs_route::load_track_graph_from_route_dir(
            scenario_path.parent().unwrap().join("routes/test"),
        )
        .expect("graph");
        let scene = crate::track::TrackScene::from_graph(graph);
        let path = build_snapped_corridor_path(&scene, &scenario, Vec3::ZERO, &tdb, None)
            .expect("corridor");
        assert!(path.active());
    }
}
