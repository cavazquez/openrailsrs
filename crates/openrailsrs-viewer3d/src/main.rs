//! Binary entry point for the experimental 3D viewer.
//!
//! Usage:
//!   openrailsrs-viewer3d [--route-root ROUTE_DIR] [route_dir | scenario.toml]
//!   openrailsrs-viewer3d --live [--route-root ROUTE_DIR] scenario.toml
//!   openrailsrs-viewer3d --track-dev [--live] [--route-root ROUTE_DIR] scenario.toml
//!   openrailsrs-viewer3d --run-corridor --live --route-root ROUTE_DIR scenario.toml
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
use bevy::window::PresentMode;
use openrailsrs_formats::Vec3 as MstsVec3;
use openrailsrs_formats::{
    msts_tile_world_origin, msts_tile_x_index_for_coord, msts_tile_z_index_for_coord,
};
use openrailsrs_route::edge_path;
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::SCENARIO_OVERLAY_FILENAME;
use openrailsrs_scenarios::load_scenario;
use openrailsrs_viewer3d::HudTitle;
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
    RunCorridorPath, run_corridor_half_width_m, tdb_radius_for_mode, tile_lab_layers,
    track_dev_render_enabled,
};
use openrailsrs_viewer3d::rolling_stock::try_load_consist_vehicles;
use openrailsrs_viewer3d::shapes::global_assets_dirs;
use openrailsrs_viewer3d::tdb_track::collect_tdb_chords;
use openrailsrs_viewer3d::teleport::TeleportDialog;
use openrailsrs_viewer3d::terrain::load_terrain_from_route_dir_near;
use openrailsrs_viewer3d::track::{TrackScene, graph_to_world};
use openrailsrs_viewer3d::track_audit::run_track_dev_audit;
use openrailsrs_viewer3d::train::{ReplayState, TRAIN_COLORS, TrainTrack, load_csv};
use openrailsrs_viewer3d::world::{
    MSTS_TILE_SIZE_M, RouteFocus, RouteWorldOffset, WorldTileStream,
    load_world_from_route_dir_near, msts_to_bevy, visible_radius_m, world_tile_center_hint,
};
use openrailsrs_viewer3d::{log_step, viewer_log};
use serde::Deserialize;

const WORLD_ANCHOR_COORD_SPACE_THRESHOLD_M: f32 = 100_000.0;

struct LaunchConfig {
    title: String,
    route_dir: PathBuf,
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
    path: PathBuf,
    route_root: Option<PathBuf>,
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

fn parse_cli() -> CliArgs {
    parse_cli_from(std::env::args().skip(1))
}

fn parse_cli_from(args: impl IntoIterator<Item = String>) -> CliArgs {
    let mut live = false;
    let mut track_dev = false;
    let mut run_corridor = false;
    let mut tile_lab = false;
    let mut route_root = None;
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
        } else if arg == "--route-root" {
            route_root = args.next().map(PathBuf::from);
        } else if let Some(value) = arg.strip_prefix("--route-root=") {
            route_root = Some(PathBuf::from(value));
        } else if !arg.starts_with('-') {
            path = Some(PathBuf::from(arg));
        }
    }
    CliArgs {
        live,
        track_dev,
        run_corridor,
        tile_lab,
        path: path.unwrap_or_else(|| PathBuf::from("examples/smoke/routes/test")),
        route_root,
    }
}

fn main() {
    init_viewer_log();
    let cli = parse_cli();
    let assets = RouteAssets::new(
        cli.route_root
            .as_deref()
            .unwrap_or(cli.path.parent().unwrap_or(&cli.path)),
    );

    let config = match build_launch_config(
        &cli.path,
        cli.live,
        cli.track_dev,
        cli.run_corridor,
        cli.tile_lab,
        cli.route_root.as_deref(),
    ) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!(
                "usage: openrailsrs-viewer3d [--live] [--track-dev|--run-corridor|--tile-lab] [--route-root ROUTE_DIR] [route_dir | scenario.toml]"
            );
            std::process::exit(1);
        }
    };

    if config.scenery_mode.is_tile_lab() {
        viewer_log!(
            "openrailsrs-viewer3d: scenery_mode=tile_lab — 1 tile, sin streaming; capas vía OPENRAILSRS_TILE_LAB_LAYERS=terrain,track,world,train"
        );
    } else if config.scenery_mode.is_track_dev() {
        viewer_log!(
            "openrailsrs-viewer3d: scenery_mode=track_dev — vía procedural desde .tdb; sin terreno/shapes/TrackObj"
        );
    } else if config.scenery_mode.is_run_corridor() {
        if assets.track_db().is_some() {
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
            && cli.path.extension().and_then(|e| e.to_str()) == Some("toml")
        {
            "\n  hint: add --live or run: cargo run -p openrailsrs-cli -- sim <scenario.toml>"
                .to_string()
        } else {
            String::new()
        },
    );
    if config.live.is_some()
        && config.scene.render_mode == openrailsrs_viewer3d::track::TrackRenderMode::Compact
        && cfg!(debug_assertions)
    {
        viewer_log!(
            "openrailsrs-viewer3d: tip — large route in debug is very slow; use \
             `cargo run --release -p openrailsrs-viewer3d -- --live …` for playable FPS"
        );
    }

    let route_focus = route_focus_for_config(&config);
    if config.elevation.is_empty() {
        viewer_log!(
            "openrailsrs-viewer3d: render height origin {:.0} m (scenery bbox y {:.0}, no terrain RAW)",
            route_focus.height_origin,
            route_focus.center.y
        );
    } else if (route_focus.height_origin - route_focus.center.y).abs() > 0.5 {
        viewer_log!(
            "openrailsrs-viewer3d: render height origin {:.0} m terrain MSL (scenery bbox y {:.0})",
            route_focus.height_origin,
            route_focus.center.y
        );
    }
    let route_offset = config
        .route_offset_override
        .unwrap_or_else(|| RouteWorldOffset::from_scene_and_world(&config.scene, &config.world));
    log_coord_debug_if_enabled(&config.scene, &config.world, route_offset);
    log_scenery_debug_if_enabled(&config.route_dir, &config.world, route_focus.center);

    if config.scenery_mode.draws_tdb_track() {
        if let Some(tdb) = assets.track_db() {
            let radius_m = tdb_radius_for_mode(config.scenery_mode);
            viewer_log!(
                "openrailsrs-viewer3d: {} — pre-audit {:.0}m radius (before Bevy)…",
                if config.scenery_mode.is_run_corridor() {
                    "run_corridor"
                } else {
                    "track_dev"
                },
                radius_m
            );
            let mut chords =
                collect_tdb_chords(tdb, &route_focus, radius_m, Some(assets.tsection()));
            if config.scenery_mode.is_run_corridor() && config.run_corridor_path.active() {
                let before = chords.len();
                chords.retain(|chord| {
                    config
                        .run_corridor_path
                        .contains_segment(chord.start_world, chord.end_world)
                });
                viewer_log!(
                    "openrailsrs-viewer3d: run_corridor — corridor filter {} → {} chord(s), width {:.0}m",
                    before,
                    chords.len(),
                    config.run_corridor_path.half_width_m * 2.0
                );
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
            );
            if config.scenery_mode.is_track_dev() && !track_dev_render_enabled() {
                viewer_log!(
                    "openrailsrs-viewer3d: track_dev — audit complete; OPENRAILSRS_TRACK_DEV_RENDER=1 to draw rails in window"
                );
            }
        }
    }

    viewer_log!("openrailsrs-viewer3d: starting Bevy app");
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: config.title.clone(),
                    resolution: (1280u32, 720u32).into(),
                    present_mode: PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(ViewerLaunchOpts {
        live: config.live.is_some(),
    })
    .insert_resource(config.scenery_mode)
    .insert_resource(config.run_corridor_path)
    .insert_resource(config.scene)
    .insert_resource(if config.scenery_mode.is_tile_lab() {
        // No streaming catalog in tile-lab: only the startup tile is shown.
        WorldTileStream::default()
    } else {
        WorldTileStream::new(&config.route_dir, &config.world, visible_radius_m())
    })
    .insert_resource(assets)
    .insert_resource(route_focus)
    .insert_resource(route_offset)
    .insert_resource(config.world)
    .insert_resource(config.terrain)
    .insert_resource(config.elevation)
    .insert_resource(config.replay)
    .insert_resource(config.consist)
    .insert_resource(HudTitle(config.title))
    .add_plugins(ViewerPlugin)
    .add_systems(Update, exit_on_esc);

    if let Some(live) = config.live {
        app.insert_resource(live);
    }

    app.run();
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
    let graph = load_track_graph_from_route_dir(route_dir).map_err(|e| e.to_string())?;
    log_step(
        &format!("loaded track graph ({} nodes)", graph.nodes_iter().count()),
        t,
    );
    let scene = TrackScene::from_graph(graph);
    let track_dev = track_dev_cli;
    let t = Instant::now();
    let mut world = if track_dev {
        viewer_log!("openrailsrs-viewer3d: track_dev — skipping .w world load");
        WorldScene::default()
    } else {
        let world_hint = world_tile_center_hint(route_dir).unwrap_or(scene.bounds.center);
        load_world_from_route_dir_near(route_dir, Some(world_hint), visible_radius_m())
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
        world.retain_within_visible_radius(&focus, visible_radius_m());
    }
    let t = Instant::now();
    let terrain = if track_dev {
        TerrainScene::default()
    } else {
        load_terrain_from_route_dir_near(route_dir, Some(focus.center), visible_radius_m())
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
    let graph = load_track_graph_from_route_dir(&graph_route_dir).map_err(|e| e.to_string())?;
    log_step(
        &format!("loaded track graph ({} nodes)", graph.nodes_iter().count()),
        t,
    );
    let scene = TrackScene::from_graph(graph);
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
    let t = Instant::now();
    let mut world = if scenery_mode.is_track_focused() {
        viewer_log!("openrailsrs-viewer3d: track-focused — skipping .w world load");
        WorldScene::default()
    } else {
        let world_hint = anchor_world
            .or_else(|| world_tile_center_hint(&route_dir))
            .unwrap_or(scene.bounds.center);
        load_world_from_route_dir_near(&route_dir, Some(world_hint), visible_radius_m())
    };
    if !scenery_mode.is_track_focused() {
        log_step(
            &format!(
                "loaded world ({} obj(s) / {} tile(s))",
                world.items.len(),
                world.tiles_loaded
            ),
            t,
        );
    }
    let route_offset_override = match (viewer3d.world_anchor, anchor_world) {
        (Some(anchor), Some(world_pos)) => {
            let graph_pos = graph_start_position(&scene, &scenario)?;
            let delta = Vec3::new(world_pos.x - graph_pos.x, 0.0, world_pos.z - graph_pos.z);
            // A small delta means the graph is already in absolute MSTS world
            // coordinates (patched x_m/y_m): the anchor is only an approximate
            // "near the start" reference, so shifting the graph by it would
            // *misalign* track vs scenery. Only shift when the delta is huge,
            // i.e. the graph is in a local/route coordinate space.
            let in_world_coords =
                Vec2::new(delta.x, delta.z).length() <= WORLD_ANCHOR_COORD_SPACE_THRESHOLD_M;
            let (applied, kind) = if in_world_coords {
                (Vec3::ZERO, "graph already in world coords; no shift")
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
                delta.x,
                delta.y,
                delta.z
            );
            Some(RouteWorldOffset { delta: applied })
        }
        _ => None,
    };
    let run_corridor_path = if scenery_mode.is_run_corridor() {
        build_run_corridor_path(
            &scene,
            &scenario,
            route_offset_override.unwrap_or_default().delta,
        )?
    } else {
        RunCorridorPath::default()
    };
    let focus = anchor_world
        .map(|center| RouteFocus {
            center,
            height_origin: center.y,
        })
        .unwrap_or_else(|| RouteFocus::from_scene_and_world(&scene, &world));
    if !scenery_mode.is_track_focused() {
        world.retain_within_visible_radius(&focus, visible_radius_m());
    }
    let t = Instant::now();
    let terrain = if scenery_mode.is_track_focused() {
        TerrainScene::default()
    } else {
        load_terrain_from_route_dir_near(&route_dir, Some(focus.center), visible_radius_m())
    };
    log_step(
        &format!("loaded terrain index ({} tile(s))", terrain.tiles_loaded),
        t,
    );
    let t = Instant::now();
    let elevation = if scenery_mode.is_track_focused() {
        TerrainElevation::default()
    } else {
        TerrainElevation::from_terrain_scene(&terrain)
    };
    log_step("loaded terrain elevation", t);

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
    }
    Ok(out)
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

fn graph_start_position(
    scene: &TrackScene,
    scenario: &openrailsrs_scenarios::ScenarioFile,
) -> Result<Vec3, String> {
    let path_edges = edge_path(
        &scene.graph,
        &scenario.route.start,
        &scenario.route.destination,
    )
    .map_err(|e| e.to_string())?;
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
    let path_edges = edge_path(
        &scene.graph,
        &scenario.route.start,
        &scenario.route.destination,
    )
    .map_err(|e| e.to_string())?;
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
