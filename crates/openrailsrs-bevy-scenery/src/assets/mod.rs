//! Bevy [`Asset`] / [`AssetLoader`] types for MSTS/Open Rails content (#48).
//!
//! v1 registers CPU-side assets and async loaders. Spawn pipelines may keep
//! using synchronous parsers; consumers can opt into `AssetServer::load`.

mod loaders;
mod types;

pub use loaders::{
    MstsAceAssetLoader, MstsRouteCatalogLoader, MstsShapeAssetLoader, MstsWorldTileAssetLoader,
};
pub use types::{
    MstsAceAsset, MstsAssetError, MstsRouteCatalogAsset, MstsShapeAsset, MstsWorldTileAsset,
};

use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::prelude::*;
use std::path::Path;

/// Registers MSTS asset types and loaders.
pub struct MstsAssetPlugin;

impl Plugin for MstsAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MstsShapeAsset>()
            .init_asset::<MstsAceAsset>()
            .init_asset::<MstsWorldTileAsset>()
            .init_asset::<MstsRouteCatalogAsset>()
            .init_asset_loader::<MstsShapeAssetLoader>()
            .init_asset_loader::<MstsAceAssetLoader>()
            .init_asset_loader::<MstsWorldTileAssetLoader>()
            .init_asset_loader::<MstsRouteCatalogLoader>();
    }
}

/// Register a filesystem [`AssetSource`] named `msts` rooted at `content_root`.
///
/// Call **before** adding [`AssetPlugin`] / `DefaultPlugins`. Paths load as
/// `msts://SHAPES/foo.s`.
pub fn register_msts_content_source(app: &mut App, content_root: impl AsRef<Path>) {
    let root = content_root.as_ref().to_string_lossy().into_owned();
    app.register_asset_source(
        AssetSourceId::from("msts"),
        AssetSourceBuilder::platform_default(&root, None),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::{AssetPlugin, LoadState};
    use std::time::Duration;

    fn msts_test_app() -> App {
        let mut app = App::new();
        // Fixtures live under this crate's `assets/msts/`.
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: crate::asset_root().to_string_lossy().into_owned(),
                ..default()
            })
            .add_plugins(MstsAssetPlugin);
        app
    }

    fn wait_loaded<A: Asset>(app: &mut App, handle: &Handle<A>, label: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let server = app.world().resource::<AssetServer>();
            match server.get_load_state(handle) {
                Some(LoadState::Loaded) => return,
                Some(LoadState::Failed(err)) => {
                    panic!("{label} failed to load: {err:?}");
                }
                Some(other) => {
                    if std::time::Instant::now() > deadline {
                        panic!("{label} timed out in state {other:?}");
                    }
                }
                None => {
                    if std::time::Instant::now() > deadline {
                        panic!("{label} has no load state");
                    }
                }
            }
        }
    }

    #[test]
    fn loads_shape_ace_world_and_catalog_fixtures() {
        let mut app = msts_test_app();
        let server = app.world().resource::<AssetServer>().clone();

        let shape: Handle<MstsShapeAsset> = server.load("msts/minimal.s");
        let ace: Handle<MstsAceAsset> = server.load("msts/test.ace");
        let world: Handle<MstsWorldTileAsset> = server.load("msts/w-001000-001000.w");
        let catalog: Handle<MstsRouteCatalogAsset> = server.load("msts/test.routecat");

        wait_loaded(&mut app, &shape, "shape");
        wait_loaded(&mut app, &ace, "ace");
        wait_loaded(&mut app, &world, "world");
        wait_loaded(&mut app, &catalog, "catalog");

        let shapes = app.world().resource::<Assets<MstsShapeAsset>>();
        let shape_asset = shapes.get(&shape).expect("shape asset");
        assert!(
            !shape_asset.shape.texture_filenames.is_empty(),
            "minimal.s should list textures"
        );

        let aces = app.world().resource::<Assets<MstsAceAsset>>();
        let ace_asset = aces.get(&ace).expect("ace asset");
        assert_eq!(ace_asset.ace.width, 1);
        assert_eq!(ace_asset.ace.height, 1);

        let worlds = app.world().resource::<Assets<MstsWorldTileAsset>>();
        let world_asset = worlds.get(&world).expect("world asset");
        assert_eq!(world_asset.tile_x, -1000);
        assert_eq!(world_asset.tile_z, -1000);
        assert!(!world_asset.world.items.is_empty());

        let catalogs = app.world().resource::<Assets<MstsRouteCatalogAsset>>();
        let cat = catalogs.get(&catalog).expect("catalog asset");
        assert_eq!(cat.shapes, vec!["minimal.s".to_string()]);
        assert_eq!(cat.textures, vec!["test.ace".to_string()]);
        assert_eq!(cat.world_tiles, vec!["w-001000-001000.w".to_string()]);

        // Shared handles: second load of the same path reuses the asset id.
        let shape2: Handle<MstsShapeAsset> = server.load("msts/minimal.s");
        assert_eq!(shape.id(), shape2.id());
    }

    #[test]
    fn shape_parse_error_includes_path_and_cause() {
        let mut app = msts_test_app();
        // Write a corrupt fixture next to assets for this test.
        let bad = crate::asset_root().join("msts/_corrupt_test.s");
        std::fs::write(&bad, b"not a valid shape").expect("write corrupt");
        let server = app.world().resource::<AssetServer>().clone();
        let handle: Handle<MstsShapeAsset> = server.load("msts/_corrupt_test.s");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let server = app.world().resource::<AssetServer>();
            match server.get_load_state(&handle) {
                Some(LoadState::Failed(err)) => {
                    let msg = format!("{err:?}");
                    let lower = msg.to_ascii_lowercase();
                    assert!(
                        lower.contains("shapeparse")
                            || lower.contains("parse")
                            || lower.contains("unexpected"),
                        "error should mention parse cause: {msg}"
                    );
                    assert!(
                        lower.contains("msts/_corrupt_test.s")
                            || lower.contains("corrupt_test"),
                        "error should include path: {msg}"
                    );
                    let _ = std::fs::remove_file(&bad);
                    return;
                }
                Some(LoadState::Loaded) => {
                    let _ = std::fs::remove_file(&bad);
                    panic!("corrupt shape should not load");
                }
                _ if std::time::Instant::now() > deadline => {
                    let _ = std::fs::remove_file(&bad);
                    panic!("timed out waiting for Failed");
                }
                _ => {}
            }
        }
    }
}
