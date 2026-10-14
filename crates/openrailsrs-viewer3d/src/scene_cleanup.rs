//! Retire the previous scene before an in-process route or cab transfer.
//! Loading is an explicit ownership boundary, not a resource replacement only.
use bevy::{asset::UntypedAssetId, prelude::*};
use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    time::Instant,
};

#[derive(Resource, Default)]
pub struct SceneCleanup {
    initialized: bool,
    scene_started: Option<Instant>,
    global_entities: HashSet<Entity>,
    global_assets: HashMap<TypeId, HashSet<UntypedAssetId>>,
    pub cycles: u64,
    pub retired_entities: u64,
    pub retired_assets: u64,
}
impl SceneCleanup {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"cycles":self.cycles,"retired_entities":self.retired_entities,"retired_assets":self.retired_assets,
            "scene_elapsed_s":self.scene_started.map(|started| started.elapsed().as_secs_f64())})
    }
}
pub fn install(app: &mut App) {
    app.init_resource::<SceneCleanup>().add_systems(
        OnEnter(crate::ViewerAppState::Loading),
        begin_loading.before(crate::route_bootstrap::setup_viewer_loading_ui),
    );
}
fn remember<T: Asset>(world: &World, cleanup: &mut SceneCleanup) {
    if let Some(assets) = world.get_resource::<Assets<T>>() {
        cleanup.global_assets.insert(
            TypeId::of::<T>(),
            assets
                .ids()
                .filter(|id| matches!(id, AssetId::Uuid { .. }))
                .map(AssetId::untyped)
                .collect(),
        );
    }
}
fn retire<T: Asset>(
    world: &mut World,
    cleanup: &mut SceneCleanup,
    extra: &HashSet<UntypedAssetId>,
) {
    let Some(mut assets) = world.get_resource_mut::<Assets<T>>() else {
        return;
    };
    let global = cleanup.global_assets.get(&TypeId::of::<T>());
    let ids = assets
        .ids()
        .filter(|id| {
            !global.is_some_and(|g| g.contains(&id.untyped())) && !extra.contains(&id.untyped())
        })
        .collect::<Vec<_>>();
    for id in ids {
        if assets.remove(id).is_some() {
            cleanup.retired_assets += 1;
        }
    }
}
fn reset<T: Resource + Default>(world: &mut World) {
    if world.contains_resource::<T>() {
        world.insert_resource(T::default());
    }
}
macro_rules! asset_types {
    ($action:ident, $world:expr, $cleanup:expr, $extra:expr) => {{
        $action::<Mesh>($world, $cleanup, $extra);
        $action::<Image>($world, $cleanup, $extra);
        $action::<StandardMaterial>($world, $cleanup, $extra);
        $action::<openrailsrs_bevy_scenery::materials::terrain::TerrainMaterial>(
            $world, $cleanup, $extra,
        );
        $action::<openrailsrs_bevy_scenery::materials::or_terrain::OrTerrainMaterial>(
            $world, $cleanup, $extra,
        );
        $action::<openrailsrs_bevy_scenery::materials::cab::OrCabMaterial>(
            $world, $cleanup, $extra,
        );
        $action::<openrailsrs_bevy_scenery::materials::scenery::OrSceneryMaterial>(
            $world, $cleanup, $extra,
        );
        $action::<openrailsrs_bevy_scenery::materials::forest::OrForestMaterial>(
            $world, $cleanup, $extra,
        );
        $action::<crate::weather_particles::ParticleMaterial>($world, $cleanup, $extra);
        $action::<crate::surface_weather::SnowMaterial>($world, $cleanup, $extra);
        $action::<crate::sky::RailwaySkyMaterial>($world, $cleanup, $extra);
        $action::<crate::moon::MoonMaterial>($world, $cleanup, $extra);
        $action::<bevy_hanabi::EffectAsset>($world, $cleanup, $extra);
    }};
}
fn remember_with_extra<T: Asset>(
    world: &mut World,
    cleanup: &mut SceneCleanup,
    _: &HashSet<UntypedAssetId>,
) {
    remember::<T>(world, cleanup);
}
pub fn begin_loading(world: &mut World) {
    let mut cleanup = world.remove_resource::<SceneCleanup>().unwrap_or_default();
    // Include loading work in the scene's real age. Presented-frame time only
    // advances in Playing and cannot identify equivalent reload checkpoints.
    cleanup.scene_started = Some(Instant::now());
    if !cleanup.initialized {
        cleanup.global_entities = world
            .query::<Entity>()
            .iter(world)
            .filter(|e| {
                world.get::<Camera3d>(*e).is_none()
                    && world.get::<Mesh3d>(*e).is_none()
                    && world.get::<Node>(*e).is_none()
                    && world.get::<Text>(*e).is_none()
                    && world.get::<Transform>(*e).is_none()
                    || world.get::<Window>(*e).is_some()
                    || world.get::<Camera2d>(*e).is_some()
                    || world.get::<Gamepad>(*e).is_some()
            })
            .collect();
        asset_types!(remember_with_extra, world, &mut cleanup, &HashSet::new());
        cleanup.initialized = true;
    }
    let retired = world
        .query::<Entity>()
        .iter(world)
        .filter(|e| {
            !cleanup.global_entities.contains(e)
                && world.get::<bevy::ecs::resource::IsResource>(*e).is_none()
                && world.get::<Window>(*e).is_none()
                && world.get::<Gamepad>(*e).is_none()
                && world.get::<Camera2d>(*e).is_none()
        })
        .collect::<Vec<_>>();
    let before = world.entities().count_spawned();
    for entity in retired {
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
    }
    cleanup.retired_entities += u64::from(before - world.entities().count_spawned());
    #[cfg(feature = "dev-inspector")]
    crate::dev_inspector::retire_dead_context_textures(world);
    // Bevy retains glyph textures across routes; keeping those images preserves
    // existing font atlas locations and accented HUD text after a transfer.
    let mut fonts = world
        .get_resource::<bevy::text::FontAtlasSet>()
        .map(|set| {
            set.values()
                .flatten()
                .map(|atlas| atlas.texture.id().untyped())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    if let Some(luts) = world.get_resource::<bevy::core_pipeline::tonemapping::TonemappingLuts>() {
        fonts.extend([
            luts.blender_filmic.id().untyped(),
            luts.agx.id().untyped(),
            luts.tony_mc_mapface.id().untyped(),
        ]);
    }
    // The inspector GC above removed only destroyed contexts. A global live
    // context may still need its Main image for a partial font-atlas update.
    #[cfg(feature = "dev-inspector")]
    if let Some(textures) =
        world.get_resource::<bevy_inspector_egui::bevy_egui::EguiManagedTextures>()
    {
        fonts.extend(
            textures
                .values()
                .map(|texture| texture.handle.id().untyped()),
        );
    }
    if let Some(mesh) = world.get_resource::<bevy_hanabi::DefaultMesh>() {
        fonts.insert(mesh.0.id().untyped());
    }
    fonts.extend(crate::particle_textures::preserved_ids().map(AssetId::untyped));
    fonts.insert(crate::ground_fog::density_image_id().untyped());
    asset_types!(retire, world, &mut cleanup, &fonts);

    reset::<crate::world::WorldShapeLodCache>(world);
    reset::<crate::world::WorldSceneryStreamState>(world);
    reset::<crate::world::WorldLodCameraState>(world);
    reset::<crate::world_tile_index::WorldTileEntityIndex>(world);
    reset::<crate::world_tile_index::WorldShapeLiveRefs>(world);
    reset::<crate::tile_bundle::TileBundleHandles>(world);
    reset::<crate::tdb_track::TdbTrackStream>(world);
    reset::<crate::traffic::TrainRenderCache>(world);
    reset::<crate::scenery_lod::Silhouettes>(world);
    reset::<crate::track_position::TrackPositionResolverCache>(world);
    reset::<crate::weather_particles::WeatherParticles>(world);
    reset::<crate::train_effects::TrainEffects>(world);
    reset::<crate::destination_boards::Boards>(world);
    reset::<crate::precipitation::PrecipitationState>(world);
    reset::<crate::cab_view::CabInteriorState>(world);
    reset::<crate::cab_cvf::CabCvfState>(world);
    reset::<crate::cab_cvf_overlay::CabCvfOverlayState>(world);
    reset::<crate::etcs::EtcsUiState>(world);
    reset::<crate::camera::LiveDriverCab>(world);
    reset::<crate::camera::PassengerCamState>(world);
    reset::<crate::camera::PassengerSeatCatalog>(world);
    reset::<crate::camera::CameraFollowTarget>(world);
    reset::<crate::floating_origin::FloatingOrigin>(world);
    reset::<crate::world_operations::RefillTargets>(world);
    reset::<crate::performance::JourneyPerformance>(world);
    reset::<crate::wet_surfaces::WetSurfaces>(world);
    reset::<crate::surface_weather::SnowMaterials>(world);
    reset::<crate::building_windows::WindowSchedules>(world);
    reset::<crate::station_lighting::StationLighting>(world);
    reset::<crate::effect_obstacles::EffectObstacles>(world);
    reset::<crate::streaming_forecast::StreamingForecast>(world);
    reset::<crate::camera_collision::CameraCollision>(world);
    reset::<crate::train_connections::JacobsJoints>(world);
    reset::<crate::train_connections::ConnectionDiagnostics>(world);
    world.remove_resource::<crate::gameplay::GameplayMarkerMaterials>();
    reset::<crate::gameplay::StopPassTracker>(world);
    world.remove_resource::<crate::world::WorldSpawnProgress>();
    world.remove_resource::<crate::terrain_spawn::TerrainSpawnProgress>();
    world.remove_resource::<crate::live::LiveDrive>();
    crate::player_ui::reset_scene_ui(world);
    cleanup.cycles += 1;
    crate::viewer_log!(
        "scene cleanup: {} route transitions, {} entities and {} assets retired",
        cleanup.cycles,
        cleanup.retired_entities,
        cleanup.retired_assets
    );
    world.insert_resource(cleanup);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scene_age_is_unavailable_before_loading_and_reports_a_real_loading_clock() {
        let mut world = World::new();
        world.init_resource::<SceneCleanup>();
        assert!(world.resource::<SceneCleanup>().report()["scene_elapsed_s"].is_null());
        begin_loading(&mut world);
        let report = world.resource::<SceneCleanup>().report();
        let age = report["scene_elapsed_s"].as_f64().unwrap();
        assert!(age.is_finite() && age >= 0.0);
        assert_eq!(report["cycles"], 1);
        assert!(world.resource::<SceneCleanup>().scene_started.is_some());
    }

    #[test]
    fn repeated_route_transfers_preserve_globals_and_release_old_scene_owners() {
        let mut world = World::new();
        world.init_resource::<SceneCleanup>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let window = world.spawn(Window::default()).id();
        let camera = world.spawn(Camera2d).id();
        let global = Handle::<Image>::default();
        world
            .resource_mut::<Assets<Image>>()
            .insert(global.id(), Image::default())
            .unwrap();
        let preview = world
            .spawn((
                Camera3d::default(),
                PointLight::default(),
                Transform::default(),
            ))
            .id();
        begin_loading(&mut world);
        assert!(
            world.get_entity(preview).is_err(),
            "The first menu-to-game transfer must retire its 3D preview too"
        );
        #[derive(Resource)]
        struct LateResource(u32);
        world.insert_resource(LateResource(42));
        for _ in 0..5 {
            let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
            let texture = world.resource_mut::<Assets<Image>>().add(Image::default());
            let material = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(StandardMaterial {
                    base_color_texture: Some(texture.clone()),
                    ..default()
                });
            let train = world.spawn((Mesh3d(mesh), MeshMaterial3d(material))).id();
            let child = world.spawn((Transform::default(), ChildOf(train))).id();
            begin_loading(&mut world);
            assert_eq!(
                world.resource::<LateResource>().0,
                42,
                "Bevy resource entities created after the baseline are global owners, not scenery"
            );
            assert!(world.get_entity(train).is_err());
            assert!(world.get_entity(child).is_err());
            assert!(world.get_entity(window).is_ok());
            assert!(world.get_entity(camera).is_ok());
            assert_eq!(world.resource::<Assets<Mesh>>().len(), 0);
            assert_eq!(world.resource::<Assets<StandardMaterial>>().len(), 0);
            assert_eq!(world.resource::<Assets<Image>>().len(), 1);
            assert!(world.resource::<Assets<Image>>().contains(global.id()));
        }
        assert_eq!(world.resource::<SceneCleanup>().cycles, 6);
    }
}
