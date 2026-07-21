//! CPU-side Bevy [`Asset`] wrappers for MSTS formats (#48, #53).

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::reflect::TypePath;
use openrailsrs_ace::AceFile;
use openrailsrs_formats::{ElevationGrid, FeatureGrid, ShapeFile, TerrainFile, WorldFile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::load_diagnostics::MstsLoadDiagnostics;

/// Parsed `.s` shape retained as a Bevy asset.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct MstsShapeAsset {
    pub shape: ShapeFile,
    pub source_path: PathBuf,
}

/// Decoded `.ace` texture retained as a Bevy asset (CPU mip0 + metadata).
#[derive(Asset, TypePath, Clone, Debug)]
pub struct MstsAceAsset {
    pub ace: AceFile,
    pub source_path: PathBuf,
}

/// Parsed WORLD tile (`.w`).
#[derive(Asset, TypePath, Clone, Debug)]
pub struct MstsWorldTileAsset {
    pub world: WorldFile,
    pub tile_x: i32,
    pub tile_z: i32,
    pub source_path: PathBuf,
}

/// Status of `_Y.RAW` / `_F.RAW` sidecars for a terrain tile (#53).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerrainRawStatus {
    /// Y present; F present or not required.
    Complete,
    MissingY,
    /// Y ok but named F buffer missing.
    MissingF,
    MissingBoth,
}

impl TerrainRawStatus {
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }

    pub fn y_missing(self) -> bool {
        matches!(self, Self::MissingY | Self::MissingBoth)
    }
}

/// Parsed terrain tile (`.y` / `.t`) plus optional RAW grids (#53).
#[derive(Asset, TypePath, Clone, Debug)]
pub struct MstsTerrainTileAsset {
    pub terrain: TerrainFile,
    pub tile_x: i32,
    pub tile_z: i32,
    pub source_path: PathBuf,
    pub y_raw_path: Option<PathBuf>,
    pub f_raw_path: Option<PathBuf>,
    pub elevation: Option<ElevationGrid>,
    pub features: Option<FeatureGrid>,
    pub raw_status: TerrainRawStatus,
    pub diag: MstsLoadDiagnostics,
}

/// Aggregate readiness of a composite tile bundle (#53).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TileBundleStatus {
    /// Both world and terrain present; terrain RAW complete.
    Ready,
    /// At least one component usable; another missing/partial.
    Partial,
    /// No usable world or terrain component.
    Failed,
}

/// JSON manifest for [`MstsTileBundleAsset`] (`.tilebundle`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TileBundleManifest {
    pub tile_x: i32,
    pub tile_z: i32,
    #[serde(default)]
    pub world: Option<String>,
    #[serde(default)]
    pub terrain: Option<String>,
}

/// Discovered absolute paths for one tile coordinate (#53).
#[derive(Clone, Debug, Default)]
pub struct TileBundlePaths {
    pub tile_x: i32,
    pub tile_z: i32,
    pub world: Option<PathBuf>,
    pub terrain: Option<PathBuf>,
}

impl TileBundlePaths {
    pub fn to_manifest(&self) -> TileBundleManifest {
        TileBundleManifest {
            tile_x: self.tile_x,
            tile_z: self.tile_z,
            world: self
                .world
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            terrain: self
                .terrain
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        }
    }
}

/// Resolve WORLD + terrain paths for `(tile_x, tile_z)` under a route directory.
pub fn discover_tile_bundle_paths(
    route_dir: impl AsRef<std::path::Path>,
    tile_x: i32,
    tile_z: i32,
) -> TileBundlePaths {
    let route_dir = route_dir.as_ref();
    TileBundlePaths {
        tile_x,
        tile_z,
        world: openrailsrs_formats::resolve_world_tile_file(route_dir, tile_x, tile_z),
        terrain: openrailsrs_formats::resolve_hash_terrain_tile_file(route_dir, tile_x, tile_z),
    }
}

/// Write a `.tilebundle` JSON manifest (paths as given — usually asset-relative).
pub fn write_tile_bundle_manifest(
    path: impl AsRef<std::path::Path>,
    manifest: &TileBundleManifest,
) -> Result<(), MstsAssetError> {
    let path = path.as_ref();
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| MstsAssetError::BundleParse {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| MstsAssetError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    std::fs::write(path, bytes).map_err(|source| MstsAssetError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// Composite tile: WORLD + terrain handles and observable status (#53).
#[derive(Asset, TypePath, Clone, Debug)]
pub struct MstsTileBundleAsset {
    pub tile_x: i32,
    pub tile_z: i32,
    pub world: Option<Handle<MstsWorldTileAsset>>,
    pub terrain: Option<Handle<MstsTerrainTileAsset>>,
    /// Snapshot of terrain RAW status when the bundle loader resolved terrain (if any).
    pub terrain_raw_status: Option<TerrainRawStatus>,
    pub status: TileBundleStatus,
    pub diag: MstsLoadDiagnostics,
    pub source_path: PathBuf,
}

/// Lightweight route content index (JSON `.routecat`).
#[derive(Asset, TypePath, Clone, Debug, Serialize, Deserialize)]
pub struct MstsRouteCatalogAsset {
    #[serde(default)]
    pub shapes: Vec<String>,
    #[serde(default)]
    pub textures: Vec<String>,
    #[serde(default)]
    pub world_tiles: Vec<String>,
}

/// Typed load failures for MSTS asset loaders.
#[derive(Debug, Error)]
pub enum MstsAssetError {
    #[error("I/O error loading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse shape {path}: {message}")]
    ShapeParse { path: String, message: String },
    #[error("failed to decode ACE {path}: {message}")]
    AceDecode { path: String, message: String },
    #[error("failed to parse world tile {path}: {message}")]
    WorldParse { path: String, message: String },
    #[error("failed to parse route catalog {path}: {message}")]
    CatalogParse { path: String, message: String },
    #[error("failed to parse terrain tile {path}: {message}")]
    TerrainParse { path: String, message: String },
    #[error("failed to parse tile bundle {path}: {message}")]
    BundleParse { path: String, message: String },
}
