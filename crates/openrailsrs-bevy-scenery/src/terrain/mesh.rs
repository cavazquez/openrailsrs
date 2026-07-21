//! Patch mesh buffers → Bevy [`Mesh`], offsets and merge helpers.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_formats::TerrainMeshData;

/// MSTS/OR patch side length (16 cells × 8 m).
pub const TERRAIN_PATCH_SIZE_M: f32 = 128.0;

/// World-space offset for a textured patch inside a tile (viewer / tile-local space).
#[inline]
pub fn terrain_patch_offset_in_tile(px: u32, pz: u32) -> Vec3 {
    Vec3::new(
        px as f32 * TERRAIN_PATCH_SIZE_M,
        0.0,
        pz as f32 * TERRAIN_PATCH_SIZE_M,
    )
}

/// Patch offset when the tile is centered on the origin (render3d convention).
#[inline]
pub fn terrain_patch_offset_centered(px: u32, pz: u32, half_tile_m: f32) -> Vec3 {
    Vec3::new(
        px as f32 * TERRAIN_PATCH_SIZE_M - half_tile_m,
        0.0,
        pz as f32 * TERRAIN_PATCH_SIZE_M - half_tile_m,
    )
}

pub fn empty_terrain_mesh_data() -> TerrainMeshData {
    TerrainMeshData {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
    }
}

/// Append `src` into `dst`, translating patch-local positions by `offset`.
pub fn append_terrain_mesh_data(dst: &mut TerrainMeshData, src: &TerrainMeshData, offset: Vec3) {
    append_terrain_mesh_data_owned(dst, src.clone(), offset);
}

/// Consume `src` while appending (avoids cloning large attribute buffers — #60).
pub fn append_terrain_mesh_data_owned(
    dst: &mut TerrainMeshData,
    mut src: TerrainMeshData,
    offset: Vec3,
) {
    let base = dst.positions.len() as u32;
    if offset != Vec3::ZERO {
        for p in &mut src.positions {
            p[0] += offset.x;
            p[1] += offset.y;
            p[2] += offset.z;
        }
    }
    dst.positions.append(&mut src.positions);
    dst.normals.append(&mut src.normals);
    dst.uvs.append(&mut src.uvs);
    dst.indices
        .extend(src.indices.into_iter().map(|i| i + base));
}

pub fn mesh_from_terrain_data(data: &TerrainMeshData, height_origin: f32) -> Mesh {
    mesh_from_terrain_data_owned(data.clone(), height_origin)
}

/// Consume mesh data and optionally rebase heights by `height_origin`.
pub fn mesh_from_terrain_data_owned(mut data: TerrainMeshData, height_origin: f32) -> Mesh {
    if height_origin != 0.0 {
        for p in &mut data.positions {
            p[1] -= height_origin;
        }
    }
    mesh_from_terrain_buffers(data.positions, data.normals, data.uvs, data.indices)
}

/// Build a triangle-list mesh from raw attribute buffers (patch or chunk).
pub fn mesh_from_terrain_buffers(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
