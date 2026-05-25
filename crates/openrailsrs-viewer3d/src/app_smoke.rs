//! Lightweight Bevy world smoke tests (`run_system_once`, no window/render loop).

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, SignalAspect, TrackGraph, TrackSignal};

    use crate::camera::{
        CameraFollowMode, CameraFollowTarget, CameraMode, OrbitDistanceLimit, OrbitState,
        cycle_follow_mode, follow_train_camera, spawn_camera,
    };
    use crate::rolling_stock::TrainConsistScene;
    use crate::shapes::RouteAssets;
    use crate::signals::spawn_signal_markers;
    use crate::terrain::TerrainElevation;
    use crate::track::{TrackScene, frame_orbit_camera_on_track, spawn_track_meshes};
    use crate::train::{
        CsvRow, ReplayState, TrainMarker, TrainTrack, spawn_train_markers, update_train_markers,
    };

    fn tiny_graph_with_signal() -> TrackGraph {
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

    fn sample_replay_track() -> TrainTrack {
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

    fn with_scene_replay(scene: TrackScene, replay: ReplayState, f: impl FnOnce(&mut World)) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_asset::<Image>();
        app.init_asset::<StandardMaterial>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<CameraMode>();
        app.init_resource::<CameraFollowMode>();
        app.init_resource::<CameraFollowTarget>();
        app.init_resource::<OrbitDistanceLimit>();
        app.insert_resource(TerrainElevation::default());
        app.insert_resource(scene);
        app.insert_resource(replay);
        app.insert_resource(TrainConsistScene::default());
        app.insert_resource(RouteAssets::new("examples/smoke/routes/test"));
        app.update();

        f(app.world_mut());
    }

    fn count_named(world: &mut World, prefix: &str) -> usize {
        world
            .query::<&Name>()
            .iter(world)
            .filter(|name| name.as_str().starts_with(prefix))
            .count()
    }

    #[test]
    fn spawn_systems_create_track_signal_and_train() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        let replay = ReplayState::new("test".into(), vec![sample_replay_track()]);

        with_scene_replay(scene, replay, |world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_signal_markers).unwrap();
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(frame_orbit_camera_on_track).unwrap();
            world.run_system_once(spawn_train_markers).unwrap();
            world.flush();

            assert_eq!(count_named(world, "edge:"), 1);
            assert_eq!(count_named(world, "node:"), 2);
            assert_eq!(count_named(world, "signal:"), 1);
            assert_eq!(count_named(world, "train:"), 1);
            assert!(world.query::<&OrbitState>().single(world).is_ok());
        });
    }

    #[test]
    fn cycle_follow_mode_and_follow_camera_run() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        let replay = ReplayState::new("test".into(), vec![sample_replay_track()]);

        with_scene_replay(scene, replay, |world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(frame_orbit_camera_on_track).unwrap();
            world.run_system_once(spawn_train_markers).unwrap();
            world.flush();

            {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::KeyT);
            }
            world.run_system_once(cycle_follow_mode).unwrap();
            assert_eq!(
                *world.resource::<CameraFollowMode>(),
                CameraFollowMode::OrbitFollow
            );

            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::OrbitFollow;
            world.resource_mut::<ReplayState>().paused = true;
            world.resource_mut::<ReplayState>().t_sim = 5.0;

            for mut orbit in world.query::<&mut OrbitState>().iter_mut(world) {
                orbit.focus = Vec3::ZERO;
                orbit.distance = 10.0;
            }

            world.run_system_once(update_train_markers).unwrap();
            for _ in 0..5 {
                world
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_millis(50));
                world.run_system_once(follow_train_camera).unwrap();
            }

            let orbit = world.query::<&OrbitState>().single(world).expect("orbit");
            assert!(orbit.distance >= 80.0);
            assert_eq!(world.query::<&TrainMarker>().iter(world).count(), 1);
        });
    }
}
