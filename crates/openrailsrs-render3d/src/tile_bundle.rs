//! Bridge from [`MstsTileBundleAsset`] to render3d [`TileEntry`] (#53).

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    MstsLoadDiagnostics, MstsTerrainTileAsset, MstsTileBundleAsset, MstsWorldTileAsset,
    TileBundleStatus, snapshot_from_parsed,
};

use crate::objects::{self, ObjectMarker};
use crate::runtime::TileEntry;
use crate::terrain::{self, TileGeometry};
use crate::tile_parse::tile_entry_from_snapshot;
use crate::track::TrackRibbon;

/// Strong handles for tiles loaded via AssetServer (exact unload by dropping).
#[derive(Resource, Default)]
pub struct TileBundleHandles {
    pub by_tile: HashMap<(i32, i32), Handle<MstsTileBundleAsset>>,
}

impl TileBundleHandles {
    pub fn insert(&mut self, tile_x: i32, tile_z: i32, handle: Handle<MstsTileBundleAsset>) {
        self.by_tile.insert((tile_x, tile_z), handle);
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

/// Kick off an AssetServer load of a `.tilebundle` path.
pub fn request_tile_bundle(
    server: &AssetServer,
    asset_path: impl Into<String>,
) -> Handle<MstsTileBundleAsset> {
    server.load(asset_path.into())
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
        terr_asset.map(|t| t.raw_status).or(bundle.terrain_raw_status),
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

/// Poll loaded bundles and materialize any that are Ready/Partial into the catalog.
pub fn materialize_loaded_tile_bundles(
    handles: &TileBundleHandles,
    bundles: &Assets<MstsTileBundleAsset>,
    worlds: &Assets<MstsWorldTileAsset>,
    terrains: &Assets<MstsTerrainTileAsset>,
    route: &Path,
    catalog: &mut crate::stream::TileCatalog,
) {
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
        // Wait until dependency assets are in `Assets` (LoadState already Loaded).
        if bundle.world.as_ref().is_some_and(|h| worlds.get(h).is_none()) {
            continue;
        }
        if bundle
            .terrain
            .as_ref()
            .is_some_and(|h| terrains.get(h).is_none())
        {
            continue;
        }
        let offset = Vec3::ZERO;
        if let Some(entry) =
            try_materialize_tile_entry(bundle, worlds, terrains, route, offset)
        {
            catalog.entries.push(entry);
        }
    }
}

/// Bevy system: append catalog entries from AssetServer-backed tile bundles (#53).
pub fn materialize_tile_bundle_system(
    handles: Res<TileBundleHandles>,
    bundles: Res<Assets<MstsTileBundleAsset>>,
    worlds: Res<Assets<MstsWorldTileAsset>>,
    terrains: Res<Assets<MstsTerrainTileAsset>>,
    route: Res<crate::runtime::RouteDir>,
    mut catalog: ResMut<crate::stream::TileCatalog>,
) {
    materialize_loaded_tile_bundles(
        &handles,
        &bundles,
        &worlds,
        &terrains,
        &route.0,
        &mut catalog,
    );
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
                ..default()
            })
            .add_plugins(MstsAssetPlugin)
            .init_resource::<TileBundleHandles>();
        app
    }

    #[test]
    fn asset_server_loads_complete_and_missing_raw_bundles() {
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
        assert!(!app
            .world()
            .resource::<TileBundleHandles>()
            .by_tile
            .contains_key(&(-1000, -1000)));
    }
}
