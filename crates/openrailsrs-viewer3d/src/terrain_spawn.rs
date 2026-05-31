//! Terrain mesh spawning, both immediate and progressive.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_formats::{
    build_patch_mesh_data_sampled, build_tile_mesh_data_sampled, msts_display_tile_x_from_internal,
    msts_tile_world_origin, terrain_patches_per_side,
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
) -> (usize, usize) {
    let tile = &current.tile;
    let patch_set = match tile.primary_patch_set() {
        Some(set) => set,
        None => return (0, 0),
    };
    let display_x = msts_display_tile_x_from_internal(tile.tile_x);
    let (wx, wz) = msts_tile_world_origin(display_x, tile.tile_z);
    let tile_origin = Vec3::new(wx - render_origin.x, 0.0, wz - render_origin.z);
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
            ));
            spawned += 1;
        }
    }
    (spawned, holed)
}

fn spawn_legacy_tile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    current: &LoadedTerrainTile,
    tile_cache: &TerrainTileCache,
    material: &Handle<StandardMaterial>,
    render_origin: Vec3,
    height_origin: f32,
) {
    let tile = &current.tile;
    let data = build_tile_mesh_data_sampled(
        tile.samples.sample_size,
        terrain_patches_per_side(current.grid.nsamples),
        |ux, uz| tile_cache.sample_elevation(current, ux, uz),
        |ux, uz| tile_cache.sample_hidden(current, ux, uz),
    );
    let display_x = msts_display_tile_x_from_internal(tile.tile_x);
    let (wx, wz) = msts_tile_world_origin(display_x, tile.tile_z);
    let translation = Vec3::new(wx - render_origin.x, 0.0, wz - render_origin.z);
    commands.spawn((
        Mesh3d(meshes.add(mesh_from_terrain_data(&data, height_origin))),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(translation),
        Name::new(format!("terrain:{}:{}", tile.tile_x, tile.tile_z)),
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
    fn spawn_scene_tile(
        &mut self,
        terrain_tile: &TerrainTile,
        route_dir: &Path,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        terrain_materials: &mut Assets<TerrainMaterial>,
    ) {
        let display_x = msts_display_tile_x_from_internal(terrain_tile.tile_x);
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
            .get_display(display_x, terrain_tile.tile_z)
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
    progress: Option<ResMut<TerrainSpawnProgress>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Some(mut progress) = progress else {
        return;
    };
    let start = progress.tile_index;
    let end = (start + TERRAIN_TILES_PER_FRAME).min(terrain.tiles.len());
    for terrain_tile in &terrain.tiles[start..end] {
        progress.spawn_scene_tile(
            terrain_tile,
            &route_dir.route_dir,
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
            &mut commands,
            &mut meshes,
            &mut images,
            &mut terrain_materials,
        );
    }
    progress.log_summary();
}
