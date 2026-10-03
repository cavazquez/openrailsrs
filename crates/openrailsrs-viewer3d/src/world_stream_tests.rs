//! Exercise the actual activation, progressive spawn, origin and unload systems.
use super::*;

fn parsed_tile(tile_x: i32, source: &str) -> WorldFile {
    let ast = openrailsrs_formats::parser::parse_from_first_paren(source).unwrap();
    WorldFile::from_ast(&ast, tile_x, 0)
}

fn settle(app: &mut App) {
    for _ in 0..80 {
        app.update();
        if !app.world().contains_resource::<WorldSpawnProgress>() {
            app.update(); // Flush indices and the final activation scan.
            if !app.world().contains_resource::<WorldSpawnProgress>() {
                return;
            }
        }
    }
    panic!("stream cycle did not finish");
}

fn entities_on_tile(app: &mut App, tile_x: i32) -> Vec<(String, Transform)> {
    let world = app.world_mut();
    let mut query = world.query::<(&WorldTileBound, &Name, &Transform)>();
    query
        .iter(world)
        .filter(|(bound, _, _)| bound.tile_x == tile_x)
        .map(|(_, name, tf)| (name.as_str().to_string(), *tf))
        .collect()
}

#[test]
fn streaming_revisits_loaded_tiles_rebases_queued_meshes_and_releases_gpu_tiles() {
    let route = tempfile::tempdir().unwrap();
    std::fs::create_dir(route.path().join("SHAPES")).unwrap();
    std::fs::write(
        route.path().join("SHAPES/minimal.s"),
        include_bytes!("../../openrailsrs-formats/tests/fixtures/minimal.s"),
    )
    .unwrap();
    let near = parsed_tile(
        0,
        r#"(Tr_Worldfile
      (Static (UiD 1) (FileName "minimal.s") (Position 0 0 0) (QDirection 0 0 0 1)))"#,
    );
    let far = parsed_tile(
        2,
        r#"(Tr_Worldfile
      (Static (UiD 1) (FileName "minimal.s") (Position 0 0 0) (QDirection 0 0 0 1))
      (Forest (UiD 2) (Area 20 20) (TreeSize 4 8) (Population 3) (Position 0 0 0))
      (Transfer (UiD 3) (Width 20) (Height 20) (Position 0 0 0) (QDirection 0 0 0 1))
      (Dyntrack (UiD 4) (Position 0 0 0) (QDirection 0 0 0 1))
      (HWater (UiD 5) (Size 20 20) (Position 0 0 0)))"#,
    );
    let mut scene = WorldScene::default();
    append_world_tile_with_density(&mut scene, &near, None, 99);
    append_world_tile_with_density(&mut scene, &far, None, 99);
    assert_eq!(
        scene.items.len(),
        6,
        "all CPU objects must survive prefetch"
    );
    let stream = WorldTileStream::new(route.path(), &scene, visible_radius_m());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<openrailsrs_bevy_scenery::OrForestMaterial>>()
        .init_resource::<WorldShapeLodCache>()
        .init_resource::<WorldSceneryStreamState>()
        .init_resource::<crate::world_tile_index::WorldTileEntityIndex>()
        .init_resource::<crate::world_tile_index::WorldShapeLiveRefs>()
        .init_resource::<crate::tile_bundle::TileBundleHandles>()
        .init_resource::<openrailsrs_bevy_scenery::ScenerySpawnCycle>()
        .init_resource::<crate::overhead_wire::RouteWireConfig>()
        .init_resource::<FloatingOrigin>()
        .insert_resource(ViewerSceneryMode::Full)
        .insert_resource(crate::launch::ViewerLaunchOpts {
            live: true,
            ..default()
        })
        .insert_resource(CameraFollowMode::Off)
        .insert_resource(RouteWorldOffset::default())
        .insert_resource(RouteFocus::at_world_center(Vec3::ZERO, None))
        .insert_resource(crate::view_window::ViewWindow::default())
        .insert_resource(TrackScene::from_graph(openrailsrs_track::TrackGraph::new()))
        .insert_resource(RouteAssets::new(route.path()))
        .insert_resource(scene)
        .insert_resource(stream)
        .add_systems(
            Update,
            (
                crate::floating_origin::apply_floating_origin,
                progressive_world_spawn_system,
                crate::world_tile_index::index_world_tile_bound_added,
                crate::world_tile_index::track_world_shape_live_refs_added,
                crate::world_tile_index::index_world_tile_bound_removed,
                crate::world_tile_index::track_world_shape_live_refs_removed,
                world_tile_unload_system,
                world_stream_scenery_system,
            )
                .chain(),
        );
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Transform::IDENTITY,
            crate::camera::OrbitState::default(),
        ))
        .id();
    settle(&mut app);
    assert_eq!(entities_on_tile(&mut app, 0).len(), 1);
    assert!(
        entities_on_tile(&mut app, 2).is_empty(),
        "prefetch must not build distant GPU objects"
    );

    app.world_mut()
        .resource_mut::<crate::view_window::ViewWindow>()
        .center_world
        .x = 3700.0;
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .x = 3700.0;
    // Stop just after building the queue, then force another origin shift before
    // submission. This used to displace newly streamed meshes by one rebase.
    for _ in 0..40 {
        app.update();
        if app
            .world()
            .get_resource::<WorldSpawnProgress>()
            .is_some_and(|p| p.phase == WorldSpawnPhase::SpawningEntities)
        {
            break;
        }
    }
    assert_eq!(
        app.world().resource::<WorldSpawnProgress>().phase,
        WorldSpawnPhase::SpawningEntities
    );
    app.world_mut()
        .resource_mut::<crate::view_window::ViewWindow>()
        .center_world
        .x = 4010.0;
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .x = 310.0;
    settle(&mut app);
    assert_eq!(app.world().resource::<FloatingOrigin>().shift.x, 4010.0);
    let far_entities = entities_on_tile(&mut app, 2);
    assert_eq!(
        far_entities.len(),
        7,
        "shape, forest, decal, rails, sleepers, water + reflection"
    );
    let shape = far_entities
        .iter()
        .find(|(name, _)| name.starts_with("world:"))
        .unwrap();
    assert!((shape.1.translation.x - 86.0).abs() < 0.001);
    let transfer = far_entities
        .iter()
        .find(|(name, _)| name.starts_with("transfer:"))
        .unwrap();
    assert!((transfer.1.translation.x - 86.0).abs() < 0.001);
    let forest = far_entities
        .iter()
        .find(|(name, _)| name.starts_with("forest:"))
        .unwrap();
    assert_eq!(
        forest.1.translation.x, -4010.0,
        "forest mesh vertices are focus-relative"
    );
    assert!(
        entities_on_tile(&mut app, 0).is_empty(),
        "old GPU tile must be released"
    );
    assert_eq!(
        app.world().resource::<WorldScene>().loaded_tiles.len(),
        2,
        "CPU prefetch stays available"
    );
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(
        entities_on_tile(&mut app, 2).len(),
        far_entities.len(),
        "no duplicate reactivation"
    );

    // Return without reparsing the WORLD tiles. Compaction/order changes cannot
    // change identities or block reactivation after GPU eviction.
    app.world_mut().resource_mut::<WorldScene>().items.reverse();
    app.world_mut()
        .resource_mut::<crate::view_window::ViewWindow>()
        .center_world = Vec3::ZERO;
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .x = -4010.0;
    settle(&mut app);
    assert_eq!(app.world().resource::<FloatingOrigin>().shift, Vec3::ZERO);
    assert_eq!(entities_on_tile(&mut app, 0).len(), 1);
    assert!(entities_on_tile(&mut app, 2).is_empty());
}

#[test]
fn world_source_identity_survives_density_filter_and_duplicate_native_uids() {
    let world = parsed_tile(
        0,
        r#"(Tr_Worldfile
      (Tr_Watermark 60)
      (Static (UiD 0) (FileName "a.s") (Position 0 0 0) (QDirection 0 0 0 1))
      (Tr_Watermark 0)
      (Static (UiD 0) (FileName "b.s") (Position 0 0 0) (QDirection 0 0 0 1)))"#,
    );
    let mut all = WorldScene::default();
    let mut filtered = WorldScene::default();
    append_world_tile_with_density(&mut all, &world, None, 99);
    append_world_tile_with_density(&mut filtered, &world, None, 49);
    assert_eq!(all.items.len(), 2);
    assert_eq!(filtered.items.len(), 1);
    assert_ne!(all.items[0].key(), all.items[1].key());
    assert_eq!(all.items[1].key(), filtered.items[0].key());
}
