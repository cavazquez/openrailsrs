//! Floating-origin system tests (function kept for regression; disabled at runtime).

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use crate::camera::spawn_camera;
    use crate::floating_origin::{
        FLOATING_ORIGIN_THRESHOLD_M, FloatingOrigin, apply_floating_origin,
    };
    use crate::test_harness::{tiny_graph_with_signal, with_replay_world};
    use crate::track::TrackScene;
    use crate::train::ReplayState;

    fn setup_camera_and_prop(world: &mut World, cam_pos: Vec3, prop_pos: Vec3) -> Entity {
        world.run_system_once(spawn_camera).unwrap();
        let cam_ent = world
            .query_filtered::<Entity, With<Camera3d>>()
            .single(world)
            .expect("camera entity");
        world
            .entity_mut(cam_ent)
            .insert(Transform::from_translation(cam_pos));
        world
            .spawn((Name::new("prop"), Transform::from_translation(prop_pos)))
            .id()
    }

    #[test]
    fn floating_origin_recentres_without_query_conflict() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        with_replay_world(scene, ReplayState::default(), |world| {
            setup_camera_and_prop(
                world,
                Vec3::new(500.0, 0.0, 0.0),
                Vec3::new(500.0, 10.0, 0.0),
            );
            world.run_system_once(apply_floating_origin).unwrap();
            let cam = world
                .query_filtered::<&Transform, With<Camera3d>>()
                .single(world)
                .expect("camera");
            assert!(cam.translation.length() < FLOATING_ORIGIN_THRESHOLD_M);
        });
    }

    #[test]
    fn floating_origin_shifts_camera_and_prop_together() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        with_replay_world(scene, ReplayState::default(), |world| {
            let cam_pos = Vec3::new(500.0, 2.0, -30.0);
            let prop_pos = Vec3::new(520.0, 10.0, -30.0);
            let before = prop_pos - cam_pos;
            let prop_ent = setup_camera_and_prop(world, cam_pos, prop_pos);
            world.run_system_once(apply_floating_origin).unwrap();
            let cam = world
                .query_filtered::<&Transform, With<Camera3d>>()
                .single(world)
                .expect("camera");
            let prop = world.get::<Transform>(prop_ent).expect("prop");
            let after = prop.translation - cam.translation;
            assert!(cam.translation.length() < FLOATING_ORIGIN_THRESHOLD_M);
            assert!((before - after).length() < 1e-3);
        });
    }

    #[test]
    fn floating_origin_accumulates_shift_resource() {
        let scene = TrackScene::from_graph(tiny_graph_with_signal());
        with_replay_world(scene, ReplayState::default(), |world| {
            setup_camera_and_prop(world, Vec3::new(400.0, 0.0, 0.0), Vec3::ZERO);
            world.run_system_once(apply_floating_origin).unwrap();
            assert!((world.resource::<FloatingOrigin>().shift.x - 400.0).abs() < 1e-3);
        });
    }

    #[test]
    fn route_focus_chiltern_like_render_y_in_entity_space() {
        use crate::world::RouteFocus;

        let focus = RouteFocus {
            center: Vec3::new(12_494_846.0, 82.0, 30_600_240.0),
            height_origin: 13_184.0,
        };
        let scenery = focus.to_render(Vec3::new(12_494_900.0, 85.0, 30_600_300.0));
        let surface = focus.to_render_surface(Vec3::new(12_494_900.0, 13_200.0, 30_600_300.0));
        assert!(scenery.y.abs() < 100.0);
        assert!(surface.y.abs() < 100.0);
    }
}
