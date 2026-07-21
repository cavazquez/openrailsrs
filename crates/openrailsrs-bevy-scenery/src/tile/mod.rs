//! Canonical CPU tile projection (`MstsTileSnapshot`) shared by render3d / viewer (#112).
//!
//! Holds WORLD + terrain data and diagnostics without App, camera, RouteFocus, or GPU state.
//! App-specific types (`TileEntry`, `WorldScene`, …) adapt from this snapshot.

mod classify;
mod snapshot;

pub use classify::{
    MstsClassifiedWorldItem, MstsForestPatch, MstsHWaterPatch, MstsTransferPatch,
    MstsWorldItemKind, classify_world_file, classify_world_item, item_transform,
    matrix3x3_to_rotation_scale, qdir_to_quat,
};
pub use snapshot::{
    MstsTileSnapshot, MstsTileTerrainSnapshot, MstsTileWorldSnapshot, elevation_base_y,
    load_msts_tile_snapshot, load_msts_tile_snapshot_from_paths, load_msts_tile_terrain_snapshot,
    load_msts_tile_world_snapshot, resolve_world_tile_path, snapshot_from_parsed,
};
