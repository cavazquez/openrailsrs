//! Terrain mesh spawning, both immediate and progressive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_formats::{
    build_patch_mesh_data_sampled, build_tile_mesh_data_sampled, msts_tile_world_origin,
    terrain_patches_per_side,
};

use crate::shapes::RouteAssets;
use crate::terrain::{
    TerrainScene, TerrainTile, mesh_from_terrain_data, terrain_patch_offset_in_tile,
};
use crate::terrain_assets::terrain_material_textures;
use crate::terrain_material::TerrainMaterial;
use crate::terrain_sampler::{LoadedTerrainTile, TerrainTileCache};
use crate::{log_step, viewer_log};

const COLOR_TERRAIN_FALLBACK: Color = Color::srgb(0.28, 0.42, 0.22);
const TERRAIN_TILES_PER_FRAME: usize = 1;

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
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
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
    fallback_tex: &Handle<Image>,
    render_origin: Vec3,
    height_origin: f32,
    origin_shift: Vec3,
) -> (usize, usize) {
    let tile = &current.tile;
    let patch_set = match tile.primary_patch_set() {
        Some(set) => set,
        None => return (0, 0),
    };
    let (wx, wz) = msts_tile_world_origin(tile.tile_x, tile.tile_z);
    let tile_origin = Vec3::new(
        wx - render_origin.x - origin_shift.x,
        0.0,
        wz - render_origin.z - origin_shift.z,
    );
    let mut spawned = 0usize;
    let mut holed = 0usize;

    for pz in 0..patch_set.npatches {
        for px in 0..patch_set.npatches {
            let Some(patch) = patch_set.patch_at(px, pz) else {
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

            let mesh_data = build_patch_mesh_data_sampled(
                tile.samples.sample_size,
                px,
                pz,
                Some(patch),
                true,
                |ux, uz| tile_cache.sample_elevation(current, ux, uz),
                |ux, uz| tile_cache.sample_hidden(current, ux, uz),
            );
            if current
                .features
                .as_ref()
                .is_some_and(|f| f.patch_has_hidden_vertices(px, pz))
            {
                holed += 1;
            }

            let (base, overlay, overlay_scale) = terrain_material_textures(
                route_dir,
                images,
                texture_cache,
                shader,
                fallback_tex.clone(),
            );
            let material = materials.add(TerrainMaterial {
                overlay_scale,
                base_texture: base,
                overlay_texture: overlay,
            });

            let patch_offset = terrain_patch_offset_in_tile(px, pz);
            commands.spawn((
                Mesh3d(meshes.add(mesh_from_terrain_data(&mesh_data, height_origin))),
                MeshMaterial3d(material),
                Transform::from_translation(tile_origin + patch_offset),
                Name::new(format!(
                    "terrain-patch:{}:{}:{}:{}",
                    tile.tile_x, tile.tile_z, px, pz
                )),
                TerrainTileTag {
                    tile_x: tile.tile_x,
                    tile_z: tile.tile_z,
                },
            ));
            spawned += 1;
        }
    }
    (spawned, holed)
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
        Mesh3d(meshes.add(mesh_from_terrain_data(&data, height_origin))),
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
    fallback_tex: &Handle<Image>,
    fallback_material: &Handle<StandardMaterial>,
    render_origin: Vec3,
    height_origin: f32,
    origin_shift: Vec3,
) -> (bool, usize, usize) {
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
        let (patches, holed) = spawn_textured_patches(
            commands,
            meshes,
            terrain_materials,
            images,
            route_dir,
            current,
            tile_cache,
            texture_cache,
            fallback_tex,
            render_origin,
            height_origin,
            origin_shift,
        );
        if patches > 0 {
            return (true, patches, holed);
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
    (true, 0, 0)
}

#[derive(Resource)]
pub struct TerrainSpawnProgress {
    started: Instant,
    tile_index: usize,
    spawned_tiles: usize,
    spawned_patches: usize,
    holed_patches: usize,
    tile_cache: TerrainTileCache,
    texture_cache: HashMap<String, Handle<Image>>,
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
            holed_patches: 0,
            tile_cache: TerrainTileCache::from_scene_tiles(&terrain.tiles),
            texture_cache: HashMap::new(),
            fallback_tex: fallback_terrain_image(images),
            fallback_material,
            render_origin: focus.center,
            height_origin: focus.height_origin,
        }
    }

    fn log_summary(&self) {
        if self.spawned_patches > 0 {
            viewer_log!(
                "openrailsrs-viewer3d: {} terrain tile(s), {} textured patch(es){}",
                self.spawned_tiles,
                self.spawned_patches,
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
            fallback_tex,
            fallback_material,
            render_origin,
            height_origin,
            spawned_tiles,
            spawned_patches,
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
        let (spawned, patches, holed) = spawn_loaded_terrain_tile(
            commands,
            meshes,
            images,
            terrain_materials,
            route_dir,
            &loaded,
            &*tile_cache,
            texture_cache,
            &fallback_tex,
            &fallback_material,
            *render_origin,
            *height_origin,
            origin_shift,
        );
        if spawned {
            *spawned_tiles += 1;
            *spawned_patches += patches;
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
#[derive(Resource)]
pub struct TerrainTileStream {
    catalog: std::collections::HashMap<(i32, i32), PathBuf>,
    loaded: std::collections::HashSet<(i32, i32)>,
    #[allow(dead_code)]
    route_dir: PathBuf,
    radius_m: f32,
    last_center_tile: Option<(i32, i32)>,
    pending_spawn: Vec<(i32, i32)>,
    tile_cache: TerrainTileCache,
    texture_cache: std::collections::HashMap<String, Handle<Image>>,
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
        let catalog = crate::terrain::discover_terrain_tile_entries(route_dir, None, f32::MAX)
            .into_iter()
            .map(|(x, z, p)| ((x, z), p))
            .collect();
        let loaded = terrain.tiles.iter().map(|t| (t.tile_x, t.tile_z)).collect();
        Self {
            catalog,
            loaded,
            route_dir: route_dir.to_path_buf(),
            radius_m,
            last_center_tile: None,
            pending_spawn: Vec::new(),
            tile_cache: TerrainTileCache::from_scene_tiles(&terrain.tiles),
            texture_cache: std::collections::HashMap::new(),
            fallback_tex: None,
            fallback_material: None,
            render_origin: focus.center,
            height_origin: focus.height_origin,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn terrain_tile_stream_system(
    mut terrain: ResMut<TerrainScene>,
    mut elevation: ResMut<crate::terrain::TerrainElevation>,
    mut stream: ResMut<TerrainTileStream>,
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
    use crate::terrain::TerrainTile;
    use crate::terrain_io::load_tile_data;
    use crate::world::MSTS_TILE_SIZE_M;
    use openrailsrs_formats::{
        TerrainFile, msts_tile_world_origin, msts_tile_x_index_for_coord,
        msts_tile_z_index_for_coord,
    };
    use std::sync::Arc;

    let center = window.center_world;
    let tile = MSTS_TILE_SIZE_M as f32;
    let tile_x = msts_tile_x_index_for_coord(center.x);
    let tile_z = msts_tile_z_index_for_coord(center.z);
    if stream.last_center_tile == Some((tile_x, tile_z)) {
        return;
    }
    stream.last_center_tile = Some((tile_x, tile_z));

    let radius_tiles = (stream.radius_m / tile).ceil() as i32 + 1;
    let mut loaded_now = 0usize;
    for dtx in -radius_tiles..=radius_tiles {
        for dtz in -radius_tiles..=radius_tiles {
            let tx = tile_x + dtx;
            let tz = tile_z + dtz;
            let key = (tx, tz);
            if stream.loaded.contains(&key) {
                continue;
            }
            let (ox, oz) = msts_tile_world_origin(tx, tz);
            let tcx = ox + tile * 0.5;
            let tcz = oz + tile * 0.5;
            if Vec2::new(tcx - center.x, tcz - center.z).length() > stream.radius_m + tile {
                continue;
            }
            let Some(path) = stream.catalog.get(&key).cloned() else {
                continue;
            };
            let Ok(file) = TerrainFile::from_path_with_coords(&path, tx, tz) else {
                continue;
            };
            let data = load_tile_data(&file, &path).map(Arc::new);
            let (wx, wz) = msts_tile_world_origin(tx, tz);
            terrain.tiles.push(TerrainTile {
                tile_x: tx,
                tile_z: tz,
                translation: Vec3::new(wx, 0.0, wz),
                path,
                file,
                data,
            });
            terrain.tiles_loaded += 1;
            stream.loaded.insert(key);
            stream.pending_spawn.push(key);
            if let Some(last) = terrain.tiles.last() {
                stream.tile_cache.insert_scene_tile(last);
                elevation.merge_tile(tx, tz, Some(last));
            }
            loaded_now += 1;
        }
    }
    if loaded_now > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: terrain-stream — +{loaded_now} tile(s) near ({tile_x},{tile_z})"
        );
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
        holed_patches: 0,
        tile_cache: stream.tile_cache.clone(),
        texture_cache: std::mem::take(&mut stream.texture_cache),
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
    stream.tile_cache = scratch.tile_cache;
}

#[allow(clippy::too_many_arguments)]
pub fn terrain_tile_unload_system(
    mut terrain: ResMut<TerrainScene>,
    mut elevation: ResMut<crate::terrain::TerrainElevation>,
    mut stream: ResMut<TerrainTileStream>,
    window: Res<crate::view_window::ViewWindow>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    mut commands: Commands,
    tagged: Query<(Entity, &TerrainTileTag)>,
) {
    if !opts.live || !mode.loads_msts_scenery() || mode.is_tile_lab() {
        return;
    }
    use crate::world::{MSTS_TILE_SIZE_M, tile_center_distance_m};
    let tile = MSTS_TILE_SIZE_M as f32;
    let unload_radius = window.radius_m + tile + 64.0;
    let center = window.center_world;
    let mut unloaded = std::collections::HashSet::new();
    stream.loaded.retain(|key| {
        let keep = tile_center_distance_m(key.0, key.1, center) <= unload_radius;
        if !keep {
            unloaded.insert(*key);
        }
        keep
    });
    if unloaded.is_empty() {
        return;
    }
    terrain
        .tiles
        .retain(|t| !unloaded.contains(&(t.tile_x, t.tile_z)));
    for key in &unloaded {
        elevation.remove_tile(key.0, key.1);
    }
    for (entity, tag) in tagged.iter() {
        if unloaded.contains(&(tag.tile_x, tag.tile_z)) {
            commands.entity(entity).despawn();
        }
    }
}
