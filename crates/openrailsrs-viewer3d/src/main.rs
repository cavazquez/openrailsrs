//! Binary entry point for the experimental 3D viewer.
//!
//! Usage:
//!   openrailsrs-viewer3d [--route-root ROUTE_DIR] [route_dir | scenario.toml]
//!   openrailsrs-viewer3d --live [--cab-fov DEG] [--route-root ROUTE_DIR] scenario.toml
//!   openrailsrs-viewer3d --track-dev [--live] [--route-root ROUTE_DIR] scenario.toml
//!   openrailsrs-viewer3d --audit-placement [--route-root ROUTE_DIR] scenario.toml
//!   openrailsrs-viewer3d --audit-tr-item [--route-root ROUTE_DIR] scenario.toml
//!
//! Env: `OPENRAILSRS_PRESENT_MODE=auto_vsync|fifo|mailbox|immediate|auto_no_vsync`
//! (default `auto_vsync`; prefer over `auto_no_vsync` on RADV/X11).
//!
//! - `route_dir` — static graph only (default: `examples/smoke/routes/test`).
//! - `scenario.toml` — graph + animated train marker(s) from simulation CSV.
//! - `--route-root` — load MSTS/OR scenery assets from an external route dir while keeping
//!   the scenario/track graph from `scenario.toml`.
//! - `--run-corridor` — minimal train + `.tdb` procedural track view; no world/terrain scenery.
//! - `--track-dev` — grafo `.tdb` + vía procedural continua; sin terreno/shapes/`TrackObj`.
//! - `--live scenario.toml` — run physics in real time (no CSV); drive with arrow keys.
//!
//! Generate CSV for replay mode, e.g.:
//!   cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml
//!
//! Controls:
//!
//! - `F1` / `F2`   — orbit / fly camera.
//! - Orbit: drag (LMB/RMB) = rotate, Shift+drag or WASD = pan, wheel = zoom.
//! - Live orbit pan: `I`/`K` forward/back (W/S stay free); `F2` = fly camera (WASD roam).
//! - Fly: WASD move, `Q`/`E` up/down (`Space` = up unless live/replay loaded).
//! - Replay: `Space` pause, `R` reset, `+`/`-` speed, `T` cycle camera follow (when CSV loaded).
//! - Live: `↑`/`↓` throttle/brake, `Space` emergency, `H` horn, `C` cab panel, `+`/`-` sim speed, `T` camera, `G` teleport.
//! - Multi-train replay: `[` / `]` (or Shift+T) cycle which train the follow camera tracks.
//! - `G`           — teleport dialog (type x,y,z).
//! - `P`           — toggle rain streaks.
//! - `Esc`         — quit (closes teleport dialog first if open).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::prelude::*;
use bevy::state::condition::in_state;
use bevy::window::PresentMode;
use openrailsrs_formats::RouteFile;
use openrailsrs_formats::Vec3 as MstsVec3;
use openrailsrs_formats::{
    msts_tile_world_origin, msts_tile_x_index_for_coord, msts_tile_z_index_for_coord,
};
use openrailsrs_route::load_route_from_dir;
use openrailsrs_scenarios::SCENARIO_OVERLAY_FILENAME;
use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
use openrailsrs_sim::path::resolve_scenario_route_edges;
use openrailsrs_viewer3d::LiveDrive;
use openrailsrs_viewer3d::RouteAssets;
use openrailsrs_viewer3d::TerrainElevation;
use openrailsrs_viewer3d::TerrainScene;
use openrailsrs_viewer3d::TrainConsistScene;
use openrailsrs_viewer3d::ViewerLaunchOpts;
use openrailsrs_viewer3d::ViewerPlugin;
use openrailsrs_viewer3d::ViewerSceneryMode;
use openrailsrs_viewer3d::WorldScene;
use openrailsrs_viewer3d::init_viewer_log;
use openrailsrs_viewer3d::launch::{
    RunCorridorPath, clamp_viewing_distance_m, run_corridor_half_width_m,
    run_corridor_scenery_enabled, scenery_content_radius_m, set_viewing_distance_m,
    tdb_radius_for_mode, tile_lab_layers, view_radius_m, view_unload_radius_m,
    viewing_distance_tile_ring,
};
use openrailsrs_viewer3d::placement_audit::{
    CHILTERN_BIRMINGHAM_TILE, WorldAnchorInput, log_placement_audit, run_placement_audit,
};
use openrailsrs_viewer3d::rolling_stock::try_load_consist_vehicles;
use openrailsrs_viewer3d::route_bootstrap::{
    PendingRouteLoad, RouteLoadBundle, ViewerAppState, ViewerBootClock,
    log_time_to_first_presented_frame, poll_route_load, setup_viewer_loading_ui,
    update_loading_screen_progress,
};
use openrailsrs_viewer3d::shapes::global_assets_dirs;
use openrailsrs_viewer3d::tdb_track::collect_tdb_chords;
use openrailsrs_viewer3d::teleport::TeleportDialog;
use openrailsrs_viewer3d::terrain::load_terrain_from_route_dir_near;
use openrailsrs_viewer3d::tr_item_audit::{log_tr_item_audit, run_tr_item_audit_for_route};
use openrailsrs_viewer3d::tr_item_index::TrItemWorldIndex;
use openrailsrs_viewer3d::track::{TrackScene, graph_to_world};
use openrailsrs_viewer3d::track_audit::run_track_dev_audit;
use openrailsrs_viewer3d::track_position::{
    anchor_delta_xz, build_snapped_corridor_path, route_start_bevy,
};
use openrailsrs_viewer3d::train::{ReplayState, TRAIN_COLORS, TrainTrack, load_csv};
use openrailsrs_viewer3d::world::{
    MSTS_TILE_SIZE_M, RouteFocus, RouteWorldOffset, load_world_from_route_dir_near, msts_to_bevy,
    world_tile_center_hint,
};
use openrailsrs_viewer3d::{log_step, viewer_log};
use serde::Deserialize;

const WORLD_ANCHOR_COORD_SPACE_THRESHOLD_M: f32 = 100_000.0;

struct LaunchConfig {
    title: String,
    route_dir: PathBuf,
    scenario: Option<openrailsrs_scenarios::ScenarioFile>,
    scene: TrackScene,
    world: WorldScene,
    terrain: TerrainScene,
    elevation: TerrainElevation,
    replay: ReplayState,
    consist: TrainConsistScene,
    live: Option<LiveDrive>,
    scenery_mode: ViewerSceneryMode,
    run_corridor_path: RunCorridorPath,
    focus_center_override: Option<Vec3>,
    route_offset_override: Option<RouteWorldOffset>,
}

struct CliArgs {
    live: bool,
    track_dev: bool,
    run_corridor: bool,
    tile_lab: bool,
    audit_placement: bool,
    audit_tr_item: bool,
    path: PathBuf,
    route_root: Option<PathBuf>,
    cab_fov_deg: Option<f32>,
    /// `--viewing-distance METERS` (Open Rails–style horizon).
    viewing_distance_m: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SceneryModeToml {
    #[default]
    Full,
    TrackDev,
    RunCorridor,
}

impl From<SceneryModeToml> for ViewerSceneryMode {
    fn from(value: SceneryModeToml) -> Self {
        match value {
            SceneryModeToml::Full => Self::Full,
            SceneryModeToml::TrackDev => Self::TrackDev,
            SceneryModeToml::RunCorridor => Self::RunCorridor,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Viewer3dToml {
    #[serde(default)]
    viewer3d: Viewer3dConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Viewer3dConfig {
    #[serde(default)]
    world_anchor: Option<WorldAnchorToml>,
    #[serde(default)]
    scenery_mode: SceneryModeToml,
    /// Viewing / tile-stream distance in metres (default 2000 ≈ one MSTS tile).
    #[serde(default)]
    viewing_distance_m: Option<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct WorldAnchorToml {
    tile_x: i32,
    tile_z: i32,
    local_x_m: f64,
    #[serde(default)]
    local_y_m: f64,
    local_z_m: f64,
}

/// Warn when `/dev/dri` has multiple GPUs but NVIDIA userspace looks broken — a common
/// cause of Wayland `dmabufs … CoglTexture2D` protocol errors (RADV render + NVIDIA present).
fn warn_hybrid_gpu_display_if_needed() {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return;
    };
    let render_nodes = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("renderD"))
        .count();
    if render_nodes < 2 {
        return;
    }
    let nvidia_smi = std::process::Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .ok();
    let nvidia_broken = nvidia_smi.as_ref().is_none_or(|o| {
        !o.status.success()
            || String::from_utf8_lossy(&o.stderr).contains("Driver/library version mismatch")
            || String::from_utf8_lossy(&o.stdout).contains("Driver/library version mismatch")
    });
    if !nvidia_broken {
        return;
    }
    eprintln!(
        "openrailsrs-viewer3d: hybrid GPU detected ({render_nodes} render nodes) but NVIDIA \
         userspace looks broken (nvidia-smi Driver/library version mismatch).\n\
         Wayland often crashes with: failed to import supplied dmabufs / CoglTexture2D.\n\
         Fix: reboot to reload the NVIDIA kernel module, then retry. Alternatives: log into \
         an Xorg session, or make Mutter use the AMD iGPU as primary (see docs/VIEWER3D.md)."
    );
}

/// `OPENRAILSRS_PRESENT_MODE`: `auto_vsync` (default), `auto_no_vsync`, `fifo`, `mailbox`, `immediate`.
fn present_mode_from_env() -> PresentMode {
    match std::env::var("OPENRAILSRS_PRESENT_MODE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "auto_no_vsync" | "novsync" | "no_vsync" => PresentMode::AutoNoVsync,
        "fifo" | "vsync" => PresentMode::Fifo,
        "mailbox" => PresentMode::Mailbox,
        "immediate" => PresentMode::Immediate,
        "auto_vsync" | "auto" | "" => PresentMode::AutoVsync,
        other => {
            eprintln!(
                "openrailsrs-viewer3d: unknown OPENRAILSRS_PRESENT_MODE={other:?}, using AutoVsync"
            );
            PresentMode::AutoVsync
        }
    }
}

fn parse_cli() -> CliArgs {
    parse_cli_from(std::env::args().skip(1))
}

fn parse_cli_from(args: impl IntoIterator<Item = String>) -> CliArgs {
    let mut live = false;
    let mut track_dev = false;
    let mut run_corridor = false;
    let mut tile_lab = false;
    let mut route_root = None;
    let mut cab_fov_deg = None;
    let mut viewing_distance_m = None;
    let mut audit_placement = false;
    let mut audit_tr_item = false;
    let mut path = None;
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        if arg == "--live" {
            live = true;
        } else if arg == "--track-dev" {
            track_dev = true;
        } else if arg == "--run-corridor" {
            run_corridor = true;
        } else if arg == "--tile-lab" {
            tile_lab = true;
        } else if arg == "--audit-placement" {
            audit_placement = true;
        } else if arg == "--audit-tr-item" {
            audit_tr_item = true;
        } else if arg == "--route-root" {
            route_root = args.next().map(PathBuf::from);
        } else if let Some(value) = arg.strip_prefix("--route-root=") {
            route_root = Some(PathBuf::from(value));
        } else if arg == "--cab-fov" {
            cab_fov_deg = args.next().and_then(|v| v.parse().ok());
        } else if let Some(value) = arg.strip_prefix("--cab-fov=") {
            cab_fov_deg = value.parse().ok();
        } else if arg == "--viewing-distance" {
            viewing_distance_m = args.next().and_then(|v| v.parse().ok());
        } else if let Some(value) = arg.strip_prefix("--viewing-distance=") {
            viewing_distance_m = value.parse().ok();
        } else if !arg.starts_with('-') {
            path = Some(PathBuf::from(arg));
        }
    }
    CliArgs {
        live,
        track_dev,
        run_corridor,
        tile_lab,
        audit_placement,
        audit_tr_item,
        path: path.unwrap_or_else(|| PathBuf::from("examples/smoke/routes/test")),
        route_root,
        cab_fov_deg,
        viewing_distance_m,
    }
}

fn main() {
    init_viewer_log();
    let cli = parse_cli();

    if cli.audit_placement {
        if let Err(err) = run_audit_placement_mode(&cli) {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }

    if cli.audit_tr_item {
        if let Err(err) = run_audit_tr_item_mode(&cli) {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }

    // Must run before route load: WORLD/terrain discovery uses view_radius_m().
    apply_viewing_distance_policy(&cli, &cli.path);

    let boot = Instant::now();
    let path = cli.path.clone();
    let live = cli.live;
    let track_dev = cli.track_dev;
    let run_corridor = cli.run_corridor;
    let tile_lab = cli.tile_lab;
    let route_root = cli.route_root.clone();
    let cab_fov_deg = cli.cab_fov_deg;
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<RouteLoadBundle, String>>(1);
    std::thread::Builder::new()
        .name("route-load".into())
        .spawn(move || {
            let result = load_route_bundle_for_viewer(
                &path,
                live,
                track_dev,
                run_corridor,
                tile_lab,
                route_root.as_deref(),
                cab_fov_deg,
            );
            let _ = tx.send(result);
        })
        .expect("spawn route-load thread");

    viewer_log!("openrailsrs-viewer3d: starting Bevy app (route load in background, #55)");
    // #82: do not log time_to_window here — App/plugins/event loop have not started.
    // See `log_time_to_first_presented_frame` → `[PERF] time_to_first_presented_ms=…`.

    let win_w = std::env::var("OPENRAILSRS_WINDOW_WIDTH")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(1280)
        .max(64);
    let win_h = std::env::var("OPENRAILSRS_WINDOW_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(720)
        .max(64);
    // Screenshots / visual goldens: lock physical pixels (ignore HiDPI scale).
    let screenshot_lock = std::env::var_os("OPENRAILSRS_SCREENSHOT").is_some_and(|v| !v.is_empty());
    let mut resolution: bevy::window::WindowResolution = (win_w, win_h).into();
    if screenshot_lock {
        resolution =
            bevy::window::WindowResolution::new(win_w, win_h).with_scale_factor_override(1.0);
    }
    // Default AutoVsync (Fifo): AutoNoVsync/Immediate often yields
    // `Surface::configure → Invalid surface` on RADV+X11 (Mesa 26 / Raphael iGPU).
    // Override: OPENRAILSRS_PRESENT_MODE=auto_no_vsync|fifo|mailbox|immediate|auto_vsync
    let present_mode = present_mode_from_env();
    warn_hybrid_gpu_display_if_needed();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            // Allow absolute WORLD/terrain paths inside generated `.tilebundle` manifests (#111).
            .set(openrailsrs_viewer3d::tile_bundle::viewer_asset_plugin())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "openrailsrs-viewer3d".into(),
                    resolution,
                    present_mode,
                    // Keep goldens from inheriting a maximized/restored desktop size.
                    resizable: !screenshot_lock,
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(PendingRouteLoad {
        rx: std::sync::Mutex::new(rx),
        started: boot,
    })
    .insert_resource(ViewerBootClock::new(boot))
    .add_plugins(ViewerPlugin)
    .add_systems(Startup, setup_viewer_loading_ui)
    .add_systems(
        Update,
        (
            log_time_to_first_presented_frame,
            poll_route_load.run_if(in_state(ViewerAppState::Loading)),
            update_loading_screen_progress,
            exit_on_esc,
        ),
    );

    app.run();
}

fn load_route_bundle_for_viewer(
    path: &Path,
    live: bool,
    track_dev: bool,
    run_corridor: bool,
    tile_lab: bool,
    route_root: Option<&Path>,
    cab_fov_deg: Option<f32>,
) -> Result<RouteLoadBundle, String> {
    let mut config =
        build_launch_config(path, live, track_dev, run_corridor, tile_lab, route_root)?;

    let assets = RouteAssets::new(&config.route_dir);

    if config.scenery_mode.is_run_corridor() && config.run_corridor_path.active() {
        if let (Some(scenario), Some(tdb)) = (&config.scenario, assets.track_db()) {
            let delta = config
                .route_offset_override
                .map(|o| o.delta)
                .unwrap_or_default();
            match build_snapped_corridor_path(
                &config.scene,
                scenario,
                delta,
                tdb,
                Some(assets.tsection()),
            ) {
                Ok(snapped) => config.run_corridor_path = snapped,
                Err(err) => {
                    viewer_log!("openrailsrs-viewer3d: run_corridor TDB snap failed: {err}")
                }
            }
        }
    }

    if config.scenery_mode.is_tile_lab() {
        viewer_log!(
            "openrailsrs-viewer3d: scenery_mode=tile_lab — 1 tile, sin streaming; capas vía OPENRAILSRS_TILE_LAB_LAYERS=terrain,track,world,train"
        );
    } else if config.scenery_mode.is_track_dev() {
        viewer_log!(
            "openrailsrs-viewer3d: scenery_mode=track_dev — vía procedural desde .tdb; sin terreno/shapes/TrackObj"
        );
    } else if config.scenery_mode.is_run_corridor() {
        if run_corridor_scenery_enabled() {
            viewer_log!(
                "openrailsrs-viewer3d: scenery_mode=run_corridor+scenery — tren + vía .tdb + WORLD/terreno (OPENRAILSRS_RUN_CORRIDOR_SCENERY)"
            );
        } else if assets.track_db().is_some() {
            viewer_log!(
                "openrailsrs-viewer3d: scenery_mode=run_corridor — tren + vía .tdb; sin WORLD/terreno/objetos"
            );
        } else {
            viewer_log!(
                "openrailsrs-viewer3d: scenery_mode=run_corridor — tren + vía lógica (grafo track.toml); sin WORLD/terreno/objetos"
            );
        }
    }

    let node_count = config.scene.graph.nodes_iter().count();
    let edge_count = config.scene.edge_count;
    viewer_log!(
        "openrailsrs-viewer3d: {} ({} nodes, {} edges, render={}{}{}{}{})",
        config.title,
        node_count,
        edge_count,
        config.scene.render_mode.label(),
        if config.world.is_empty() {
            String::new()
        } else {
            format!(
                ", {} world obj(s) / {} tile(s)",
                config.world.items.len(),
                config.world.tiles_loaded
            )
        },
        if config.terrain.is_empty() {
            String::new()
        } else {
            format!(", {} terrain tile(s)", config.terrain.tiles_loaded)
        },
        if config.live.is_some() {
            ", live sim".to_string()
        } else if config.replay.is_active() {
            format!(", {} train(s) replay", config.replay.tracks.len())
        } else {
            String::new()
        },
        if config.live.is_none()
            && !config.replay.is_active()
            && path.extension().and_then(|e| e.to_str()) == Some("toml")
        {
            "\n  hint: add --live or run: cargo run -p openrailsrs-cli -- sim <scenario.toml>"
                .to_string()
        } else {
            String::new()
        },
    );

    let route_focus = route_focus_for_config(&config);
    let route_offset = config
        .route_offset_override
        .unwrap_or_else(|| RouteWorldOffset::from_scene_and_world(&config.scene, &config.world));
    log_coord_debug_if_enabled(&config.scene, &config.world, route_offset);
    log_scenery_debug_if_enabled(&config.route_dir, &config.world, route_focus.center);

    if config.scenery_mode.draws_tdb_track() {
        if let Some(tdb) = assets.track_db() {
            let radius_m = tdb_radius_for_mode(config.scenery_mode);
            let mut chords =
                collect_tdb_chords(tdb, &route_focus, radius_m, Some(assets.tsection()));
            if config.scenery_mode.is_run_corridor() && config.run_corridor_path.active() {
                chords.retain(|chord| {
                    config
                        .run_corridor_path
                        .contains_segment(chord.start_world, chord.end_world)
                });
            }
            let audit_route_dir = config
                .scenery_mode
                .is_track_dev()
                .then_some(config.route_dir.as_path());
            run_track_dev_audit(
                tdb,
                &config.scene,
                &route_focus,
                route_offset,
                radius_m,
                &chords,
                audit_route_dir,
                Some(assets.tsection()),
            );
        }
    }

    Ok(RouteLoadBundle {
        title: config.title,
        route_dir: config.route_dir,
        scene: config.scene,
        world: config.world,
        terrain: config.terrain,
        elevation: config.elevation,
        replay: config.replay,
        consist: config.consist,
        live: config.live,
        scenery_mode: config.scenery_mode,
        run_corridor_path: config.run_corridor_path,
        route_focus,
        route_offset,
        assets,
        launch_opts: ViewerLaunchOpts { live, cab_fov_deg },
    })
}

fn build_launch_config(
    arg: &Path,
    live: bool,
    track_dev_cli: bool,
    run_corridor_cli: bool,
    tile_lab_cli: bool,
    route_root: Option<&Path>,
) -> Result<LaunchConfig, String> {
    if arg.extension().and_then(|e| e.to_str()) == Some("toml") {
        load_from_scenario(
            arg,
            live,
            track_dev_cli,
            run_corridor_cli,
            tile_lab_cli,
            route_root,
        )
    } else if live {
        Err("--live requires a scenario.toml path".into())
    } else if run_corridor_cli {
        Err("--run-corridor requires a scenario.toml path".into())
    } else if tile_lab_cli {
        Err("--tile-lab requires a scenario.toml path (needs [viewer3d.world_anchor])".into())
    } else if route_root.is_some() {
        Err("--route-root is only valid with a scenario.toml path".into())
    } else {
        load_from_route_dir(arg, track_dev_cli)
    }
}

fn resolve_scenery_mode(
    track_dev_cli: bool,
    run_corridor_cli: bool,
    tile_lab_cli: bool,
    from_toml: SceneryModeToml,
) -> ViewerSceneryMode {
    if tile_lab_cli {
        ViewerSceneryMode::TileLab
    } else if track_dev_cli {
        ViewerSceneryMode::TrackDev
    } else if run_corridor_cli {
        ViewerSceneryMode::RunCorridor
    } else {
        from_toml.into()
    }
}

fn load_from_route_dir(route_dir: &Path, track_dev_cli: bool) -> Result<LaunchConfig, String> {
    let t = Instant::now();
    let loaded = load_route_from_dir(route_dir).map_err(|e| e.to_string())?;
    log_step(
        &format!(
            "loaded track graph ({} nodes, {} msts aliases)",
            loaded.graph.nodes_iter().count(),
            loaded.msts_aliases.len()
        ),
        t,
    );
    let scene = TrackScene::from_loaded_route(loaded);
    let track_dev = track_dev_cli;
    let t = Instant::now();
    let mut world = if track_dev {
        viewer_log!("openrailsrs-viewer3d: track_dev — skipping .w world load");
        WorldScene::default()
    } else {
        let world_hint = world_tile_center_hint(route_dir).unwrap_or(scene.bounds.center);
        load_world_from_route_dir_near(route_dir, Some(world_hint), view_radius_m())
    };
    if !track_dev {
        log_step(
            &format!(
                "loaded world ({} obj(s) / {} tile(s))",
                world.items.len(),
                world.tiles_loaded
            ),
            t,
        );
    }
    let focus = RouteFocus::from_scene_and_world(&scene, &world);
    if !track_dev {
        world.retain_within_visible_radius(&focus, scenery_content_radius_m());
    }
    let t = Instant::now();
    let terrain = if track_dev {
        TerrainScene::default()
    } else {
        load_terrain_from_route_dir_near(route_dir, Some(focus.center), view_radius_m())
    };
    log_step(
        &format!("loaded terrain index ({} tile(s))", terrain.tiles_loaded),
        t,
    );
    let t = Instant::now();
    let elevation = if track_dev {
        TerrainElevation::default()
    } else {
        TerrainElevation::from_terrain_scene(&terrain)
    };
    log_step("loaded terrain elevation", t);
    Ok(LaunchConfig {
        title: format!("openrailsrs-viewer3d — {}", route_dir.display()),
        route_dir: route_dir.to_path_buf(),
        scenario: None,
        scene,
        world,
        terrain,
        elevation,
        replay: ReplayState::default(),
        consist: TrainConsistScene::default(),
        live: None,
        scenery_mode: resolve_scenery_mode(track_dev_cli, false, false, SceneryModeToml::Full),
        run_corridor_path: RunCorridorPath::default(),
        focus_center_override: None,
        route_offset_override: None,
    })
}

fn load_from_scenario(
    path: &Path,
    live: bool,
    track_dev_cli: bool,
    run_corridor_cli: bool,
    tile_lab_cli: bool,
    route_root: Option<&Path>,
) -> Result<LaunchConfig, String> {
    let scenario_dir = path
        .parent()
        .ok_or("scenario path has no parent directory")?;
    let t = Instant::now();
    let scenario = load_scenario(path).map_err(|e| e.to_string())?;
    log_step(
        &format!("loaded scenario \"{}\"", scenario.scenario.name),
        t,
    );
    let graph_route_dir = scenario_dir.join(&scenario.route.path);
    let route_dir = route_root
        .map(Path::to_path_buf)
        .unwrap_or_else(|| graph_route_dir.clone());
    if route_root.is_some() {
        viewer_log!(
            "openrailsrs-viewer3d: using external route root {} (graph from {})",
            route_dir.display(),
            graph_route_dir.display()
        );
    }
    let t = Instant::now();
    let loaded = load_route_from_dir(&graph_route_dir).map_err(|e| e.to_string())?;
    log_step(
        &format!(
            "loaded track graph ({} nodes, {} msts aliases)",
            loaded.graph.nodes_iter().count(),
            loaded.msts_aliases.len()
        ),
        t,
    );
    let scene = TrackScene::from_loaded_route(loaded);
    let viewer3d = load_viewer3d_config(path, scenario_dir)?;
    let scenery_mode = resolve_scenery_mode(
        track_dev_cli,
        run_corridor_cli,
        tile_lab_cli,
        viewer3d.scenery_mode,
    );
    let anchor_world = viewer3d.world_anchor.map(world_anchor_position);
    if scenery_mode.is_tile_lab() {
        let anchor = anchor_world
            .ok_or("--tile-lab requires [viewer3d.world_anchor] in the scenario/overlay")?;
        return load_tile_lab(&scenario, scenario_dir, &route_dir, scene, anchor);
    }
    let route_offset_override = match (viewer3d.world_anchor, anchor_world) {
        (Some(anchor), Some(world_pos)) => {
            let graph_pos = graph_start_position(&scene, &scenario)?;
            log_trk_route_start_vs_anchor(&route_dir, world_pos, anchor);
            let delta = Vec3::new(world_pos.x - graph_pos.x, 0.0, world_pos.z - graph_pos.z);
            let dist_xz = Vec2::new(delta.x, delta.z).length();
            // When the overlay supplies an OR log anchor, it is the visual ground truth
            // (activity start / platform). Patched graph coords can still be ~km off (Chiltern).
            let (applied, kind) = if dist_xz <= 0.5 {
                (Vec3::ZERO, "graph start matches anchor")
            } else if dist_xz <= WORLD_ANCHOR_COORD_SPACE_THRESHOLD_M {
                (delta, "anchor trim (graph vs OR placement)")
            } else {
                (delta, "coordinate-space shift")
            };
            viewer_log!(
                "openrailsrs-viewer3d: viewer3d world anchor tile {},{} local {:.1},{:.1},{:.1} -> delta {:.0},{:.0},{:.0} m ({kind})",
                anchor.tile_x,
                anchor.tile_z,
                anchor.local_x_m,
                anchor.local_y_m,
                anchor.local_z_m,
                applied.x,
                applied.y,
                applied.z
            );
            Some(RouteWorldOffset { delta: applied })
        }
        _ => None,
    };
    log_trk_route_start_vs_anchor_optional(&route_dir, anchor_world);
    let scenery_load_center = anchor_world
        .or_else(|| {
            if live {
                graph_start_position(&scene, &scenario)
                    .ok()
                    .map(|p| p + route_offset_override.unwrap_or_default().delta)
            } else {
                None
            }
        })
        .or_else(|| world_tile_center_hint(&route_dir))
        .unwrap_or(scene.bounds.center);
    let t = Instant::now();
    let mut world = if scenery_mode.loads_msts_scenery() {
        if live {
            viewer_log!(
                "openrailsrs-viewer3d: loading world near ({:.0}, {:.0}, {:.0})",
                scenery_load_center.x,
                scenery_load_center.y,
                scenery_load_center.z
            );
        }
        load_world_from_route_dir_near(&route_dir, Some(scenery_load_center), view_radius_m())
    } else {
        viewer_log!("openrailsrs-viewer3d: track-focused — skipping .w world load");
        WorldScene::default()
    };
    if scenery_mode.loads_msts_scenery() {
        log_step(
            &format!(
                "loaded world ({} obj(s) / {} tile(s))",
                world.items.len(),
                world.tiles_loaded
            ),
            t,
        );
    }
    let run_corridor_path = if scenery_mode.is_run_corridor() {
        build_run_corridor_path(
            &scene,
            &scenario,
            route_offset_override.unwrap_or_default().delta,
        )?
    } else {
        RunCorridorPath::default()
    };
    let focus_center = anchor_world.unwrap_or(scenery_load_center);
    let provisional_focus = RouteFocus {
        center: focus_center,
        height_origin: focus_center.y,
    };
    if scenery_mode.loads_msts_scenery() {
        world.retain_within_visible_radius(&provisional_focus, scenery_content_radius_m());
    }
    let t = Instant::now();
    let terrain = if scenery_mode.loads_msts_scenery() {
        load_terrain_from_route_dir_near(&route_dir, Some(scenery_load_center), view_radius_m())
    } else {
        TerrainScene::default()
    };
    log_step(
        &format!("loaded terrain index ({} tile(s))", terrain.tiles_loaded),
        t,
    );
    let t = Instant::now();
    let elevation = if scenery_mode.loads_msts_scenery() {
        TerrainElevation::from_terrain_scene(&terrain)
    } else {
        TerrainElevation::default()
    };
    log_step("loaded terrain elevation", t);

    let focus = if anchor_world.is_some() {
        RouteFocus::at_world_center(focus_center, Some(&elevation))
    } else {
        RouteFocus::from_scene_world_and_elevation(&scene, &world, Some(&elevation))
    };
    if focus.height_origin != focus_center.y {
        viewer_log!(
            "openrailsrs-viewer3d: render height origin {:.1} m terrain MSL (anchor scenery y {:.1})",
            focus.height_origin,
            focus_center.y
        );
    }

    let t = Instant::now();
    let consist = load_train_consists(scenario_dir, &scenario);
    log_step(
        &format!(
            "loaded train consist(s) ({} vehicle(s))",
            consist.total_vehicles()
        ),
        t,
    );

    let (replay, live_drive) = if live {
        let t = Instant::now();
        let drive = LiveDrive::from_scenario_path(path)?;
        log_step("initialized live drive session", t);
        viewer_log!(
            "openrailsrs-viewer3d: live drive on \"{}\" (dt={:.2}s, ↑/↓ throttle/brake, F2 fly, G teleport)",
            drive.session.scenario_name,
            drive.session.dt,
        );
        (ReplayState::default(), Some(drive))
    } else {
        let mut tracks = Vec::new();
        let primary_csv = scenario_dir.join(&scenario.output.csv);
        let rows = load_csv(&primary_csv);
        if !rows.is_empty() {
            tracks.push(TrainTrack {
                label: "primary".into(),
                color: TRAIN_COLORS[0],
                rows,
            });
        }
        for (i, extra) in scenario.extra_trains.iter().enumerate() {
            let csv_path = scenario_dir.join(&extra.output_csv);
            let rows = load_csv(&csv_path);
            if !rows.is_empty() {
                tracks.push(TrainTrack {
                    label: extra.id.clone(),
                    color: TRAIN_COLORS[(i + 1) % TRAIN_COLORS.len()],
                    rows,
                });
            }
        }
        (
            ReplayState::new(scenario.scenario.name.clone(), tracks),
            None,
        )
    };

    Ok(LaunchConfig {
        title: if live {
            format!("openrailsrs-viewer3d LIVE — {}", scenario.scenario.name)
        } else {
            format!("openrailsrs-viewer3d — {}", scenario.scenario.name)
        },
        route_dir,
        scenario: Some(scenario),
        scene,
        world,
        terrain,
        elevation,
        replay,
        consist,
        live: live_drive,
        scenery_mode,
        run_corridor_path,
        focus_center_override: anchor_world,
        route_offset_override,
    })
}

/// `--tile-lab`: carga UN solo tile (el del anchor) con capas opt-in para
/// validar un elemento a la vez (terreno → vía → objetos → tren).
fn load_tile_lab(
    scenario: &openrailsrs_scenarios::ScenarioFile,
    scenario_dir: &Path,
    route_dir: &Path,
    scene: TrackScene,
    anchor: Vec3,
) -> Result<LaunchConfig, String> {
    let layers = tile_lab_layers();
    let tile_x = msts_tile_x_index_for_coord(anchor.x);
    let tile_z = msts_tile_z_index_for_coord(anchor.z);
    let (ox, oz) = msts_tile_world_origin(tile_x, tile_z);
    let half = MSTS_TILE_SIZE_M as f32 * 0.5;
    let tile_center = Vec3::new(ox + half, anchor.y, oz + half);
    viewer_log!(
        "openrailsrs-viewer3d: tile-lab — tile ({tile_x},{tile_z}), capas: {} (OPENRAILSRS_TILE_LAB_LAYERS)",
        layers.label()
    );

    let world = if layers.world {
        let t = Instant::now();
        let mut w = load_world_from_route_dir_near(route_dir, Some(tile_center), 1.0);
        w.items.retain(|o| o.tile_x == tile_x && o.tile_z == tile_z);
        w.tiles_loaded = usize::from(!w.items.is_empty());
        log_step(
            &format!("tile-lab: loaded world tile ({} obj(s))", w.items.len()),
            t,
        );
        w
    } else {
        WorldScene::default()
    };

    let terrain = if layers.terrain {
        let t = Instant::now();
        let mut ts = load_terrain_from_route_dir_near(route_dir, Some(tile_center), 1.0);
        ts.tiles
            .retain(|t| t.tile_x == tile_x && t.tile_z == tile_z);
        ts.tiles_loaded = ts.tiles.len();
        log_step(
            &format!("tile-lab: loaded terrain ({} tile(s))", ts.tiles.len()),
            t,
        );
        ts
    } else {
        TerrainScene::default()
    };
    let elevation = TerrainElevation::from_terrain_scene(&terrain);

    let scene = if layers.track {
        // Solo nodos/aristas dentro del tile (+margen): el grafo completo de la
        // ruta mide cientos de km y arruinaría bbox, esferas de nodo y domo.
        let margin = 64.0;
        let in_tile = |x: f64, z: f64| {
            x >= (ox - margin) as f64
                && x <= (ox + 2.0 * half + margin) as f64
                && z >= (oz - margin) as f64
                && z <= (oz + 2.0 * half + margin) as f64
        };
        let mut sub = openrailsrs_track::TrackGraph::new();
        for (_, node) in scene.graph.nodes_iter() {
            if in_tile(node.x_m, node.y_m) {
                let _ = sub.insert_node(node.clone());
            }
        }
        let mut kept_edges = 0usize;
        for (_, edge) in scene.graph.edges_iter() {
            if sub.node(&edge.from.0).is_some() && sub.node(&edge.to.0).is_some() {
                let _ = sub.insert_edge(edge.clone());
                kept_edges += 1;
            }
        }
        viewer_log!(
            "openrailsrs-viewer3d: tile-lab — track filtrado al tile: {} nodo(s), {} edge(s)",
            sub.nodes_iter().count(),
            kept_edges
        );
        TrackScene::from_graph(sub)
    } else {
        TrackScene::from_graph(openrailsrs_track::TrackGraph::new())
    };

    let (replay, consist) = if layers.train {
        let consist = load_train_consists(scenario_dir, scenario);
        let mut tracks = Vec::new();
        let rows = load_csv(&scenario_dir.join(&scenario.output.csv));
        if !rows.is_empty() {
            tracks.push(TrainTrack {
                label: "primary".into(),
                color: TRAIN_COLORS[0],
                rows,
            });
        }
        (
            ReplayState::new(scenario.scenario.name.clone(), tracks),
            consist,
        )
    } else {
        (ReplayState::default(), TrainConsistScene::default())
    };

    Ok(LaunchConfig {
        title: format!(
            "openrailsrs-viewer3d TILE-LAB ({tile_x},{tile_z}) — {}",
            scenario.scenario.name
        ),
        route_dir: route_dir.to_path_buf(),
        scenario: Some(scenario.clone()),
        scene,
        world,
        terrain,
        elevation,
        replay,
        consist,
        live: None,
        scenery_mode: ViewerSceneryMode::TileLab,
        run_corridor_path: RunCorridorPath::default(),
        focus_center_override: Some(tile_center),
        // Coordenadas absolutas correctas: nunca desplazar el grafo en tile-lab.
        route_offset_override: Some(RouteWorldOffset::default()),
    })
}

fn route_focus_for_config(config: &LaunchConfig) -> RouteFocus {
    if let Some(center) = config.focus_center_override {
        RouteFocus::at_world_center(center, Some(&config.elevation).filter(|e| !e.is_empty()))
    } else {
        RouteFocus::from_scene_world_and_elevation(
            &config.scene,
            &config.world,
            if config.elevation.is_empty() {
                None
            } else {
                Some(&config.elevation)
            },
        )
    }
}

fn load_viewer3d_config(path: &Path, scenario_dir: &Path) -> Result<Viewer3dConfig, String> {
    let mut out = Viewer3dConfig::default();
    for file in [
        path.to_path_buf(),
        scenario_dir.join(SCENARIO_OVERLAY_FILENAME),
    ] {
        if !file.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
        let parsed: Viewer3dToml = toml::from_str(&text)
            .map_err(|e| format!("failed to parse viewer3d config in {}: {e}", file.display()))?;
        if parsed.viewer3d.world_anchor.is_some() {
            out.world_anchor = parsed.viewer3d.world_anchor;
        }
        out.scenery_mode = parsed.viewer3d.scenery_mode;
        if parsed.viewer3d.viewing_distance_m.is_some() {
            out.viewing_distance_m = parsed.viewer3d.viewing_distance_m;
        }
    }
    Ok(out)
}

/// Priority: `--viewing-distance` → `[viewer3d].viewing_distance_m` → env → default 2000 m.
fn apply_viewing_distance_policy(cli: &CliArgs, scenario_path: &Path) {
    let from_cli = cli.viewing_distance_m.and_then(clamp_viewing_distance_m);
    let from_config = scenario_path
        .parent()
        .and_then(|dir| load_viewer3d_config(scenario_path, dir).ok())
        .and_then(|cfg| cfg.viewing_distance_m)
        .and_then(clamp_viewing_distance_m);
    if let Some(meters) = from_cli.or(from_config) {
        set_viewing_distance_m(meters);
    }
    viewer_log!(
        "openrailsrs-viewer3d: viewing distance {:.0} m (~{} tile ring); content keep {:.0} m; unload @{:.0} m",
        view_radius_m(),
        viewing_distance_tile_ring(),
        scenery_content_radius_m(),
        view_unload_radius_m()
    );
}

fn world_anchor_position(anchor: WorldAnchorToml) -> Vec3 {
    // Signed internal tile coords, straight from the OR log values.
    msts_to_bevy(
        anchor.tile_x,
        anchor.tile_z,
        MstsVec3 {
            x: anchor.local_x_m,
            y: anchor.local_y_m,
            z: anchor.local_z_m,
        },
    )
}

fn world_anchor_input(anchor: WorldAnchorToml) -> WorldAnchorInput {
    WorldAnchorInput {
        tile_x: anchor.tile_x,
        tile_z: anchor.tile_z,
        local_x_m: anchor.local_x_m,
        local_y_m: anchor.local_y_m,
        local_z_m: anchor.local_z_m,
    }
}

fn log_trk_route_start_vs_anchor(route_dir: &Path, anchor_bevy: Vec3, anchor: WorldAnchorToml) {
    match RouteFile::from_route_dir(route_dir) {
        Ok(route) => {
            if let Some(path) = route.source_path.as_ref() {
                viewer_log!("openrailsrs-viewer3d: .trk {}", path.display());
            }
            match route.route_start {
                Some(start) => {
                    let trk_bevy = route_start_bevy(start);
                    let dist = anchor_delta_xz(anchor_bevy, trk_bevy);
                    viewer_log!(
                        "openrailsrs-viewer3d: .trk RouteStart tile {},{} local {:.1},{:.1} vs world_anchor tile {},{} — delta XZ {:.1} m",
                        start.tile_x,
                        start.tile_z,
                        start.local_x_m,
                        start.local_z_m,
                        anchor.tile_x,
                        anchor.tile_z,
                        dist
                    );
                }
                None => viewer_log!(
                    "openrailsrs-viewer3d: .trk has no RouteStart (using world_anchor / graph fallback)"
                ),
            }
        }
        Err(err) => viewer_log!("openrailsrs-viewer3d: failed to load .trk: {err}"),
    }
}

fn log_trk_route_start_vs_anchor_optional(route_dir: &Path, anchor_world: Option<Vec3>) {
    if anchor_world.is_some() {
        return;
    }
    match RouteFile::from_route_dir(route_dir) {
        Ok(route) => {
            if let Some(path) = route.source_path.as_ref() {
                viewer_log!("openrailsrs-viewer3d: .trk {}", path.display());
            }
            match route.route_start {
                Some(start) => {
                    let trk_bevy = route_start_bevy(start);
                    viewer_log!(
                        "openrailsrs-viewer3d: .trk RouteStart tile {},{} local {:.1},{:.1} → bevy ({:.0},{:.0},{:.0})",
                        start.tile_x,
                        start.tile_z,
                        start.local_x_m,
                        start.local_z_m,
                        trk_bevy.x,
                        trk_bevy.y,
                        trk_bevy.z
                    );
                }
                None => viewer_log!(
                    "openrailsrs-viewer3d: .trk has no RouteStart (using graph / scenery fallback)"
                ),
            }
        }
        Err(err) => viewer_log!("openrailsrs-viewer3d: failed to load .trk: {err}"),
    }
}

fn run_audit_placement_mode(cli: &CliArgs) -> Result<(), String> {
    if cli.path.extension().is_none_or(|e| e != "toml") {
        return Err("--audit-placement requires a scenario.toml path".into());
    }
    let scenario_dir = cli.path.parent().ok_or("scenario path has no parent")?;
    let mut scenario = load_scenario(&cli.path).map_err(|e| e.to_string())?;
    let _ = apply_scenario_runtime_overlay_dir(&mut scenario, scenario_dir);
    let graph_route_dir = scenario_dir.join(&scenario.route.path);
    let route_dir = cli
        .route_root
        .clone()
        .unwrap_or_else(|| graph_route_dir.clone());
    let loaded = load_route_from_dir(&graph_route_dir).map_err(|e| e.to_string())?;
    let scene = TrackScene::from_loaded_route(loaded);
    let viewer3d = load_viewer3d_config(&cli.path, scenario_dir)?;
    let anchor_world = viewer3d.world_anchor.map(world_anchor_position);
    let anchor_input = viewer3d.world_anchor.map(world_anchor_input);
    let offset = match anchor_world {
        Some(world_pos) => {
            let graph_pos = graph_start_position(&scene, &scenario)?;
            RouteWorldOffset {
                delta: Vec3::new(world_pos.x - graph_pos.x, 0.0, world_pos.z - graph_pos.z),
            }
        }
        None => RouteWorldOffset::default(),
    };
    let mut stop_nodes: Vec<&str> = scenario
        .route
        .stops
        .iter()
        .map(|s| s.node.as_str())
        .collect();
    if stop_nodes.is_empty() {
        stop_nodes.push("n10778");
        stop_nodes.push(&scenario.route.start);
    }
    let tile = anchor_input
        .map(|a| (a.tile_x, a.tile_z))
        .unwrap_or(CHILTERN_BIRMINGHAM_TILE);
    let report = run_placement_audit(
        &route_dir,
        &scene,
        &scenario,
        offset,
        anchor_input,
        tile,
        &stop_nodes,
    );
    log_placement_audit(&report);
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    if let Ok(path) = std::env::var("OPENRAILSRS_PLACEMENT_AUDIT") {
        std::fs::write(&path, &json).map_err(|e| format!("write {path}: {e}"))?;
        viewer_log!("openrailsrs-viewer3d: placement audit written to {path}");
    } else {
        println!("{json}");
    }
    Ok(())
}

fn run_audit_tr_item_mode(cli: &CliArgs) -> Result<(), String> {
    if cli.path.extension().is_none_or(|e| e != "toml") {
        return Err("--audit-tr-item requires a scenario.toml path".into());
    }
    let scenario_dir = cli.path.parent().ok_or("scenario path has no parent")?;
    let scenario = load_scenario(&cli.path).map_err(|e| e.to_string())?;
    let graph_route_dir = scenario_dir.join(&scenario.route.path);
    let route_dir = cli
        .route_root
        .clone()
        .unwrap_or_else(|| graph_route_dir.clone());
    let viewer3d = load_viewer3d_config(&cli.path, scenario_dir)?;
    let tile = viewer3d
        .world_anchor
        .map(|a| (a.tile_x, a.tile_z))
        .or(Some(CHILTERN_BIRMINGHAM_TILE));
    let world = load_world_from_route_dir_near(
        &route_dir,
        viewer3d.world_anchor.map(world_anchor_position),
        8000.0,
    );
    let index = TrItemWorldIndex::rebuild_from_scene(&world);
    let report = run_tr_item_audit_for_route(&route_dir, Some(&index), tile);
    log_tr_item_audit(&report);
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    if let Ok(path) = std::env::var("OPENRAILSRS_TR_ITEM_AUDIT") {
        std::fs::write(&path, &json).map_err(|e| format!("write {path}: {e}"))?;
        viewer_log!("openrailsrs-viewer3d: tr_item audit written to {path}");
    } else {
        println!("{json}");
    }
    Ok(())
}

fn graph_start_position(
    scene: &TrackScene,
    scenario: &openrailsrs_scenarios::ScenarioFile,
) -> Result<Vec3, String> {
    let path_edges =
        resolve_scenario_route_edges(&scene.graph, &scenario.route).map_err(|e| e.to_string())?;
    let mut remaining = scenario.route.start_offset_m.unwrap_or(0.0).max(0.0);
    for edge_id in path_edges {
        let edge = scene
            .graph
            .edge(&edge_id)
            .ok_or_else(|| format!("missing edge {edge_id}"))?;
        let from = scene
            .graph
            .node(&edge.from.0)
            .ok_or_else(|| format!("missing node {}", edge.from.0))?;
        let to = scene
            .graph
            .node(&edge.to.0)
            .ok_or_else(|| format!("missing node {}", edge.to.0))?;
        if remaining <= edge.length_m || edge.length_m <= 0.0 {
            let frac = if edge.length_m > 0.0 {
                (remaining / edge.length_m).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let x_m = from.x_m + frac * (to.x_m - from.x_m);
            let y_m = from.y_m + frac * (to.y_m - from.y_m);
            return Ok(graph_to_world(x_m, y_m));
        }
        remaining -= edge.length_m;
    }
    let node = scene
        .graph
        .node(&scenario.route.start)
        .ok_or_else(|| format!("missing start node {}", scenario.route.start))?;
    Ok(graph_to_world(node.x_m, node.y_m))
}

fn build_run_corridor_path(
    scene: &TrackScene,
    scenario: &openrailsrs_scenarios::ScenarioFile,
    route_delta: Vec3,
) -> Result<RunCorridorPath, String> {
    let path_edges =
        resolve_scenario_route_edges(&scene.graph, &scenario.route).map_err(|e| e.to_string())?;
    let mut points = Vec::new();
    for edge_id in path_edges {
        let edge = scene
            .graph
            .edge(&edge_id)
            .ok_or_else(|| format!("missing edge {edge_id}"))?;
        let from = scene
            .graph
            .node(&edge.from.0)
            .ok_or_else(|| format!("missing node {}", edge.from.0))?;
        let to = scene
            .graph
            .node(&edge.to.0)
            .ok_or_else(|| format!("missing node {}", edge.to.0))?;
        let mut a = graph_to_world(from.x_m, from.y_m) + route_delta;
        let mut b = graph_to_world(to.x_m, to.y_m) + route_delta;
        if let Some(last) = points.last() {
            let last: Vec3 = *last;
            if last.distance_squared(b) < last.distance_squared(a) {
                std::mem::swap(&mut a, &mut b);
            }
        }
        if points
            .last()
            .is_none_or(|last: &Vec3| last.distance_squared(a) > 1.0)
        {
            points.push(a);
        }
        points.push(b);
    }
    let path = RunCorridorPath {
        points_world: points,
        half_width_m: run_corridor_half_width_m(),
    };
    viewer_log!(
        "openrailsrs-viewer3d: run_corridor — scenario path {} point(s), width {:.0}m",
        path.points_world.len(),
        path.half_width_m * 2.0
    );
    Ok(path)
}

/// Emite un diagnóstico de coordenadas cuando `OPENRAILSRS_COORD_DEBUG=1`.
///
/// Para cada tile con objetos `.w` muestra la posición del primer objeto en Bevy world space
/// y la compara con el centro de bounds del grafo de vías (después de aplicar el offset).
/// Si la diferencia supera 50 m hay un desalineamiento potencial que necesita investigación.
fn log_coord_debug_if_enabled(
    scene: &TrackScene,
    world: &WorldScene,
    route_offset: RouteWorldOffset,
) {
    if std::env::var_os("OPENRAILSRS_COORD_DEBUG").is_none() {
        return;
    }
    let graph_center = scene.bounds.center + route_offset.delta;
    viewer_log!(
        "openrailsrs-viewer3d: [coord-debug] graph center (Bevy world): ({:.1}, {:.1}, {:.1})  offset=({:.1},{:.1},{:.1})",
        graph_center.x,
        graph_center.y,
        graph_center.z,
        route_offset.delta.x,
        route_offset.delta.y,
        route_offset.delta.z,
    );

    // Muestra los primeros 5 objetos de mundo con su posición en Bevy world space.
    for obj in world.items.iter().take(5) {
        let tile_label = format!("({},{})", obj.tile_x, obj.tile_z);
        let dx = obj.position.x - graph_center.x;
        let dz = obj.position.z - graph_center.z;
        let dist = (dx * dx + dz * dz).sqrt();
        viewer_log!(
            "openrailsrs-viewer3d: [coord-debug] obj {:?} tile={} bevy=({:.1},{:.1}) dist_to_graph={:.0}m",
            obj.kind,
            tile_label,
            obj.position.x,
            obj.position.z,
            dist,
        );
        if dist > 50_000.0 {
            viewer_log!(
                "openrailsrs-viewer3d: [coord-debug] ⚠ LARGE OFFSET {:.0}m — posible error en conversión de tiles",
                dist
            );
        }
    }
}

fn log_scenery_debug_if_enabled(route_dir: &Path, world: &WorldScene, center: Vec3) {
    if std::env::var_os("OPENRAILSRS_SCENERY_DEBUG").is_none() {
        return;
    }
    let mut within_250 = 0usize;
    let mut within_1000 = 0usize;
    let mut within_2000 = 0usize;
    let mut by_kind: HashMap<&'static str, usize> = HashMap::new();
    let mut nearest: Vec<(&str, &str, f32, Option<&str>, bool)> = Vec::new();
    let assets = RouteAssets::new(route_dir);
    let globals: Vec<_> = global_assets_dirs(route_dir)
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    viewer_log!(
        "openrailsrs-viewer3d: scenery-debug GLOBAL roots: {}",
        if globals.is_empty() {
            "(none — set OPENRAILSRS_MSTS_CONTENT)".into()
        } else {
            globals.join(", ")
        }
    );
    let mut missing_shapes = 0usize;

    for obj in &world.items {
        let dist = Vec2::new(obj.position.x - center.x, obj.position.z - center.z).length();
        if dist <= 250.0 {
            within_250 += 1;
        }
        if dist <= 1000.0 {
            within_1000 += 1;
        }
        if dist <= 2000.0 {
            within_2000 += 1;
            *by_kind.entry(obj.kind).or_default() += 1;
            let shape_name = obj.shape_file.as_deref();
            let shape_missing = shape_name.is_some_and(|name| {
                name.to_ascii_lowercase().ends_with(".s")
                    && assets.resolve_world_shape(obj.kind, name).is_none()
            });
            if shape_missing {
                missing_shapes += 1;
            }
            nearest.push((
                obj.kind,
                obj.label.as_str(),
                dist,
                shape_name,
                shape_missing,
            ));
        }
    }

    nearest.sort_by(|a, b| a.2.total_cmp(&b.2));
    viewer_log!(
        "openrailsrs-viewer3d: scenery-debug center {:.1},{:.1},{:.1}: {} obj(s) <=250m, {} <=1000m, {} <=2000m, {} missing .s <=2000m",
        center.x,
        center.y,
        center.z,
        within_250,
        within_1000,
        within_2000,
        missing_shapes
    );
    let mut kinds: Vec<_> = by_kind.into_iter().collect();
    kinds.sort_by_key(|(kind, _)| *kind);
    for (kind, count) in kinds {
        viewer_log!("openrailsrs-viewer3d: scenery-debug kind {kind}: {count}");
    }
    for (kind, label, dist, shape, missing) in nearest.into_iter().take(12) {
        viewer_log!(
            "openrailsrs-viewer3d: scenery-debug near {:>7.1}m {:<8} {}{}{}",
            dist,
            kind,
            label,
            shape.map(|s| format!(" shape={s}")).unwrap_or_default(),
            if missing { " MISSING" } else { "" }
        );
    }

    let focus = openrailsrs_viewer3d::world::RouteFocus {
        center,
        height_origin: center.y,
    };
    let audit = openrailsrs_viewer3d::scenery_audit::audit_world_shapes_near(
        world, &focus, &assets, 2000.0, 24,
    );
    audit.log_report("near focus pre-spawn");
    openrailsrs_viewer3d::scenery_audit::log_oversized_shapes_near(
        world, &focus, &assets, 500.0, 400, 20,
    );
    openrailsrs_viewer3d::scenery_audit::log_lighting_diag_near(world, &focus, &assets, 300.0, 24);
}

fn load_train_consists(
    scenario_dir: &Path,
    scenario: &openrailsrs_scenarios::ScenarioFile,
) -> TrainConsistScene {
    let mut by_label = HashMap::new();
    match try_load_consist_vehicles(scenario_dir, &scenario.train.consist) {
        Some(vehicles) => {
            by_label.insert("primary".into(), vehicles);
        }
        None => {
            viewer_log!(
                "openrailsrs-viewer3d: warning: could not load consist {}",
                scenario_dir.join(&scenario.train.consist).display()
            );
        }
    }
    for extra in &scenario.extra_trains {
        match try_load_consist_vehicles(scenario_dir, &extra.consist) {
            Some(vehicles) => {
                by_label.insert(extra.id.clone(), vehicles);
            }
            None => {
                viewer_log!(
                    "openrailsrs-viewer3d: warning: could not load consist for train '{}': {}",
                    extra.id,
                    scenario_dir.join(&extra.consist).display()
                );
            }
        }
    }
    let mut scene = TrainConsistScene::default();
    scene.set_scenario_dir(scenario_dir.to_path_buf());
    scene.primary_consist_rel = Some(scenario.train.consist.clone());
    scene.by_label = by_label;
    scene
}

fn exit_on_esc(
    keys: Res<ButtonInput<KeyCode>>,
    mut dialog: ResMut<TeleportDialog>,
    mut exit: MessageWriter<AppExit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if dialog.open {
        openrailsrs_viewer3d::teleport::close_teleport_dialog(&mut dialog);
        return;
    }
    exit.write(AppExit::Success);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> CliArgs {
        parse_cli_from(items.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parse_cli_accepts_route_root_before_scenario() {
        let cli = args(&[
            "--live",
            "--route-root",
            "/routes/Chiltern",
            "examples/chiltern/scenario.toml",
        ]);

        assert!(cli.live);
        assert!(!cli.track_dev);
        assert!(!cli.run_corridor);
        assert_eq!(cli.route_root, Some(PathBuf::from("/routes/Chiltern")));
        assert_eq!(cli.path, PathBuf::from("examples/chiltern/scenario.toml"));
    }

    #[test]
    fn parse_cli_accepts_route_root_equals_form() {
        let cli = args(&[
            "--track-dev",
            "--route-root=/routes/Chiltern",
            "examples/chiltern/scenario.toml",
        ]);

        assert!(cli.track_dev);
        assert_eq!(cli.route_root, Some(PathBuf::from("/routes/Chiltern")));
    }

    #[test]
    fn parse_cli_accepts_cab_fov() {
        let cli = args(&[
            "--live",
            "--cab-fov",
            "72",
            "examples/chiltern/scenario.toml",
        ]);
        assert!((cli.cab_fov_deg.unwrap() - 72.0).abs() < f32::EPSILON);

        let cli = args(&["--cab-fov=65", "examples/chiltern/scenario.toml"]);
        assert!((cli.cab_fov_deg.unwrap() - 65.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_cli_accepts_audit_placement() {
        let cli = args(&[
            "--audit-placement",
            "--route-root=/routes/Chiltern",
            "examples/chiltern/scenario.toml",
        ]);
        assert!(cli.audit_placement);
        assert_eq!(cli.route_root, Some(PathBuf::from("/routes/Chiltern")));
    }

    #[test]
    fn parse_cli_accepts_audit_tr_item() {
        let cli = args(&[
            "--audit-tr-item",
            "--route-root=/routes/Chiltern",
            "examples/chiltern/scenario.toml",
        ]);
        assert!(cli.audit_tr_item);
        assert_eq!(cli.route_root, Some(PathBuf::from("/routes/Chiltern")));
    }

    #[test]
    fn chiltern_world_anchor_trims_graph_start_placement() {
        let scenario_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let scenario_path = scenario_dir.join("scenario.toml");
        if !scenario_path.is_file() {
            return;
        }
        let config = load_from_scenario(&scenario_path, true, false, false, false, None)
            .expect("chiltern scenario");
        let offset = config.route_offset_override.unwrap_or_default();
        let dist = Vec2::new(offset.delta.x, offset.delta.z).length();
        // After TrackPDP spawn (#127 / #126), graph start matches the OR world anchor.
        assert!(
            dist < 50.0,
            "expected graph start aligned with OR world anchor, got {dist:.0} m trim"
        );
    }

    #[test]
    fn parse_cli_accepts_run_corridor() {
        let cli = args(&[
            "--run-corridor",
            "--live",
            "--route-root=/routes/Chiltern",
            "examples/chiltern/scenario.toml",
        ]);

        assert!(cli.live);
        assert!(cli.run_corridor);
        assert!(!cli.track_dev);
        assert_eq!(cli.route_root, Some(PathBuf::from("/routes/Chiltern")));
    }
}
