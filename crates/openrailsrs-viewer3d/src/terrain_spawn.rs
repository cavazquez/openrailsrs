//! Terrain mesh spawning, both immediate and progressive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{Task, futures::check_ready};
use openrailsrs_bevy_scenery::{
    TerrainMeshMode, mesh_from_terrain_data_owned, set_terrain_repeat_sampler,
};
use openrailsrs_formats::{
    build_tile_mesh_data_sampled, msts_tile_world_origin, terrain_patches_per_side,
};

use crate::shapes::RouteAssets;
use crate::terrain::{TerrainScene, TerrainTile};
use crate::terrain_assets::{terrain_material_textures, terrain_shader_material_key};
use crate::terrain_material::TerrainMaterial;
use crate::terrain_sampler::{LoadedTerrainTile, TerrainTileCache};
use crate::{log_step, viewer_log};

const COLOR_TERRAIN_FALLBACK: Color = Color::srgb(0.28, 0.42, 0.22);

/// viewer3d terrain entity strategy (#122): merge by material key.
const _: TerrainMeshMode = TerrainMeshMode::ChunkMerge;

fn fallback_terrain_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut img = Image::new_fill(
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[70, 107, 56, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    set_terrain_repeat_sampler(&mut img);
    images.add(img)
}

#[allow(clippy::too_many_arguments)]
fn spawn_textured_patches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<TerrainMaterial>,
    images: &mut Assets<Image>,
    route_dir: &Path,
    current: &LoadedTerrainTile,
    tile_cache: &TerrainTileCache,
    texture_cache: &mut HashMap<String, Handle<Image>>,
    material_cache: &mut HashMap<String, Handle<TerrainMaterial>>,
    fallback_tex: &Handle<Image>,
    render_origin: Vec3,
    height_origin: f32,
    origin_shift: Vec3,
) -> (usize, usize, usize) {
    let tile = &current.tile;
    if tile.primary_patch_set().is_none() {
        return (0, 0, 0);
    }
    let (wx, wz) = msts_tile_world_origin(tile.tile_x, tile.tile_z);
    let tile_origin = Vec3::new(
        wx - render_origin.x - origin_shift.x,
        0.0,
        wz - render_origin.z - origin_shift.z,
    );

    // Use the same native geometry builder as asynchronous terrain preparation.
    let merged = crate::terrain_prepare::prepare_patch_chunks(current, tile_cache);
    for key in merged.keys() {
        if material_cache.contains_key(key) {
            continue;
        }
        let Some(shader) = tile
            .shaders
            .iter()
            .find(|s| terrain_shader_material_key(s) == *key)
        else {
            continue;
        };
        let (base, overlay, overlay_scale) = terrain_material_textures(
            route_dir,
            images,
            texture_cache,
            shader,
            fallback_tex.clone(),
        );
        material_cache.insert(
            key.clone(),
            materials.add(TerrainMaterial {
                overlay_scale,
                base_texture: base,
                overlay_texture: overlay,
                surface_weather: Vec2::ZERO,
                enhancement: Vec4::ZERO,
            }),
        );
    }

    let mut patch_count = 0usize;
    let mut holed = 0usize;
    let mut entities = 0usize;
    for (key, chunk) in merged {
        patch_count += chunk.patch_count;
        holed += chunk.holed_patches;
        let Some(material) = material_cache.get(&key).cloned() else {
            continue;
        };
        if chunk.mesh.indices.is_empty() {
            continue;
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh_from_terrain_data_owned(chunk.mesh, height_origin))),
            MeshMaterial3d(material),
            Transform::from_translation(tile_origin),
            Name::new(format!(
                "terrain-chunk:{}:{}:{}",
                tile.tile_x, tile.tile_z, key
            )),
            TerrainTileTag {
                tile_x: tile.tile_x,
                tile_z: tile.tile_z,
            },
        ));
        entities += 1;
    }
    (patch_count, entities, holed)
}

#[allow(clippy::too_many_arguments)]
fn spawn_legacy_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    current: &LoadedTerrainTile,
    tile_cache: &TerrainTileCache,
    material: &Handle<StandardMaterial>,
    render_origin: Vec3,
    height_origin: f32,
    origin_shift: Vec3,
) {
    let tile = &current.tile;
    let data = build_tile_mesh_data_sampled(
        tile.samples.sample_size,
        terrain_patches_per_side(current.grid.nsamples),
        |ux, uz| tile_cache.sample_elevation(current, ux, uz),
        |ux, uz| tile_cache.sample_hidden(current, ux, uz),
    );
    let (wx, wz) = msts_tile_world_origin(tile.tile_x, tile.tile_z);
    let translation = Vec3::new(
        wx - render_origin.x - origin_shift.x,
        0.0,
        wz - render_origin.z - origin_shift.z,
    );
    commands.spawn((
        Mesh3d(meshes.add(mesh_from_terrain_data_owned(data, height_origin))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation),
        Name::new(format!("terrain:{}:{}", tile.tile_x, tile.tile_z)),
        TerrainTileTag {
            tile_x: tile.tile_x,
            tile_z: tile.tile_z,
        },
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_loaded_terrain_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    terrain_materials: &mut Assets<TerrainMaterial>,
    route_dir: &Path,
    current: &LoadedTerrainTile,
    tile_cache: &TerrainTileCache,
    texture_cache: &mut HashMap<String, Handle<Image>>,
    material_cache: &mut HashMap<String, Handle<TerrainMaterial>>,
    fallback_tex: &Handle<Image>,
    fallback_material: &Handle<StandardMaterial>,
    render_origin: Vec3,
    height_origin: f32,
    origin_shift: Vec3,
) -> (bool, usize, usize, usize) {
    let tile = &current.tile;
    let grid = &current.grid;

    if std::env::var("OPENRAILSRS_TERRAIN_DEBUG").is_ok() {
        let min_h = grid.elevations.iter().cloned().fold(f32::MAX, f32::min);
        let max_h = grid.elevations.iter().cloned().fold(f32::MIN, f32::max);
        viewer_log!(
            "openrailsrs-viewer3d: terrain-debug tile {}:{} floor={:.2} scale={:.6} size={:.1} elev=[{:.1}..{:.1}] range={:.1}m",
            tile.tile_x,
            tile.tile_z,
            tile.samples.sample_floor,
            tile.samples.sample_scale,
            tile.samples.sample_size,
            min_h,
            max_h,
            max_h - min_h,
        );
    }

    if tile.has_textured_patches() {
        let (patches, entities, holed) = spawn_textured_patches(
            commands,
            meshes,
            terrain_materials,
            images,
            route_dir,
            current,
            tile_cache,
            texture_cache,
            material_cache,
            fallback_tex,
            render_origin,
            height_origin,
            origin_shift,
        );
        if patches > 0 {
            return (true, patches, entities, holed);
        }
    }

    spawn_legacy_tile(
        commands,
        meshes,
        current,
        tile_cache,
        fallback_material,
        render_origin,
        height_origin,
        origin_shift,
    );
    (true, 0, 1, 0)
}

#[derive(Resource)]
pub struct TerrainSpawnProgress {
    started: Instant,
    tile_index: usize,
    spawned_tiles: usize,
    spawned_patches: usize,
    spawned_chunks: usize,
    holed_patches: usize,
    tile_cache: TerrainTileCache,
    texture_cache: HashMap<String, Handle<Image>>,
    material_cache: HashMap<String, Handle<TerrainMaterial>>,
    fallback_tex: Handle<Image>,
    fallback_material: Handle<StandardMaterial>,
    render_origin: Vec3,
    height_origin: f32,
    preparation: Option<Task<crate::terrain_prepare::PreparedTerrainTile>>,
    prepared: Option<crate::terrain_prepare::PreparedTerrainTile>,
}

impl TerrainSpawnProgress {
    fn new(
        terrain: &TerrainScene,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        focus: &crate::world::RouteFocus,
    ) -> Self {
        let fallback_material = materials.add(StandardMaterial {
            base_color: COLOR_TERRAIN_FALLBACK,
            perceptual_roughness: 0.95,
            metallic: 0.0,
            double_sided: false,
            ..default()
        });
        Self {
            started: Instant::now(),
            tile_index: 0,
            spawned_tiles: 0,
            spawned_patches: 0,
            spawned_chunks: 0,
            holed_patches: 0,
            tile_cache: TerrainTileCache::from_scene_tiles(&terrain.tiles),
            texture_cache: HashMap::new(),
            material_cache: HashMap::new(),
            fallback_tex: fallback_terrain_image(images),
            fallback_material,
            render_origin: focus.center,
            height_origin: focus.height_origin,
            preparation: None,
            prepared: None,
        }
    }

    fn log_summary(&self) {
        if self.spawned_patches > 0 {
            viewer_log!(
                "openrailsrs-viewer3d: {} terrain tile(s), {} patch(es) → {} chunk(s)/{} material(s){}",
                self.spawned_tiles,
                self.spawned_patches,
                self.spawned_chunks,
                self.material_cache.len(),
                if self.holed_patches > 0 {
                    format!(" ({} with holes)", self.holed_patches)
                } else {
                    String::new()
                }
            );
        } else if self.spawned_tiles > 0 {
            viewer_log!(
                "openrailsrs-viewer3d: {} terrain tile(s) with heightfield mesh",
                self.spawned_tiles
            );
        }
        log_step("spawned terrain meshes (progressive)", self.started);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_scene_tile(
        &mut self,
        terrain_tile: &TerrainTile,
        route_dir: &Path,
        origin_shift: Vec3,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        terrain_materials: &mut Assets<TerrainMaterial>,
    ) {
        let TerrainSpawnProgress {
            tile_cache,
            texture_cache,
            material_cache,
            fallback_tex,
            fallback_material,
            render_origin,
            height_origin,
            spawned_tiles,
            spawned_patches,
            spawned_chunks,
            holed_patches,
            ..
        } = self;
        let Some(loaded) = tile_cache
            .get_display(terrain_tile.tile_x, terrain_tile.tile_z)
            .cloned()
        else {
            return;
        };
        let fallback_tex = fallback_tex.clone();
        let fallback_material = fallback_material.clone();
        let (spawned, patches, chunks, holed) = spawn_loaded_terrain_tile(
            commands,
            meshes,
            images,
            terrain_materials,
            route_dir,
            &loaded,
            &*tile_cache,
            texture_cache,
            material_cache,
            &fallback_tex,
            &fallback_material,
            *render_origin,
            *height_origin,
            origin_shift,
        );
        if spawned {
            *spawned_tiles += 1;
            *spawned_patches += patches;
            *spawned_chunks += chunks;
            *holed_patches += holed;
        }
    }
}

/// Begin progressive terrain spawn (continues in [`progressive_terrain_spawn_system`]).
pub fn init_terrain_spawn_progress(
    terrain: Res<TerrainScene>,
    focus: Res<crate::world::RouteFocus>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
) {
    if terrain.is_empty() {
        return;
    }
    viewer_log!(
        "openrailsrs-viewer3d: progressive terrain spawn — {} tile(s)",
        terrain.tiles.len()
    );
    let mut progress = TerrainSpawnProgress::new(&terrain, &mut images, &mut std_materials, &focus);
    progress.texture_cache.reserve(terrain.tiles.len());
    commands.insert_resource(progress);
}

/// Continue terrain spawn across frames so the window can open before all tiles are meshed.
#[allow(clippy::too_many_arguments)]
pub fn progressive_terrain_spawn_system(
    route_dir: Res<RouteAssets>,
    terrain: Res<TerrainScene>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    progress: Option<ResMut<TerrainSpawnProgress>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    stream: Option<ResMut<TerrainTileStream>>,
) {
    let Some(mut progress) = progress else {
        return;
    };
    let progress = &mut *progress;
    if let Some(task) = progress.preparation.as_mut() {
        let Some(prepared) = check_ready(task) else {
            return;
        };
        progress.preparation = None;
        progress.prepared = Some(prepared);
    }
    if let Some(mut prepared) = progress.prepared.take() {
        let published = prepared.publish(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut terrain_materials,
            &mut progress.texture_cache,
            &mut progress.material_cache,
            &progress.fallback_tex,
            &progress.fallback_material,
            &route_dir.route_dir,
            progress.render_origin,
            crate::floating_origin::horizontal_shift(origin.shift),
        );
        progress.spawned_patches += published.patches;
        progress.spawned_chunks += published.chunks;
        progress.holed_patches += published.holed;
        if published.done {
            progress.spawned_tiles += 1;
            progress.tile_index += 1;
        } else {
            progress.prepared = Some(prepared);
            return;
        }
    }
    if progress.tile_index >= terrain.tiles.len() {
        progress.log_summary();
        if let Some(mut stream) = stream {
            stream
                .texture_cache
                .extend(std::mem::take(&mut progress.texture_cache));
            stream
                .material_cache
                .extend(std::mem::take(&mut progress.material_cache));
            stream.fallback_tex = Some(progress.fallback_tex.clone());
            stream.fallback_material = Some(progress.fallback_material.clone());
        }
        commands.remove_resource::<TerrainSpawnProgress>();
    } else {
        let tile = &terrain.tiles[progress.tile_index];
        if let Some(current) = progress
            .tile_cache
            .get_display(tile.tile_x, tile.tile_z)
            .cloned()
        {
            progress.preparation = Some(crate::terrain_prepare::start_preparation(
                current,
                progress.tile_cache.clone(),
                route_dir.route_dir.clone(),
                progress.height_origin,
            ));
        } else {
            progress.tile_index += 1;
        }
    }
}

/// Spawn all terrain meshes immediately. Kept for tests and non-interactive harnesses.
#[allow(clippy::too_many_arguments)]
pub fn spawn_terrain_meshes(
    route_dir: Res<RouteAssets>,
    terrain: Res<TerrainScene>,
    focus: Res<crate::world::RouteFocus>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
) {
    if terrain.is_empty() {
        return;
    }

    viewer_log!(
        "openrailsrs-viewer3d: spawning terrain meshes ({} tile(s))",
        terrain.tiles.len()
    );
    let mut progress = TerrainSpawnProgress::new(&terrain, &mut images, &mut std_materials, &focus);
    for terrain_tile in &terrain.tiles {
        progress.spawn_scene_tile(
            terrain_tile,
            &route_dir.route_dir,
            crate::floating_origin::horizontal_shift(origin.shift),
            &mut commands,
            &mut meshes,
            &mut images,
            &mut terrain_materials,
        );
    }
    progress.log_summary();
}

/// Tag for terrain meshes spawned from a MSTS tile (mobile stream unload).
#[derive(Component, Clone, Copy, Debug)]
pub struct TerrainTileTag {
    pub tile_x: i32,
    pub tile_z: i32,
}

/// Incremental terrain tile load around the mobile view window (live full mode).
///
/// Hot path requests `.tilebundle` via AssetServer (#111); GPU spawn stays in
/// [`terrain_tile_spawn_stream_system`].
#[derive(Resource)]
pub struct TerrainTileStream {
    catalog: std::collections::HashMap<(i32, i32), PathBuf>,
    loaded: std::collections::HashSet<(i32, i32)>,
    /// AssetServer request in flight (not yet in [`TerrainScene`]).
    pending_load: std::collections::HashSet<(i32, i32)>,
    route_dir: PathBuf,
    radius_m: f32,
    last_center_tile: Option<(i32, i32)>,
    pending_spawn: Vec<(i32, i32)>,
    tile_cache: TerrainTileCache,
    texture_cache: std::collections::HashMap<String, Handle<Image>>,
    material_cache: std::collections::HashMap<String, Handle<TerrainMaterial>>,
    fallback_tex: Option<Handle<Image>>,
    fallback_material: Option<Handle<StandardMaterial>>,
    render_origin: Vec3,
    height_origin: f32,
    preparation: Option<(
        (i32, i32),
        Task<crate::terrain_prepare::PreparedTerrainTile>,
    )>,
    prepared: Option<crate::terrain_prepare::PreparedTerrainTile>,
}

impl TerrainTileStream {
    pub(crate) fn pending_work(&self) -> usize {
        self.pending_load.len()
            + self.pending_spawn.len()
            + usize::from(self.preparation.is_some())
            + usize::from(self.prepared.is_some())
    }

    pub fn new(
        route_dir: &Path,
        terrain: &TerrainScene,
        focus: &crate::world::RouteFocus,
        radius_m: f32,
    ) -> Self {
        // Hash TILES and legacy TERRAIN/.t share the same case-insensitive discovery.
        let catalog = crate::terrain::discover_terrain_tile_entries(route_dir, None, f32::MAX)
            .into_iter()
            .map(|(x, z, p)| ((x, z), p))
            .collect();
        let loaded = terrain.tiles.iter().map(|t| (t.tile_x, t.tile_z)).collect();
        Self {
            catalog,
            loaded,
            pending_load: std::collections::HashSet::new(),
            route_dir: route_dir.to_path_buf(),
            radius_m,
            last_center_tile: None,
            pending_spawn: Vec::new(),
            tile_cache: TerrainTileCache::from_scene_tiles(&terrain.tiles),
            texture_cache: std::collections::HashMap::new(),
            material_cache: std::collections::HashMap::new(),
            fallback_tex: None,
            fallback_material: None,
            render_origin: focus.center,
            height_origin: focus.height_origin,
            preparation: None,
            prepared: None,
        }
    }
}

/// Request terrain `.tilebundle` loads near the view window (#111).
#[allow(clippy::too_many_arguments)]
pub fn terrain_tile_stream_system(
    mut stream: ResMut<TerrainTileStream>,
    mut handles: ResMut<crate::tile_bundle::TileBundleHandles>,
    asset_server: Res<AssetServer>,
    window: Res<crate::view_window::ViewWindow>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    progress: Option<Res<TerrainSpawnProgress>>,
) {
    if !opts.live || !mode.loads_msts_scenery() || mode.is_tile_lab() {
        return;
    }
    if progress.is_some() {
        return;
    }
    use crate::world::view_stream_window_policy;
    use openrailsrs_bevy_scenery::stream::TileCoord;
    use openrailsrs_formats::{msts_tile_x_index_for_coord, msts_tile_z_index_for_coord};
    use std::collections::HashSet;

    let center = window.center_world;
    let tile_x = msts_tile_x_index_for_coord(center.x);
    let tile_z = msts_tile_z_index_for_coord(center.z);
    if stream.last_center_tile == Some((tile_x, tile_z)) {
        return;
    }
    stream.last_center_tile = Some((tile_x, tile_z));

    let policy = view_stream_window_policy(stream.radius_m);
    let cam_coord = TileCoord::new(tile_x, tile_z);
    let known: HashSet<TileCoord> = stream
        .loaded
        .iter()
        .chain(stream.pending_load.iter())
        .copied()
        .map(TileCoord::from)
        .collect();
    // Prefer catalog keys that fall inside the load window (same Chebyshev policy as WORLD).
    let candidates = stream.catalog.keys().copied().map(TileCoord::from);
    let stream_diff = policy.diff(cam_coord, &known, candidates);

    let mut requested = 0usize;
    for tile in &stream_diff.to_load {
        let key = (tile.x, tile.z);
        if stream.loaded.contains(&key) || stream.pending_load.contains(&key) {
            continue;
        }
        let Some(path) = stream.catalog.get(&key).cloned() else {
            continue;
        };
        if crate::tile_bundle::ensure_tile_bundle_handle(
            &mut handles,
            &asset_server,
            &stream.route_dir,
            tile.x,
            tile.z,
            None,
            Some(path.as_path()),
        )
        .is_some()
        {
            stream.pending_load.insert(key);
            requested += 1;
        }
    }
    if requested > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: requested {requested} terrain tilebundle(s) near ({tile_x},{tile_z})"
        );
    }
}

/// Materialize Ready/Partial terrain from AssetServer tile bundles (#111).
#[allow(clippy::too_many_arguments)]
pub fn terrain_tile_bundle_materialize_system(
    mut terrain: ResMut<TerrainScene>,
    mut elevation: ResMut<crate::terrain::TerrainElevation>,
    mut stream: ResMut<TerrainTileStream>,
    handles: Res<crate::tile_bundle::TileBundleHandles>,
    asset_server: Res<AssetServer>,
    bundles: Res<Assets<openrailsrs_bevy_scenery::MstsTileBundleAsset>>,
    worlds: Res<Assets<openrailsrs_bevy_scenery::MstsWorldTileAsset>>,
    terrains: Res<Assets<openrailsrs_bevy_scenery::MstsTerrainTileAsset>>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    progress: Option<Res<TerrainSpawnProgress>>,
) {
    if !opts.live || !mode.loads_msts_scenery() || mode.is_tile_lab() || progress.is_some() {
        return;
    }
    if stream.pending_load.is_empty() {
        return;
    }
    let pending: Vec<(i32, i32)> = stream.pending_load.iter().copied().collect();
    let mut loaded_now = 0usize;
    for key in pending {
        let Some(handle) = handles.get(key.0, key.1).cloned() else {
            stream.pending_load.remove(&key);
            continue;
        };
        match crate::tile_bundle::tile_bundle_load_outcome(&asset_server, &handle) {
            crate::tile_bundle::TileBundleLoadOutcome::Pending => continue,
            crate::tile_bundle::TileBundleLoadOutcome::Failed => {
                crate::tile_bundle::record_bundle_load_failure(
                    &mut terrain.load_diag,
                    key.0,
                    key.1,
                    &format!("tilebundle ({},{})", key.0, key.1),
                );
                stream.pending_load.remove(&key);
                stream.loaded.insert(key);
                continue;
            }
            crate::tile_bundle::TileBundleLoadOutcome::Loaded => {}
        }
        let Some(bundle) = bundles.get(&handle) else {
            continue;
        };
        let Some(tile) = crate::tile_bundle::try_materialize_terrain_bundle(
            bundle,
            &worlds,
            &terrains,
            &mut terrain,
        ) else {
            // Deps not ready yet, or Failed/no terrain — if Failed, stop retrying.
            if bundle.status == openrailsrs_bevy_scenery::TileBundleStatus::Failed
                || bundle.terrain.is_none()
            {
                stream.pending_load.remove(&key);
                stream.loaded.insert(key);
            }
            continue;
        };
        stream.pending_load.remove(&key);
        stream.loaded.insert(key);
        stream.pending_spawn.push(key);
        stream.tile_cache.insert_scene_tile(&tile);
        elevation.merge_tile(key.0, key.1, Some(&tile));
        loaded_now += 1;
    }
    if loaded_now > 0 {
        viewer_log!("openrailsrs-viewer3d: terrain-stream — +{loaded_now} tile(s) via tilebundle");
    }
}

#[allow(clippy::too_many_arguments)]
pub fn terrain_tile_spawn_stream_system(
    route_dir: Res<RouteAssets>,
    terrain: Res<TerrainScene>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mut stream: ResMut<TerrainTileStream>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    progress: Option<Res<TerrainSpawnProgress>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
) {
    if !opts.live || mode.is_track_focused() || mode.is_tile_lab() || progress.is_some() {
        return;
    }
    if let Some((key, task)) = stream.preparation.as_mut() {
        let key = *key;
        let Some(prepared) = check_ready(task) else {
            return;
        };
        stream.preparation = None;
        // An unloaded sector must not reappear when its old worker completes.
        if stream.loaded.contains(&key) {
            stream.prepared = Some(prepared);
        }
    }
    if stream.prepared.is_none() {
        if stream.pending_spawn.is_empty() {
            return;
        }
        let key = stream.pending_spawn.remove(0);
        if !terrain
            .tiles
            .iter()
            .any(|tile| (tile.tile_x, tile.tile_z) == key)
        {
            return;
        }
        let Some(current) = stream.tile_cache.get_display(key.0, key.1).cloned() else {
            return;
        };
        stream.preparation = Some((
            key,
            crate::terrain_prepare::start_preparation(
                current,
                stream.tile_cache.clone(),
                route_dir.route_dir.clone(),
                stream.height_origin,
            ),
        ));
        return;
    }
    if stream.fallback_tex.is_none() {
        stream.fallback_tex = Some(fallback_terrain_image(&mut images));
        stream.fallback_material = Some(std_materials.add(StandardMaterial {
            base_color: COLOR_TERRAIN_FALLBACK,
            perceptual_roughness: 0.95,
            metallic: 0.0,
            double_sided: false,
            ..default()
        }));
    }
    let stream = &mut *stream;
    let mut prepared = stream.prepared.take().unwrap();
    if !stream.loaded.contains(&prepared.key) {
        return;
    }
    let published = prepared.publish(
        &mut commands,
        &mut meshes,
        &mut images,
        &mut terrain_materials,
        &mut stream.texture_cache,
        &mut stream.material_cache,
        stream.fallback_tex.as_ref().unwrap(),
        stream.fallback_material.as_ref().unwrap(),
        &route_dir.route_dir,
        stream.render_origin,
        crate::floating_origin::horizontal_shift(origin.shift),
    );
    if !published.done {
        stream.prepared = Some(prepared);
    }
}

/// Release terrain meshes for unloaded tiles and drop material/texture cache
/// entries no longer referenced by remaining tiles (#51).
fn evict_unreferenced_terrain_assets(
    stream: &mut TerrainTileStream,
    live_material_ids: &std::collections::HashSet<AssetId<TerrainMaterial>>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    terrain_materials: &mut Assets<TerrainMaterial>,
    released_meshes: impl IntoIterator<Item = AssetId<Mesh>>,
) -> (usize, usize, usize) {
    let mut meshes_removed = 0usize;
    for id in released_meshes {
        if meshes.remove(id).is_some() {
            meshes_removed += 1;
        }
    }

    let mut materials_removed = 0usize;
    let stale_mats: Vec<String> = stream
        .material_cache
        .iter()
        .filter(|(_, handle)| !live_material_ids.contains(&handle.id()))
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale_mats {
        if let Some(handle) = stream.material_cache.remove(&key)
            && terrain_materials.remove(handle.id()).is_some()
        {
            materials_removed += 1;
        }
    }

    let mut still_needed_images = std::collections::HashSet::new();
    if let Some(fallback) = &stream.fallback_tex {
        still_needed_images.insert(fallback.id());
    }
    for handle in stream.material_cache.values() {
        if let Some(mat) = terrain_materials.get(handle) {
            still_needed_images.insert(mat.base_texture.id());
            still_needed_images.insert(mat.overlay_texture.id());
        }
    }

    let mut textures_removed = 0usize;
    let stale_tex: Vec<String> = stream
        .texture_cache
        .iter()
        .filter(|(_, handle)| !still_needed_images.contains(&handle.id()))
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale_tex {
        if let Some(handle) = stream.texture_cache.remove(&key)
            && images.remove(handle.id()).is_some()
        {
            textures_removed += 1;
        }
    }

    (meshes_removed, materials_removed, textures_removed)
}

#[allow(clippy::too_many_arguments)]
pub fn terrain_tile_unload_system(
    mut terrain: ResMut<TerrainScene>,
    mut elevation: ResMut<crate::terrain::TerrainElevation>,
    mut stream: ResMut<TerrainTileStream>,
    mut handles: ResMut<crate::tile_bundle::TileBundleHandles>,
    window: Res<crate::view_window::ViewWindow>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    tagged: Query<(
        Entity,
        &TerrainTileTag,
        &Mesh3d,
        &MeshMaterial3d<TerrainMaterial>,
    )>,
) {
    if !opts.live || !mode.loads_msts_scenery() || mode.is_tile_lab() {
        return;
    }
    use crate::world::view_stream_window_policy;
    use openrailsrs_bevy_scenery::stream::TileCoord;
    use openrailsrs_formats::{msts_tile_x_index_for_coord, msts_tile_z_index_for_coord};
    let center = window.center_world;
    let policy = view_stream_window_policy(window.radius_m.max(stream.radius_m));
    let cam_coord = TileCoord::new(
        msts_tile_x_index_for_coord(center.x),
        msts_tile_z_index_for_coord(center.z),
    );
    let loaded_coords: std::collections::HashSet<TileCoord> = stream
        .loaded
        .iter()
        .chain(stream.pending_load.iter())
        .copied()
        .map(TileCoord::from)
        .collect();
    let stream_diff = policy.diff_disk(cam_coord, &loaded_coords);
    let mut unloaded = std::collections::HashSet::new();
    for tile in &stream_diff.to_unload {
        let key = (tile.x, tile.z);
        unloaded.insert(key);
        stream.loaded.remove(&key);
        stream.pending_load.remove(&key);
        stream.pending_spawn.retain(|k| *k != key);
    }
    if unloaded.is_empty() {
        return;
    }
    // Release shared tilebundle handles (#111). World unload may also release the same key.
    handles.release_all(unloaded.iter());
    terrain
        .tiles
        .retain(|t| !unloaded.contains(&(t.tile_x, t.tile_z)));
    for key in &unloaded {
        elevation.remove_tile(key.0, key.1);
        stream.tile_cache.remove_display(key.0, key.1);
    }

    let mut live_material_ids = std::collections::HashSet::new();
    let mut released_meshes = Vec::new();
    let mut despawned = 0usize;
    for (entity, tag, mesh3d, mat3d) in tagged.iter() {
        if unloaded.contains(&(tag.tile_x, tag.tile_z)) {
            released_meshes.push(mesh3d.id());
            commands.entity(entity).despawn();
            despawned += 1;
        } else {
            live_material_ids.insert(mat3d.id());
        }
    }
    let (meshes_removed, materials_removed, textures_removed) = evict_unreferenced_terrain_assets(
        &mut stream,
        &live_material_ids,
        &mut meshes,
        &mut images,
        &mut terrain_materials,
        released_meshes,
    );
    viewer_log!(
        "openrailsrs-viewer3d: unloaded {} terrain tile(s) (despawned {}; freed {} mesh(es)/{} material(s)/{} texture(s); cache {}/{} )",
        unloaded.len(),
        despawned,
        meshes_removed,
        materials_removed,
        textures_removed,
        stream.material_cache.len(),
        stream.texture_cache.len()
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleared_initial_terrain_finishes_pending_spawn_without_panicking() {
        use super::*;
        let terrain = TerrainScene::default();
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.,
        };
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut progress = TerrainSpawnProgress::new(&terrain, &mut images, &mut materials, &focus);
        progress.tile_index = 8;
        let mut app = App::new();
        app.insert_resource(terrain)
            .insert_resource(progress)
            .insert_resource(images)
            .insert_resource(materials)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .init_resource::<crate::floating_origin::FloatingOrigin>()
            .insert_resource(RouteAssets::new(crate::test_harness::smoke_route_dir()))
            .add_systems(Update, progressive_terrain_spawn_system);
        app.update();
        assert!(!app.world().contains_resource::<TerrainSpawnProgress>());
    }
    use openrailsrs_bevy_scenery::append_terrain_mesh_data;
    use openrailsrs_formats::TerrainMeshData;

    use super::*;

    #[test]
    fn terrain_created_during_rebase_stays_aligned_with_native_coordinates() {
        use crate::camera::{CameraFollowMode, OrbitState};
        use crate::floating_origin::{FloatingOrigin, apply_floating_origin};
        use crate::launch::{ViewerLaunchOpts, ViewerSceneryMode};
        use openrailsrs_formats::{ElevationGrid, TerrainFile, TerrainSamples};
        use std::sync::Arc;

        let (wx, wz) = msts_tile_world_origin(3, -2);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(RouteAssets::new(PathBuf::from("nonexistent-test-route")))
            .insert_resource(crate::world::RouteFocus {
                center: Vec3::new(wx, 0.0, wz),
                height_origin: 30.0,
            })
            .insert_resource(TerrainScene {
                tiles_loaded: 1,
                tiles: vec![TerrainTile {
                    tile_x: 3,
                    tile_z: -2,
                    translation: Vec3::ZERO,
                    path: PathBuf::from("test.t"),
                    file: TerrainFile {
                        tile_x: 3,
                        tile_z: -2,
                        samples: TerrainSamples {
                            nsamples: 16,
                            sample_size: 8.0,
                            ..Default::default()
                        },
                        shaders: Vec::new(),
                        patch_sets: Vec::new(),
                    },
                    data: Some(Arc::new(crate::terrain_io::TerrainTileData {
                        grid: Arc::new(ElevationGrid {
                            nsamples: 16,
                            elevations: vec![40.0; 16 * 16],
                        }),
                        features: None,
                    })),
                }],
                ..Default::default()
            })
            .insert_resource(ViewerSceneryMode::Full)
            .insert_resource(ViewerLaunchOpts::default())
            .insert_resource(CameraFollowMode::Off)
            .insert_resource(FloatingOrigin {
                shift: Vec3::new(25.0, 0.0, 0.0),
            })
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<TerrainMaterial>>()
            .add_systems(Startup, init_terrain_spawn_progress)
            .add_systems(Update, apply_floating_origin)
            .add_systems(
                Update,
                progressive_terrain_spawn_system.after(apply_floating_origin),
            );
        app.world_mut().spawn((
            Camera3d::default(),
            OrbitState::default(),
            Transform::from_xyz(300.0, 5.0, -400.0),
        ));
        // Commands create terrain after rebasing. It must include the new shift,
        // rather than remaining displaced underneath a neighbouring tile.
        app.update();
        // A second rebase occurs after the worker was scheduled. Its output
        // must use the publication origin, not the origin captured at launch.
        let mut camera = app
            .world_mut()
            .query_filtered::<&mut Transform, With<Camera3d>>();
        camera.single_mut(app.world_mut()).unwrap().translation = Vec3::new(500.0, 5.0, 500.0);
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while app.world().contains_resource::<TerrainSpawnProgress>() {
            assert!(Instant::now() < deadline, "terrain worker failed to finish");
            app.update();
            std::thread::yield_now();
        }
        let shift = app.world().resource::<FloatingOrigin>().shift;
        assert_eq!(shift, Vec3::new(825.0, 0.0, 100.0));
        let mut terrain = app
            .world_mut()
            .query_filtered::<(&Transform, &Mesh3d), With<TerrainTileTag>>();
        let (transform, mesh) = terrain.single(app.world()).unwrap();
        assert_eq!(transform.translation, -shift);
        let positions = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&mesh.0)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(
            positions[0][1], 10.0,
            "MSL height is independent of XZ rebasing"
        );
    }

    #[test]
    fn append_terrain_mesh_data_offsets_and_reindexes() {
        let mut dst = TerrainMeshData {
            positions: vec![[0.0, 0.0, 0.0]],
            normals: vec![[0.0, 1.0, 0.0]],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
        };
        let src = TerrainMeshData {
            positions: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            normals: vec![[0.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            uvs: vec![[0.5, 0.5], [1.0, 1.0]],
            indices: vec![0, 1, 0],
        };
        append_terrain_mesh_data(&mut dst, &src, Vec3::new(128.0, 0.0, 256.0));
        assert_eq!(dst.positions.len(), 3);
        assert_eq!(dst.positions[1], [129.0, 2.0, 259.0]);
        assert_eq!(dst.positions[2], [132.0, 5.0, 262.0]);
        assert_eq!(dst.indices, vec![0, 1, 2, 1]);
    }

    #[test]
    fn viewer_uses_chunk_merge_mode() {
        assert_eq!(TerrainMeshMode::ChunkMerge.label(), "chunk_merge");
    }
}
