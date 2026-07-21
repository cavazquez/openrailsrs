//! Canonical CPU tile snapshot: coords + terrain + classified WORLD + diagnostics (#112).

use std::path::{Path, PathBuf};

use openrailsrs_formats::{
    ElevationGrid, FeatureGrid, TerrainFile, WorldFile, read_f_raw, read_y_raw,
};

use crate::assets::TerrainRawStatus;
use crate::load_diagnostics::{MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics};
use crate::tile::classify::{MstsClassifiedWorldItem, classify_world_file};

/// Terrain half of a tile snapshot (parsed `.t`/`.y` + optional RAW grids).
#[derive(Clone, Debug)]
pub struct MstsTileTerrainSnapshot {
    pub terrain: TerrainFile,
    pub elevation: Option<ElevationGrid>,
    pub features: Option<FeatureGrid>,
    pub raw_status: TerrainRawStatus,
    pub source_path: Option<PathBuf>,
}

impl MstsTileTerrainSnapshot {
    /// Minimum elevation (MSL) used as Y=0 in centered render frames; `0.0` if no grid.
    pub fn base_y(&self) -> f32 {
        self.elevation
            .as_ref()
            .map(elevation_base_y)
            .unwrap_or(0.0)
    }
}

/// WORLD half of a tile snapshot (raw file + classified items).
#[derive(Clone, Debug)]
pub struct MstsTileWorldSnapshot {
    pub world: WorldFile,
    pub items: Vec<MstsClassifiedWorldItem>,
    pub source_path: Option<PathBuf>,
}

/// Canonical CPU projection of one WORLD+terrain tile.
///
/// Independent of Bevy `App`, camera, RouteFocus, floating origin, VSM, and cab.
#[derive(Clone, Debug, Default)]
pub struct MstsTileSnapshot {
    pub tile_x: i32,
    pub tile_z: i32,
    pub terrain: Option<MstsTileTerrainSnapshot>,
    pub world: Option<MstsTileWorldSnapshot>,
    pub diag: MstsLoadDiagnostics,
}

impl MstsTileSnapshot {
    pub fn has_elevation(&self) -> bool {
        self.terrain
            .as_ref()
            .and_then(|t| t.elevation.as_ref())
            .is_some()
    }

    pub fn classified_item_count(&self) -> usize {
        self.world.as_ref().map(|w| w.items.len()).unwrap_or(0)
    }

    pub fn kind_counts(&self) -> Vec<(crate::tile::MstsWorldItemKind, usize)> {
        use crate::tile::MstsWorldItemKind;
        use std::collections::BTreeMap;
        let mut map = BTreeMap::new();
        if let Some(world) = &self.world {
            for item in &world.items {
                *map.entry(item.kind).or_insert(0usize) += 1;
            }
        }
        // Stable order for golden tests.
        [
            MstsWorldItemKind::Static,
            MstsWorldItemKind::Track,
            MstsWorldItemKind::Dyntrack,
            MstsWorldItemKind::Signal,
            MstsWorldItemKind::Forest,
            MstsWorldItemKind::HWater,
            MstsWorldItemKind::Pickup,
            MstsWorldItemKind::Transfer,
            MstsWorldItemKind::Hazard,
            MstsWorldItemKind::Other,
        ]
        .into_iter()
        .filter_map(|k| map.get(&k).copied().map(|n| (k, n)))
        .collect()
    }
}

/// Minimum elevation in an elevation grid (finite samples only).
pub fn elevation_base_y(grid: &ElevationGrid) -> f32 {
    let min = grid
        .elevations
        .iter()
        .copied()
        .filter(|h| h.is_finite())
        .fold(f32::INFINITY, f32::min);
    if min.is_finite() { min } else { 0.0 }
}

/// Resolve `.w` path for a tile (includes smoke `w-000000-000000.w` quirk).
pub fn resolve_world_tile_path(route_dir: &Path, tile_x: i32, tile_z: i32) -> Option<PathBuf> {
    if let Some(path) = openrailsrs_formats::resolve_world_tile_file(route_dir, tile_x, tile_z) {
        return Some(path);
    }
    if tile_x == 0 && tile_z == 0 {
        for dir in openrailsrs_formats::world_subdirs(route_dir) {
            let candidate = dir.join("w-000000-000000.w");
            if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&candidate) {
                if resolved.is_file() {
                    return Some(resolved);
                }
            }
        }
    }
    None
}

/// Load WORLD + terrain for `(tile_x, tile_z)` under a route directory.
pub fn load_msts_tile_snapshot(route_dir: &Path, tile_x: i32, tile_z: i32) -> MstsTileSnapshot {
    let world_path = resolve_world_tile_path(route_dir, tile_x, tile_z);
    let terrain_path =
        openrailsrs_formats::resolve_hash_terrain_tile_file(route_dir, tile_x, tile_z);
    load_msts_tile_snapshot_from_paths(
        world_path.as_deref(),
        terrain_path.as_deref(),
        tile_x,
        tile_z,
        Some(route_dir),
    )
}

/// Load only the WORLD half (for object-only adapters).
pub fn load_msts_tile_world_snapshot(
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
) -> MstsTileSnapshot {
    let world_path = resolve_world_tile_path(route_dir, tile_x, tile_z);
    load_msts_tile_snapshot_from_paths(
        world_path.as_deref(),
        None,
        tile_x,
        tile_z,
        Some(route_dir),
    )
}

/// Load only the terrain half.
pub fn load_msts_tile_terrain_snapshot(
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
) -> MstsTileSnapshot {
    let terrain_path =
        openrailsrs_formats::resolve_hash_terrain_tile_file(route_dir, tile_x, tile_z);
    load_msts_tile_snapshot_from_paths(None, terrain_path.as_deref(), tile_x, tile_z, None)
}

/// Build a snapshot from explicit WORLD / terrain paths (fixtures, AssetServer sidecars).
pub fn load_msts_tile_snapshot_from_paths(
    world_path: Option<&Path>,
    terrain_path: Option<&Path>,
    tile_x: i32,
    tile_z: i32,
    route_dir: Option<&Path>,
) -> MstsTileSnapshot {
    let mut diag = MstsLoadDiagnostics::default();
    let world = world_path.and_then(|path| load_world_half(path, tile_x, tile_z, route_dir, &mut diag));
    let terrain = terrain_path.and_then(|path| load_terrain_half(path, tile_x, tile_z, &mut diag));
    MstsTileSnapshot {
        tile_x,
        tile_z,
        terrain,
        world,
        diag,
    }
}

/// Assemble a snapshot from already-parsed components (AssetServer / #53 bridge).
pub fn snapshot_from_parsed(
    tile_x: i32,
    tile_z: i32,
    world: Option<WorldFile>,
    world_path: Option<PathBuf>,
    terrain: Option<TerrainFile>,
    elevation: Option<ElevationGrid>,
    features: Option<FeatureGrid>,
    raw_status: Option<TerrainRawStatus>,
    terrain_path: Option<PathBuf>,
    route_dir: Option<&Path>,
    diag: MstsLoadDiagnostics,
) -> MstsTileSnapshot {
    let world = world.map(|w| {
        let items = classify_world_file(&w, route_dir);
        MstsTileWorldSnapshot {
            world: w,
            items,
            source_path: world_path,
        }
    });
    let terrain = terrain.map(|t| MstsTileTerrainSnapshot {
        terrain: t,
        elevation,
        features,
        raw_status: raw_status.unwrap_or(TerrainRawStatus::MissingY),
        source_path: terrain_path,
    });
    MstsTileSnapshot {
        tile_x,
        tile_z,
        terrain,
        world,
        diag,
    }
}

fn load_world_half(
    path: &Path,
    tile_x: i32,
    tile_z: i32,
    route_dir: Option<&Path>,
    diag: &mut MstsLoadDiagnostics,
) -> Option<MstsTileWorldSnapshot> {
    match WorldFile::from_path(path) {
        Ok(mut world) => {
            // Filename coords win when the file itself had no/zero indices.
            if world.tile_x == 0 && world.tile_z == 0 && (tile_x != 0 || tile_z != 0) {
                world.tile_x = tile_x;
                world.tile_z = tile_z;
            }
            diag.record_path_loaded(path, MstsAssetKind::World);
            let items = classify_world_file(&world, route_dir);
            Some(MstsTileWorldSnapshot {
                world,
                items,
                source_path: Some(path.to_path_buf()),
            })
        }
        Err(e) => {
            diag.record_failed_at(
                path.display().to_string(),
                MstsAssetKind::World,
                MstsLoadCause::Parse,
                e.to_string(),
                Some(tile_x),
                Some(tile_z),
            );
            None
        }
    }
}

fn load_terrain_half(
    path: &Path,
    tile_x: i32,
    tile_z: i32,
    diag: &mut MstsLoadDiagnostics,
) -> Option<MstsTileTerrainSnapshot> {
    let terrain = match TerrainFile::from_path_with_coords(path, tile_x, tile_z) {
        Ok(t) => {
            diag.record_path_loaded(path, MstsAssetKind::Terrain);
            t
        }
        Err(e) => {
            diag.record_failed_at(
                path.display().to_string(),
                MstsAssetKind::Terrain,
                MstsLoadCause::Parse,
                e.to_string(),
                Some(tile_x),
                Some(tile_z),
            );
            return None;
        }
    };

    let y_name = terrain.samples.y_buffer_file.trim();
    let f_name = terrain.samples.f_buffer_file.trim();

    let mut elevation = None;
    let mut y_missing = !y_name.is_empty();
    if y_name.is_empty() {
        y_missing = true;
        diag.record_failed_at(
            path.display().to_string(),
            MstsAssetKind::Terrain,
            MstsLoadCause::Missing,
            "terrain_sample_ybuffer empty",
            Some(tile_x),
            Some(tile_z),
        );
    } else {
        let y_path = terrain.y_raw_path(path);
        match read_y_raw(&y_path, &terrain.samples) {
            Ok(grid) => {
                elevation = Some(grid);
                y_missing = false;
                diag.record_loaded(MstsAssetKind::Terrain);
            }
            Err(e) => {
                let cause = if y_path.is_file() {
                    MstsLoadCause::Parse
                } else {
                    MstsLoadCause::Missing
                };
                diag.record_failed_at(
                    y_path.display().to_string(),
                    MstsAssetKind::Terrain,
                    cause,
                    e.to_string(),
                    Some(tile_x),
                    Some(tile_z),
                );
            }
        }
    }

    let mut features = None;
    let mut f_missing = false;
    if !f_name.is_empty() {
        f_missing = true;
        let f_path = terrain.f_raw_path(path);
        match read_f_raw(&f_path, &terrain.samples) {
            Ok(grid) => {
                features = Some(grid);
                f_missing = false;
                diag.record_loaded(MstsAssetKind::Terrain);
            }
            Err(e) => {
                let cause = if f_path.is_file() {
                    MstsLoadCause::Parse
                } else {
                    MstsLoadCause::Missing
                };
                diag.record_failed_at(
                    f_path.display().to_string(),
                    MstsAssetKind::Terrain,
                    cause,
                    e.to_string(),
                    Some(tile_x),
                    Some(tile_z),
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

    Some(MstsTileTerrainSnapshot {
        terrain,
        elevation,
        features,
        raw_status,
        source_path: Some(path.to_path_buf()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::MstsWorldItemKind;

    fn complete_fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/msts/tiles/complete")
    }

    #[test]
    fn complete_fixture_snapshot_structural_golden() {
        let dir = complete_fixture_dir();
        let world = dir.join("w-001000-001000.w");
        let terrain = dir.join("minimal_terrain.y");
        assert!(world.is_file(), "fixture world missing");
        assert!(terrain.is_file(), "fixture terrain missing");

        let snap = load_msts_tile_snapshot_from_paths(
            Some(&world),
            Some(&terrain),
            -1000,
            -1000,
            None,
        );

        assert_eq!(snap.tile_x, -1000);
        assert_eq!(snap.tile_z, -1000);
        assert!(snap.has_elevation(), "complete fixture must have Y.RAW");
        assert_eq!(
            snap.terrain.as_ref().map(|t| t.raw_status),
            Some(TerrainRawStatus::Complete)
        );

        let world = snap.world.as_ref().expect("world half");
        assert_eq!(world.items.len(), 5, "fixture has 5 positioned items");
        assert_eq!(
            snap.kind_counts(),
            vec![
                (MstsWorldItemKind::Static, 1),
                (MstsWorldItemKind::Track, 1),
                (MstsWorldItemKind::Dyntrack, 1),
                (MstsWorldItemKind::Signal, 1),
                (MstsWorldItemKind::Forest, 1),
            ]
        );

        let forest = world
            .items
            .iter()
            .find(|i| i.kind == MstsWorldItemKind::Forest)
            .expect("forest");
        assert_eq!(forest.position, [200.0, 0.0, -75.0]);
        assert!(forest.forest.is_some());
        assert_eq!(
            forest.forest.as_ref().unwrap().tree_texture.as_deref(),
            Some("pine.ace")
        );

        let track = world
            .items
            .iter()
            .find(|i| i.kind == MstsWorldItemKind::Track)
            .expect("track");
        assert_eq!(track.file_name.as_deref(), Some("rail_straight.s"));

        assert!(snap.diag.loaded >= 2, "world + terrain (+ Y) loaded");
        assert!(snap.diag.totals_ok(), "{}", snap.diag.summary_line());
    }

    #[test]
    fn missing_raw_fixture_records_diag() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/msts/tiles/missing_raw");
        let snap = load_msts_tile_snapshot_from_paths(
            Some(&dir.join("w-001000-001000.w")),
            Some(&dir.join("minimal_terrain.y")),
            -1001,
            -1000,
            None,
        );
        assert!(snap.world.is_some());
        assert!(snap.terrain.is_some());
        assert!(!snap.has_elevation());
        assert!(matches!(
            snap.terrain.as_ref().unwrap().raw_status,
            TerrainRawStatus::MissingY | TerrainRawStatus::MissingBoth
        ));
        assert!(snap.diag.failed > 0);
        assert!(snap.diag.totals_ok());
    }

    #[test]
    fn chiltern_smoke_or_skip() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let (tx, tz) = (-6082, 14925);
        if resolve_world_tile_path(&route, tx, tz).is_none() {
            return;
        }
        let snap = load_msts_tile_snapshot(&route, tx, tz);
        assert_eq!((snap.tile_x, snap.tile_z), (tx, tz));
        assert!(
            snap.classified_item_count() > 0,
            "Chiltern populated tile should classify WORLD items"
        );
        assert!(snap.diag.totals_ok());
    }
}
