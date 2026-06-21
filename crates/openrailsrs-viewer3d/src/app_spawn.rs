//! Startup spawn smoke tests (smoke route fixtures).

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::*;

    use crate::dyntrack::spawn_dyntrack_segments;
    use crate::forest::spawn_forest_patches;
    use crate::scene::spawn_ground_and_lights;
    use crate::sky::spawn_sky_dome;
    use crate::terrain::spawn_terrain_meshes;
    use crate::test_harness::{count_named, smoke_route_dir, with_route_dir_world};
    use crate::track::TrackScene;
    use crate::track::spawn_track_meshes;
    use crate::water::spawn_water_patches;
    use crate::world::RouteFocus;
    use crate::world::spawn_world_boxes;

    #[test]
    fn spawn_track_meshes_smoke_route() {
        let route_dir = smoke_route_dir();
        if !route_dir.join("track.toml").exists() {
            return;
        }
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_track_meshes).unwrap();
            world.flush();
            assert!(count_named(world, "logical-track:") >= 1);
            assert!(count_named(world, "node:") >= 2);
        });
    }

    #[test]
    fn spawn_terrain_meshes_smoke_route() {
        let route_dir = smoke_route_dir();
        if !route_dir.join("TERRAIN").exists() {
            return;
        }
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_terrain_meshes).unwrap();
            world.flush();
            assert!(
                world.query::<&Mesh3d>().iter(world).count() >= 1
                    || count_named(world, "terrain:") >= 1,
                "expected terrain mesh entities"
            );
        });
    }

    #[test]
    fn spawn_world_boxes_smoke_route() {
        let route_dir = smoke_route_dir();
        if !route_dir.join("WORLD").exists() && !route_dir.join("world").exists() {
            return;
        }
        with_route_dir_world(&route_dir, |world| {
            // Smoke `.w` mixes yard-local Static (120 m) with far TrackObj/Signal (5–7 km).
            // Default focus uses the world bbox centre and culls the yard fixture.
            let yard_static = world
                .resource::<crate::world::WorldScene>()
                .items
                .iter()
                .find(|o| o.kind == "Static")
                .map(|o| o.position)
                .unwrap_or_else(|| world.resource::<TrackScene>().bounds.center);
            world.insert_resource(RouteFocus::at_world_center(yard_static, None));
            world.run_system_once(spawn_world_boxes).unwrap();
            world.flush();
            let named = count_named(world, "world:")
                + count_named(world, "world:mesh")
                + count_named(world, "world:merged");
            let merged = count_named(world, "world-boxes:");
            assert!(named + merged >= 1, "expected world placeholders or meshes");
        });
    }

    #[test]
    fn spawn_forest_patches_smoke_route() {
        let route_dir = smoke_route_dir();
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_forest_patches).unwrap();
            world.flush();
            let _trees = count_named(world, "forest:");
        });
    }

    #[test]
    fn spawn_water_patches_smoke_route() {
        let route_dir = smoke_route_dir();
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_water_patches).unwrap();
            world.flush();
        });
    }

    #[test]
    fn spawn_dyntrack_segments_smoke_route() {
        let route_dir = smoke_route_dir();
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_dyntrack_segments).unwrap();
            world.flush();
        });
    }

    #[test]
    fn spawn_ground_and_sky() {
        let route_dir = smoke_route_dir();
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_ground_and_lights).unwrap();
            world.run_system_once(spawn_sky_dome).unwrap();
            world.flush();
            assert!(world.query::<&DirectionalLight>().iter(world).count() >= 1);
            assert!(
                count_named(world, "sky:") >= 1
                    || world.query::<&Mesh3d>().iter(world).count() >= 1
            );
        });
    }

    #[test]
    fn viewer_startup_chain_smoke_route() {
        let route_dir = smoke_route_dir();
        if !route_dir.join("track.toml").exists() {
            return;
        }
        with_route_dir_world(&route_dir, |world| {
            world.run_system_once(spawn_ground_and_lights).unwrap();
            world.run_system_once(spawn_sky_dome).unwrap();
            world.run_system_once(spawn_terrain_meshes).unwrap();
            world.run_system_once(spawn_track_meshes).unwrap();
            world.run_system_once(spawn_dyntrack_segments).unwrap();
            world.run_system_once(spawn_forest_patches).unwrap();
            world.run_system_once(spawn_water_patches).unwrap();
            world.run_system_once(spawn_world_boxes).unwrap();
            world.flush();
            assert!(count_named(world, "logical-track:") >= 1);
        });
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT and examples/chiltern route assets"]
    fn chiltern_live_startup_no_panic() {
        let path = crate::test_harness::chiltern_scenario_path();
        if !path.exists() {
            return;
        }
        if std::env::var("OPENRAILSRS_MSTS_CONTENT").is_err() {
            return;
        }
        let live = crate::live::LiveDrive::from_scenario_path(&path).expect("chiltern live");
        let scenario_dir = path.parent().unwrap();
        let route_dir = scenario_dir.join("routes/chiltern");
        if !route_dir.exists() {
            let route_dir = scenario_dir.join(
                openrailsrs_scenarios::load_scenario(&path)
                    .unwrap()
                    .route
                    .path,
            );
            let mut app = crate::test_harness::minimal_app();
            crate::test_harness::insert_route_dir_bundle(&mut app, &route_dir);
            app.insert_resource(live);
            app.update();
            app.world_mut().run_system_once(spawn_track_meshes).unwrap();
            app.world_mut()
                .run_system_once(spawn_terrain_meshes)
                .unwrap();
            return;
        }
        let mut app = crate::test_harness::minimal_app();
        crate::test_harness::insert_route_dir_bundle(&mut app, &route_dir);
        app.insert_resource(live);
        app.update();
        app.world_mut().run_system_once(spawn_track_meshes).unwrap();
        app.world_mut()
            .run_system_once(spawn_terrain_meshes)
            .unwrap();
    }
}
