//! Terrain tile sampling across MSTS tile boundaries.

use std::collections::HashMap;
use std::sync::Arc;

use openrailsrs_formats::{
    ElevationGrid, FeatureGrid, TerrainFile, msts_display_tile_x_from_internal,
    msts_tile_world_origin,
};

use crate::terrain::TerrainTile;
use crate::world::MSTS_TILE_SIZE_M;

#[derive(Clone)]
pub(crate) struct LoadedTerrainTile {
    pub(crate) tile: TerrainFile,
    pub(crate) grid: Arc<ElevationGrid>,
    pub(crate) features: Option<Arc<FeatureGrid>>,
}

impl LoadedTerrainTile {
    fn from_scene_tile(terrain_tile: &TerrainTile) -> Option<Self> {
        let data = terrain_tile.data.as_ref()?;
        Some(Self {
            tile: terrain_tile.file.clone(),
            grid: data.grid.clone(),
            features: data.features.clone(),
        })
    }
}

pub(crate) struct TerrainTileCache {
    tiles: HashMap<(i32, i32), LoadedTerrainTile>,
}

impl TerrainTileCache {
    pub(crate) fn from_scene_tiles(tiles: &[TerrainTile]) -> Self {
        let mut out = HashMap::new();
        for terrain_tile in tiles {
            let Some(tile) = LoadedTerrainTile::from_scene_tile(terrain_tile) else {
                continue;
            };
            out.insert(Self::display_key(&tile.tile), tile);
        }
        Self { tiles: out }
    }

    #[cfg(test)]
    pub(crate) fn from_loaded_tiles_for_test(tiles: Vec<LoadedTerrainTile>) -> Self {
        Self {
            tiles: tiles
                .into_iter()
                .map(|tile| (Self::display_key(&tile.tile), tile))
                .collect(),
        }
    }

    pub(crate) fn get_display(&self, display_x: i32, display_z: i32) -> Option<&LoadedTerrainTile> {
        self.tiles.get(&(display_x, display_z))
    }

    pub(crate) fn display_key(tile: &TerrainFile) -> (i32, i32) {
        (msts_display_tile_x_from_internal(tile.tile_x), tile.tile_z)
    }

    pub(crate) fn tile_key_for_sample(tile: &TerrainFile, ux: i32, uz: i32) -> (i32, i32) {
        let (display_x, display_z) = Self::display_key(tile);
        let (ox, oz) = msts_tile_world_origin(display_x, display_z);
        let sample_size = tile.samples.sample_size as f32;
        let wx = ox + ux as f32 * sample_size;
        let wz = oz + uz as f32 * sample_size;
        let tile_size = MSTS_TILE_SIZE_M as f32;
        (
            (wx / tile_size).floor() as i32,
            (wz / tile_size).floor() as i32,
        )
    }

    fn sample_coord_in_tile(
        current: &TerrainFile,
        target: &TerrainFile,
        ux: i32,
        uz: i32,
    ) -> (isize, isize) {
        let (current_display_x, current_display_z) = Self::display_key(current);
        let (target_display_x, target_display_z) = Self::display_key(target);
        let (current_ox, current_oz) = msts_tile_world_origin(current_display_x, current_display_z);
        let (target_ox, target_oz) = msts_tile_world_origin(target_display_x, target_display_z);
        let current_sample_size = current.samples.sample_size as f32;
        let target_sample_size = target.samples.sample_size as f32;
        let wx = current_ox + ux as f32 * current_sample_size;
        let wz = current_oz + uz as f32 * current_sample_size;
        (
            ((wx - target_ox) / target_sample_size).round() as isize,
            ((wz - target_oz) / target_sample_size).round() as isize,
        )
    }

    pub(crate) fn sample_elevation(&self, current: &LoadedTerrainTile, ux: i32, uz: i32) -> f32 {
        if ux >= 0
            && uz >= 0
            && (ux as usize) < current.grid.nsamples
            && (uz as usize) < current.grid.nsamples
        {
            return current.grid.elevation_at(ux as usize, uz as usize);
        }
        let key = Self::tile_key_for_sample(&current.tile, ux, uz);
        let Some(target) = self.tiles.get(&key) else {
            return current.grid.elevation_at_clamped(ux as isize, uz as isize);
        };
        let (sx, sz) = Self::sample_coord_in_tile(&current.tile, &target.tile, ux, uz);
        target.grid.elevation_at_clamped(sx, sz)
    }

    pub(crate) fn sample_hidden(&self, current: &LoadedTerrainTile, ux: i32, uz: i32) -> bool {
        if ux >= 0
            && uz >= 0
            && (ux as usize) < current.grid.nsamples
            && (uz as usize) < current.grid.nsamples
        {
            return current
                .features
                .as_ref()
                .is_some_and(|features| features.is_vertex_hidden(ux as usize, uz as usize));
        }
        let key = Self::tile_key_for_sample(&current.tile, ux, uz);
        let Some(target) = self.tiles.get(&key) else {
            return false;
        };
        let Some(features) = target.features.as_ref() else {
            return false;
        };
        let (sx, sz) = Self::sample_coord_in_tile(&current.tile, &target.tile, ux, uz);
        if sx < 0 || sz < 0 {
            return false;
        }
        features.is_vertex_hidden(sx as usize, sz as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{TerrainSamples, build_patch_mesh_data_sampled};

    fn test_loaded_tile(tile_x: i32, tile_z: i32, height: f32) -> LoadedTerrainTile {
        test_loaded_tile_with_nsamples(tile_x, tile_z, 256, height)
    }

    fn test_loaded_tile_with_nsamples(
        tile_x: i32,
        tile_z: i32,
        nsamples: usize,
        height: f32,
    ) -> LoadedTerrainTile {
        LoadedTerrainTile {
            tile: TerrainFile {
                tile_x,
                tile_z,
                samples: TerrainSamples {
                    nsamples: nsamples as u32,
                    sample_size: 8.0,
                    ..Default::default()
                },
                shaders: Vec::new(),
                patch_sets: Vec::new(),
            },
            grid: Arc::new(ElevationGrid {
                nsamples,
                elevations: vec![height; nsamples * nsamples],
            }),
            features: None,
        }
    }

    fn test_feature_grid(hidden_samples: &[(usize, usize)]) -> FeatureGrid {
        let mut flags = vec![0u8; 256 * 256];
        for &(x, z) in hidden_samples {
            flags[z * 256 + x] = 0x04;
        }
        FeatureGrid {
            nsamples: 256,
            flags,
        }
    }

    #[test]
    fn mesh_vertex_sampling_wraps_east_edge_to_loaded_neighbor() {
        let current = test_loaded_tile(0, 0, 1.0);
        let east = test_loaded_tile(-1, 0, 42.0);
        let cache = TerrainTileCache::from_loaded_tiles_for_test(vec![current, east]);
        let current = cache.get_display(0, 0).unwrap();
        assert_eq!(cache.sample_elevation(current, 255, 0), 1.0);
        assert_eq!(cache.sample_elevation(current, 256, 0), 42.0);
    }

    #[test]
    fn mesh_vertex_sampling_wraps_z_edges_to_expected_neighbors() {
        let current = test_loaded_tile(0, 0, 1.0);
        let north = test_loaded_tile(0, 1, 77.0);
        let south = test_loaded_tile(0, -1, 13.0);
        assert_eq!(
            TerrainTileCache::tile_key_for_sample(&current.tile, 0, 256),
            (0, 1)
        );
        assert_eq!(
            TerrainTileCache::tile_key_for_sample(&current.tile, 0, -1),
            (0, -1)
        );

        let cache = TerrainTileCache::from_loaded_tiles_for_test(vec![current, north, south]);
        let current = cache.get_display(0, 0).unwrap();
        assert_eq!(cache.sample_elevation(current, 0, 256), 77.0);
        assert_eq!(cache.sample_elevation(current, 0, -1), 13.0);
    }

    #[test]
    fn large_physical_tile_samples_inside_its_own_grid_before_neighbor_lookup() {
        let mut current = test_loaded_tile_with_nsamples(0, 0, 512, 1.0);
        Arc::make_mut(&mut current.grid).elevations[256] = 99.0;
        let east = test_loaded_tile(-1, 0, 42.0);

        let cache = TerrainTileCache::from_loaded_tiles_for_test(vec![current, east]);
        let current = cache.get_display(0, 0).unwrap();

        assert_eq!(
            TerrainTileCache::tile_key_for_sample(&current.tile, 256, 0),
            (1, 0)
        );
        assert_eq!(cache.sample_elevation(current, 256, 0), 99.0);
    }

    #[test]
    fn hidden_flags_wrap_to_loaded_neighbor_edges() {
        let current = test_loaded_tile(0, 0, 1.0);
        let mut east = test_loaded_tile(-1, 0, 1.0);
        east.features = Some(Arc::new(test_feature_grid(&[(0, 0)])));

        let cache = TerrainTileCache::from_loaded_tiles_for_test(vec![current, east]);
        let current = cache.get_display(0, 0).unwrap();

        assert!(!cache.sample_hidden(current, 255, 0));
        assert!(cache.sample_hidden(current, 256, 0));
    }

    #[test]
    fn neighbor_hidden_edge_removes_border_triangles() {
        let current = test_loaded_tile(0, 0, 1.0);
        let mut east = test_loaded_tile(-1, 0, 1.0);
        east.features = Some(Arc::new(test_feature_grid(&[(0, 0), (0, 1)])));

        let cache = TerrainTileCache::from_loaded_tiles_for_test(vec![current, east]);
        let current = cache.get_display(0, 0).unwrap();
        let full = build_patch_mesh_data_sampled(
            8.0,
            15,
            0,
            None,
            false,
            |ux, uz| cache.sample_elevation(current, ux, uz),
            |ux, uz| cache.sample_hidden(current, ux, uz),
        );
        let holed = build_patch_mesh_data_sampled(
            8.0,
            15,
            0,
            None,
            true,
            |ux, uz| cache.sample_elevation(current, ux, uz),
            |ux, uz| cache.sample_hidden(current, ux, uz),
        );

        assert!(
            holed.indices.len() < full.indices.len(),
            "hidden vertices in the east neighbor should remove current-tile border triangles"
        );
        assert!(
            holed.indices.windows(3).all(|tri| !tri.contains(&16)),
            "triangles using the hidden east-edge vertex should be omitted"
        );
    }
}
