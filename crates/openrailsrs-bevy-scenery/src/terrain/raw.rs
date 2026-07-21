//! Decode `_Y.RAW` / `_F.RAW` for a parsed [`TerrainFile`].

use std::path::Path;
use std::sync::Arc;

use openrailsrs_formats::{ElevationGrid, FeatureGrid, TerrainFile, read_f_raw, read_y_raw};

/// Elevation + optional hole/feature grid for one terrain tile.
#[derive(Clone, Debug)]
pub struct TerrainTileRawData {
    pub grid: Arc<ElevationGrid>,
    pub features: Option<Arc<FeatureGrid>>,
}

/// Load `_Y.RAW` (required) and `_F.RAW` (optional holes) next to `tile_path`.
///
/// `tile_path` is the `.t` / `.y` path used by [`TerrainFile::y_raw_path`].
pub fn load_tile_raw(tile: &TerrainFile, tile_path: &Path) -> Option<TerrainTileRawData> {
    let grid = Arc::new(read_y_raw(&tile.y_raw_path(tile_path), &tile.samples).ok()?);
    let features = if tile.samples.f_buffer_file.trim().is_empty() {
        None
    } else {
        read_f_raw(&tile.f_raw_path(tile_path), &tile.samples)
            .ok()
            .map(Arc::new)
    };
    Some(TerrainTileRawData { grid, features })
}
