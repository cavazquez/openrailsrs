//! CPU terrain preparation separated from bounded publication of Bevy assets.
//! Workers preserve native heights, hidden vertices and material chunks. Tile
//! translation uses the current floating origin when a chunk is published.

use crate::{
    terrain_assets::{
        prepare_terrain_images, terrain_material_textures, terrain_shader_material_key,
    },
    terrain_material::TerrainMaterial,
    terrain_sampler::{LoadedTerrainTile, TerrainTileCache},
    terrain_spawn::TerrainTileTag,
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task},
};
use openrailsrs_bevy_scenery::{
    MergedTerrainChunk, merge_patch_into_chunks, mesh_from_terrain_data_owned, reduce_chunk_maps,
    terrain_patch_offset_in_tile,
};
use openrailsrs_formats::{
    TerrainPatch, TerrainShader, build_patch_mesh_data_sampled, build_tile_mesh_data_sampled,
    msts_tile_world_origin, terrain_patches_per_side,
};
use rayon::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) fn prepare_patch_chunks(
    current: &LoadedTerrainTile,
    cache: &TerrainTileCache,
) -> HashMap<String, MergedTerrainChunk> {
    let tile = &current.tile;
    let Some(set) = tile.primary_patch_set() else {
        return HashMap::new();
    };
    let mut jobs: Vec<(u32, u32, TerrainPatch, String)> = Vec::new();
    for z in 0..set.npatches {
        for x in 0..set.npatches {
            let Some(patch) = set
                .patch_at(x, z)
                .filter(|patch| patch.drawing_enabled())
                .cloned()
            else {
                continue;
            };
            let Some(shader) = tile
                .shaders
                .get(patch.shader_index as usize)
                .or_else(|| tile.shaders.first())
            else {
                continue;
            };
            jobs.push((x, z, patch, terrain_shader_material_key(shader)));
        }
    }
    jobs.into_par_iter()
        .fold(HashMap::new, |mut chunks, (x, z, patch, key)| {
            let data = build_patch_mesh_data_sampled(
                tile.samples.sample_size,
                x,
                z,
                Some(&patch),
                true,
                |ux, uz| cache.sample_elevation(current, ux, uz),
                |ux, uz| cache.sample_hidden(current, ux, uz),
            );
            let holed = current
                .features
                .as_ref()
                .is_some_and(|flags| flags.patch_has_hidden_vertices(x, z));
            merge_patch_into_chunks(
                &mut chunks,
                key,
                data,
                terrain_patch_offset_in_tile(x, z),
                holed,
            );
            chunks
        })
        .reduce(HashMap::new, reduce_chunk_maps)
}

struct PreparedChunk {
    key: String,
    shader: TerrainShader,
    mesh: Mesh,
    patches: usize,
    holed: usize,
}

pub(crate) struct PreparedTerrainTile {
    pub(crate) key: (i32, i32),
    images: VecDeque<(String, Image)>,
    chunks: VecDeque<PreparedChunk>,
    fallback: Option<Mesh>,
}

pub(crate) fn prepare_tile(
    current: &LoadedTerrainTile,
    cache: &TerrainTileCache,
    route_dir: &Path,
    height_origin: f32,
) -> PreparedTerrainTile {
    let tile = &current.tile;
    let key = (tile.tile_x, tile.tile_z);
    let mut prepared = PreparedTerrainTile {
        key,
        images: VecDeque::new(),
        chunks: VecDeque::new(),
        fallback: None,
    };
    if tile.has_textured_patches() {
        let mut chunks: Vec<_> = prepare_patch_chunks(current, cache).into_iter().collect();
        chunks.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for (key, chunk) in chunks {
            if chunk.mesh.indices.is_empty() {
                continue;
            }
            if let Some(shader) = tile
                .shaders
                .iter()
                .find(|shader| terrain_shader_material_key(shader) == key)
            {
                prepared.chunks.push_back(PreparedChunk {
                    key,
                    shader: shader.clone(),
                    patches: chunk.patch_count,
                    holed: chunk.holed_patches,
                    mesh: mesh_from_terrain_data_owned(chunk.mesh, height_origin),
                });
            }
        }
        if !prepared.chunks.is_empty() {
            let shaders: Vec<_> = prepared
                .chunks
                .iter()
                .map(|chunk| chunk.shader.clone())
                .collect();
            prepared.images = prepare_terrain_images(route_dir, &shaders).into();
            return prepared;
        }
    }
    let data = build_tile_mesh_data_sampled(
        tile.samples.sample_size,
        terrain_patches_per_side(current.grid.nsamples),
        |x, z| cache.sample_elevation(current, x, z),
        |x, z| cache.sample_hidden(current, x, z),
    );
    prepared.fallback = Some(mesh_from_terrain_data_owned(data, height_origin));
    prepared
}

pub(crate) fn start_preparation(
    current: LoadedTerrainTile,
    cache: TerrainTileCache,
    route_dir: PathBuf,
    height_origin: f32,
) -> Task<PreparedTerrainTile> {
    AsyncComputeTaskPool::get()
        .spawn(async move { prepare_tile(&current, &cache, &route_dir, height_origin) })
}

#[derive(Default)]
pub(crate) struct PublishedTerrain {
    pub(crate) patches: usize,
    pub(crate) chunks: usize,
    pub(crate) holed: usize,
    pub(crate) done: bool,
}

impl PreparedTerrainTile {
    /// No terrain triangulation, native file reads or texture decompression are
    /// performed here. At least one publication can progress on a slow device.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<TerrainMaterial>,
        textures: &mut HashMap<String, Handle<Image>>,
        material_cache: &mut HashMap<String, Handle<TerrainMaterial>>,
        fallback_tex: &Handle<Image>,
        fallback_material: &Handle<StandardMaterial>,
        route_dir: &Path,
        render_origin: Vec3,
        origin_shift: Vec3,
    ) -> PublishedTerrain {
        let start = Instant::now();
        let budget = Duration::from_millis(4);
        let mut result = PublishedTerrain::default();
        while let Some((key, image)) = self.images.pop_front() {
            textures.entry(key).or_insert_with(|| images.add(image));
            if start.elapsed() >= budget {
                return result;
            }
        }
        let (x, z) = msts_tile_world_origin(self.key.0, self.key.1);
        let transform = Transform::from_xyz(
            x - render_origin.x - origin_shift.x,
            0.,
            z - render_origin.z - origin_shift.z,
        );
        while let Some(chunk) = self.chunks.pop_front() {
            let material = material_cache
                .entry(chunk.key.clone())
                .or_insert_with(|| {
                    let (base, overlay, scale) = terrain_material_textures(
                        route_dir,
                        images,
                        textures,
                        &chunk.shader,
                        fallback_tex.clone(),
                    );
                    materials.add(TerrainMaterial {
                        base_texture: base,
                        overlay_texture: overlay,
                        overlay_scale: scale,
                        surface_weather: Vec2::ZERO,
                    })
                })
                .clone();
            commands.spawn((
                Mesh3d(meshes.add(chunk.mesh)),
                MeshMaterial3d(material),
                transform,
                TerrainTileTag {
                    tile_x: self.key.0,
                    tile_z: self.key.1,
                },
                Name::new(format!(
                    "terrain-chunk:{}:{}:{}",
                    self.key.0, self.key.1, chunk.key
                )),
            ));
            result.patches += chunk.patches;
            result.holed += chunk.holed;
            result.chunks += 1;
            if start.elapsed() >= budget {
                return result;
            }
        }
        if let Some(mesh) = self.fallback.take() {
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(fallback_material.clone()),
                transform,
                TerrainTileTag {
                    tile_x: self.key.0,
                    tile_z: self.key.1,
                },
                Name::new(format!("terrain:{}:{}", self.key.0, self.key.1)),
            ));
            result.chunks += 1;
        }
        result.done = true;
        result
    }
}
