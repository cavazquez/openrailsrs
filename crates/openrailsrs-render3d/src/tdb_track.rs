//! Vía procedural desde el grafo `.tdb` — adapter sobre `bevy-scenery::spawn::tdb_track`.
//!
//! Conserva carga de contexto, `TileHeightIndex` (altura/VSM) y proyección a escena.
//! La recolección de chords, UKFS world placement y transforms métricos viven en el SSOT.

use std::collections::HashMap;
use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

use bevy::math::{EulerRot, Quat, Vec3};
use openrailsrs_bevy_scenery::spawn::tdb_track::{
    ChordCollectLimits, FocusQuery, MSTS_TILE_SIZE_M, TdbChord, collect_tdb_chords as collect_ssot,
    procedural_fallback_shaped_chords, route_has_ukfs_tsection as route_has_ukfs_ssot,
    shaped_chords_from_tdb, ukfs_placements_world, world_to_scene_xz as world_to_scene_xz_ssot,
    world_to_tile_local as world_to_tile_local_ssot,
    world_to_tile_local_centered as world_to_tile_local_centered_ssot,
    scene_xz_to_world as scene_xz_to_world_ssot,
};
use openrailsrs_formats::{
    TSectionCatalog, TrackDbFile, TrackNodeKind, TrackVectorGeometry, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord,
};

use crate::track::TILE_SIZE_M;

/// Radio de recolección extra alrededor del tile (m).
const TILE_CHORD_MARGIN_M: f32 = 128.0;
const DEFAULT_SECTION_LENGTH_M: f32 = 25.0;

#[derive(Clone, Debug)]
pub struct TdbContext {
    pub track_db: TrackDbFile,
    pub tsection: TSectionCatalog,
    pub sections_by_shape: HashMap<u32, Vec<TdbSectionAnchor>>,
}

#[derive(Clone, Copy, Debug)]
pub struct TdbSectionAnchor {
    pub bevy_x: f32,
    pub bevy_z: f32,
    pub heading_deg: f64,
}

pub fn load_tdb_context(route_dir: &Path) -> Option<TdbContext> {
    let track_db = load_track_db(route_dir)?;
    let tsection = load_tsection_catalog(route_dir);
    let sections_by_shape = build_tdb_section_index(&track_db);
    Some(TdbContext {
        track_db,
        tsection,
        sections_by_shape,
    })
}

fn heading_from_vector_geometry(geometry: TrackVectorGeometry) -> Option<f64> {
    let (x0, _, z0) = geometry.start.bevy_position();
    let (x1, _, z1) = geometry.end.bevy_position();
    let dx = x1 - x0;
    let dz = z1 - z0;
    if dx * dx + dz * dz < 0.01 {
        return None;
    }
    Some((dx as f64).atan2(dz as f64).to_degrees())
}

fn build_tdb_section_index(track_db: &TrackDbFile) -> HashMap<u32, Vec<TdbSectionAnchor>> {
    let mut geometry_by_node: HashMap<u32, TrackVectorGeometry> = HashMap::new();
    for node in &track_db.nodes {
        if let TrackNodeKind::Vector {
            geometry: Some(geom),
            ..
        } = &node.kind
        {
            geometry_by_node.insert(node.id, *geom);
        }
    }

    let mut out: HashMap<u32, Vec<TdbSectionAnchor>> = HashMap::new();
    for (shape_idx, entries) in track_db.index_vector_sections_by_shape() {
        let mut anchors = Vec::new();
        for entry in entries {
            let heading = entry.record.heading_deg().or_else(|| {
                geometry_by_node
                    .get(&entry.node_id)
                    .copied()
                    .and_then(heading_from_vector_geometry)
            });
            let Some(heading_deg) = heading else {
                continue;
            };
            let (bevy_x, _, bevy_z) = entry.record.start.bevy_position();
            anchors.push(TdbSectionAnchor {
                bevy_x,
                bevy_z,
                heading_deg,
            });
        }
        if !anchors.is_empty() {
            out.insert(shape_idx, anchors);
        }
    }
    out
}

/// Posición Bevy XZ absoluta de un `TrackObj` (tile MSTS + coords del `.w`).
pub fn trackobj_bevy_world_xz(
    tile_x: i32,
    tile_z: i32,
    obj: &crate::objects::ObjectMarker,
) -> (f32, f32) {
    (
        tile_x as f32 * TILE_SIZE_M + obj.position.x,
        -(tile_z as f32 * TILE_SIZE_M) + obj.position.z,
    )
}

/// Ajusta el yaw del `.w` con el rumbo del `.tdb` cuando hay un ancla cercana (paridad viewer3d).
pub fn refine_trackobj_rotation(
    sections_by_shape: &HashMap<u32, Vec<TdbSectionAnchor>>,
    tile_x: i32,
    tile_z: i32,
    obj: &crate::objects::ObjectMarker,
) -> Quat {
    let Some(shape_idx) = obj.section_idx else {
        return obj.rotation;
    };
    let Some(entries) = sections_by_shape.get(&shape_idx) else {
        return obj.rotation;
    };
    let (wx, wz) = trackobj_bevy_world_xz(tile_x, tile_z, obj);
    const MAX_DIST_M: f32 = 25.0;
    let max_dist_sq = MAX_DIST_M * MAX_DIST_M;
    let mut best: Option<(f32, f64)> = None;
    for entry in entries {
        let dx = entry.bevy_x - wx;
        let dz = entry.bevy_z - wz;
        let dist_sq = dx * dx + dz * dz;
        if dist_sq <= max_dist_sq
            && best
                .map(|(best_dist, _)| dist_sq < best_dist)
                .unwrap_or(true)
        {
            best = Some((dist_sq, entry.heading_deg));
        }
    }
    let Some((_, heading_deg)) = best else {
        return obj.rotation;
    };
    let (yaw, pitch, roll) = obj.rotation.to_euler(EulerRot::YXZ);
    let tdb_yaw = heading_deg.to_radians() as f32;
    if (yaw - tdb_yaw).abs() < 0.01 {
        return obj.rotation;
    }
    Quat::from_euler(EulerRot::YXZ, tdb_yaw, pitch, roll)
}

fn load_tsection_catalog(route_dir: &Path) -> TSectionCatalog {
    if let Ok(catalog) = TSectionCatalog::load_for_route(route_dir) {
        if !catalog.shapes.is_empty() {
            return catalog;
        }
    }
    TSectionCatalog::default()
}

fn load_track_db(route_dir: &Path) -> Option<TrackDbFile> {
    let Ok(entries) = std::fs::read_dir(route_dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tdb"))
        {
            continue;
        }
        if let Ok(mut tdb) = TrackDbFile::from_path(&path) {
            let tit = path.with_extension("tit");
            if tit.is_file() {
                let _ = tdb.merge_tit_speed_posts(&tit);
            }
            return Some(tdb);
        }
    }
    None
}

fn tile_focus(center_tile_x: i32, center_tile_z: i32, grid_radius: u32) -> FocusQuery {
    let extra = grid_radius as f32 * TILE_SIZE_M;
    FocusQuery::for_tile(
        center_tile_x,
        center_tile_z,
        TILE_SIZE_M,
        TILE_CHORD_MARGIN_M,
        extra,
    )
}

fn collect_tdb_chords_full(
    ctx: &TdbContext,
    center_tile_x: i32,
    center_tile_z: i32,
    grid_radius: u32,
) -> Vec<TdbChord> {
    collect_ssot(
        &ctx.track_db,
        &tile_focus(center_tile_x, center_tile_z, grid_radius),
        Some(&ctx.tsection),
        ChordCollectLimits::PER_VECTOR_ONLY,
    )
}

/// Acordes `.tdb` cerca del tile `(center_x, center_z)` y tiles vecinos (`grid_radius`).
pub fn collect_tdb_chords(
    ctx: &TdbContext,
    center_tile_x: i32,
    center_tile_z: i32,
    grid_radius: u32,
) -> Vec<(Vec3, Vec3)> {
    collect_tdb_chords_full(ctx, center_tile_x, center_tile_z, grid_radius)
        .into_iter()
        .map(|c| (c.start_world, c.end_world))
        .collect()
}

/// Acordes `.tdb` con `shape_idx` (para vía UKFS / procedural).
pub fn collect_tdb_shaped_chords(
    ctx: &TdbContext,
    center_tile_x: i32,
    center_tile_z: i32,
    grid_radius: u32,
) -> Vec<(Vec3, Vec3, u32)> {
    let chords = collect_tdb_chords_full(ctx, center_tile_x, center_tile_z, grid_radius);
    shaped_chords_from_tdb(&chords, true)
        .into_iter()
        .filter(|(_, _, shape_idx)| *shape_idx != 0)
        .collect()
}

/// Instancia UKFS a colocar a lo largo de un acorde `.tdb` (coords escena).
#[derive(Clone, Copy, Debug)]
pub struct TdbUkfsInstance {
    pub section_idx: u32,
    pub position: Vec3,
    pub rotation: Quat,
}

/// Genera instancias de shapes UKFS a lo largo de acordes `.tdb` (espacio escena).
pub fn tdb_ukfs_instances_for_tile(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
    center_tile: (i32, i32),
    heights: &TileHeightIndex,
) -> Vec<TdbUkfsInstance> {
    tdb_ukfs_instances_scene(shaped_chords, tsection, center_tile, heights)
}

/// Segmentos procedurales desde acordes `.tdb` (espacio escena).
pub fn tdb_procedural_segments_for_tile(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
    center_tile: (i32, i32),
    heights: &TileHeightIndex,
) -> Vec<crate::dyntrack::ProceduralTrackSegment> {
    tdb_procedural_segments_scene(shaped_chords, tsection, center_tile, heights)
}

/// Rutas MSTS nativas con catálogo UKFS en `tsection.dat`.
pub fn route_has_ukfs_tsection(tsection: &TSectionCatalog) -> bool {
    route_has_ukfs_ssot(tsection)
}

/// Acordes que no llevan mesh UKFS (p. ej. `RoadShape`) → fallback procedural.
pub fn tdb_procedural_chords_for_tile(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
) -> Vec<(Vec3, Vec3, u32)> {
    procedural_fallback_shaped_chords(shaped_chords, tsection)
}

/// Convierte posición Bevy world → coords locales del tile (origen en esquina SW).
pub fn world_to_tile_local(world: Vec3, tile_x: i32, tile_z: i32) -> (f32, f32) {
    world_to_tile_local_ssot(world, tile_x, tile_z, TILE_SIZE_M)
}

/// Convierte posición Bevy world → coords locales centradas del tile (espacio terreno/objetos).
pub fn world_to_tile_local_centered(world: Vec3, tile_x: i32, tile_z: i32) -> (f32, f32) {
    world_to_tile_local_centered_ssot(world, tile_x, tile_z, TILE_SIZE_M)
}

/// World Bevy → XZ en espacio de escena (origen = centro del tile focal).
pub fn world_to_scene_xz(world: Vec3, center_tile_x: i32, center_tile_z: i32) -> (f32, f32) {
    world_to_scene_xz_ssot(world, center_tile_x, center_tile_z, TILE_SIZE_M)
}

/// XZ de escena → world Bevy (inverso de [`world_to_scene_xz`]).
pub fn scene_xz_to_world(x: f32, z: f32, center_tile_x: i32, center_tile_z: i32) -> (f32, f32) {
    scene_xz_to_world_ssot(x, z, center_tile_x, center_tile_z, TILE_SIZE_M)
}

const _: () = assert!(TILE_SIZE_M == MSTS_TILE_SIZE_M);

/// Referencia vertical de escena para colocar vía `.tdb`.
///
/// Owned (no lifetime) so a batch can build the index **once** and reuse it
/// across all tiles in the Track phase / stream catalog (#63).
#[derive(Clone)]
pub struct TileHeightIndex {
    tiles: Vec<(i32, i32, crate::terrain::TileHeight)>,
    scene_base_y: f32,
}

impl TileHeightIndex {
    /// Build from borrowed height rows (clones each [`TileHeight`] once).
    pub fn from_tile_heights<'a>(
        rows: impl IntoIterator<Item = (i32, i32, &'a crate::terrain::TileHeight)>,
        center_tile: (i32, i32),
    ) -> Self {
        let tiles: Vec<_> = rows
            .into_iter()
            .map(|(x, z, height)| (x, z, height.clone()))
            .collect();
        let scene_base_y = tiles
            .iter()
            .find(|(x, z, _)| *x == center_tile.0 && *z == center_tile.1)
            .or_else(|| tiles.first())
            .map(|(_, _, height)| height.base_y())
            .unwrap_or(0.0);
        Self {
            tiles,
            scene_base_y,
        }
    }

    /// Compatibility wrapper over a temporary slice of references.
    pub fn new(
        tiles: &[(i32, i32, &crate::terrain::TileHeight)],
        center_tile: (i32, i32),
    ) -> Self {
        Self::from_tile_heights(
            tiles.iter().map(|(x, z, h)| (*x, *z, *h)),
            center_tile,
        )
    }

    /// Sorted `(tile_x, tile_z)` keys — fingerprint for cache invalidation (#63).
    pub fn fingerprint(&self) -> Vec<(i32, i32)> {
        let mut keys: Vec<_> = self.tiles.iter().map(|(x, z, _)| (*x, *z)).collect();
        keys.sort_unstable();
        keys
    }

    pub fn scene_base_y(&self) -> f32 {
        self.scene_base_y
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Y local del heightfield (misma convención que el mesh de terreno).
    fn terrain_local_y(&self, world: Vec3) -> Option<f32> {
        let tx = msts_tile_x_index_for_coord(world.x);
        let tz = msts_tile_z_index_for_coord(world.z);
        let (_, _, height) = self.tiles.iter().find(|(x, z, _)| *x == tx && *z == tz)?;
        let (lx, lz) = world_to_tile_local_centered(Vec3::new(world.x, 0.0, world.z), tx, tz);
        Some(height.local_y(lx, lz))
    }

    /// MSL en metros: heightfield si hay `.t`, si no el nodo `.tdb`.
    fn msl_at_world(&self, world: Vec3) -> f32 {
        let tx = msts_tile_x_index_for_coord(world.x);
        let tz = msts_tile_z_index_for_coord(world.z);
        if let Some((_, _, height)) = self.tiles.iter().find(|(x, z, _)| *x == tx && *z == tz) {
            let (lx, lz) = world_to_tile_local_centered(Vec3::new(world.x, 0.0, world.z), tx, tz);
            return height.local_y(lx, lz) + height.base_y();
        }
        world.y
    }

    fn unified_scene_y(&self, world: Vec3) -> f32 {
        self.msl_at_world(world) - self.scene_base_y
    }

    fn tile_index_at(&self, world: Vec3) -> Option<(i32, i32)> {
        let tx = msts_tile_x_index_for_coord(world.x);
        let tz = msts_tile_z_index_for_coord(world.z);
        if self.tiles.iter().any(|(x, z, _)| *x == tx && *z == tz) {
            Some((tx, tz))
        } else {
            None
        }
    }

    /// ¿Usar MSL unificada en todo el acorde? (tiles sin `.t`, cruza tiles o barranco falso)
    fn chord_uses_unified_msl(&self, start: Vec3, end: Vec3, center_tile: (i32, i32)) -> bool {
        let tile_start = self.tile_index_at(start);
        let tile_end = self.tile_index_at(end);
        if tile_start.is_none() || tile_end.is_none() {
            return true;
        }
        if tile_start != tile_end {
            return true;
        }
        let t0 = self.terrain_local_y(start).unwrap();
        let t1 = self.terrain_local_y(end).unwrap();
        let msl0 = start.y;
        let msl1 = end.y;
        let (sx, sz) = world_to_scene_xz(start, center_tile.0, center_tile.1);
        let (ex, ez) = world_to_scene_xz(end, center_tile.0, center_tile.1);
        let horiz = ((ex - sx).powi(2) + (ez - sz).powi(2)).sqrt();
        if horiz < 0.5 {
            return false;
        }
        let terrain_grade = (t1 - t0).abs() / horiz;
        let msl_grade = (msl1 - msl0).abs() / horiz;
        const TERRAIN_SPIKE_GRADE: f32 = 0.25;
        const MSL_FLAT_GRADE: f32 = 0.08;
        terrain_grade > TERRAIN_SPIKE_GRADE && msl_grade < MSL_FLAT_GRADE
    }

    fn chord_param_t(&self, world: Vec3, start: Vec3, end: Vec3) -> f32 {
        let dx = end.x - start.x;
        let dz = end.z - start.z;
        let len_sq = dx * dx + dz * dz;
        if len_sq < 1e-6 {
            return 0.0;
        }
        let wx = world.x - start.x;
        let wz = world.z - start.z;
        ((wx * dx + wz * dz) / len_sq).clamp(0.0, 1.0)
    }

    fn interpolated_local_on_chord(&self, world: Vec3, start: Vec3, end: Vec3) -> f32 {
        let t = self.chord_param_t(world, start, end);
        let y0 = self
            .terrain_local_y(start)
            .unwrap_or_else(|| self.unified_scene_y(start));
        let y1 = self
            .terrain_local_y(end)
            .unwrap_or_else(|| self.unified_scene_y(end));
        y0 + (y1 - y0) * t
    }

    /// Altura de riel coherente con el acorde (evita picos y despegues del terreno).
    pub fn rail_y_on_chord(
        &self,
        world: Vec3,
        chord_start: Vec3,
        chord_end: Vec3,
        center_tile: (i32, i32),
    ) -> f32 {
        use crate::track::RAIL_LIFT_M;
        if self.chord_uses_unified_msl(chord_start, chord_end, center_tile) {
            return (world.y - self.scene_base_y) + RAIL_LIFT_M;
        }
        if let Some(local) = self.terrain_local_y(world) {
            return local + RAIL_LIFT_M;
        }
        self.interpolated_local_on_chord(world, chord_start, chord_end) + RAIL_LIFT_M
    }

    /// Altura de riel en un punto aislado (p. ej. UKFS sin contexto de acorde).
    #[allow(dead_code)]
    pub fn rail_y_at_world(&self, world: Vec3) -> f32 {
        use crate::track::RAIL_LIFT_M;
        if let Some(local) = self.terrain_local_y(world) {
            return local + RAIL_LIFT_M;
        }
        self.unified_scene_y(world) + RAIL_LIFT_M
    }

    #[allow(dead_code)]
    pub fn resolve_rail_endpoint_y(
        &self,
        start: Vec3,
        end: Vec3,
        center_tile: (i32, i32),
    ) -> (f32, f32) {
        (
            self.rail_y_on_chord(start, start, end, center_tile),
            self.rail_y_on_chord(end, start, end, center_tile),
        )
    }

    pub fn rail_y_at_scene(
        &self,
        scene_x: f32,
        scene_z: f32,
        msl_y: f32,
        center_tile: (i32, i32),
        chord_start: Vec3,
        chord_end: Vec3,
    ) -> f32 {
        let (wx, wz) = scene_xz_to_world(scene_x, scene_z, center_tile.0, center_tile.1);
        self.rail_y_on_chord(
            Vec3::new(wx, msl_y, wz),
            chord_start,
            chord_end,
            center_tile,
        )
    }
}

/// Genera instancias UKFS en espacio de escena (tile central en origen).
pub fn tdb_ukfs_instances_scene(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
    center_tile: (i32, i32),
    heights: &TileHeightIndex,
) -> Vec<TdbUkfsInstance> {
    let mut out = Vec::new();
    for &(start, end, shape_idx) in shaped_chords {
        for p in ukfs_placements_world(
            &[(start, end, shape_idx)],
            tsection,
            DEFAULT_SECTION_LENGTH_M,
        ) {
            let (sx, sz) = world_to_scene_xz(p.position, center_tile.0, center_tile.1);
            let y = heights.rail_y_on_chord(p.position, start, end, center_tile);
            out.push(TdbUkfsInstance {
                section_idx: p.shape_idx,
                position: Vec3::new(sx, y, sz),
                rotation: p.rotation,
            });
        }
    }
    out
}

/// Segmentos procedurales `.tdb` en espacio de escena.
pub fn tdb_procedural_segments_scene(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
    center_tile: (i32, i32),
    heights: &TileHeightIndex,
) -> Vec<crate::dyntrack::ProceduralTrackSegment> {
    use crate::dyntrack::{MSTS_STANDARD_HALF_GAUGE_M, ProceduralTrackSegment};

    shaped_chords
        .iter()
        .filter_map(|(start, end, shape_idx)| {
            let (sx, sz) = world_to_scene_xz(*start, center_tile.0, center_tile.1);
            let (ex, ez) = world_to_scene_xz(*end, center_tile.0, center_tile.1);
            let y0 = heights.rail_y_on_chord(*start, *start, *end, center_tile);
            let y1 = heights.rail_y_on_chord(*end, *start, *end, center_tile);
            let chord = Vec3::new(ex - sx, y1 - y0, ez - sz);
            let len = chord.length();
            if len < 0.5 {
                return None;
            }
            let pos = Vec3::new(sx, y0, sz);
            let rot = crate::dyntrack::quat_align_positive_z_to(chord);
            let link = tsection
                .procedural_links_primary_path(*shape_idx)
                .into_iter()
                .next();
            let half_gauge = link
                .as_ref()
                .map(|l| l.dims.half_gauge_m as f32)
                .filter(|g| *g > 0.1)
                .unwrap_or(MSTS_STANDARD_HALF_GAUGE_M);
            Some(ProceduralTrackSegment {
                position: pos,
                rotation: rot,
                length_m: Some(len),
                half_gauge_m: Some(half_gauge),
                curve_radius_m: None,
                curve_angle_deg: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::load_tile_geometry;
    use crate::track::build_tdb_track_ribbon;
    use openrailsrs_bevy_scenery::spawn::tdb_track::tdb_chord_geometry_hash;

    #[test]
    fn ssot_chord_hash_matches_adapter_pairs() {
        // Synthetic: empty TDB yields empty hash consistently.
        let tdb = TrackDbFile::default();
        let focus = FocusQuery::new(Vec3::ZERO, 100.0);
        let chords = collect_ssot(&tdb, &focus, None, ChordCollectLimits::PER_VECTOR_ONLY);
        assert_eq!(tdb_chord_geometry_hash(&chords), tdb_chord_geometry_hash(&[]));
    }

    #[test]
    fn watersnake_center_tile_ribbon_supports_default_camera() {
        use crate::player_spawn::default_track_camera_pose;
        use crate::tdb_track::{collect_tdb_chords, load_tdb_context};
        use crate::{TileEntry, TilesToRender};

        let route = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/Routes/Watersnake"))
            .filter(|p| p.join("Watersnake.tdb").is_file());
        let Some(route) = route else {
            return;
        };
        let ctx = load_tdb_context(&route).expect("tdb");
        let (tx, tz) = (-6144, 14900);
        let chords = collect_tdb_chords(&ctx, tx, tz, 2);
        let tile = load_tile_geometry(&route, tx, tz).expect("tile");
        let heights = [(tx, tz, &tile.height)];
        let height_index = TileHeightIndex::new(&heights, (tx, tz));
        let ribbon = build_tdb_track_ribbon(&chords, tx, tz, &height_index, 2);
        eprintln!(
            "Watersnake center ribbon: {} segments from {} chords",
            ribbon.segment_count(),
            chords.len()
        );
        let entry = TileEntry {
            geometry: tile,
            world_offset: Vec3::ZERO,
            track: ribbon.clone(),
            objects: Vec::new(),
        };
        let pose = default_track_camera_pose(&TilesToRender(vec![entry]));
        eprintln!("default camera pose: {pose:?}");
        assert!(
            ribbon.segment_count() > 0,
            "tile central debe tener cinta para posicionar cámara"
        );
        assert!(pose.is_some(), "default_track_camera_pose debe resolver");
    }

    #[test]
    fn new_forest_tdb_loads_and_has_chords_near_tile() {
        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("Watersnake.tdb").is_file().then_some(p)
            });
        let Some(route) = route else {
            return;
        };
        let ctx = load_tdb_context(&route).expect("tdb");
        assert!(
            ctx.tsection.shapes.len() > 1000,
            "GLOBAL tsection debería aportar formas, tuvo {}",
            ctx.tsection.shapes.len()
        );
        assert!(
            ctx.track_db.nodes.len() > 100,
            "Watersnake.tdb debería tener muchos nodos"
        );
        let chords = collect_tdb_chords(&ctx, -6131, 14898, 0);
        assert!(
            chords.len() > 50,
            "esperaba acordes cerca del tile principal, tuvo {}",
            chords.len()
        );
        let tile = load_tile_geometry(&route, -6131, 14898).expect("tile");
        let (tx, tz) = (-6131, 14898);
        let heights = [(tx, tz, &tile.height)];
        let height_index = TileHeightIndex::new(&heights, (tx, tz));
        let ribbon = build_tdb_track_ribbon(&chords, tx, tz, &height_index, 0);
        assert!(
            ribbon.segment_count() > 20,
            "esperaba vía visible, tuvo {} segmentos",
            ribbon.segment_count()
        );
    }

    #[test]
    fn watersnake_procedural_segments_avoid_vertical_spikes() {
        let route = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/Routes/Watersnake"))
            .filter(|p| p.join("Watersnake.tdb").is_file());
        let Some(route) = route else {
            return;
        };
        let ctx = load_tdb_context(&route).expect("tdb");
        let (tx, tz) = (-6144, 14900);
        let radius = 2u32;
        let shaped = collect_tdb_shaped_chords(&ctx, tx, tz, radius);
        let mut height_geoms = Vec::new();
        for dx in -(radius as i32)..=(radius as i32) {
            for dz in -(radius as i32)..=(radius as i32) {
                let gx = tx + dx;
                let gz = tz + dz;
                if let Ok(tile) = load_tile_geometry(&route, gx, gz) {
                    height_geoms.push((gx, gz, tile));
                }
            }
        }
        let height_buf: Vec<_> = height_geoms
            .iter()
            .map(|(x, z, t)| (*x, *z, &t.height))
            .collect();
        let height_index = TileHeightIndex::new(&height_buf, (tx, tz));
        let segments =
            tdb_procedural_segments_scene(&shaped, &ctx.tsection, (tx, tz), &height_index);
        let mut max_grade = 0.0f32;
        for seg in &segments {
            let forward = seg.rotation * Vec3::Z;
            let horiz = (forward.x * forward.x + forward.z * forward.z).sqrt();
            if horiz > 0.01 {
                let grade = forward.y.abs() / horiz;
                max_grade = max_grade.max(grade);
            }
        }
        eprintln!(
            "Watersnake r={radius}: {} procedural segments, max pitch grade={max_grade:.3}",
            segments.len()
        );
        assert!(
            max_grade < 0.35,
            "pendiente máxima demasiado pronunciada (spike vertical?): {max_grade:.3}"
        );
    }

    #[test]
    fn height_index_rail_y_stable_across_owned_rebuild() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let (tx, tz) = (-6082, 14925);
        let Ok(tile) = load_tile_geometry(&route, tx, tz) else {
            eprintln!("skip: Chiltern tile missing");
            return;
        };
        let rows = [(tx, tz, &tile.height)];
        let a = TileHeightIndex::new(&rows, (tx, tz));
        let b = TileHeightIndex::from_tile_heights([(tx, tz, &tile.height)], (tx, tz));
        let world = Vec3::new(
            tx as f32 * crate::track::TILE_SIZE_M + 100.0,
            tile.height.base_y() + 10.0,
            -(tz as f32 * crate::track::TILE_SIZE_M + 100.0),
        );
        let ya = a.rail_y_at_world(world);
        let yb = b.rail_y_at_world(world);
        assert!(
            (ya - yb).abs() < 1e-4,
            "owned rebuild must keep rail Y (a={ya} b={yb})"
        );
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.scene_base_y(), b.scene_base_y());
    }

    #[test]
    fn new_forest_central_tile_gets_tdb_ukfs_instances() {
        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("Watersnake.tdb").is_file().then_some(p)
            });
        let Some(route) = route else {
            return;
        };
        let ctx = load_tdb_context(&route).expect("tdb");
        let (tx, tz) = (-6144, 14900);
        let shaped = collect_tdb_shaped_chords(&ctx, tx, tz, 0);
        let tile = load_tile_geometry(&route, tx, tz).expect("tile");
        let heights = [(tx, tz, &tile.height)];
        let height_index = TileHeightIndex::new(&heights, (tx, tz));
        let instances =
            tdb_ukfs_instances_for_tile(&shaped, &ctx.tsection, (tx, tz), &height_index);
        eprintln!(
            "NF central ({tx},{tz}): {} chords, {} ukfs instances",
            shaped.len(),
            instances.len()
        );
        assert!(
            instances.len() > 40,
            "tile central deberia tener decenas de instancias UKFS desde .tdb, tuvo {}",
            instances.len()
        );
        for inst in instances.iter().take(5) {
            assert!(
                inst.position.y.is_finite() && inst.position.y.abs() < 500.0,
                "Y escena UKFS fuera de rango sensato: {}",
                inst.position.y
            );
            assert!(
                inst.position.x.abs() <= 5000.0 && inst.position.z.abs() <= 5000.0,
                "XZ escena UKFS fuera del radio esperado: {:?}",
                inst.position
            );
        }
    }
}
