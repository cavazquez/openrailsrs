//! Shared MSTS/OR terrain mesh, RAW decode helpers, chunk merge and material keys (#122).
//!
//! Apps choose [`TerrainMeshMode`]:
//! - [`TerrainMeshMode::Patch`] — one entity per drawable patch (`render3d`)
//! - [`TerrainMeshMode::ChunkMerge`] — merge by material key (`viewer3d` / #60)
//!
//! Material GPU contracts live in [`crate::materials`] (#121). This module owns
//! CPU-side UV scale, holes-aware mesh buffers, and cache keys.

mod chunk;
mod material_key;
mod mesh;
mod raw;
mod textures;

pub use chunk::{MergedTerrainChunk, merge_patch_into_chunks, reduce_chunk_maps};
pub use material_key::{
    terrain_material_cache_key, terrain_shader_material_key, terrain_shader_overlay_scale,
};
pub use mesh::{
    TERRAIN_PATCH_SIZE_M, append_terrain_mesh_data, append_terrain_mesh_data_owned,
    empty_terrain_mesh_data, mesh_from_terrain_buffers, mesh_from_terrain_data,
    mesh_from_terrain_data_owned, terrain_patch_offset_centered, terrain_patch_offset_in_tile,
};
pub use raw::{TerrainTileRawData, load_tile_raw};
pub use textures::{sanitize_terrain_base_rgba, set_terrain_repeat_sampler};

/// How a tile’s drawable patches become Bevy mesh entities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TerrainMeshMode {
    /// One [`Mesh`] / entity per drawable patch (render3d baseline).
    #[default]
    Patch,
    /// Fold patches that share a material key into fewer tile-space chunks (#60).
    ChunkMerge,
}

impl TerrainMeshMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::ChunkMerge => "chunk_merge",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;
    use openrailsrs_formats::TerrainMeshData;

    #[test]
    fn mesh_mode_labels() {
        assert_eq!(TerrainMeshMode::Patch.label(), "patch");
        assert_eq!(TerrainMeshMode::ChunkMerge.label(), "chunk_merge");
    }

    #[test]
    fn append_offsets_and_reindexes() {
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
    fn patch_offset_is_index_times_128m() {
        assert_eq!(
            terrain_patch_offset_in_tile(0, 0),
            Vec3::new(0.0, 0.0, 0.0)
        );
        assert_eq!(
            terrain_patch_offset_in_tile(1, 0),
            Vec3::new(TERRAIN_PATCH_SIZE_M, 0.0, 0.0)
        );
        assert_eq!(
            terrain_patch_offset_in_tile(0, 1),
            Vec3::new(0.0, 0.0, TERRAIN_PATCH_SIZE_M)
        );
    }

    #[test]
    fn centered_offset_matches_render3d_convention() {
        let half = 8.0 * TERRAIN_PATCH_SIZE_M * 0.5;
        let o = terrain_patch_offset_centered(2, 3, half);
        assert!((o.x - (2.0 * TERRAIN_PATCH_SIZE_M - half)).abs() < 1e-4);
        assert!((o.z - (3.0 * TERRAIN_PATCH_SIZE_M - half)).abs() < 1e-4);
    }
}
