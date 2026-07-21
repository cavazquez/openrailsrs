//! CPU-side Bevy [`Asset`] wrappers for MSTS formats (#48).

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::reflect::TypePath;
use openrailsrs_ace::AceFile;
use openrailsrs_formats::{ShapeFile, WorldFile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
}
