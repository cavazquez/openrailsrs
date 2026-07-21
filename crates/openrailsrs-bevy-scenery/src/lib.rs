//! Shared Bevy layer for MSTS/Open Rails scenery rendering.
//!
//! See `docs/BEVY_ARCHITECTURE.md`.

pub mod assets;
pub mod atmosphere;
pub mod catalog;
pub mod gpu;
pub mod lighting;
pub mod load_diagnostics;
pub mod materials;
pub mod shapes;
pub mod spawn;
pub mod stream;
pub mod terrain;
pub mod textures;
pub mod tile;
pub mod ui;
pub mod vsm;

#[cfg(test)]
pub mod test_harness;

pub use assets::{
    MstsAceAsset, MstsAceAssetLoader, MstsAssetError, MstsAssetPlugin, MstsRouteCatalogAsset,
    MstsRouteCatalogLoader, MstsShapeAsset, MstsShapeAssetLoader, MstsTerrainTileAsset,
    MstsTerrainTileAssetLoader, MstsTileBundleAsset, MstsTileBundleLoader, MstsWorldTileAsset,
    MstsWorldTileAssetLoader, TerrainRawStatus, TileBundleManifest, TileBundlePaths,
    TileBundleStatus, discover_tile_bundle_paths, register_msts_content_source,
    reset_terrain_tile_parse_count, terrain_tile_parse_count, terrain_tile_parse_count_for,
    write_tile_bundle_manifest,
};
pub use atmosphere::{
    distance_fog, fog_visibility_from_tile_span, sky_clear_color, sky_palette, spawn_sky_dome,
};
pub use catalog::{MstsRouteCatalog, index_shapes_tree, route_pack_dir};
pub use lighting::{SceneSunLight, directional_light_from_sun, sun_transform};
pub use load_diagnostics::{LoadFailure, MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics};
pub use materials::{
    OrCabMaterial, OrForestMaterial, OrSceneryMaterial, OrTerrainMaterial, TerrainMaterial,
    TerrainPipelineFlags, create_or_cab_material, create_or_forest_material,
    or_cab_shaders_enabled, or_scenery_shaders_enabled,
};
pub use spawn::forest::{
    DEFAULT_TREE_HEIGHT_M, DEFAULT_TREE_WIDTH_M, TreePlacement, append_tree_billboard,
    build_forest_patch_mesh, forest_rng01, forest_tree_size, scatter_trees_in_patch,
};
pub use spawn::transfer::{TRANSFER_ALPHA_CUTOFF, TransferHeightSampler, build_transfer_mesh};
pub use spawn::water::{
    COLOR_WATER, COLOR_WATER_REFLECT, WATER_LIFT_M, WATER_UV_TILES, build_water_plane_mesh,
    reflection_material as water_reflection_material, water_material,
};
pub use spawn::world::{
    FnPlacementAdapter, IdentityPlacementAdapter, PlacementAdapter, PlannedShapePartSpawn,
    ResolvedShapePart, ShapePartId, TileOffsetPlacementAdapter, WorldObjectPlacement,
    WorldObjectPose, cached_shape_parts, object_placement, plan_parts_with_ids,
    plan_shape_part_spawns, resolve_shape_parts, spawn_resolved_shape_parts,
    spawn_resolved_shape_parts_bound, spawn_shape_parts_with, spawn_standard_shape_parts,
    spawn_standard_shape_parts_bound, world_item_placement, world_item_rotation_scale,
};
pub use spawn::{
    ScenerySpawnBudgets, ScenerySpawnCycle, ScenerySpawnMode, ScenerySpawnPhase,
    ScenerySpawnPlugin, ScenerySpawnProgress, ScenerySpawnSet, SessionShapeCache,
    SessionShapeCacheTelemetry, ShapeCacheKey, scenery_spawn_cycle_active,
};
pub use stream::{
    StreamDiff, StreamWindowPolicy, TILE_SIZE_M, TileBound, TileCoord, meters_to_tile_radius,
};
pub use terrain::{
    MergedTerrainChunk, TERRAIN_PATCH_SIZE_M, TerrainMeshMode, TerrainTileRawData,
    append_terrain_mesh_data, append_terrain_mesh_data_owned, empty_terrain_mesh_data,
    load_tile_raw, merge_patch_into_chunks, mesh_from_terrain_buffers,
    mesh_from_terrain_data_owned, reduce_chunk_maps, sanitize_terrain_base_rgba,
    set_terrain_repeat_sampler, terrain_material_cache_key, terrain_patch_offset_centered,
    terrain_patch_offset_in_tile, terrain_shader_material_key, terrain_shader_overlay_scale,
};
pub use tile::{
    MstsClassifiedWorldItem, MstsForestPatch, MstsHWaterPatch, MstsTileSnapshot,
    MstsTileTerrainSnapshot, MstsTileWorldSnapshot, MstsTransferPatch, MstsWorldItemKind,
    classify_world_file, classify_world_item, elevation_base_y, item_transform,
    load_msts_tile_snapshot, load_msts_tile_snapshot_from_paths, load_msts_tile_terrain_snapshot,
    load_msts_tile_world_snapshot, matrix3x3_to_rotation_scale, qdir_to_quat,
    resolve_world_tile_path, snapshot_from_parsed,
};

use std::path::PathBuf;

use bevy::asset::AssetPlugin;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

/// Absolute path to this crate's `assets/` directory (shaders, etc.).
pub fn asset_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Registers OR material plugins. Call before app-specific systems.
pub struct OrSceneryPlugins;

impl Plugin for OrSceneryPlugins {
    fn build(&self, app: &mut App) {
        app.add_plugins(MstsAssetPlugin)
            .add_plugins(ScenerySpawnPlugin)
            .add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .add_plugins(MaterialPlugin::<OrTerrainMaterial>::default())
            .add_plugins(MaterialPlugin::<OrSceneryMaterial>::default())
            .add_plugins(MaterialPlugin::<OrCabMaterial>::default())
            .add_plugins(MaterialPlugin::<OrForestMaterial>::default());
    }
}

/// Asset plugin pointed at [`asset_root`] — use when the app has no local `assets/`.
pub fn shared_asset_plugin() -> AssetPlugin {
    AssetPlugin {
        file_path: asset_root().to_string_lossy().into_owned(),
        ..default()
    }
}
