//! Terrain mesh spawning, both immediate and progressive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_bevy_scenery::{
    MergedTerrainChunk, TerrainMeshMode, merge_patch_into_chunks, mesh_from_terrain_data_owned,
    reduce_chunk_maps, set_terrain_repeat_sampler, terrain_patch_offset_in_tile,
};
use openrailsrs_formats::{
    TerrainPatch, build_patch_mesh_data_sampled, build_tile_mesh_data_sampled,
    msts_tile_world_origin, terrain_patches_per_side,
};
use rayon::prelude::*;

use crate::shapes::RouteAssets;
use crate::terrain::{TerrainScene, TerrainTile};
use crate::terrain_assets::{terrain_material_textures, terrain_shader_material_key};
use crate::terrain_material::TerrainMaterial;
use crate::terrain_sampler::{LoadedTerrainTile, TerrainTileCache};
use crate::{log_step, viewer_log};

const COLOR_TERRAIN_FALLBACK: Color = Color::srgb(0.28, 0.42, 0.22);
/// Merged material chunks are cheap enough to finish the initial tile set in one
/// Update, avoiding wall-clock inflation from interleaving with WORLD spawn (#60).
const TERRAIN_TILES_PER_FRAME: usize = 8;

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
    let patch_set = match tile.primary_patch_set() {
        Some(set) => set,
        None => return (0, 0, 0),
    };
    let (wx, wz) = msts_tile_world_origin(tile.tile_x, tile.tile_z);
    let tile_origin = Vec3::new(
        wx - render_origin.x - origin_shift.x,
        0.0,
        wz - render_origin.z - origin_shift.z,
    );

    // Collect drawable patches, then build meshes in parallel before merging (#60).
    let mut jobs: Vec<(u32, u32, TerrainPatch, String)> = Vec::new();
    for pz in 0..patch_set.npatches {
        for px in 0..patch_set.npatches {
            let Some(patch) = patch_set.patch_at(px, pz).cloned() else {
                continue;
            };
            if !patch.drawing_enabled() {
                continue;
            }
            let shader = tile
                .shaders
                .get(patch.shader_index as usize)
                .or_else(|| tile.shaders.first());
            let Some(shader) = shader else {
                continue;
            };
            let key = terrain_shader_material_key(shader);
            if !material_cache.contains_key(&key) {
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
                    }),
                );
            }
            jobs.push((px, pz, patch, key));
        }
    }

    let sample_size = tile.samples.sample_size;
    // Parallel build+merge by material key via bevy-scenery::terrain (#60 / #122).
    let merged: HashMap<String, MergedTerrainChunk> = jobs
        .into_par_iter()
        .fold(HashMap::new, |mut acc, (px, pz, patch, key)| {
            let mesh_data = build_patch_mesh_data_sampled(
                sample_size,
                px,
                pz,
                Some(&patch),
                true,
                |ux, uz| tile_cache.sample_elevation(current, ux, uz),
                |ux, uz| tile_cache.sample_hidden(current, ux, uz),
            );
            let patch_holed = current
                .features
                .as_ref()
                .is_some_and(|f| f.patch_has_hidden_vertices(px, pz));
            merge_patch_into_chunks(
                &mut acc,
                key,
                mesh_data,
                terrain_patch_offset_in_tile(px, pz),
                patch_holed,
            );
            acc
        })
        .reduce(HashMap::new, reduce_chunk_maps);

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
) {
    let Some(mut progress) = progress else {
        return;
    };
    let origin_shift = crate::floating_origin::horizontal_shift(origin.shift);
    let start = progress.tile_index;
    let end = (start + TERRAIN_TILES_PER_FRAME).min(terrain.tiles.len());
    for terrain_tile in &terrain.tiles[start..end] {
        progress.spawn_scene_tile(
            terrain_tile,
            &route_dir.route_dir,
            origin_shift,
            &mut commands,
            &mut meshes,
            &mut images,
            &mut terrain_materials,
        );
    }
    progress.tile_index = end;
    if progress.tile_index >= terrain.tiles.len() {
        progress.log_summary();
        commands.remove_resource::<TerrainSpawnProgress>();
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
}

impl TerrainTileStream {
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
    if stream.pending_spawn.is_empty() {
        return;
    }
    let key = stream.pending_spawn.remove(0);
    let Some(tile) = terrain.tiles.iter().find(|t| (t.tile_x, t.tile_z) == key) else {
        return;
    };
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
    let mut scratch = TerrainSpawnProgress {
        started: Instant::now(),
        tile_index: 0,
        spawned_tiles: 0,
        spawned_patches: 0,
        spawned_chunks: 0,
        holed_patches: 0,
        tile_cache: stream.tile_cache.clone(),
        texture_cache: std::mem::take(&mut stream.texture_cache),
        material_cache: std::mem::take(&mut stream.material_cache),
        fallback_tex: stream.fallback_tex.clone().unwrap(),
        fallback_material: stream.fallback_material.clone().unwrap(),
        render_origin: stream.render_origin,
        height_origin: stream.height_origin,
    };
    scratch.spawn_scene_tile(
        tile,
        &route_dir.route_dir,
        crate::floating_origin::horizontal_shift(origin.shift),
        &mut commands,
        &mut meshes,
        &mut images,
        &mut terrain_materials,
    );
    stream.texture_cache = scratch.texture_cache;
    stream.material_cache = scratch.material_cache;
    stream.tile_cache = scratch.tile_cache;
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
        if let Some(handle) = stream.material_cache.remove(&key) {
            if terrain_materials.remove(handle.id()).is_some() {
                materials_removed += 1;
            }
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
        if let Some(handle) = stream.texture_cache.remove(&key) {
            if images.remove(handle.id()).is_some() {
                textures_removed += 1;
            }
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
    use openrailsrs_bevy_scenery::append_terrain_mesh_data;
    use openrailsrs_formats::TerrainMeshData;

    use super::*;

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
