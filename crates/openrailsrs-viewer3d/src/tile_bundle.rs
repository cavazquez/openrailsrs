//! AssetServer-backed tile streaming for viewer3d (#111).
//!
//! Hot path: request `.tilebundle` → wait Ready/Partial → materialize into
//! [`crate::world::WorldScene`] / [`crate::terrain::TerrainScene`].
//!
//! Bootstrap (`load_world_from_route_dir_*` / `load_terrain_from_route_dir_*`) still
//! parses synchronously via `from_path`; migrating that is a follow-up.
//!
//! TODO(#112): build scenes from [`openrailsrs_bevy_scenery::MstsTileSnapshot`]
//! (classified items + terrain RAW) instead of re-walking `WorldItem` /
//! `TerrainFile` in app-local helpers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bevy::asset::LoadState;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics, MstsTerrainTileAsset, MstsTileBundleAsset,
    MstsWorldTileAsset, TileBundleManifest, TileBundleStatus, discover_tile_bundle_paths,
    write_tile_bundle_manifest,
};
use openrailsrs_formats::msts_tile_world_origin;

use crate::terrain::{TerrainScene, TerrainTile};
use crate::terrain_io::TerrainTileData;
use crate::world::{WorldItemWindow, WorldScene, append_world_tile};

/// Strong handles for tiles loaded via AssetServer (exact unload by dropping).
#[derive(Resource, Default)]
pub struct TileBundleHandles {
    pub by_tile: HashMap<(i32, i32), Handle<MstsTileBundleAsset>>,
}

impl TileBundleHandles {
    pub fn insert(&mut self, tile_x: i32, tile_z: i32, handle: Handle<MstsTileBundleAsset>) {
        self.by_tile.insert((tile_x, tile_z), handle);
    }

    pub fn get(&self, tile_x: i32, tile_z: i32) -> Option<&Handle<MstsTileBundleAsset>> {
        self.by_tile.get(&(tile_x, tile_z))
    }

    /// Drop the strong handle for this tile (AssetServer may GC unused deps).
    pub fn release(&mut self, tile_x: i32, tile_z: i32) -> bool {
        self.by_tile.remove(&(tile_x, tile_z)).is_some()
    }

    pub fn release_all<'a, I>(&mut self, keys: I)
    where
        I: IntoIterator<Item = &'a (i32, i32)>,
    {
        for key in keys {
            self.by_tile.remove(key);
        }
    }
}

/// Kick off an AssetServer load of a `.tilebundle` path (asset-root relative or absolute).
pub fn request_tile_bundle(
    server: &AssetServer,
    asset_path: impl Into<String>,
) -> Handle<MstsTileBundleAsset> {
    server.load(asset_path.into())
}

/// Discover WORLD/terrain for `(tile_x, tile_z)`, write a `.tilebundle` under the route,
/// and request it via AssetServer.
///
/// Manifest paths are absolute so nested loaders resolve outside the shared asset root
/// (viewer uses [`UnapprovedPathMode::Allow`] — see `viewer_asset_plugin`).
pub fn request_route_tile_bundle(
    server: &AssetServer,
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
    world_path: Option<&Path>,
    terrain_path: Option<&Path>,
) -> Option<Handle<MstsTileBundleAsset>> {
    let discovered = discover_tile_bundle_paths(route_dir, tile_x, tile_z);
    let world = world_path
        .map(Path::to_path_buf)
        .or(discovered.world);
    let terrain = terrain_path
        .map(Path::to_path_buf)
        .or(discovered.terrain);
    if world.is_none() && terrain.is_none() {
        return None;
    }

    let manifest = TileBundleManifest {
        tile_x,
        tile_z,
        world: world
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        terrain: terrain
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    };
    let out_dir = route_dir.join(".openrailsrs").join("tilebundles");
    let out_path = out_dir.join(format!("{tile_x}_{tile_z}.tilebundle"));
    if write_tile_bundle_manifest(&out_path, &manifest).is_err() {
        return None;
    }
    Some(request_tile_bundle(
        server,
        out_path.to_string_lossy().into_owned(),
    ))
}

/// Ensure a handle exists in [`TileBundleHandles`] for this tile (reuse if present).
pub fn ensure_tile_bundle_handle(
    handles: &mut TileBundleHandles,
    server: &AssetServer,
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
    world_path: Option<&Path>,
    terrain_path: Option<&Path>,
) -> Option<Handle<MstsTileBundleAsset>> {
    if let Some(h) = handles.get(tile_x, tile_z) {
        return Some(h.clone());
    }
    let handle =
        request_route_tile_bundle(server, route_dir, tile_x, tile_z, world_path, terrain_path)?;
    handles.insert(tile_x, tile_z, handle.clone());
    Some(handle)
}

/// Whether a bundle asset and its declared deps are present in `Assets` stores.
pub fn bundle_deps_ready(
    bundle: &MstsTileBundleAsset,
    worlds: &Assets<MstsWorldTileAsset>,
    terrains: &Assets<MstsTerrainTileAsset>,
) -> bool {
    if bundle.status == TileBundleStatus::Failed {
        return true;
    }
    if bundle
        .world
        .as_ref()
        .is_some_and(|h| worlds.get(h).is_none())
    {
        return false;
    }
    if bundle
        .terrain
        .as_ref()
        .is_some_and(|h| terrains.get(h).is_none())
    {
        return false;
    }
    true
}

/// Append WORLD items from a loaded world-tile asset (#111).
pub fn materialize_world_from_asset(
    scene: &mut WorldScene,
    world_asset: &MstsWorldTileAsset,
    item_window: Option<WorldItemWindow>,
) {
    append_world_tile(scene, &world_asset.world, item_window);
    scene
        .load_diag
        .record_path_loaded(&world_asset.source_path, MstsAssetKind::World);
}

/// Build a [`TerrainTile`] from a loaded terrain asset (elevation may be absent → Partial).
pub fn terrain_tile_from_asset(terr: &MstsTerrainTileAsset) -> TerrainTile {
    let data = terr.elevation.as_ref().map(|grid| {
        Arc::new(TerrainTileData {
            grid: Arc::new(grid.clone()),
            features: terr.features.clone().map(Arc::new),
        })
    });
    let (wx, wz) = msts_tile_world_origin(terr.tile_x, terr.tile_z);
    TerrainTile {
        tile_x: terr.tile_x,
        tile_z: terr.tile_z,
        translation: Vec3::new(wx, 0.0, wz),
        path: terr.source_path.clone(),
        file: terr.terrain.clone(),
        data,
    }
}

/// Append a terrain tile from a loaded terrain asset; merge loader diagnostics (#54).
pub fn materialize_terrain_from_asset(scene: &mut TerrainScene, terr: &MstsTerrainTileAsset) {
    if scene
        .tiles
        .iter()
        .any(|t| t.tile_x == terr.tile_x && t.tile_z == terr.tile_z)
    {
        return;
    }
    scene.load_diag.merge_from(&terr.diag);
    if terr.elevation.is_some() {
        scene
            .load_diag
            .record_path_loaded(&terr.source_path, MstsAssetKind::Terrain);
    }
    scene.tiles.push(terrain_tile_from_asset(terr));
    scene.tiles_loaded = scene.tiles.len();
}

/// Materialize WORLD side of a Ready/Partial bundle. Returns `true` when the world
/// side was applied (or there is no world handle to wait for).
pub fn try_materialize_world_bundle(
    bundle: &MstsTileBundleAsset,
    worlds: &Assets<MstsWorldTileAsset>,
    terrains: &Assets<MstsTerrainTileAsset>,
    scene: &mut WorldScene,
    item_window: Option<WorldItemWindow>,
) -> bool {
    if bundle.status == TileBundleStatus::Failed {
        scene.load_diag.merge_from(&bundle.diag);
        return true;
    }
    if !bundle_deps_ready(bundle, worlds, terrains) {
        return false;
    }
    scene.load_diag.merge_from(&bundle.diag);
    if let Some(h) = bundle.world.as_ref() {
        if let Some(w) = worlds.get(h) {
            materialize_world_from_asset(scene, w, item_window);
        } else {
            scene.load_diag.record_failed_at(
                bundle.source_path.display().to_string(),
                MstsAssetKind::World,
                MstsLoadCause::Missing,
                "world handle missing after bundle load",
                Some(bundle.tile_x),
                Some(bundle.tile_z),
            );
        }
    }
    true
}

/// Materialize terrain side of a Ready/Partial bundle.
///
/// Returns `Some` when a terrain tile was appended. Returns `None` when deps are
/// still loading, the bundle failed, or the bundle has no terrain component.
pub fn try_materialize_terrain_bundle(
    bundle: &MstsTileBundleAsset,
    worlds: &Assets<MstsWorldTileAsset>,
    terrains: &Assets<MstsTerrainTileAsset>,
    scene: &mut TerrainScene,
) -> Option<TerrainTile> {
    if bundle.status == TileBundleStatus::Failed {
        scene.load_diag.merge_from(&bundle.diag);
        return None;
    }
    if !bundle_deps_ready(bundle, worlds, terrains) {
        return None;
    }
    scene.load_diag.merge_from(&bundle.diag);
    let Some(h) = bundle.terrain.as_ref() else {
        return None;
    };
    let Some(terr) = terrains.get(h) else {
        scene.load_diag.record_failed_at(
            bundle.source_path.display().to_string(),
            MstsAssetKind::Terrain,
            MstsLoadCause::Missing,
            "terrain handle missing after bundle load",
            Some(bundle.tile_x),
            Some(bundle.tile_z),
        );
        return None;
    };
    materialize_terrain_from_asset(scene, terr);
    scene
        .tiles
        .iter()
        .find(|t| t.tile_x == terr.tile_x && t.tile_z == terr.tile_z)
        .cloned()
}

/// Poll AssetServer load state for a pending tile bundle handle.
pub fn tile_bundle_load_outcome(
    server: &AssetServer,
    handle: &Handle<MstsTileBundleAsset>,
) -> TileBundleLoadOutcome {
    match server.get_load_state(handle) {
        Some(LoadState::Loaded) => TileBundleLoadOutcome::Loaded,
        Some(LoadState::Failed(_)) => TileBundleLoadOutcome::Failed,
        _ => TileBundleLoadOutcome::Pending,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileBundleLoadOutcome {
    Pending,
    Loaded,
    Failed,
}

/// Record a failed AssetServer load into scene diagnostics (#54 / #78).
pub fn record_bundle_load_failure(diag: &mut MstsLoadDiagnostics, tile_x: i32, tile_z: i32, path: &str) {
    diag.record_failed_at(
        path.to_string(),
        MstsAssetKind::World,
        MstsLoadCause::Parse,
        "tilebundle AssetServer load failed",
        Some(tile_x),
        Some(tile_z),
    );
}

/// Asset plugin for viewer3d: shared scenery root + allow absolute route paths in manifests.
pub fn viewer_asset_plugin() -> bevy::asset::AssetPlugin {
    bevy::asset::AssetPlugin {
        file_path: openrailsrs_bevy_scenery::asset_root()
            .to_string_lossy()
            .into_owned(),
        unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::{AssetPlugin, LoadState};
    use openrailsrs_bevy_scenery::{MstsAssetPlugin, TerrainRawStatus, TileBundleStatus};
    use std::time::Duration;

    fn wait_loaded<A: Asset>(app: &mut App, handle: &Handle<A>, label: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let server = app.world().resource::<AssetServer>();
            match server.get_load_state(handle) {
                Some(LoadState::Loaded) => return,
                Some(LoadState::Failed(err)) => panic!("{label} failed: {err:?}"),
                Some(other) if std::time::Instant::now() > deadline => {
                    panic!("{label} timed out in {other:?}")
                }
                None if std::time::Instant::now() > deadline => {
                    panic!("{label} has no load state")
                }
                _ => {}
            }
        }
    }

    fn fixture_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin {
                file_path: openrailsrs_bevy_scenery::asset_root()
                    .to_string_lossy()
                    .into_owned(),
                unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
                ..default()
            })
            .add_plugins(MstsAssetPlugin)
            .init_resource::<TileBundleHandles>();
        app
    }

    #[test]
    fn stream_materializes_complete_and_missing_raw_fixtures() {
        let mut app = fixture_app();
        let server = app.world().resource::<AssetServer>().clone();
        let complete = request_tile_bundle(&server, "msts/tiles/complete/complete.tilebundle");
        let missing =
            request_tile_bundle(&server, "msts/tiles/missing_raw/missing_raw.tilebundle");
        wait_loaded(&mut app, &complete, "complete");
        wait_loaded(&mut app, &missing, "missing_raw");

        {
            let mut handles = app.world_mut().resource_mut::<TileBundleHandles>();
            handles.insert(-1000, -1000, complete.clone());
            handles.insert(-1001, -1000, missing.clone());
        }

        let bundles = app.world().resource::<Assets<MstsTileBundleAsset>>();
        let c = bundles.get(&complete).unwrap().clone();
        let m = bundles.get(&missing).unwrap().clone();
        assert_eq!(c.status, TileBundleStatus::Ready);
        assert_eq!(c.terrain_raw_status, Some(TerrainRawStatus::Complete));
        assert_eq!(m.status, TileBundleStatus::Partial);

        // Wait nested world/terrain assets.
        if let Some(h) = c.world.as_ref() {
            wait_loaded(&mut app, h, "complete world");
        }
        if let Some(h) = c.terrain.as_ref() {
            wait_loaded(&mut app, h, "complete terrain");
        }
        if let Some(h) = m.world.as_ref() {
            wait_loaded(&mut app, h, "missing world");
        }
        if let Some(h) = m.terrain.as_ref() {
            wait_loaded(&mut app, h, "missing terrain");
        }

        let worlds = app.world().resource::<Assets<MstsWorldTileAsset>>();
        let terrains = app.world().resource::<Assets<MstsTerrainTileAsset>>();
        let bundles = app.world().resource::<Assets<MstsTileBundleAsset>>();
        let c = bundles.get(&complete).unwrap();
        let m = bundles.get(&missing).unwrap();

        let mut world_scene = WorldScene::default();
        assert!(try_materialize_world_bundle(
            c,
            worlds,
            terrains,
            &mut world_scene,
            None
        ));
        assert!(
            !world_scene.items.is_empty(),
            "complete fixture must yield world objects"
        );
        assert!(world_scene.load_diag.loaded > 0 || world_scene.tiles_loaded > 0);

        let mut terrain_scene = TerrainScene::default();
        let tile = try_materialize_terrain_bundle(c, worlds, terrains, &mut terrain_scene)
            .expect("complete terrain");
        assert!(tile.data.is_some(), "complete RAW elevation present");

        let mut world_partial = WorldScene::default();
        assert!(try_materialize_world_bundle(
            m,
            worlds,
            terrains,
            &mut world_partial,
            None
        ));
        assert!(!world_partial.items.is_empty());
        let mut terrain_partial = TerrainScene::default();
        let partial_tile =
            try_materialize_terrain_bundle(m, worlds, terrains, &mut terrain_partial);
        // Missing Y.RAW → Partial; terrain asset may still materialize without elevation.
        if let Some(t) = partial_tile {
            assert!(t.data.is_none(), "missing RAW should omit elevation grid");
        }
        assert!(
            !m.diag.failures.is_empty() || m.diag.failed > 0,
            "missing RAW must propagate diagnostics (#54)"
        );

        app.world_mut()
            .resource_mut::<TileBundleHandles>()
            .release(-1000, -1000);
        assert!(!app
            .world()
            .resource::<TileBundleHandles>()
            .by_tile
            .contains_key(&(-1000, -1000)));
    }
}
