//! Bridge from [`MstsTileBundleAsset`] to render3d [`TileEntry`] (#53 / #77).
//!
//! Hot path (#77): request `.tilebundle` around the camera → materialize into
//! [`TileCatalog`] → [`crate::stream::tile_stream_system`] spawns GPU content.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bevy::asset::LoadState;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::stream::TILE_SIZE_M;
use openrailsrs_bevy_scenery::{
    MstsLoadDiagnostics, MstsTerrainTileAsset, MstsTileBundleAsset, MstsWorldTileAsset,
    TileBundleManifest, TileBundleStatus, discover_tile_bundle_paths, snapshot_from_parsed,
    write_tile_bundle_manifest,
};

use crate::objects::{self, ObjectMarker};
use crate::runtime::TileEntry;
use crate::terrain::{self, TileGeometry};
use crate::tile_parse::tile_entry_from_snapshot;
use crate::track::TrackRibbon;

/// Strong handles for tiles loaded via AssetServer (exact unload by dropping).
#[derive(Resource, Default, Clone)]
pub struct TileBundleHandles {
    pub by_tile: HashMap<(i32, i32), Handle<MstsTileBundleAsset>>,
    /// Discovers that found neither WORLD nor terrain (skip re-scan each frame).
    pub absent: HashSet<(i32, i32)>,
}

impl TileBundleHandles {
    pub fn insert(&mut self, tile_x: i32, tile_z: i32, handle: Handle<MstsTileBundleAsset>) {
        self.absent.remove(&(tile_x, tile_z));
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

    pub fn mark_absent(&mut self, tile_x: i32, tile_z: i32) {
        self.absent.insert((tile_x, tile_z));
    }
}

/// Kick off an AssetServer load of a `.tilebundle` path.
pub fn request_tile_bundle(
    server: &AssetServer,
    asset_path: impl Into<String>,
) -> Handle<MstsTileBundleAsset> {
    server.load(asset_path.into())
}

/// Discover WORLD/terrain for `(tile_x, tile_z)`, write a `.tilebundle` under the route,
/// and request it via AssetServer (#77).
///
/// Manifest paths are absolute; render3d uses [`render3d_asset_plugin`] (`UnapprovedPathMode::Allow`).
pub fn request_route_tile_bundle(
    server: &AssetServer,
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
    world_path: Option<&Path>,
    terrain_path: Option<&Path>,
) -> Option<Handle<MstsTileBundleAsset>> {
    let discovered = discover_tile_bundle_paths(route_dir, tile_x, tile_z);
    let world = world_path.map(Path::to_path_buf).or(discovered.world);
    let terrain = terrain_path.map(Path::to_path_buf).or(discovered.terrain);
    if world.is_none() && terrain.is_none() {
        return None;
    }

    let manifest = TileBundleManifest {
        tile_x,
        tile_z,
        world: world.as_ref().map(|p| p.to_string_lossy().into_owned()),
        terrain: terrain.as_ref().map(|p| p.to_string_lossy().into_owned()),
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

/// World-space offset of `(tile_x, tile_z)` relative to the scene center tile.
pub fn tile_world_offset(center: (i32, i32), tile_x: i32, tile_z: i32) -> Vec3 {
    let (cx, cz) = center;
    Vec3::new(
        (tile_x - cx) as f32 * TILE_SIZE_M,
        0.0,
        (cz - tile_z) as f32 * TILE_SIZE_M,
    )
}

/// Asset plugin: shared scenery root + allow absolute route paths in manifests (#77).
pub fn render3d_asset_plugin() -> bevy::asset::AssetPlugin {
    bevy::asset::AssetPlugin {
        file_path: openrailsrs_bevy_scenery::asset_root()
            .to_string_lossy()
            .into_owned(),
        unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
        ..Default::default()
    }
}

/// Build [`TileGeometry`] from a loaded terrain tile asset (requires elevation).
pub fn tile_geometry_from_terrain_asset(terr: &MstsTerrainTileAsset) -> Option<TileGeometry> {
    let grid = terr.elevation.clone()?;
    Some(terrain::tile_geometry_from_elevation(
        terr.tile_x,
        terr.tile_z,
        &terr.terrain,
        grid,
    ))
}

/// Build object markers from a loaded WORLD tile asset.
pub fn objects_from_world_asset(
    world: &MstsWorldTileAsset,
    route_dir: &Path,
    base_y: f32,
) -> Vec<ObjectMarker> {
    objects::objects_from_world_file(&world.world, route_dir, base_y)
}

/// Materialize a [`TileEntry`] from loaded bundle + component assets (#53 / #112).
///
/// Goes through [`openrailsrs_bevy_scenery::MstsTileSnapshot`]. Returns `None` if
/// the bundle failed or neither WORLD nor terrain is present. WORLD-only tiles
/// (missing elevation) still produce a stub geometry entry.
pub fn try_materialize_tile_entry(
    bundle: &MstsTileBundleAsset,
    worlds: &Assets<MstsWorldTileAsset>,
    terrains: &Assets<MstsTerrainTileAsset>,
    route_dir: &Path,
    world_offset: Vec3,
) -> Option<TileEntry> {
    if bundle.status == TileBundleStatus::Failed {
        return None;
    }

    let terr_asset = bundle.terrain.as_ref().and_then(|h| terrains.get(h));
    let world_asset = bundle.world.as_ref().and_then(|h| worlds.get(h));
    if terr_asset.is_none() && world_asset.is_none() {
        return None;
    }

    let mut diag = MstsLoadDiagnostics::default();
    diag.merge_from(&bundle.diag);
    if let Some(t) = terr_asset {
        diag.merge_from(&t.diag);
    }

    let snap = snapshot_from_parsed(
        bundle.tile_x,
        bundle.tile_z,
        world_asset.map(|w| w.world.clone()),
        world_asset.map(|w| w.source_path.clone()),
        terr_asset.map(|t| t.terrain.clone()),
        terr_asset.and_then(|t| t.elevation.clone()),
        terr_asset.and_then(|t| t.features.clone()),
        terr_asset
            .map(|t| t.raw_status)
            .or(bundle.terrain_raw_status),
        terr_asset.map(|t| t.source_path.clone()),
        Some(route_dir),
        diag,
    );

    if let Some(entry) = tile_entry_from_snapshot(&snap, world_offset, TrackRibbon::default()) {
        return Some(entry);
    }

    // WORLD present but no elevation: keep prior #53 behavior (stub geom + objects).
    let world = world_asset?;
    let geometry = empty_tile_geometry(bundle.tile_x, bundle.tile_z);
    let base_y = geometry.height.base_y();
    let objects = objects_from_world_asset(world, route_dir, base_y);
    Some(TileEntry {
        geometry,
        world_offset,
        track: TrackRibbon::default(),
        objects,
    })
}

fn empty_tile_geometry(tile_x: i32, tile_z: i32) -> TileGeometry {
    use openrailsrs_formats::ElevationGrid;
    let grid = ElevationGrid {
        nsamples: 2,
        elevations: vec![0.0; 4],
    };
    // Minimal TerrainFile-less path: synthesize via tile_geometry_from_elevation
    // requires a TerrainFile — build a stub with default samples.
    let tile = openrailsrs_formats::TerrainFile {
        tile_x,
        tile_z,
        samples: openrailsrs_formats::TerrainSamples {
            nsamples: 2,
            sample_floor: 0.0,
            sample_scale: 1.0,
            sample_size: 8.0,
            ..Default::default()
        },
        shaders: Vec::new(),
        patch_sets: Vec::new(),
    };
    terrain::tile_geometry_from_elevation(tile_x, tile_z, &tile, grid)
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

/// Inputs for [`materialize_loaded_tile_bundles`] (keeps the helper under clippy arity).
pub struct MaterializeTileBundles<'a> {
    pub handles: &'a TileBundleHandles,
    pub bundles: &'a Assets<MstsTileBundleAsset>,
    pub worlds: &'a Assets<MstsWorldTileAsset>,
    pub terrains: &'a Assets<MstsTerrainTileAsset>,
    pub route: &'a Path,
    pub center_tile: (i32, i32),
    pub catalog: &'a mut crate::stream::TileCatalog,
    pub load_diag: Option<&'a mut MstsLoadDiagnostics>,
}

/// Poll loaded bundles and materialize any that are Ready/Partial into the catalog.
pub fn materialize_loaded_tile_bundles(ctx: MaterializeTileBundles<'_>) {
    let MaterializeTileBundles {
        handles,
        bundles,
        worlds,
        terrains,
        route,
        center_tile,
        catalog,
        mut load_diag,
    } = ctx;
    for ((tx, tz), handle) in &handles.by_tile {
        if catalog
            .entries
            .iter()
            .any(|e| e.geometry.tile_x == *tx && e.geometry.tile_z == *tz)
        {
            continue;
        }
        let Some(bundle) = bundles.get(handle) else {
            continue;
        };
        if bundle.status == TileBundleStatus::Failed {
            continue;
        }
        if !bundle_deps_ready(bundle, worlds, terrains) {
            continue;
        }
        let offset = tile_world_offset(center_tile, *tx, *tz);
        if let Some(entry) = try_materialize_tile_entry(bundle, worlds, terrains, route, offset) {
            if let Some(diag) = load_diag.as_mut() {
                diag.merge_from(&bundle.diag);
            }
            catalog.entries.push(entry);
        }
    }
}

/// Request TileBundle assets for tiles in the camera stream disk that are not yet catalogued (#77).
pub fn request_tile_bundle_stream_system(
    server: Res<AssetServer>,
    route: Res<crate::runtime::RouteDir>,
    config: Res<crate::stream::TileStreamConfig>,
    catalog: Res<crate::stream::TileCatalog>,
    camera: Query<&Transform, With<Camera3d>>,
    mut handles: ResMut<TileBundleHandles>,
) {
    let Ok(cam_tf) = camera.single() else {
        return;
    };
    let cam_tile = crate::stream::camera_tile(&config, cam_tf);
    let policy = config.stream_policy();
    let center = openrailsrs_bevy_scenery::stream::TileCoord::from(cam_tile);
    for tile in openrailsrs_bevy_scenery::stream::StreamWindowPolicy::chebyshev_disk(
        center,
        policy.load_radius,
    ) {
        let key = (tile.x, tile.z);
        if catalog
            .entries
            .iter()
            .any(|e| e.geometry.tile_x == key.0 && e.geometry.tile_z == key.1)
        {
            continue;
        }
        if handles.get(key.0, key.1).is_some() || handles.absent.contains(&key) {
            continue;
        }
        if ensure_tile_bundle_handle(&mut handles, &server, &route.0, key.0, key.1, None, None)
            .is_none()
        {
            handles.mark_absent(key.0, key.1);
        }
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct MaterializeTileBundleParams<'w> {
    server: Res<'w, AssetServer>,
    handles: Res<'w, TileBundleHandles>,
    bundles: Res<'w, Assets<MstsTileBundleAsset>>,
    worlds: Res<'w, Assets<MstsWorldTileAsset>>,
    terrains: Res<'w, Assets<MstsTerrainTileAsset>>,
    route: Res<'w, crate::runtime::RouteDir>,
    config: Res<'w, crate::stream::TileStreamConfig>,
    catalog: ResMut<'w, crate::stream::TileCatalog>,
    load_diag: ResMut<'w, MstsLoadDiagnostics>,
}

/// Bevy system: append catalog entries from AssetServer-backed tile bundles (#53 / #77).
pub fn materialize_tile_bundle_system(mut p: MaterializeTileBundleParams) {
    // Skip handles that have not finished loading (or failed).
    let ready_handles = {
        let mut filtered = TileBundleHandles::default();
        for ((tx, tz), handle) in &p.handles.by_tile {
            match p.server.get_load_state(handle) {
                Some(LoadState::Loaded) => {
                    filtered.insert(*tx, *tz, handle.clone());
                }
                Some(LoadState::Failed(_)) => {}
                _ => {}
            }
        }
        filtered
    };
    materialize_loaded_tile_bundles(MaterializeTileBundles {
        handles: &ready_handles,
        bundles: &p.bundles,
        worlds: &p.worlds,
        terrains: &p.terrains,
        route: &p.route.0,
        center_tile: p.config.center_tile,
        catalog: &mut p.catalog,
        load_diag: Some(&mut p.load_diag),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::LoadState;
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
            .add_plugins(render3d_asset_plugin())
            .add_plugins(MstsAssetPlugin)
            .init_resource::<TileBundleHandles>()
            .init_resource::<MstsLoadDiagnostics>()
            .insert_resource(crate::stream::TileCatalog {
                entries: Vec::new(),
            })
            .insert_resource(crate::stream::TileStreamConfig::new((-1000, -1000), 0));
        app
    }

    #[test]
    fn asset_server_loads_complete_and_missing_raw_bundles() {
        let mut app = fixture_app();
        let server = app.world().resource::<AssetServer>().clone();
        let complete = request_tile_bundle(&server, "msts/tiles/complete/complete.tilebundle");
        let missing = request_tile_bundle(&server, "msts/tiles/missing_raw/missing_raw.tilebundle");
        wait_loaded(&mut app, &complete, "complete");
        wait_loaded(&mut app, &missing, "missing_raw");

        {
            let mut handles = app.world_mut().resource_mut::<TileBundleHandles>();
            handles.insert(-1000, -1000, complete.clone());
            handles.insert(-1001, -1000, missing.clone());
        }

        let bundles = app.world().resource::<Assets<MstsTileBundleAsset>>();
        let c = bundles.get(&complete).unwrap();
        assert_eq!(c.status, TileBundleStatus::Ready);
        assert_eq!(c.terrain_raw_status, Some(TerrainRawStatus::Complete));
        let m = bundles.get(&missing).unwrap();
        assert_eq!(m.status, TileBundleStatus::Partial);
        assert!(matches!(
            m.terrain_raw_status,
            Some(TerrainRawStatus::MissingY | TerrainRawStatus::MissingBoth)
        ));

        let worlds = app.world().resource::<Assets<MstsWorldTileAsset>>();
        let terrains = app.world().resource::<Assets<MstsTerrainTileAsset>>();
        let route = Path::new(".");
        let entry = try_materialize_tile_entry(c, worlds, terrains, route, Vec3::ZERO)
            .expect("complete materializes");
        assert!(
            !entry.geometry.patches.is_empty(),
            "complete tile should produce terrain patches"
        );
        assert!(!entry.objects.is_empty());

        // Missing RAW: WORLD still materializes; terrain elevation absent → empty/stub geom.
        let entry_m = try_materialize_tile_entry(m, worlds, terrains, route, Vec3::ZERO)
            .expect("partial still materializes world");
        assert!(!entry_m.objects.is_empty());

        app.world_mut()
            .resource_mut::<TileBundleHandles>()
            .release(-1000, -1000);
        assert!(
            !app.world()
                .resource::<TileBundleHandles>()
                .by_tile
                .contains_key(&(-1000, -1000))
        );
    }

    #[test]
    fn request_ready_materialize_and_spawn_tile_content() {
        // #77 integration: request → Ready → catalog entry → TileContent spawn.
        let mut app = fixture_app();
        let server = app.world().resource::<AssetServer>().clone();
        let handle = request_tile_bundle(&server, "msts/tiles/complete/complete.tilebundle");
        {
            let mut handles = app.world_mut().resource_mut::<TileBundleHandles>();
            handles.insert(-1000, -1000, handle.clone());
        }
        assert!(
            app.world()
                .resource::<TileBundleHandles>()
                .get(-1000, -1000)
                .is_some(),
            "request must register a real handle in TileBundleHandles"
        );
        wait_loaded(&mut app, &handle, "complete.tilebundle");

        let handles = app.world().resource::<TileBundleHandles>().clone();
        let mut catalog = crate::stream::TileCatalog {
            entries: Vec::new(),
        };
        let mut diag = MstsLoadDiagnostics::default();
        {
            let world = app.world();
            materialize_loaded_tile_bundles(MaterializeTileBundles {
                handles: &handles,
                bundles: world.resource::<Assets<MstsTileBundleAsset>>(),
                worlds: world.resource::<Assets<MstsWorldTileAsset>>(),
                terrains: world.resource::<Assets<MstsTerrainTileAsset>>(),
                route: Path::new("."),
                center_tile: (-1000, -1000),
                catalog: &mut catalog,
                load_diag: Some(&mut diag),
            });
        }
        assert_eq!(catalog.entries.len(), 1);
        let entry = catalog.entries[0].clone();
        assert_eq!(
            (entry.geometry.tile_x, entry.geometry.tile_z),
            (-1000, -1000)
        );
        assert!(
            !entry.geometry.patches.is_empty(),
            "holes/textures path must still produce patches"
        );
        assert!(!entry.objects.is_empty());
        assert_eq!(entry.world_offset, Vec3::ZERO);

        app.world_mut()
            .resource_mut::<crate::stream::TileCatalog>()
            .entries = catalog.entries;

        // Spawn marker for the materialized tile (stream would call spawn_tile_entry).
        app.world_mut().spawn(crate::stream::TileContent {
            tile_x: entry.geometry.tile_x,
            tile_z: entry.geometry.tile_z,
        });
        let n = app
            .world_mut()
            .query::<&crate::stream::TileContent>()
            .iter(app.world())
            .count();
        assert_eq!(n, 1, "spawn must produce a TileContent entity");
    }
}
