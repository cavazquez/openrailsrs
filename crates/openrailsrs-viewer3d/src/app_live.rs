//! Live-drive system smoke tests.

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use crate::cab_render::{
        camera_layers_driver, camera_layers_outdoor, sync_camera_render_layers,
        tag_train_exterior_render_layers,
    };
    use bevy::camera::visibility::RenderLayers;

    use crate::camera::{CameraFollowMode, OrbitState, spawn_camera};
    use crate::launch::ViewerSceneryMode;
    use crate::live::{
        LiveTrainBody, LiveTrainCameraFrame, LiveTrainMarker, advance_live_sim,
        enable_live_defaults, live_driver_input, spawn_live_train, update_driver_train_visibility,
        update_live_train_marker,
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
    fn enable_live_defaults_starts_in_chase_at_train() {
        with_live_world(|world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world.run_system_once(enable_live_defaults).unwrap();
            assert_eq!(
                *world.resource::<CameraFollowMode>(),
                CameraFollowMode::ChaseCam
            );
            let orbit = *world.query::<&OrbitState>().single(world).expect("orbit");
            let frame = *world.resource::<LiveTrainCameraFrame>();
            assert!((orbit.distance - frame.chase_distance_m).abs() < 1e-3);
            let train = *world
                .query_filtered::<&Transform, With<LiveTrainMarker>>()
                .single(world)
                .expect("live train");
            let expected_focus = frame.focus_from_train(&train) + Vec3::Y * 2.0;
            assert!(
                orbit.focus.distance(expected_focus) < 1e-3,
                "focus should sit beyond the leading vehicle: {:?} vs {:?}",
                orbit.focus,
                expected_focus,
            );
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
    fn run_corridor_live_defaults_frame_the_consist() {
        with_live_world(|world| {
            world.insert_resource(ViewerSceneryMode::RunCorridor);
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world.run_system_once(enable_live_defaults).unwrap();
            assert_eq!(
                *world.resource::<CameraFollowMode>(),
                CameraFollowMode::ChaseCam
            );
            let orbit = world.query::<&OrbitState>().single(world).expect("orbit");
            let frame = *world.resource::<LiveTrainCameraFrame>();
            assert!((orbit.distance - frame.chase_distance_m).abs() < 1e-3);
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

    /// CAB-B regression (#168): DriverCam → ChaseCam restores exterior Visibility and
    /// outdoor camera layers `[0,1]` with every `LiveTrainBody` tagged on layer 1.
    #[test]
    fn driver_to_chase_restores_exterior_visibility_and_layers() {
        with_live_world(|world| {
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world
                .run_system_once(tag_train_exterior_render_layers)
                .unwrap();
            world.flush();

            let body_count = world
                .query_filtered::<Entity, With<LiveTrainBody>>()
                .iter(world)
                .count();
            assert!(body_count >= 1, "need LiveTrainBody entities");

            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::DriverCam;
            world
                .run_system_once(update_driver_train_visibility)
                .unwrap();
            world.run_system_once(sync_camera_render_layers).unwrap();

            let driver_layers = world
                .query_filtered::<&RenderLayers, With<Camera3d>>()
                .single(world)
                .expect("camera");
            assert_eq!(*driver_layers, camera_layers_driver());

            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::ChaseCam;
            world
                .run_system_once(update_driver_train_visibility)
                .unwrap();
            world.run_system_once(sync_camera_render_layers).unwrap();
            world
                .run_system_once(tag_train_exterior_render_layers)
                .unwrap();
            world.flush();

            let all_bodies_visible = world
                .query_filtered::<&Visibility, With<LiveTrainBody>>()
                .iter(world)
                .all(|v| *v == Visibility::Visible);
            assert!(
                all_bodies_visible,
                "all LiveTrainBody must be Visible in ChaseCam after DriverCam"
            );

            let outdoor = camera_layers_outdoor();
            let cam_layers = world
                .query_filtered::<&RenderLayers, With<Camera3d>>()
                .single(world)
                .expect("camera layers chase");
            assert_eq!(*cam_layers, outdoor, "chase camera must use layers [0,1]");
            assert!(
                outdoor.intersects(&RenderLayers::layer(1)),
                "outdoor mask must include train exterior layer 1"
            );

            let layer1 = RenderLayers::layer(1);
            let tagged_on_l1 = world
                .query_filtered::<&RenderLayers, With<LiveTrainBody>>()
                .iter(world)
                .filter(|layers| layers.intersects(&layer1))
                .count();
            assert_eq!(
                tagged_on_l1, body_count,
                "every LiveTrainBody must be tagged on layer 1 (got {tagged_on_l1}/{body_count})"
            );
        });
    }

    #[test]
    fn live_driver_input_throttle_brake_and_pause() {
        with_live_world(|world| {
            {
                let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::KeyD); // OR ControlThrottleIncrease
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
                keys.press(KeyCode::Backspace); // OR emergency
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

    #[test]
    fn sync_camera_render_layers_excludes_exterior_in_driver_cam() {
        with_live_world(|world| {
            world.run_system_once(spawn_camera).unwrap();
            world.run_system_once(spawn_live_train).unwrap();
            world
                .run_system_once(tag_train_exterior_render_layers)
                .unwrap();
            world.flush();

            let exterior = camera_layers_outdoor();
            let driver = camera_layers_driver();
            let train_layer = RenderLayers::layer(1);
            assert!(exterior.intersects(&train_layer));
            assert!(!driver.intersects(&train_layer));

            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::DriverCam;
            world.run_system_once(sync_camera_render_layers).unwrap();
            let cam_layers = world
                .query_filtered::<&RenderLayers, With<Camera3d>>()
                .single(world)
                .expect("camera layers");
            assert_eq!(*cam_layers, driver);

            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::ChaseCam;
            world.run_system_once(sync_camera_render_layers).unwrap();
            let cam_layers = world
                .query_filtered::<&RenderLayers, With<Camera3d>>()
                .single(world)
                .expect("camera layers chase");
            assert_eq!(*cam_layers, exterior);
        });
    }
}
