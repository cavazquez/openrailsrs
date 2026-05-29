//! Binary entry point for the experimental 3D viewer.
//!
//! Usage:
//!   openrailsrs-viewer3d [route_dir | scenario.toml]
//!   openrailsrs-viewer3d --live scenario.toml
//!
//! - `route_dir` — static graph only (default: `examples/smoke/routes/test`).
//! - `scenario.toml` — graph + animated train marker(s) from simulation CSV.
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

use bevy::prelude::*;
use bevy::window::PresentMode;
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::load_scenario;
use openrailsrs_viewer3d::HudTitle;
use openrailsrs_viewer3d::LiveDrive;
use openrailsrs_viewer3d::RouteAssets;
use openrailsrs_viewer3d::TerrainElevation;
use openrailsrs_viewer3d::TerrainScene;
use openrailsrs_viewer3d::TrainConsistScene;
use openrailsrs_viewer3d::ViewerLaunchOpts;
use openrailsrs_viewer3d::ViewerPlugin;
use openrailsrs_viewer3d::WorldScene;
use openrailsrs_viewer3d::rolling_stock::try_load_consist_vehicles;
use openrailsrs_viewer3d::teleport::TeleportDialog;
use openrailsrs_viewer3d::terrain::load_terrain_from_route_dir_near;
use openrailsrs_viewer3d::track::TrackScene;
use openrailsrs_viewer3d::train::{ReplayState, TRAIN_COLORS, TrainTrack, load_csv};
use openrailsrs_viewer3d::world::RouteFocus;
use openrailsrs_viewer3d::world::RouteWorldOffset;
use openrailsrs_viewer3d::world::VISIBLE_RADIUS_M;
use openrailsrs_viewer3d::world::load_world_from_route_dir;

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
}

struct CliArgs {
    live: bool,
    path: PathBuf,
}

fn parse_cli() -> CliArgs {
    let mut live = false;
    let mut path = None;
    for arg in std::env::args().skip(1) {
        if arg == "--live" {
            live = true;
        } else if !arg.starts_with('-') {
            path = Some(PathBuf::from(arg));
        }
    }
    CliArgs {
        live,
        path: path.unwrap_or_else(|| PathBuf::from("examples/smoke/routes/test")),
    }
}

fn main() {
    let cli = parse_cli();

    let config = match build_launch_config(&cli.path, cli.live) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("usage: openrailsrs-viewer3d [--live] [route_dir | scenario.toml]");
            std::process::exit(1);
        }
    };

    let node_count = config.scene.graph.nodes_iter().count();
    let edge_count = config.scene.edge_count;
    eprintln!(
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
        eprintln!(
            "openrailsrs-viewer3d: tip — large route in debug is very slow; use \
             `cargo run --release -p openrailsrs-viewer3d -- --live …` for playable FPS"
        );
    }

    let route_focus = RouteFocus::from_scene_world_and_elevation(
        &config.scene,
        &config.world,
        if config.elevation.is_empty() {
            None
        } else {
            Some(&config.elevation)
        },
    );
    if route_focus.height_origin != route_focus.center.y {
        eprintln!(
            "openrailsrs-viewer3d: render height origin {:.0} m (scenery bbox y {:.0})",
            route_focus.height_origin, route_focus.center.y
        );
    }
    let route_offset = RouteWorldOffset::from_scene_and_world(&config.scene, &config.world);

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
    .insert_resource(config.scene)
    .insert_resource(RouteAssets::new(config.route_dir))
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

fn build_launch_config(arg: &Path, live: bool) -> Result<LaunchConfig, String> {
    if arg.extension().and_then(|e| e.to_str()) == Some("toml") {
        load_from_scenario(arg, live)
    } else if live {
        Err("--live requires a scenario.toml path".into())
    } else {
        load_from_route_dir(arg)
    }
}

fn load_from_route_dir(route_dir: &Path) -> Result<LaunchConfig, String> {
    let graph = load_track_graph_from_route_dir(route_dir).map_err(|e| e.to_string())?;
    let scene = TrackScene::from_graph(graph);
    let world = load_world_from_route_dir(route_dir);
    let focus = RouteFocus::from_scene_and_world(&scene, &world);
    let terrain = load_terrain_from_route_dir_near(route_dir, Some(focus.center), VISIBLE_RADIUS_M);
    let elevation =
        TerrainElevation::load_from_route_dir_near(route_dir, Some(focus.center), VISIBLE_RADIUS_M);
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
    })
}

fn load_from_scenario(path: &Path, live: bool) -> Result<LaunchConfig, String> {
    let scenario_dir = path
        .parent()
        .ok_or("scenario path has no parent directory")?;
    let scenario = load_scenario(path).map_err(|e| e.to_string())?;
    let route_dir = scenario_dir.join(&scenario.route.path);
    let graph = load_track_graph_from_route_dir(&route_dir).map_err(|e| e.to_string())?;
    let scene = TrackScene::from_graph(graph);
    let world = load_world_from_route_dir(&route_dir);
    let focus = RouteFocus::from_scene_and_world(&scene, &world);
    let terrain =
        load_terrain_from_route_dir_near(&route_dir, Some(focus.center), VISIBLE_RADIUS_M);
    let elevation = TerrainElevation::load_from_route_dir_near(
        &route_dir,
        Some(focus.center),
        VISIBLE_RADIUS_M,
    );

    let consist = load_train_consists(scenario_dir, &scenario);

    let (replay, live_drive) = if live {
        let drive = LiveDrive::from_scenario_path(path)?;
        eprintln!(
            "openrailsrs-viewer3d: live drive on \"{}\" (dt={:.2}s, ↑/↓ throttle/brake, F2 fly, G teleport)",
            drive.session.scenario_name, drive.session.dt,
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
    })
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
            eprintln!(
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
                eprintln!(
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
