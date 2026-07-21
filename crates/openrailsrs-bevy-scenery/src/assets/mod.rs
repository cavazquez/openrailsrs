//! Bevy [`Asset`] / [`AssetLoader`] types for MSTS/Open Rails content (#48, #53).
//!
//! v1 registers CPU-side assets and async loaders. Spawn pipelines may keep
//! using synchronous parsers; consumers can opt into `AssetServer::load`.
//! Composite [`MstsTileBundleAsset`] exposes WORLD+terrain lifecycle per tile.

mod loaders;
mod types;

pub use loaders::{
    MstsAceAssetLoader, MstsRouteCatalogLoader, MstsShapeAssetLoader, MstsTerrainTileAssetLoader,
    MstsTileBundleLoader, MstsWorldTileAssetLoader,
};
pub use types::{
    MstsAceAsset, MstsAssetError, MstsRouteCatalogAsset, MstsShapeAsset, MstsTerrainTileAsset,
    MstsTileBundleAsset, MstsWorldTileAsset, TerrainRawStatus, TileBundleManifest,
    TileBundlePaths, TileBundleStatus, discover_tile_bundle_paths, write_tile_bundle_manifest,
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
            .init_asset::<MstsTerrainTileAsset>()
            .init_asset::<MstsTileBundleAsset>()
            .init_asset_loader::<MstsShapeAssetLoader>()
            .init_asset_loader::<MstsAceAssetLoader>()
            .init_asset_loader::<MstsWorldTileAssetLoader>()
            .init_asset_loader::<MstsRouteCatalogLoader>()
            .init_asset_loader::<MstsTerrainTileAssetLoader>()
            .init_asset_loader::<MstsTileBundleLoader>();
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

    #[test]
    fn tile_bundle_complete_is_ready() {
        let mut app = msts_test_app();
        let server = app.world().resource::<AssetServer>().clone();
        let handle: Handle<MstsTileBundleAsset> =
            server.load("msts/tiles/complete/complete.tilebundle");
        wait_loaded(&mut app, &handle, "complete.tilebundle");

        let bundles = app.world().resource::<Assets<MstsTileBundleAsset>>();
        let bundle = bundles.get(&handle).expect("bundle");
        assert_eq!(bundle.tile_x, -1000);
        assert_eq!(bundle.tile_z, -1000);
        assert_eq!(bundle.status, TileBundleStatus::Ready);
        assert_eq!(bundle.terrain_raw_status, Some(TerrainRawStatus::Complete));
        assert!(bundle.world.is_some());
        assert!(bundle.terrain.is_some());

        let world_h = bundle.world.clone().unwrap();
        let terr_h = bundle.terrain.clone().unwrap();
        wait_loaded(&mut app, &world_h, "bundle world");
        wait_loaded(&mut app, &terr_h, "bundle terrain");

        let worlds = app.world().resource::<Assets<MstsWorldTileAsset>>();
        assert!(!worlds.get(&world_h).unwrap().world.items.is_empty());
        let terrains = app.world().resource::<Assets<MstsTerrainTileAsset>>();
        let terr = terrains.get(&terr_h).unwrap();
        assert!(terr.elevation.is_some());
        assert_eq!(terr.raw_status, TerrainRawStatus::Complete);
    }

    #[test]
    fn tile_bundle_missing_raw_is_partial_with_diag() {
        let mut app = msts_test_app();
        let server = app.world().resource::<AssetServer>().clone();
        let handle: Handle<MstsTileBundleAsset> =
            server.load("msts/tiles/missing_raw/missing_raw.tilebundle");
        wait_loaded(&mut app, &handle, "missing_raw.tilebundle");

        let bundles = app.world().resource::<Assets<MstsTileBundleAsset>>();
        let bundle = bundles.get(&handle).expect("bundle");
        assert_eq!(bundle.status, TileBundleStatus::Partial);
        assert!(
            matches!(
                bundle.terrain_raw_status,
                Some(TerrainRawStatus::MissingY | TerrainRawStatus::MissingBoth)
            ),
            "expected missing Y RAW, got {:?}",
            bundle.terrain_raw_status
        );
        assert!(bundle.world.is_some(), "world must still load");
        assert!(
            !bundle.diag.failures.is_empty() || bundle.diag.failed > 0,
            "missing RAW must be recorded in diagnostics"
        );

        let world_h = bundle.world.clone().unwrap();
        wait_loaded(&mut app, &world_h, "missing_raw world");
        let worlds = app.world().resource::<Assets<MstsWorldTileAsset>>();
        assert!(!worlds.get(&world_h).unwrap().world.items.is_empty());
    }

    #[test]
    fn tile_bundle_unload_drops_asset() {
        let mut app = msts_test_app();
        let server = app.world().resource::<AssetServer>().clone();
        let handle: Handle<MstsTileBundleAsset> =
            server.load("msts/tiles/complete/complete.tilebundle");
        wait_loaded(&mut app, &handle, "complete for unload");
        let id = handle.id();
        assert!(app
            .world()
            .resource::<Assets<MstsTileBundleAsset>>()
            .get(id)
            .is_some());

        drop(handle);
        // Allow Bevy to process dropped strong handles.
        for _ in 0..8 {
            app.update();
        }
        assert!(
            app.world()
                .resource::<Assets<MstsTileBundleAsset>>()
                .get(id)
                .is_none(),
            "bundle asset should unload when strong handles are dropped"
        );
    }
}
