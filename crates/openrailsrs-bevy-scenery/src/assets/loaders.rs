//! [`AssetLoader`] implementations for MSTS formats (#48, #53).

use std::path::{Path, PathBuf};

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use openrailsrs_ace::AceFile;
use openrailsrs_formats::{
    ShapeFile, TerrainFile, WorldFile, parse_tile_xz_from_filename, read_f_raw_bytes,
    read_y_raw_bytes,
};
use serde_json::Value as JsonValue;

use crate::load_diagnostics::{MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics};

use super::types::{
    MstsAceAsset, MstsAssetError, MstsRouteCatalogAsset, MstsShapeAsset, MstsTerrainTileAsset,
    MstsTileBundleAsset, MstsWorldTileAsset, TerrainRawStatus, TileBundleManifest,
    TileBundleStatus,
};

fn path_label(load_context: &LoadContext<'_>) -> String {
    load_context.path().to_string()
}

fn path_buf(load_context: &LoadContext<'_>) -> PathBuf {
    PathBuf::from(load_context.path().path())
}

fn sibling_asset_path(load_context: &LoadContext<'_>, file_name: &str) -> PathBuf {
    let parent = load_context
        .path()
        .path()
        .parent()
        .unwrap_or_else(|| Path::new(""));
    parent.join(file_name)
}

#[derive(Default, TypePath)]
pub struct MstsShapeAssetLoader;

impl AssetLoader for MstsShapeAssetLoader {
    type Asset = MstsShapeAsset;
    type Settings = ();
    type Error = MstsAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let path = path_label(load_context);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|source| {
            MstsAssetError::Io {
                path: path.clone(),
                source,
            }
        })?;
        let shape = ShapeFile::from_bytes(&bytes).map_err(|e| MstsAssetError::ShapeParse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        Ok(MstsShapeAsset {
            shape,
            source_path: path_buf(load_context),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["s", "S"]
    }
}

#[derive(Default, TypePath)]
pub struct MstsAceAssetLoader;

impl AssetLoader for MstsAceAssetLoader {
    type Asset = MstsAceAsset;
    type Settings = ();
    type Error = MstsAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let path = path_label(load_context);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|source| {
            MstsAssetError::Io {
                path: path.clone(),
                source,
            }
        })?;
        let ace = AceFile::read_bytes(&bytes).map_err(|e| MstsAssetError::AceDecode {
            path: path.clone(),
            message: e.to_string(),
        })?;
        Ok(MstsAceAsset {
            ace,
            source_path: path_buf(load_context),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["ace", "ACE"]
    }
}

#[derive(Default, TypePath)]
pub struct MstsWorldTileAssetLoader;

impl AssetLoader for MstsWorldTileAssetLoader {
    type Asset = MstsWorldTileAsset;
    type Settings = ();
    type Error = MstsAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let path = path_label(load_context);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|source| {
            MstsAssetError::Io {
                path: path.clone(),
                source,
            }
        })?;
        let hint = Path::new(load_context.path().path());
        let world =
            WorldFile::from_bytes(&bytes, Some(hint)).map_err(|e| MstsAssetError::WorldParse {
                path: path.clone(),
                message: e.to_string(),
            })?;
        Ok(MstsWorldTileAsset {
            tile_x: world.tile_x,
            tile_z: world.tile_z,
            world,
            source_path: path_buf(load_context),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["w", "W"]
    }
}

#[derive(Default, TypePath)]
pub struct MstsRouteCatalogLoader;

impl AssetLoader for MstsRouteCatalogLoader {
    type Asset = MstsRouteCatalogAsset;
    type Settings = ();
    type Error = MstsAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let path = path_label(load_context);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|source| {
            MstsAssetError::Io {
                path: path.clone(),
                source,
            }
        })?;
        let value: JsonValue =
            serde_json::from_slice(&bytes).map_err(|e| MstsAssetError::CatalogParse {
                path: path.clone(),
                message: e.to_string(),
            })?;
        let catalog: MstsRouteCatalogAsset =
            serde_json::from_value(value).map_err(|e| MstsAssetError::CatalogParse {
                path: path.clone(),
                message: e.to_string(),
            })?;
        Ok(catalog)
    }

    fn extensions(&self) -> &[&str] {
        &["routecat"]
    }
}

#[derive(Default, TypePath)]
pub struct MstsTerrainTileAssetLoader;

impl AssetLoader for MstsTerrainTileAssetLoader {
    type Asset = MstsTerrainTileAsset;
    type Settings = ();
    type Error = MstsAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let path = path_label(load_context);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|source| {
            MstsAssetError::Io {
                path: path.clone(),
                source,
            }
        })?;
        let hint = Path::new(load_context.path().path());
        let (tx, tz) = parse_tile_xz_from_filename(hint).unwrap_or((0, 0));
        let terrain = TerrainFile::from_bytes(&bytes, Some(hint), tx, tz).map_err(|e| {
            MstsAssetError::TerrainParse {
                path: path.clone(),
                message: e.to_string(),
            }
        })?;

        let mut diag = MstsLoadDiagnostics::default();
        diag.record_loaded(MstsAssetKind::Terrain);

        let y_name = terrain.samples.y_buffer_file.trim().to_string();
        let f_name = terrain.samples.f_buffer_file.trim().to_string();
        let y_rel = if y_name.is_empty() {
            None
        } else {
            Some(sibling_asset_path(load_context, &y_name))
        };
        let f_rel = if f_name.is_empty() {
            None
        } else {
            Some(sibling_asset_path(load_context, &f_name))
        };

        let mut elevation = None;
        let mut y_missing = !y_name.is_empty();
        if let Some(y_path) = y_rel.clone() {
            let y_key = y_path.to_string_lossy().into_owned();
            match load_context.read_asset_bytes(y_key.clone()).await {
                Ok(raw) => match read_y_raw_bytes(&raw, &terrain.samples) {
                    Ok(grid) => {
                        elevation = Some(grid);
                        y_missing = false;
                        diag.record_loaded(MstsAssetKind::Terrain);
                    }
                    Err(e) => {
                        diag.record_failed_at(
                            y_key,
                            MstsAssetKind::Terrain,
                            MstsLoadCause::Parse,
                            e.to_string(),
                            Some(terrain.tile_x),
                            Some(terrain.tile_z),
                        );
                    }
                },
                Err(_) => {
                    diag.record_failed_at(
                        y_key,
                        MstsAssetKind::Terrain,
                        MstsLoadCause::Missing,
                        "Y.RAW missing",
                        Some(terrain.tile_x),
                        Some(terrain.tile_z),
                    );
                }
            }
        } else {
            y_missing = true;
            diag.record_failed_at(
                path.clone(),
                MstsAssetKind::Terrain,
                MstsLoadCause::Missing,
                "terrain_sample_ybuffer empty",
                Some(terrain.tile_x),
                Some(terrain.tile_z),
            );
        }

        let mut features = None;
        let mut f_missing = false;
        if let Some(f_path) = f_rel.clone() {
            f_missing = true;
            let f_key = f_path.to_string_lossy().into_owned();
            match load_context.read_asset_bytes(f_key.clone()).await {
                Ok(raw) => match read_f_raw_bytes(raw, &terrain.samples) {
                    Ok(grid) => {
                        features = Some(grid);
                        f_missing = false;
                        diag.record_loaded(MstsAssetKind::Terrain);
                    }
                    Err(e) => {
                        diag.record_failed_at(
                            f_key,
                            MstsAssetKind::Terrain,
                            MstsLoadCause::Parse,
                            e.to_string(),
                            Some(terrain.tile_x),
                            Some(terrain.tile_z),
                        );
                    }
                },
                Err(_) => {
                    diag.record_failed_at(
                        f_key,
                        MstsAssetKind::Terrain,
                        MstsLoadCause::Missing,
                        "F.RAW missing",
                        Some(terrain.tile_x),
                        Some(terrain.tile_z),
                    );
                }
            }
        }

        let raw_status = match (y_missing, f_missing) {
            (false, false) => TerrainRawStatus::Complete,
            (true, false) => TerrainRawStatus::MissingY,
            (false, true) => TerrainRawStatus::MissingF,
            (true, true) => TerrainRawStatus::MissingBoth,
        };

        Ok(MstsTerrainTileAsset {
            tile_x: terrain.tile_x,
            tile_z: terrain.tile_z,
            terrain,
            source_path: path_buf(load_context),
            y_raw_path: y_rel,
            f_raw_path: f_rel,
            elevation,
            features,
            raw_status,
            diag,
        })
    }

    fn extensions(&self) -> &[&str] {
        &["y", "Y", "t", "T"]
    }
}

#[derive(Default, TypePath)]
pub struct MstsTileBundleLoader;

impl AssetLoader for MstsTileBundleLoader {
    type Asset = MstsTileBundleAsset;
    type Settings = ();
    type Error = MstsAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let path = path_label(load_context);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|source| {
            MstsAssetError::Io {
                path: path.clone(),
                source,
            }
        })?;
        let manifest: TileBundleManifest =
            serde_json::from_slice(&bytes).map_err(|e| MstsAssetError::BundleParse {
                path: path.clone(),
                message: e.to_string(),
            })?;

        let mut diag = MstsLoadDiagnostics::default();
        let world_path = manifest.world.clone();
        let terrain_path = manifest.terrain.clone();
        let world = match world_path {
            Some(ref p) => {
                diag.record_loaded(MstsAssetKind::World);
                Some(load_context.load::<MstsWorldTileAsset>(p.clone()))
            }
            None => {
                diag.record_failed_at(
                    path.clone(),
                    MstsAssetKind::World,
                    MstsLoadCause::Missing,
                    "world path omitted in tilebundle",
                    Some(manifest.tile_x),
                    Some(manifest.tile_z),
                );
                None
            }
        };

        let mut terrain_raw_status = None;
        let terrain = match terrain_path {
            Some(ref p) => {
                // Immediate load so missing RAW is reflected in bundle status (#53).
                match load_context
                    .load_builder()
                    .load_value::<MstsTerrainTileAsset>(p.clone())
                    .await
                {
                    Ok(loaded) => {
                        let asset = loaded.take();
                        terrain_raw_status = Some(asset.raw_status);
                        diag.merge_from(&asset.diag);
                        // Keep a shared Handle for consumers / dependency tracking.
                        Some(load_context.load::<MstsTerrainTileAsset>(p.clone()))
                    }
                    Err(e) => {
                        diag.record_failed_at(
                            p.clone(),
                            MstsAssetKind::Terrain,
                            MstsLoadCause::Parse,
                            e.to_string(),
                            Some(manifest.tile_x),
                            Some(manifest.tile_z),
                        );
                        None
                    }
                }
            }
            None => {
                diag.record_failed_at(
                    path.clone(),
                    MstsAssetKind::Terrain,
                    MstsLoadCause::Missing,
                    "terrain path omitted in tilebundle",
                    Some(manifest.tile_x),
                    Some(manifest.tile_z),
                );
                None
            }
        };

        let raw_ok = terrain_raw_status.is_some_and(|s| s.is_complete());
        let status = match (world.is_some(), terrain.is_some(), raw_ok) {
            (true, true, true) => TileBundleStatus::Ready,
            (false, false, _) => TileBundleStatus::Failed,
            _ => TileBundleStatus::Partial,
        };

        Ok(MstsTileBundleAsset {
            tile_x: manifest.tile_x,
            tile_z: manifest.tile_z,
            world,
            terrain,
            terrain_raw_status,
            status,
            diag,
            source_path: path_buf(load_context),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["tilebundle"]
    }
}
