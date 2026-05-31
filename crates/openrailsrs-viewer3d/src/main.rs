//! Binary entry point for the experimental 3D viewer.
//!
//! Usage:
//!   openrailsrs-viewer3d [route_dir | scenario.toml]
//!   openrailsrs-viewer3d --live scenario.toml
//!   openrailsrs-viewer3d --track-dev [--live] scenario.toml
//!
//! - `route_dir` — static graph only (default: `examples/smoke/routes/test`).
//! - `scenario.toml` — graph + animated train marker(s) from simulation CSV.
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
use openrailsrs_viewer3d::launch::{track_dev_render_enabled, track_dev_tdb_radius_m};
use openrailsrs_viewer3d::rolling_stock::try_load_consist_vehicles;
use openrailsrs_viewer3d::shapes::global_assets_dirs;
use openrailsrs_viewer3d::tdb_track::collect_tdb_chords;
use openrailsrs_viewer3d::teleport::TeleportDialog;
use openrailsrs_viewer3d::terrain::load_terrain_from_route_dir_near;
use openrailsrs_viewer3d::track::{TrackScene, graph_to_world};
use openrailsrs_viewer3d::track_audit::run_track_dev_audit;
use openrailsrs_viewer3d::train::{ReplayState, TRAIN_COLORS, TrainTrack, load_csv};
use openrailsrs_viewer3d::world::{
    RouteFocus, RouteWorldOffset, VISIBLE_RADIUS_M, WorldTileStream,
    load_world_from_route_dir_near, msts_to_bevy, world_tile_center_hint,
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
    focus_center_override: Option<Vec3>,
    route_offset_override: Option<RouteWorldOffset>,
}

struct CliArgs {
    live: bool,
    track_dev: bool,
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SceneryModeToml {
    #[default]
    Full,
    TrackDev,
}

impl From<SceneryModeToml> for ViewerSceneryMode {
    fn from(value: SceneryModeToml) -> Self {
        match value {
            SceneryModeToml::Full => Self::Full,
            SceneryModeToml::TrackDev => Self::TrackDev,
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
    let mut live = false;
    let mut track_dev = false;
    let mut path = None;
    for arg in std::env::args().skip(1) {
        if arg == "--live" {
            live = true;
        } else if arg == "--track-dev" {
            track_dev = true;
        } else if !arg.starts_with('-') {
            path = Some(PathBuf::from(arg));
        }
    }
    CliArgs {
        live,
        track_dev,
        path: path.unwrap_or_else(|| PathBuf::from("examples/smoke/routes/test")),
    }
}

fn main() {
    init_viewer_log();
    let cli = parse_cli();

    let config = match build_launch_config(&cli.path, cli.live, cli.track_dev) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!(
                "usage: openrailsrs-viewer3d [--live] [--track-dev] [route_dir | scenario.toml]"
            );
            std::process::exit(1);
        }
    };

    if config.scenery_mode.is_track_dev() {
        viewer_log!(
            "openrailsrs-viewer3d: scenery_mode=track_dev — vía procedural desde .tdb; sin terreno/shapes/TrackObj"
        );
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
    log_scenery_debug_if_enabled(&config.route_dir, &config.world, route_focus.center);

    let assets = RouteAssets::new(&config.route_dir);
    if config.scenery_mode.is_track_dev() {
        if let Some(tdb) = assets.track_db() {
            let radius_m = track_dev_tdb_radius_m();
            viewer_log!(
                "openrailsrs-viewer3d: track_dev — pre-audit {:.0}m radius (before Bevy)…",
                radius_m
            );
            let chords = collect_tdb_chords(tdb, &route_focus, radius_m);
            run_track_dev_audit(
                tdb,
                &config.scene,
                &route_focus,
                route_offset,
                radius_m,
                &chords,
            );
            if !track_dev_render_enabled() {
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
    .insert_resource(config.scene)
    .insert_resource(WorldTileStream::new(
        &config.route_dir,
        &config.world,
        VISIBLE_RADIUS_M,
    ))
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
) -> Result<LaunchConfig, String> {
    if arg.extension().and_then(|e| e.to_str()) == Some("toml") {
        load_from_scenario(arg, live, track_dev_cli)
    } else if live {
        Err("--live requires a scenario.toml path".into())
    } else {
        load_from_route_dir(arg, track_dev_cli)
    }
}

fn resolve_scenery_mode(track_dev_cli: bool, from_toml: SceneryModeToml) -> ViewerSceneryMode {
    if track_dev_cli {
        ViewerSceneryMode::TrackDev
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
        load_world_from_route_dir_near(route_dir, Some(world_hint), VISIBLE_RADIUS_M)
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
        world.retain_within_visible_radius(&focus, VISIBLE_RADIUS_M);
    }
    let t = Instant::now();
    let terrain = if track_dev {
        TerrainScene::default()
    } else {
        load_terrain_from_route_dir_near(route_dir, Some(focus.center), VISIBLE_RADIUS_M)
    };
    log_step(
        &format!("loaded terrain index ({} tile(s))", terrain.tiles_loaded),
        t,
    );
    let t = Instant::now();
    let elevation = if track_dev {
        TerrainElevation::default()
    } else {
        TerrainElevation::load_from_route_dir_near(route_dir, Some(focus.center), VISIBLE_RADIUS_M)
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
        scenery_mode: resolve_scenery_mode(track_dev_cli, SceneryModeToml::Full),
        focus_center_override: None,
        route_offset_override: None,
    })
}

fn load_from_scenario(
    path: &Path,
    live: bool,
    track_dev_cli: bool,
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
    let route_dir = scenario_dir.join(&scenario.route.path);
    let t = Instant::now();
    let graph = load_track_graph_from_route_dir(&route_dir).map_err(|e| e.to_string())?;
    log_step(
        &format!("loaded track graph ({} nodes)", graph.nodes_iter().count()),
        t,
    );
    let scene = TrackScene::from_graph(graph);
    let viewer3d = load_viewer3d_config(path, scenario_dir)?;
    let scenery_mode = resolve_scenery_mode(track_dev_cli, viewer3d.scenery_mode);
    let anchor_world = viewer3d.world_anchor.map(world_anchor_position);
    let t = Instant::now();
    let mut world = if scenery_mode.is_track_dev() {
        viewer_log!("openrailsrs-viewer3d: track_dev — skipping .w world load");
        WorldScene::default()
    } else {
        let world_hint = anchor_world
            .or_else(|| world_tile_center_hint(&route_dir))
            .unwrap_or(scene.bounds.center);
        load_world_from_route_dir_near(&route_dir, Some(world_hint), VISIBLE_RADIUS_M)
    };
    if !scenery_mode.is_track_dev() {
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
            if Vec2::new(delta.x, delta.z).length() <= WORLD_ANCHOR_COORD_SPACE_THRESHOLD_M {
                viewer_log!(
                    "openrailsrs-viewer3d: viewer3d world anchor tile {},{} local {:.1},{:.1},{:.1} -> focus only (graph already in MSTS world; anchor delta {:.0},{:.0},{:.0} m)",
                    anchor.tile_x,
                    anchor.tile_z,
                    anchor.local_x_m,
                    anchor.local_y_m,
                    anchor.local_z_m,
                    delta.x,
                    delta.y,
                    delta.z
                );
                Some(RouteWorldOffset::default())
            } else {
                viewer_log!(
                    "openrailsrs-viewer3d: viewer3d world anchor tile {},{} local {:.1},{:.1},{:.1} -> offset {:.0},{:.0},{:.0} m",
                    anchor.tile_x,
                    anchor.tile_z,
                    anchor.local_x_m,
                    anchor.local_y_m,
                    anchor.local_z_m,
                    delta.x,
                    delta.y,
                    delta.z
                );
                Some(RouteWorldOffset { delta })
            }
        }
        _ => None,
    };
    let focus = anchor_world
        .map(|center| RouteFocus {
            center,
            height_origin: center.y,
        })
        .unwrap_or_else(|| RouteFocus::from_scene_and_world(&scene, &world));
    if !scenery_mode.is_track_dev() {
        world.retain_within_visible_radius(&focus, VISIBLE_RADIUS_M);
    }
    let t = Instant::now();
    let terrain = if scenery_mode.is_track_dev() {
        TerrainScene::default()
    } else {
        load_terrain_from_route_dir_near(&route_dir, Some(focus.center), VISIBLE_RADIUS_M)
    };
    log_step(
        &format!("loaded terrain index ({} tile(s))", terrain.tiles_loaded),
        t,
    );
    let t = Instant::now();
    let elevation = if scenery_mode.is_track_dev() {
        TerrainElevation::default()
    } else {
        TerrainElevation::load_from_route_dir_near(&route_dir, Some(focus.center), VISIBLE_RADIUS_M)
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
        focus_center_override: anchor_world,
        route_offset_override,
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
    let display_tile_x = if anchor.tile_x < 0 {
        -anchor.tile_x
    } else {
        anchor.tile_x
    };
    msts_to_bevy(
        display_tile_x,
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
