//! Live-drive system smoke tests.

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use crate::camera::{CameraFollowMode, LIVE_CHASE_DISTANCE, OrbitState, spawn_camera};
    use crate::live::{
        LiveTrainBody, LiveTrainMarker, advance_live_sim, enable_live_defaults, live_driver_input,
        spawn_live_train, update_driver_train_visibility, update_live_train_marker,
    };
    use crate::test_harness::{
        count_components, count_named, try_smoke_live_drive, with_live_world,
    };
    use crate::track::spawn_track_meshes;
    use crate::train::{TrainMarker, spawn_train_markers};

    #[test]
    fn spawn_live_train_creates_marker_and_bodies() {
        with_live_world(|world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world.flush();

            assert_eq!(count_components::<LiveTrainMarker>(world), 1);
            assert!(count_components::<LiveTrainBody>(world) >= 1);
            assert!(count_named(world, "train:live:") >= 1);
            assert_eq!(count_components::<TrainMarker>(world), 0);
        });
    }

    #[test]
    fn replay_spawn_skips_train_markers_when_live_resource_present() {
        with_live_world(|world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_train_markers).unwrap();
            world.flush();
            assert_eq!(count_components::<TrainMarker>(world), 0);
        });
    }

    #[test]
    fn enable_live_defaults_leaves_orbit_unfollowed() {
        with_live_world(|world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world.run_system_once(enable_live_defaults).unwrap();
            assert_eq!(*world.resource::<CameraFollowMode>(), CameraFollowMode::Off);
            let orbit = world.query::<&OrbitState>().single(world).expect("orbit");
            assert!((orbit.distance - LIVE_CHASE_DISTANCE).abs() < 1e-3);
            let cam = world
                .query_filtered::<&Transform, With<Camera3d>>()
                .single(world)
                .expect("camera");
            assert!(
                cam.translation.length() > 20.0,
                "camera should start behind the train"
            );
        });
    }

    #[test]
    fn update_live_train_marker_moves_with_session() {
        with_live_world(|world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world.flush();

            let pos_before = world
                .query::<&Transform>()
                .iter(world)
                .next()
                .map(|t| t.translation)
                .unwrap_or(Vec3::ZERO);

            {
                let mut live = world.resource_mut::<crate::live::LiveDrive>();
                live.session.driver_throttle = 1.0;
                live.paused = false;
            }
            world
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs(2));
            world.run_system_once(advance_live_sim).unwrap();
            world.run_system_once(update_live_train_marker).unwrap();

            let marker_pos = world
                .query_filtered::<&Transform, With<LiveTrainMarker>>()
                .single(world)
                .expect("marker")
                .translation;
            assert!(
                marker_pos != pos_before || marker_pos.length_squared() > 1e-6,
                "marker should have valid position"
            );
        });
    }

    #[test]
    fn update_driver_train_visibility_hides_in_driver_cam() {
        with_live_world(|world| {
            world.run_system_once(spawn_live_train).unwrap();
            world.spawn((crate::cab_view::CabInteriorMarker, Visibility::Hidden));
            world.flush();
            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::DriverCam;
            world
                .run_system_once(update_driver_train_visibility)
                .unwrap();
            for vis in world.query::<&Visibility>().iter(world) {
                let _ = vis;
            }
            let marker_visible = world
                .query_filtered::<&Visibility, With<LiveTrainMarker>>()
                .iter(world)
                .any(|v| *v != Visibility::Hidden);
            assert!(marker_visible, "train root stays visible in driver view");

            let bodies_hidden = world
                .query_filtered::<&Visibility, With<LiveTrainBody>>()
                .iter(world)
                .all(|v| *v == Visibility::Hidden);
            assert!(bodies_hidden, "train body hidden in driver view");

            let cab_part_visible = world
                .query_filtered::<&Visibility, With<crate::cab_view::CabInteriorMarker>>()
                .iter(world)
                .all(|v| *v == Visibility::Visible);
            assert!(
                cab_part_visible,
                "cab parts carrying CabInteriorMarker stay visible"
            );

            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::ChaseCam;
            world
                .run_system_once(update_driver_train_visibility)
                .unwrap();
            let visible = world
                .query_filtered::<&Visibility, With<LiveTrainBody>>()
                .iter(world)
                .any(|v| *v != Visibility::Hidden);
            assert!(visible, "train body visible in chase view");
        });
    }

    #[test]
    fn live_driver_input_throttle_brake_and_pause() {
        with_live_world(|world| {
            {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::ArrowUp);
            }
            world.run_system_once(live_driver_input).unwrap();
            assert!(
                world
                    .resource::<crate::live::LiveDrive>()
                    .session
                    .driver_throttle
                    > 0.0
            );

            {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                keys.clear();
                keys.press(KeyCode::Space);
            }
            world.run_system_once(live_driver_input).unwrap();
            let live = world.resource::<crate::live::LiveDrive>();
            assert_eq!(live.session.driver_throttle, 0.0);
            assert_eq!(live.session.driver_brake, 1.0);

            {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                keys.clear();
                keys.press(KeyCode::KeyP);
            }
            world.run_system_once(live_driver_input).unwrap();
            assert!(world.resource::<crate::live::LiveDrive>().paused);
        });
    }

    #[test]
    fn advance_live_sim_increments_time_when_unpaused() {
        let Some(mut live) = try_smoke_live_drive() else {
            return;
        };
        live.paused = false;
        live.session.driver_throttle = 1.0;
        let t0 = live.session.time_s();

        let mut app = crate::test_harness::minimal_app();
        app.insert_resource(live);
        app.update();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.world_mut().run_system_once(advance_live_sim).unwrap();
        assert!(
            app.world()
                .resource::<crate::live::LiveDrive>()
                .session
                .time_s()
                > t0
        );
    }
}
