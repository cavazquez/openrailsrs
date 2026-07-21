//! [`AssetLoader`] implementations for MSTS formats (#48).

use std::path::{Path, PathBuf};

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use openrailsrs_ace::AceFile;
use openrailsrs_formats::{ShapeFile, WorldFile};
use serde_json::Value as JsonValue;

use super::types::{
    MstsAceAsset, MstsAssetError, MstsRouteCatalogAsset, MstsShapeAsset, MstsWorldTileAsset,
};

fn path_label(load_context: &LoadContext<'_>) -> String {
    load_context.path().to_string()
}

fn path_buf(load_context: &LoadContext<'_>) -> PathBuf {
    PathBuf::from(load_context.path().path())
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
        // Accept JSON object; ignore unknown fields for forward compatibility.
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
