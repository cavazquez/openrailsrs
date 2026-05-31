//! Shared Bevy test helpers (`MinimalPlugins`, smoke fixtures, `run_system_once`).

#![cfg(test)]

use std::path::{Path, PathBuf};

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::load_scenario;
use openrailsrs_track::{Edge, Node, NodeKind, SignalAspect, TrackGraph, TrackSignal};

use crate::HudTitle;
use crate::camera::{
    CameraFollowMode, CameraFollowTarget, CameraMode, LiveDriverCab, OrbitDistanceLimit,
};
use crate::launch::ViewerLaunchOpts;
use crate::live::LiveDrive;
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::RouteAssets;
use crate::terrain::{TerrainElevation, TerrainScene};
use crate::terrain_material::TerrainMaterial;
use crate::track::TrackScene;
use crate::train::{CsvRow, ReplayState, TrainTrack};
use crate::world::{RouteFocus, RouteWorldOffset, VISIBLE_RADIUS_M, WorldScene};

pub fn smoke_route_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test")
}

pub fn smoke_scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml")
}

pub fn chiltern_scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern/scenario.toml")
}

pub fn tiny_graph_with_signal() -> TrackGraph {
    let mut g = TrackGraph::new();
    g.insert_node(Node {
        id: NodeId("a".into()),
        kind: NodeKind::Plain,
        x_m: 0.0,
        y_m: 0.0,
    })
    .unwrap();
    g.insert_node(Node {
        id: NodeId("b".into()),
        kind: NodeKind::Switch {
            stem_edge: EdgeId("e1".into()),
            diverging_edge: EdgeId("e2".into()),
        },
        x_m: 100.0,
        y_m: 0.0,
    })
    .unwrap();
    g.insert_edge(Edge {
        id: EdgeId("e1".into()),
        from: NodeId("a".into()),
        to: NodeId("b".into()),
        length_m: 100.0,
        speed_limit_mps: 20.0,
        grade_percent: 0.0,
    })
    .unwrap();
    g.insert_signal(TrackSignal {
        id: "sig1".into(),
        edge_id: "e1".into(),
        position_m: 50.0,
        aspect: SignalAspect::Caution,
        clear_after_s: None,
        script: None,
    })
    .unwrap();
    g
}

pub fn sample_replay_track() -> TrainTrack {
    TrainTrack {
        label: "primary".into(),
        color: Color::srgb(1.0, 0.25, 1.0),
        rows: vec![
            CsvRow {
                time_s: 0.0,
                velocity_mps: 10.0,
                edge_id: "e1".into(),
                pos_on_edge_m: 0.0,
            },
            CsvRow {
                time_s: 10.0,
                velocity_mps: 10.0,
                edge_id: "e1".into(),
                pos_on_edge_m: 100.0,
            },
        ],
    }
}

/// `MinimalPlugins` + assets used by spawn systems.
pub fn minimal_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        MaterialPlugin::<TerrainMaterial>::default(),
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<Image>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<TerrainMaterial>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.init_resource::<ButtonInput<MouseButton>>();
    app.init_resource::<CameraMode>();
    app.init_resource::<CameraFollowMode>();
    app.init_resource::<CameraFollowTarget>();
    app.init_resource::<OrbitDistanceLimit>();
    app.init_resource::<crate::floating_origin::FloatingOrigin>();
    app.init_resource::<crate::gameplay::GameplayToast>();
    app.init_resource::<LiveDriverCab>();
    app.init_resource::<crate::live::DriverCamState>();
    app.insert_resource(ViewerLaunchOpts::default());
    app.insert_resource(TerrainElevation::default());
    app.insert_resource(HudTitle("test".into()));
    app
}

pub fn insert_replay_bundle(app: &mut App, scene: TrackScene, replay: ReplayState) {
    let world_scene = WorldScene::default();
    let route_focus = RouteFocus::from_scene_and_world(&scene, &world_scene);
    let route_offset = RouteWorldOffset::from_scene_and_world(&scene, &world_scene);
    app.insert_resource(route_focus);
    app.insert_resource(route_offset);
    app.insert_resource(world_scene);
    app.insert_resource(TerrainScene::default());
    app.insert_resource(scene);
    app.insert_resource(replay);
    app.insert_resource(TrainConsistScene::default());
    app.insert_resource(RouteAssets::new(smoke_route_dir()));
}

pub fn try_smoke_live_drive() -> Option<LiveDrive> {
    let path = smoke_scenario_path();
    if !path.exists() {
        return None;
    }
    LiveDrive::from_scenario_path(&path).ok()
}

#[allow(clippy::type_complexity)]
pub fn load_smoke_route_bundle() -> Option<(
    TrackScene,
    WorldScene,
    TerrainScene,
    TerrainElevation,
    RouteFocus,
    RouteWorldOffset,
    TrainConsistScene,
    RouteAssets,
)> {
    let route_dir = smoke_route_dir();
    if !route_dir.join("track.toml").exists() {
        return None;
    }
    let graph = load_track_graph_from_route_dir(&route_dir).ok()?;
    let scene = TrackScene::from_graph(graph);
    let world = crate::world::load_world_from_route_dir(&route_dir);
    let focus = RouteFocus::from_scene_and_world(&scene, &world);
    let terrain = crate::terrain::load_terrain_from_route_dir_near(
        &route_dir,
        Some(focus.center),
        VISIBLE_RADIUS_M,
    );
    let elevation = TerrainElevation::load_from_route_dir_near(
        &route_dir,
        Some(focus.center),
        VISIBLE_RADIUS_M,
    );
    let offset = RouteWorldOffset::from_scene_and_world(&scene, &world);
    let assets = RouteAssets::new(&route_dir);

    let scenario_path = smoke_scenario_path();
    let mut consist = TrainConsistScene::default();
    if scenario_path.exists() {
        if let Ok(scenario) = load_scenario(&scenario_path) {
            let scenario_dir = scenario_path.parent().unwrap();
            consist.set_scenario_dir(scenario_dir.to_path_buf());
            if let Some(vehicles) = crate::rolling_stock::try_load_consist_vehicles(
                scenario_dir,
                &scenario.train.consist,
            ) {
                consist.by_label.insert("primary".into(), vehicles);
            }
        }
    }

    Some((
        scene, world, terrain, elevation, focus, offset, consist, assets,
    ))
}

pub fn insert_live_bundle(app: &mut App, live: LiveDrive) {
    let Some((scene, world, terrain, elevation, focus, offset, consist, assets)) =
        load_smoke_route_bundle()
    else {
        panic!("smoke route fixtures missing");
    };
    app.insert_resource(live);
    app.insert_resource(scene);
    app.insert_resource(world);
    app.insert_resource(terrain);
    app.insert_resource(elevation);
    app.insert_resource(focus);
    app.insert_resource(offset);
    app.insert_resource(consist);
    app.insert_resource(assets);
    app.insert_resource(ReplayState::default());
}

pub fn with_replay_world(scene: TrackScene, replay: ReplayState, f: impl FnOnce(&mut World)) {
    let mut app = minimal_app();
    insert_replay_bundle(&mut app, scene, replay);
    app.update();
    f(app.world_mut());
}

pub fn with_live_world(f: impl FnOnce(&mut World)) {
    let live = try_smoke_live_drive().expect("smoke scenario.toml + route");
    let mut app = minimal_app();
    insert_live_bundle(&mut app, live);
    app.update();
    f(app.world_mut());
}

pub fn count_named(world: &mut World, prefix: &str) -> usize {
    world
        .query::<&Name>()
        .iter(world)
        .filter(|name| name.as_str().starts_with(prefix))
        .count()
}

pub fn count_components<T: Component>(world: &mut World) -> usize {
    world.query::<&T>().iter(world).count()
}

/// Insert the same route resources `main` uses for a directorydiv route.
pub fn insert_route_dir_bundle(app: &mut App, route_dir: &Path) {
    let graph = load_track_graph_from_route_dir(route_dir).expect("track.toml");
    let scene = TrackScene::from_graph(graph);
    let world = crate::world::load_world_from_route_dir(route_dir);
    let focus = RouteFocus::from_scene_and_world(&scene, &world);
    let terrain = crate::terrain::load_terrain_from_route_dir_near(
        route_dir,
        Some(focus.center),
        VISIBLE_RADIUS_M,
    );
    let elevation =
        TerrainElevation::load_from_route_dir_near(route_dir, Some(focus.center), VISIBLE_RADIUS_M);
    let offset = RouteWorldOffset::from_scene_and_world(&scene, &world);
    app.insert_resource(scene);
    app.insert_resource(world);
    app.insert_resource(terrain);
    app.insert_resource(elevation);
    app.insert_resource(focus);
    app.insert_resource(offset);
    app.insert_resource(RouteAssets::new(route_dir));
}

pub fn with_route_dir_world(route_dir: &Path, f: impl FnOnce(&mut World)) {
    let mut app = minimal_app();
    insert_route_dir_bundle(&mut app, route_dir);
    app.insert_resource(ReplayState::default());
    app.insert_resource(TrainConsistScene::default());
    app.update();
    f(app.world_mut());
}
