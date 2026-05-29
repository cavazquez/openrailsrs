//! Replay-mode smoke tests (track, train, camera, precipitation).

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use crate::camera::{
        CameraFollowMode, OrbitState, cycle_follow_mode, follow_train_camera, spawn_camera,
    };
    use crate::precipitation::{
        PrecipitationState, spawn_precipitation, toggle_precipitation, update_precipitation,
    };
    use crate::signals::spawn_signal_markers;
    use crate::teleport::TeleportDialog;
    use crate::test_harness::{
        count_named, sample_replay_track, tiny_graph_with_signal, with_replay_world,
    };
    use crate::track::{TrackScene, frame_orbit_camera_on_track, spawn_track_meshes};
    use crate::train::{ReplayState, TrainMarker, spawn_train_markers, update_train_markers};

    #[test]
    fn spawn_systems_create_track_signal_and_train() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        let replay = ReplayState::new("test".into(), vec![sample_replay_track()]);

        with_replay_world(scene, replay, |world| {
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

        with_replay_world(scene, replay, |world| {
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
            assert!(orbit.distance >= crate::camera::FOLLOW_MIN_DISTANCE);
            assert_eq!(world.query::<&TrainMarker>().iter(world).count(), 1);
        });
    }

    #[test]
    fn precipitation_toggle_and_update_run_without_query_conflict() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        let replay = ReplayState::default();

        with_replay_world(scene, replay, |world| {
            world.insert_resource(PrecipitationState {
                enabled: true,
                ..Default::default()
            });
            world.insert_resource(TeleportDialog::default());
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_precipitation).unwrap();
            world.run_system_once(update_precipitation).unwrap();
            world.run_system_once(toggle_precipitation).unwrap();

            {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::KeyP);
            }
            world.run_system_once(toggle_precipitation).unwrap();
            assert!(
                !world.resource::<PrecipitationState>().enabled,
                "P should turn rain off in replay (precipitation toggle)"
            );
            world.run_system_once(update_precipitation).unwrap();
        });
    }
}
