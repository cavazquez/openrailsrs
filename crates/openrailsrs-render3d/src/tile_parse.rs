//! Parse CPU de tiles WORLD/terrain fuera del hilo de la ventana (#55).
//!
//! Thin adapter: loads [`openrailsrs_bevy_scenery::MstsTileSnapshot`] per tile
//! and materializes render3d [`TileEntry`] (#112). Camera / consist / player pose
//! stay here as render3d-specific post-processing.

use std::path::PathBuf;

use bevy::prelude::*;
use openrailsrs_bevy_scenery::{MstsLoadDiagnostics, MstsTileSnapshot, load_msts_tile_snapshot};
use openrailsrs_track::TrackGraph;

use crate::consist::{StaticConsistPlan, load_consist_at_path, resolve_player_consist_path};
use crate::objects::object_markers_from_classified;
use crate::player_spawn::{
    PlayerStartPose, default_track_camera_pose, default_trackobj_camera_pose,
    resolve_pat_start_pose, resolve_player_start_pose,
};
use crate::runtime::{TileEntry, TilesToRender};
use crate::stream::{TileCatalog, TileStreamConfig, catalog_entries_for_initial_load};
use crate::tdb_track::{self, TdbContext};
use crate::terrain::{self, TileGeometry};
use crate::track::{self, TILE_SIZE_M, TrackRibbon};

/// Parámetros para parsear el grid de tiles en background.
#[derive(Resource, Clone)]
pub struct TileParseRequest {
    pub route: PathBuf,
    pub center: (i32, i32),
    pub radius: u32,
    pub player_path: Option<PathBuf>,
    pub path_offset_m: f64,
    pub consist: Option<PathBuf>,
    /// Nombre de consist del `.act` (si hay).
    pub activity_consist: Option<String>,
    /// Path del `.act` para resolver pose del jugador.
    pub activity_path_for_pose: Option<PathBuf>,
    pub graph: Option<TrackGraph>,
    pub tdb: Option<TdbContext>,
}

/// Resultado del parse async (un solo pase por tile).
pub struct ParsedTiles {
    pub catalog: TileCatalog,
    pub tiles_to_render: TilesToRender,
    pub stream_config: TileStreamConfig,
    pub load_diag: MstsLoadDiagnostics,
    pub player_start: Option<PlayerStartPose>,
    pub consist_plan: Option<StaticConsistPlan>,
    pub scene_side_m: f32,
    pub skipped: usize,
    pub total_patches: usize,
    pub total_segments: usize,
    pub total_objects: usize,
}

/// Carga geometría/objetos/vía del radio pedido (CPU; llamar desde `AsyncComputeTaskPool`).
pub fn parse_tiles_for_load(req: TileParseRequest) -> Result<ParsedTiles, String> {
    let (cx, cz) = req.center;
    let r = req.radius as i32;
    let mut tile_coords: Vec<(i32, i32)> = Vec::new();
    tile_coords.push((cx, cz));
    for dz in -r..=r {
        for dx in -r..=r {
            if dx == 0 && dz == 0 {
                continue;
            }
            tile_coords.push((cx + dx, cz + dz));
        }
    }

    let tdb_chords = req
        .tdb
        .as_ref()
        .map(|ctx| track::collect_tdb_chords(ctx, cx, cz, req.radius));

    let mut entries: Vec<TileEntry> = Vec::new();
    let mut skipped = 0usize;
    let mut load_diag = MstsLoadDiagnostics::default();

    for &(tx, tz) in &tile_coords {
        let world_offset = Vec3::new(
            (tx - cx) as f32 * TILE_SIZE_M,
            0.0,
            (cz - tz) as f32 * TILE_SIZE_M,
        );
        let snap = load_msts_tile_snapshot(&req.route, tx, tz);
        load_diag.merge_from(&snap.diag);
        match tile_entry_from_snapshot(&snap, world_offset, TrackRibbon::default()) {
            Some(mut entry) => {
                if tdb_chords.is_none() {
                    if let Some(g) = req.graph.as_ref() {
                        entry.track = track::build_track_ribbon(g, tx, tz, &entry.geometry.height);
                    }
                }
                entries.push(entry);
            }
            None => {
                skipped += 1;
            }
        }
    }

    if let Some(chords) = &tdb_chords {
        let height_rows: Vec<_> = entries
            .iter()
            .map(|e| (e.geometry.tile_x, e.geometry.tile_z, &e.geometry.height))
            .collect();
        let height_index = tdb_track::TileHeightIndex::new(&height_rows, (cx, cz));
        let scene_ribbon =
            track::build_tdb_track_ribbon(chords, cx, cz, &height_index, req.radius);
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.geometry.tile_x == cx && e.geometry.tile_z == cz)
        {
            entry.track = scene_ribbon;
        }
    }

    if entries.is_empty() {
        return Err(format!(
            "no se pudo cargar ningún tile en el radio={r} alrededor de ({cx}, {cz})"
        ));
    }

    let total_patches: usize = entries.iter().map(|e| e.geometry.patches.len()).sum();
    let total_segments: usize = entries.iter().map(|e| e.track.segment_count()).sum();
    let total_objects: usize = entries.iter().map(|e| e.objects.len()).sum();

    let catalog = TileCatalog {
        entries: entries.clone(),
    };
    let stream_config = TileStreamConfig::new((cx, cz), req.radius);
    let initial_entries = catalog_entries_for_initial_load(&catalog, &stream_config);
    let tiles_to_render = TilesToRender(initial_entries);

    let tdb = req.tdb.as_ref().map(|c| &c.track_db);
    let from_ribbon = default_track_camera_pose(&tiles_to_render);
    let from_scenery = default_trackobj_camera_pose(&tiles_to_render);
    let player_start = req
        .activity_path_for_pose
        .as_ref()
        .and_then(|path| {
            resolve_player_start_pose(
                &req.route,
                path,
                req.graph.as_ref(),
                tdb,
                (cx, cz),
                &tiles_to_render,
            )
        })
        .or_else(|| {
            req.player_path.as_ref().and_then(|pat| {
                resolve_pat_start_pose(
                    &req.route,
                    pat,
                    req.path_offset_m.max(0.0),
                    req.graph.as_ref(),
                    tdb,
                    (cx, cz),
                    &tiles_to_render,
                )
            })
        })
        .or(from_ribbon)
        .or(from_scenery);

    let consist_plan = resolve_player_consist_path(
        &req.route,
        req.consist.as_deref(),
        req.activity_consist.as_deref(),
    )
    .and_then(|path| {
        load_consist_at_path(&path).map(|vehicles| StaticConsistPlan { vehicles })
    });

    let scene_side_m = tiles_to_render
        .0
        .first()
        .map(|e| e.geometry.side_m)
        .unwrap_or(TILE_SIZE_M);

    Ok(ParsedTiles {
        catalog,
        tiles_to_render,
        stream_config,
        load_diag,
        player_start,
        consist_plan,
        scene_side_m,
        skipped,
        total_patches,
        total_segments,
        total_objects,
    })
}

/// Materialize a render3d [`TileEntry`] from a canonical CPU snapshot (#112).
///
/// Requires terrain elevation (same rule as the previous direct `.t` load path).
pub fn tile_entry_from_snapshot(
    snap: &MstsTileSnapshot,
    world_offset: Vec3,
    track: TrackRibbon,
) -> Option<TileEntry> {
    let terr = snap.terrain.as_ref()?;
    let elevation = terr.elevation.clone()?;
    let geometry = terrain::tile_geometry_from_elevation(
        snap.tile_x,
        snap.tile_z,
        &terr.terrain,
        elevation,
    );
    let base_y = geometry.height.base_y();
    let objects = snap
        .world
        .as_ref()
        .map(|w| object_markers_from_classified(&w.items, base_y))
        .unwrap_or_default();
    Some(TileEntry {
        geometry,
        world_offset,
        track,
        objects,
    })
}

/// Convenience: snapshot → [`TileGeometry`] when elevation is present.
pub fn tile_geometry_from_snapshot(snap: &MstsTileSnapshot) -> Option<TileGeometry> {
    let terr = snap.terrain.as_ref()?;
    let elevation = terr.elevation.clone()?;
    Some(terrain::tile_geometry_from_elevation(
        snap.tile_x,
        snap.tile_z,
        &terr.terrain,
        elevation,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_bevy_scenery::load_msts_tile_snapshot_from_paths;

    #[test]
    fn parse_smoke_route_or_skip() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route.join("WORLD").is_dir() && !route.join("world").is_dir() {
            return;
        }
        let cx = -6082;
        let cz = 14925;
        let parsed = parse_tiles_for_load(TileParseRequest {
            route,
            center: (cx, cz),
            radius: 0,
            player_path: None,
            path_offset_m: 0.0,
            consist: None,
            activity_consist: None,
            activity_path_for_pose: None,
            graph: None,
            tdb: None,
        });
        let Ok(parsed) = parsed else {
            return;
        };
        assert!(
            !parsed.tiles_to_render.0.is_empty(),
            "radio 0 debe producir al menos el tile central si existe"
        );
        assert_eq!(parsed.catalog.entries.len(), parsed.tiles_to_render.0.len());
    }

    #[test]
    fn fixture_snapshot_materializes_tile_entry() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-bevy-scenery/assets/msts/tiles/complete");
        let snap = load_msts_tile_snapshot_from_paths(
            Some(&dir.join("w-001000-001000.w")),
            Some(&dir.join("minimal_terrain.y")),
            -1000,
            -1000,
            None,
        );
        let entry = tile_entry_from_snapshot(&snap, Vec3::ZERO, TrackRibbon::default())
            .expect("complete fixture → TileEntry");
        assert_eq!((entry.geometry.tile_x, entry.geometry.tile_z), (-1000, -1000));
        assert!(!entry.geometry.patches.is_empty());
        assert_eq!(entry.objects.len(), 5);
        assert!(entry.objects.iter().any(|o| o.forest.is_some()));
        assert!(
            entry
                .objects
                .iter()
                .any(|o| o.kind == crate::objects::ObjectKind::Track)
        );
    }
}
