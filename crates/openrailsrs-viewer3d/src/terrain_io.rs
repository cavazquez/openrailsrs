//! Shared terrain tile IO helpers.

use std::path::Path;
use std::sync::Arc;

use openrailsrs_formats::{ElevationGrid, FeatureGrid, TerrainFile, read_f_raw, read_y_raw};

#[derive(Clone, Debug)]
pub(crate) struct TerrainTileData {
    pub(crate) grid: Arc<ElevationGrid>,
    pub(crate) features: Option<Arc<FeatureGrid>>,
}

pub(crate) fn load_tile_data(tile: &TerrainFile, path: &Path) -> Option<TerrainTileData> {
    let grid = Arc::new(read_y_raw(&tile.y_raw_path(path), &tile.samples).ok()?);
    let features = if tile.samples.f_buffer_file.trim().is_empty() {
        None
    } else {
        read_f_raw(&tile.f_raw_path(path), &tile.samples)
            .ok()
            .map(Arc::new)
    };
    Some(TerrainTileData { grid, features })
}
