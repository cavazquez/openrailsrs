//! Chunk-merge helpers for [`super::TerrainMeshMode::ChunkMerge`] (#60 / #122).

use bevy::prelude::Vec3;
use openrailsrs_formats::TerrainMeshData;
use std::collections::HashMap;

use super::mesh::{append_terrain_mesh_data_owned, empty_terrain_mesh_data};

/// One merged tile-space chunk sharing a material cache key.
#[derive(Clone, Debug)]
pub struct MergedTerrainChunk {
    pub mesh: TerrainMeshData,
    pub patch_count: usize,
    pub holed_patches: usize,
}

impl Default for MergedTerrainChunk {
    fn default() -> Self {
        Self {
            mesh: empty_terrain_mesh_data(),
            patch_count: 0,
            holed_patches: 0,
        }
    }
}

/// Fold a single patch mesh into `chunks` under `material_key`.
///
/// `patch_offset` moves patch-local positions into tile space before merge.
pub fn merge_patch_into_chunks(
    chunks: &mut HashMap<String, MergedTerrainChunk>,
    material_key: String,
    mesh: TerrainMeshData,
    patch_offset: Vec3,
    patch_holed: bool,
) {
    let entry = chunks.entry(material_key).or_default();
    append_terrain_mesh_data_owned(&mut entry.mesh, mesh, patch_offset);
    entry.patch_count += 1;
    if patch_holed {
        entry.holed_patches += 1;
    }
}

/// Concatenate two chunk maps (e.g. after parallel fold/reduce).
pub fn reduce_chunk_maps(
    mut a: HashMap<String, MergedTerrainChunk>,
    b: HashMap<String, MergedTerrainChunk>,
) -> HashMap<String, MergedTerrainChunk> {
    for (key, chunk) in b {
        let entry = a.entry(key).or_default();
        append_terrain_mesh_data_owned(&mut entry.mesh, chunk.mesh, Vec3::ZERO);
        entry.patch_count += chunk.patch_count;
        entry.holed_patches += chunk.holed_patches;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TerrainMeshData;

    #[test]
    fn merge_counts_holes_and_patches() {
        let mut chunks = HashMap::new();
        let mesh = TerrainMeshData {
            positions: vec![[0.0, 0.0, 0.0]],
            normals: vec![[0.0, 1.0, 0.0]],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
        };
        merge_patch_into_chunks(
            &mut chunks,
            "grass|micro|32".into(),
            mesh.clone(),
            Vec3::ZERO,
            true,
        );
        merge_patch_into_chunks(
            &mut chunks,
            "grass|micro|32".into(),
            mesh,
            Vec3::new(128.0, 0.0, 0.0),
            false,
        );
        let c = chunks.get("grass|micro|32").unwrap();
        assert_eq!(c.patch_count, 2);
        assert_eq!(c.holed_patches, 1);
        assert_eq!(c.mesh.positions.len(), 2);
        assert_eq!(c.mesh.positions[1], [128.0, 0.0, 0.0]);
    }
}
