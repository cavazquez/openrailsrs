//! Shared terrain tile IO helpers.
//!
//! Thin re-export of bevy-scenery RAW loaders. Full tile CPU projection lives in
//! [`openrailsrs_bevy_scenery::MstsTileSnapshot`] (#112); TerrainScene materialization
//! from that snapshot is still TODO (paired with #111 tilebundle adoption).

use std::path::Path;

pub use openrailsrs_bevy_scenery::{TerrainTileRawData as TerrainTileData, load_tile_raw};

pub(crate) fn load_tile_data(
    tile: &openrailsrs_formats::TerrainFile,
    path: &Path,
) -> Option<TerrainTileData> {
    load_tile_raw(tile, path)
}
