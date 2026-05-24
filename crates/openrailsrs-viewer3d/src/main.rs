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
//! - Orbit: right = rotate, middle = pan, wheel = zoom.
//! - Fly: WASD move, `Q`/`E` up/down (`Space` = up unless replay is loaded).
//! - Replay: `Space` pause, `R` reset, `+`/`-` speed, `T` cycle camera follow (when CSV loaded).
//! - `Esc`         — quit.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::load_scenario;
use openrailsrs_viewer3d::ViewerPlugin;
use openrailsrs_viewer3d::track::TrackScene;
use openrailsrs_viewer3d::train::{ReplayState, TRAIN_COLORS, TrainTrack, load_csv};

struct LaunchConfig {
    title: String,
    scene: TrackScene,
    replay: ReplayState,
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
        "openrailsrs-viewer3d: {} ({} nodes, {} edges, render={}{})",
        config.title,
        node_count,
        edge_count,
        config.scene.render_mode.label(),
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
        .insert_resource(config.replay)
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
    Ok(LaunchConfig {
        title: format!("openrailsrs-viewer3d — {}", route_dir.display()),
        scene: TrackScene::from_graph(graph),
        replay: ReplayState::default(),
    })
}

fn load_from_scenario(path: &Path) -> Result<LaunchConfig, String> {
    let scenario_dir = path
        .parent()
        .ok_or("scenario path has no parent directory")?;
    let scenario = load_scenario(path).map_err(|e| e.to_string())?;
    let route_dir = scenario_dir.join(&scenario.route.path);
    let graph = load_track_graph_from_route_dir(&route_dir).map_err(|e| e.to_string())?;

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
    Ok(LaunchConfig {
        title: format!("openrailsrs-viewer3d — {}", scenario.scenario.name),
        scene: TrackScene::from_graph(graph),
        replay,
    })
}

fn exit_on_esc(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
