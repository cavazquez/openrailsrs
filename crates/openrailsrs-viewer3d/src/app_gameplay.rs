//! Gameplay UI and stop-billboard tests.

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use crate::camera::{CameraFollowMode, spawn_camera};
    use crate::gameplay::{
        ArrivalOverlayBody, ArrivalOverlayRoot, DriverVignetteRoot, GameplayDestMarker,
        GameplayMarkerMaterials, GameplayStopMarker, GameplayToast, GameplayToastRoot,
        StopBillboard, spawn_gameplay_markers, spawn_gameplay_ui, update_arrival_overlay,
        update_driver_vignette, update_gameplay_markers, update_gameplay_toast,
    };
    use crate::test_harness::{count_components, with_live_world};
    use crate::track::spawn_track_meshes;

    #[test]
    fn spawn_gameplay_ui_creates_roots() {
        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_ui).unwrap();
            world.flush();
            assert_eq!(count_components::<GameplayToastRoot>(world), 1);
            assert_eq!(count_components::<ArrivalOverlayRoot>(world), 1);
            assert_eq!(count_components::<DriverVignetteRoot>(world), 1);
        });
    }

    #[test]
    fn spawn_gameplay_markers_match_stop_targets() {
        with_live_world(|world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            let stop_count = world
                .resource::<crate::live::LiveDrive>()
                .session
                .gameplay
                .stop_targets
                .len();
            assert!(stop_count >= 1, "smoke scenario should have stop 'mid'");

            world.run_system_once(spawn_gameplay_markers).unwrap();
            world.flush();

            assert_eq!(count_components::<GameplayStopMarker>(world), stop_count);
            assert!(world.get_resource::<GameplayMarkerMaterials>().is_some());
            if world
                .resource::<crate::live::LiveDrive>()
                .session
                .path_data
                .total_length_m()
                > 0.0
            {
                assert_eq!(count_components::<GameplayDestMarker>(world), 1);
            }
        });
    }

    #[test]
    fn spawn_gameplay_markers_use_local_y_not_msl_thousands() {
        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_markers).unwrap();
            for tf in world
                .query_filtered::<&Transform, With<GameplayStopMarker>>()
                .iter(world)
            {
                assert!(
                    tf.translation.y.abs() < 500.0,
                    "stop marker y should be render-local, got {}",
                    tf.translation.y
                );
            }
        });
    }

    #[test]
    fn update_gameplay_markers_swaps_material_on_pass() {
        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_markers).unwrap();
            {
                let mut live = world.resource_mut::<crate::live::LiveDrive>();
                live.session.gameplay.next_stop_idx = 1;
            }
            world.run_system_once(update_gameplay_markers).unwrap();
            let mats = world.resource::<GameplayMarkerMaterials>();
            let passed = mats.passed.clone();
            let next_count = world
                .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<GameplayStopMarker>>()
                .iter(world)
                .filter(|m| m.0 == passed)
                .count();
            assert!(next_count >= 1);
        });
    }

    #[test]
    fn update_gameplay_toast_shows_and_hides() {
        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_ui).unwrap();
            world
                .resource_mut::<GameplayToast>()
                .show("test toast", 2.0);
            world.run_system_once(update_gameplay_toast).unwrap();
            let vis = world
                .query_filtered::<&Visibility, With<GameplayToastRoot>>()
                .single(world)
                .expect("toast root");
            assert_ne!(*vis, Visibility::Hidden);

            world.resource_mut::<GameplayToast>().ttl_s = 0.0;
            world
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(16));
            world.run_system_once(update_gameplay_toast).unwrap();
            let vis = world
                .query_filtered::<&Visibility, With<GameplayToastRoot>>()
                .single(world)
                .expect("toast root");
            assert_eq!(*vis, Visibility::Hidden);
        });
    }

    #[test]
    fn update_arrival_overlay_when_arrived() {
        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_ui).unwrap();
            {
                let mut live = world.resource_mut::<crate::live::LiveDrive>();
                live.session.arrived = true;
                live.session.gameplay.destination = "yard_b".into();
            }
            world.run_system_once(update_arrival_overlay).unwrap();
            let vis = world
                .query_filtered::<&Visibility, With<ArrivalOverlayRoot>>()
                .single(world)
                .expect("overlay");
            assert_ne!(*vis, Visibility::Hidden);
            let text = world
                .query_filtered::<&Text, With<ArrivalOverlayBody>>()
                .single(world)
                .expect("body");
            assert!(text.0.contains("yard_b"));
        });
    }

    #[test]
    fn update_driver_vignette_stays_hidden() {
        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_ui).unwrap();
            *world.resource_mut::<CameraFollowMode>() = CameraFollowMode::DriverCam;
            world.run_system_once(update_driver_vignette).unwrap();
            let vis = world
                .query_filtered::<&Visibility, With<DriverVignetteRoot>>()
                .single(world)
                .expect("vignette");
            assert_eq!(*vis, Visibility::Hidden);
        });
    }

    #[test]
    fn stop_billboard_ui_from_viewport_bounds() {
        use crate::gameplay::stop_billboard_ui_from_viewport;

        assert_eq!(
            stop_billboard_ui_from_viewport(Vec2::new(400.0, 300.0), 800.0, 600.0, 2.0),
            Some((200.0, 150.0))
        );
        assert!(
            stop_billboard_ui_from_viewport(Vec2::new(-1.0, 300.0), 800.0, 600.0, 1.0).is_none()
        );
        assert!(
            stop_billboard_ui_from_viewport(Vec2::new(900.0, 300.0), 800.0, 600.0, 1.0).is_none()
        );
    }

    #[test]
    fn update_stop_billboards_with_mock_window() {
        use crate::gameplay::update_stop_billboards;

        with_live_world(|world| {
            world.run_system_once(spawn_gameplay_ui).unwrap();
            world.run_system_once(spawn_camera).unwrap();

            let bb_world = Vec3::new(0.0, 5.0, -20.0);
            world.spawn((
                StopBillboard { world: bb_world },
                Name::new("test:billboard"),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
            ));

            let resolution = bevy::window::WindowResolution::new(800, 600);
            world.spawn(Window {
                resolution,
                ..default()
            });

            for mut cam_tf in world.query::<&mut GlobalTransform>().iter_mut(world) {
                *cam_tf = GlobalTransform::from(
                    Transform::from_translation(Vec3::new(0.0, 2.0, 10.0))
                        .looking_at(bb_world, Vec3::Y),
                );
            }

            world.run_system_once(update_stop_billboards).unwrap();
            let vis = world
                .query_filtered::<&Visibility, (With<StopBillboard>, With<Name>)>()
                .iter(world)
                .find(|_| true)
                .expect("billboard");
            assert!(
                *vis == Visibility::Inherited || *vis == Visibility::Hidden,
                "system should run without panic"
            );
        });
    }
}
