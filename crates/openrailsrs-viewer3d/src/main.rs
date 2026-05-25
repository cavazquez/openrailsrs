//! Binary entry point for the experimental 3D viewer.
//!
//! Usage:
//!   openrailsrs-viewer3d [route_dir | scenario.toml]
//!
//! - `route_dir` — static graph only (default: `examples/smoke/routes/test`).
//! - `scenario.toml` — graph + animated train marker(s) from simulation CSV.
//!
//! Generate CSV first, e.g.:
//!   cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml
//!
//! Controls:
//!
//! - `F1` / `F2`   — orbit / fly camera.
//! - Orbit: drag (LMB/RMB) = rotate, Shift+drag or WASD = pan, wheel = zoom.
//! - Fly: WASD move, `Q`/`E` up/down (`Space` = up unless replay is loaded).
//! - Replay: `Space` pause, `R` reset, `+`/`-` speed, `T` cycle camera follow (when CSV loaded).
//! - Multi-train replay: `[` / `]` (or Shift+T) cycle which train the follow camera tracks.
//! - `G`           — teleport dialog (type x,y,z).
//! - `P`           — toggle rain streaks.
//! - `Esc`         — quit (closes teleport dialog first if open).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::load_scenario;
use openrailsrs_viewer3d::HudTitle;
use openrailsrs_viewer3d::RouteAssets;
use openrailsrs_viewer3d::TerrainElevation;
use openrailsrs_viewer3d::TerrainScene;
use openrailsrs_viewer3d::TrainConsistScene;
use openrailsrs_viewer3d::ViewerPlugin;
use openrailsrs_viewer3d::WorldScene;
use openrailsrs_viewer3d::rolling_stock::try_load_consist_vehicles;
use openrailsrs_viewer3d::teleport::TeleportDialog;
use openrailsrs_viewer3d::terrain::load_terrain_from_route_dir;
use openrailsrs_viewer3d::track::TrackScene;
use openrailsrs_viewer3d::train::{ReplayState, TRAIN_COLORS, TrainTrack, load_csv};
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
}

fn main() {
    let arg = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/smoke/routes/test"));

    let config = match build_launch_config(&arg) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("usage: openrailsrs-viewer3d [route_dir | scenario.toml]");
            std::process::exit(1);
        }
    };

    let node_count = config.scene.graph.nodes_iter().count();
    let edge_count = config.scene.edge_count;
    eprintln!(
        "openrailsrs-viewer3d: {} ({} nodes, {} edges, render={}{}{}{})",
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
        if config.replay.is_active() {
            format!(", {} train(s) replay", config.replay.tracks.len())
        } else {
            String::new()
        }
    );
    if !config.replay.is_active() && arg.extension().and_then(|e| e.to_str()) == Some("toml") {
        eprintln!("hint: run simulation first to create the CSV, e.g.:");
        eprintln!("  cargo run -p openrailsrs-cli -- sim {}", arg.display());
    }

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: config.title.clone(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(config.scene)
        .insert_resource(RouteAssets::new(config.route_dir))
        .insert_resource(config.world)
        .insert_resource(config.terrain)
        .insert_resource(config.elevation)
        .insert_resource(config.replay)
        .insert_resource(config.consist)
        .insert_resource(HudTitle(config.title))
        .add_plugins(ViewerPlugin)
        .add_systems(Update, exit_on_esc)
        .run();
}

fn build_launch_config(arg: &Path) -> Result<LaunchConfig, String> {
    if arg.extension().and_then(|e| e.to_str()) == Some("toml") {
        load_from_scenario(arg)
    } else {
        load_from_route_dir(arg)
    }
}

fn load_from_route_dir(route_dir: &Path) -> Result<LaunchConfig, String> {
    let graph = load_track_graph_from_route_dir(route_dir).map_err(|e| e.to_string())?;
    let world = load_world_from_route_dir(route_dir);
    let terrain = load_terrain_from_route_dir(route_dir);
    let elevation = TerrainElevation::load_from_route_dir(route_dir);
    Ok(LaunchConfig {
        title: format!("openrailsrs-viewer3d — {}", route_dir.display()),
        route_dir: route_dir.to_path_buf(),
        scene: TrackScene::from_graph(graph),
        world,
        terrain,
        elevation,
        replay: ReplayState::default(),
        consist: TrainConsistScene::default(),
    })
}

fn load_from_scenario(path: &Path) -> Result<LaunchConfig, String> {
    let scenario_dir = path
        .parent()
        .ok_or("scenario path has no parent directory")?;
    let scenario = load_scenario(path).map_err(|e| e.to_string())?;
    let route_dir = scenario_dir.join(&scenario.route.path);
    let graph = load_track_graph_from_route_dir(&route_dir).map_err(|e| e.to_string())?;
    let world = load_world_from_route_dir(&route_dir);
    let terrain = load_terrain_from_route_dir(&route_dir);
    let elevation = TerrainElevation::load_from_route_dir(&route_dir);

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

    let replay = ReplayState::new(scenario.scenario.name.clone(), tracks);
    let consist = load_train_consists(scenario_dir, &scenario);
    Ok(LaunchConfig {
        title: format!("openrailsrs-viewer3d — {}", scenario.scenario.name),
        route_dir,
        scene: TrackScene::from_graph(graph),
        world,
        terrain,
        elevation,
        replay,
        consist,
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
    TrainConsistScene {
        scenario_dir: Some(scenario_dir.to_path_buf()),
        by_label,
    }
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
