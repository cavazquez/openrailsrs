//! Shared Bevy layer for MSTS/Open Rails scenery rendering.
//!
//! See `docs/BEVY_ARCHITECTURE.md`.

pub mod assets;
pub mod gpu;
pub mod load_diagnostics;
pub mod materials;
pub mod shapes;
pub mod spawn;
pub mod textures;
pub mod ui;
pub mod vsm;

#[cfg(test)]
pub mod test_harness;

pub use assets::{
    MstsAceAsset, MstsAceAssetLoader, MstsAssetError, MstsAssetPlugin, MstsRouteCatalogAsset,
    MstsRouteCatalogLoader, MstsShapeAsset, MstsShapeAssetLoader, MstsWorldTileAsset,
    MstsWorldTileAssetLoader, register_msts_content_source,
};
pub use load_diagnostics::{
    LoadFailure, MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics,
};
pub use materials::{
    OrCabMaterial, OrForestMaterial, OrSceneryMaterial, OrTerrainMaterial, TerrainMaterial,
    create_or_cab_material, create_or_forest_material, or_cab_shaders_enabled,
    or_scenery_shaders_enabled,
};
pub use spawn::{ScenerySpawnMode, ScenerySpawnPlugin};

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
