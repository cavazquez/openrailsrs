//! Vía procedural desde el grafo `.tdb` (TrVectorSection), adaptado de `viewer3d`.
//!
//! Emite pares de puntos Bevy world (X, Y, Z) por tramo de sección vectorial.

use std::collections::{HashMap, HashSet};
use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

use bevy::math::{EulerRot, Quat, Vec3};
use openrailsrs_formats::{
    TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind,
    TrackVectorGeometry, TrackVectorPoint, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord,
};

use crate::track::TILE_SIZE_M;

const DEFAULT_SECTION_LENGTH_M: f32 = 25.0;
/// Radio de recolección extra alrededor del tile (m).
const TILE_CHORD_MARGIN_M: f32 = 128.0;
const JUNCTION_FACE_FALLBACK_DIST_M: f32 = 60.0;
const SHORT_VECTOR_JUNCTION_FACE_FALLBACK_DIST_M: f32 = 30.0;

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

#[derive(Clone, Copy, Debug)]
struct TileFocus {
    center: Vec3,
    radius_m: f32,
}

impl TileFocus {
    fn for_tile(tile_x: i32, tile_z: i32, extra_radius_m: f32) -> Self {
        Self {
            center: Vec3::new(
                tile_x as f32 * TILE_SIZE_M,
                0.0,
                -(tile_z as f32 * TILE_SIZE_M),
            ),
            radius_m: TILE_SIZE_M * 0.5 + TILE_CHORD_MARGIN_M + extra_radius_m,
        }
    }

    fn horizontal_distance(&self, p: Vec3) -> f32 {
        let dx = p.x - self.center.x;
        let dz = p.z - self.center.z;
        (dx * dx + dz * dz).sqrt()
    }
}

#[derive(Clone, Copy, Debug)]
struct Chord {
    start: Vec3,
    end: Vec3,
    node_id: u32,
    shape_idx: u32,
}

#[derive(Clone, Copy, Debug)]
struct AnchorPoint {
    world: Vec3,
    node_id: u32,
    section_index: usize,
    shape_idx: u32,
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

/// Acordes `.tdb` cerca del tile `(center_x, center_z)` y tiles vecinos (`grid_radius`).
pub fn collect_tdb_chords(
    ctx: &TdbContext,
    center_tile_x: i32,
    center_tile_z: i32,
    grid_radius: u32,
) -> Vec<(Vec3, Vec3)> {
    let extra = grid_radius as f32 * TILE_SIZE_M;
    let focus = TileFocus::for_tile(center_tile_x, center_tile_z, extra);
    let tsection = Some(&ctx.tsection);
    let per_vector = collect_per_vector_chords(&ctx.track_db, &focus, tsection);
    let bridges = collect_junction_bridge_chords(&ctx.track_db, &focus, &per_vector);
    dedupe_chords(per_vector.into_iter().chain(bridges).collect())
        .into_iter()
        .map(|c| (c.start, c.end))
        .collect()
}

/// Acordes `.tdb` con `shape_idx` (para vía UKFS / procedural).
pub fn collect_tdb_shaped_chords(
    ctx: &TdbContext,
    center_tile_x: i32,
    center_tile_z: i32,
    grid_radius: u32,
) -> Vec<(Vec3, Vec3, u32)> {
    let extra = grid_radius as f32 * TILE_SIZE_M;
    let focus = TileFocus::for_tile(center_tile_x, center_tile_z, extra);
    let tsection = Some(&ctx.tsection);
    let per_vector = collect_per_vector_chords(&ctx.track_db, &focus, tsection);
    let bridges = collect_junction_bridge_chords(&ctx.track_db, &focus, &per_vector);
    dedupe_chords(per_vector.into_iter().chain(bridges).collect())
        .into_iter()
        .map(|c| (c.start, c.end, c.shape_idx))
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
    tsection.shapes.len() > 500
}

/// Acordes que no llevan mesh UKFS (p. ej. `RoadShape`) → fallback procedural.
pub fn tdb_procedural_chords_for_tile(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
) -> Vec<(Vec3, Vec3, u32)> {
    shaped_chords
        .iter()
        .copied()
        .filter(|(_, _, shape_idx)| {
            *shape_idx != 0
                && (tsection.is_road_shape(*shape_idx)
                    || tsection.shape_file_name(*shape_idx).is_none())
        })
        .collect()
}

fn collect_per_vector_chords(
    tdb: &TrackDbFile,
    focus: &TileFocus,
    tsection: Option<&TSectionCatalog>,
) -> Vec<Chord> {
    let mut out = Vec::new();
    for node in &tdb.nodes {
        let TrackNodeKind::Vector {
            length_m: node_length_m,
            sections,
            geometry,
            ..
        } = &node.kind
        else {
            continue;
        };
        let sections: Vec<_> = sections
            .iter()
            .copied()
            .filter(|s| s.shape_idx != 0)
            .collect();
        if sections.is_empty() {
            continue;
        }
        out.extend(collect_vector_section_chords(
            node.id,
            &sections,
            *geometry,
            *node_length_m,
            focus,
            tsection,
        ));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn collect_vector_section_chords(
    node_id: u32,
    sections: &[TrVectorSectionRecord],
    geometry: Option<TrackVectorGeometry>,
    node_length_m: f64,
    focus: &TileFocus,
    tsection: Option<&TSectionCatalog>,
) -> Vec<Chord> {
    let mut chain_hint = Some(focus.center);
    let mut out = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        let start = section_world_vec3(*section, chain_hint);
        let end = if section_index + 1 < sections.len() {
            section_world_vec3(sections[section_index + 1], Some(start))
        } else if let Some(exit) = single_section_end_world(
            *section,
            geometry,
            node_length_m,
            false,
            Some(start),
            tsection,
            sections.len(),
        ) {
            exit
        } else {
            continue;
        };
        chain_hint = Some(end);
        if focus.horizontal_distance(start) > focus.radius_m
            && focus.horizontal_distance(end) > focus.radius_m
        {
            continue;
        }
        if chord_length_xz(start, end) < 0.5 {
            continue;
        }
        out.push(Chord {
            start,
            end,
            node_id,
            shape_idx: section.shape_idx,
        });
    }
    out
}

fn dedupe_chords(chords: Vec<Chord>) -> Vec<Chord> {
    let mut seen = HashSet::new();
    chords
        .into_iter()
        .filter(|c| {
            let key = (
                (c.start.x * 2.0).round() as i32,
                (c.start.z * 2.0).round() as i32,
                (c.end.x * 2.0).round() as i32,
                (c.end.z * 2.0).round() as i32,
            );
            seen.insert(key)
        })
        .collect()
}

fn section_world_vec3(section: TrVectorSectionRecord, near_hint: Option<Vec3>) -> Vec3 {
    let (dx, _, dz) = section.start.bevy_position();
    let (near_x, near_z) = near_hint.map(|h| (h.x, h.z)).unwrap_or((dx, dz));
    let (x, y, z) = section.bevy_position_nearest_to(
        near_x,
        near_z,
        Some((section.header_tile_x, section.header_tile_z)),
    );
    Vec3::new(x, y, z)
}

fn point_world_vec3(
    point: TrackVectorPoint,
    header_tile: (i32, i32),
    near_hint: Option<Vec3>,
) -> Vec3 {
    let (dx, _, dz) = point.bevy_position();
    let (near_x, near_z) = near_hint.map(|h| (h.x, h.z)).unwrap_or((dx, dz));
    let (x, y, z) =
        point.bevy_position_nearest_to(near_x, near_z, Some(header_tile), Some(header_tile));
    Vec3::new(x, y, z)
}

fn chord_length_xz(from: Vec3, to: Vec3) -> f32 {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    (dx * dx + dz * dz).sqrt()
}

fn single_section_end_world(
    section: TrVectorSectionRecord,
    geometry: Option<TrackVectorGeometry>,
    node_length_m: f64,
    reversed: bool,
    near_hint: Option<Vec3>,
    tsection: Option<&TSectionCatalog>,
    section_count: usize,
) -> Option<Vec3> {
    let start = section_world_vec3(section, near_hint);
    if let Some(geom) = geometry {
        let header = (section.header_tile_x, section.header_tile_z);
        let end_pt = point_world_vec3(geom.end, header, near_hint);
        if chord_length_xz(start, end_pt) >= 0.5 {
            return Some(end_pt);
        }
    }
    let heading = section.heading_deg()?;
    let len = section_shape_length_m(tsection, section.shape_idx, node_length_m, section_count);
    let h = if reversed { heading + 180.0 } else { heading };
    Some(end_from_heading(start, h, len))
}

fn section_shape_length_m(
    tsection: Option<&TSectionCatalog>,
    shape_idx: u32,
    node_length_m: f64,
    section_count: usize,
) -> f32 {
    if let Some(cat) = tsection {
        if let Some(dims) = cat.procedural_dims(shape_idx) {
            if dims.length_m > 0.5 {
                return dims.length_m as f32;
            }
        }
    }
    if section_count <= 1 && node_length_m > 0.5 {
        return node_length_m as f32;
    }
    if section_count > 1 && node_length_m > 0.5 {
        return (node_length_m / section_count as f64) as f32;
    }
    DEFAULT_SECTION_LENGTH_M
}

fn end_from_heading(start: Vec3, heading_deg: f64, length_m: f32) -> Vec3 {
    let yaw = heading_deg.to_radians() as f32;
    start + Vec3::new(yaw.sin() * length_m, 0.0, yaw.cos() * length_m)
}

fn collect_junction_bridge_chords(
    tdb: &TrackDbFile,
    focus: &TileFocus,
    per_vector: &[Chord],
) -> Vec<Chord> {
    let vector_ids: HashSet<u32> = per_vector.iter().map(|c| c.node_id).collect();
    if vector_ids.is_empty() {
        return Vec::new();
    }
    let nodes_by_id: HashMap<u32, &TrackDbNode> = tdb.nodes.iter().map(|n| (n.id, n)).collect();
    let mut seen_pairs = HashSet::new();
    let mut out = Vec::new();

    for &a in &vector_ids {
        for b in connected_vector_neighbors(a, &vector_ids, &nodes_by_id) {
            let pair = if a < b { (a, b) } else { (b, a) };
            if !seen_pairs.insert(pair) {
                continue;
            }
            let Some((side_a, anchor_a, side_b)) = facing_junction_endpoints(a, b, &nodes_by_id)
            else {
                continue;
            };
            if side_a.distance(side_b) < 0.25 {
                continue;
            }
            if chord_length_xz(side_a, side_b) < 0.5 {
                continue;
            }
            if focus.horizontal_distance(side_a) > focus.radius_m
                && focus.horizontal_distance(side_b) > focus.radius_m
            {
                continue;
            }
            out.push(Chord {
                start: side_a,
                end: side_b,
                node_id: anchor_a.node_id,
                shape_idx: anchor_a.shape_idx,
            });
        }
    }
    out
}

fn connected_vector_neighbors(
    vector_id: u32,
    vector_ids: &HashSet<u32>,
    nodes_by_id: &HashMap<u32, &TrackDbNode>,
) -> Vec<u32> {
    let Some(node) = nodes_by_id.get(&vector_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pin in &node.pin_refs {
        if pin.node_id != vector_id && vector_ids.contains(&pin.node_id) {
            out.push(pin.node_id);
        }
        let Some(pin_node) = nodes_by_id.get(&pin.node_id) else {
            continue;
        };
        for next in &pin_node.pin_refs {
            if next.node_id != vector_id && vector_ids.contains(&next.node_id) {
                out.push(next.node_id);
            }
        }
    }
    out
}

fn facing_junction_endpoints(
    a: u32,
    b: u32,
    nodes_by_id: &HashMap<u32, &TrackDbNode>,
) -> Option<(Vec3, AnchorPoint, Vec3)> {
    let node_a = nodes_by_id.get(&a)?;
    let node_b = nodes_by_id.get(&b)?;

    if let Some(pin_a) = node_a.pin_refs.iter().position(|p| p.node_id == b) {
        let pin_b = node_b.pin_refs.iter().position(|p| p.node_id == a)?;
        let hint = direct_link_hint(node_a, pin_a, node_b, pin_b);
        let anchor_a = nearest_oriented_anchor(node_a, pin_a, hint)?;
        let anchor_b = nearest_oriented_anchor(node_b, pin_b, hint)?;
        return Some((anchor_a.world, anchor_a, anchor_b.world));
    }

    for (pin_a_idx, pin_a) in node_a.pin_refs.iter().enumerate() {
        let Some(mid) = nodes_by_id.get(&pin_a.node_id) else {
            continue;
        };
        if !matches!(mid.kind, TrackNodeKind::Junction { .. }) {
            continue;
        }
        if !node_b.pin_refs.iter().any(|p| p.node_id == pin_a.node_id) {
            continue;
        }
        let pin_b_idx = node_b
            .pin_refs
            .iter()
            .position(|p| p.node_id == pin_a.node_id)?;
        let hint = junction_link_hint(node_a, pin_a_idx, node_b, pin_b_idx, mid)?;
        let junction_point = mid.position?;
        let anchor_a = nearest_junction_face_anchor(node_a, pin_a_idx, junction_point, hint)?;
        let anchor_b = nearest_junction_face_anchor(node_b, pin_b_idx, junction_point, hint)?;
        return Some((anchor_a.world, anchor_a, anchor_b.world));
    }
    None
}

fn junction_link_hint(
    node_a: &TrackDbNode,
    pin_a: usize,
    node_b: &TrackDbNode,
    pin_b: usize,
    junction: &TrackDbNode,
) -> Option<Vec3> {
    if let Some(j) = node_world_position(junction) {
        return Some(j);
    }
    let a0 = vector_oriented_anchors(node_a, pin_a, None, None)
        .into_iter()
        .next()
        .map(|a| a.world);
    let b0 = vector_oriented_anchors(node_b, pin_b, None, None)
        .into_iter()
        .next()
        .map(|a| a.world);
    match (a0, b0) {
        (Some(wa), Some(wb)) => Some((wa + wb) * 0.5),
        (Some(wa), None) => Some(wa),
        (None, Some(wb)) => Some(wb),
        (None, None) => None,
    }
}

fn direct_link_hint(
    node_a: &TrackDbNode,
    pin_a: usize,
    node_b: &TrackDbNode,
    pin_b: usize,
) -> Vec3 {
    let mut pts = Vec::new();
    if let Some(a) = vector_oriented_anchors(node_a, pin_a, None, None)
        .into_iter()
        .next()
    {
        pts.push(a.world);
    }
    if let Some(b) = vector_oriented_anchors(node_b, pin_b, None, None)
        .into_iter()
        .next()
    {
        pts.push(b.world);
    }
    if pts.is_empty() {
        Vec3::ZERO
    } else {
        pts.iter().copied().sum::<Vec3>() / pts.len() as f32
    }
}

fn node_world_position(node: &TrackDbNode) -> Option<Vec3> {
    node.position.map(|p| {
        let (x, y, z) = p.bevy_position();
        Vec3::new(x, y, z)
    })
}

fn nearest_oriented_anchor(
    node: &TrackDbNode,
    entry_pin: usize,
    near: Vec3,
) -> Option<AnchorPoint> {
    vector_oriented_anchors(node, entry_pin, Some(near), None)
        .into_iter()
        .min_by(|a, b| {
            a.world
                .distance(near)
                .partial_cmp(&b.world.distance(near))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn nearest_junction_face_anchor(
    node: &TrackDbNode,
    entry_pin: usize,
    junction_point: TrackVectorPoint,
    hint: Vec3,
) -> Option<AnchorPoint> {
    let TrackNodeKind::Vector { sections, .. } = &node.kind else {
        return nearest_oriented_anchor(node, entry_pin, hint);
    };
    let fallback_dist = if sections.len() <= 2 {
        SHORT_VECTOR_JUNCTION_FACE_FALLBACK_DIST_M
    } else {
        JUNCTION_FACE_FALLBACK_DIST_M
    };
    let (jx, _, jz) = junction_point.bevy_position();
    let near_hint = Some(Vec3::new(jx, hint.y, jz));
    let ref_tile = Some(junction_point);
    let mut best: Option<AnchorPoint> = None;
    let mut best_dist = f32::INFINITY;
    for anchor in vector_oriented_anchors(node, entry_pin, near_hint, None) {
        let section = sections.get(anchor.section_index).copied();
        let mut worlds = vec![anchor.world];
        if let Some(section) = section {
            worlds.extend(
                section
                    .bevy_position_candidates(ref_tile)
                    .into_iter()
                    .map(|(x, y, z)| Vec3::new(x, y, z)),
            );
        }
        for world in worlds {
            let dist = world.distance(hint);
            if dist < best_dist {
                best_dist = dist;
                best = Some(AnchorPoint {
                    world,
                    node_id: anchor.node_id,
                    section_index: anchor.section_index,
                    shape_idx: anchor.shape_idx,
                });
            }
        }
    }
    if let Some(anchor) = best {
        if best_dist <= fallback_dist {
            return Some(anchor);
        }
        return Some(AnchorPoint {
            world: hint,
            node_id: anchor.node_id,
            section_index: anchor.section_index,
            shape_idx: anchor.shape_idx,
        });
    }
    nearest_oriented_anchor(node, entry_pin, hint)
}

fn vector_oriented_anchors(
    node: &TrackDbNode,
    entry_pin: usize,
    near_hint: Option<Vec3>,
    tsection: Option<&TSectionCatalog>,
) -> Vec<AnchorPoint> {
    let TrackNodeKind::Vector {
        sections,
        geometry,
        length_m: node_length_m,
        ..
    } = &node.kind
    else {
        return Vec::new();
    };

    let sections: Vec<_> = sections
        .iter()
        .copied()
        .filter(|s| s.shape_idx != 0)
        .collect();
    if sections.is_empty() {
        return Vec::new();
    }

    let forward: Vec<(usize, TrVectorSectionRecord)> = sections.into_iter().enumerate().collect();
    let ordered: Vec<(usize, TrVectorSectionRecord)> = if entry_pin == 0 {
        forward
    } else {
        forward.into_iter().rev().collect()
    };

    let mut out = Vec::new();
    let mut chain_hint = near_hint;
    for (idx, section) in &ordered {
        let world = section_world_vec3(*section, chain_hint);
        out.push(AnchorPoint {
            world,
            node_id: node.id,
            section_index: *idx,
            shape_idx: section.shape_idx,
        });
        chain_hint = Some(world);
    }

    if let Some((idx, section)) = ordered.last().copied() {
        let section_count = ordered.len();
        if let Some(exit) = single_section_end_world(
            section,
            *geometry,
            *node_length_m,
            entry_pin != 0,
            chain_hint,
            tsection,
            section_count,
        ) && out
            .last()
            .is_none_or(|last| last.world.distance(exit) > 0.5)
        {
            out.push(AnchorPoint {
                world: exit,
                node_id: node.id,
                section_index: idx,
                shape_idx: section.shape_idx,
            });
        }
    }
    out
}

/// Convierte posición Bevy world → coords locales del tile (origen en esquina SW).
pub fn world_to_tile_local(world: Vec3, tile_x: i32, tile_z: i32) -> (f32, f32) {
    let cx = tile_x as f32 * TILE_SIZE_M;
    let cz = -(tile_z as f32 * TILE_SIZE_M);
    (world.x - cx, world.z - cz)
}

/// Convierte posición Bevy world → coords locales centradas del tile (espacio terreno/objetos).
pub fn world_to_tile_local_centered(world: Vec3, tile_x: i32, tile_z: i32) -> (f32, f32) {
    const HALF: f32 = crate::track::TILE_SIZE_M * 0.5;
    let (lx, lz) = world_to_tile_local(world, tile_x, tile_z);
    (lx - HALF, lz - HALF)
}

/// World Bevy → XZ en espacio de escena (origen = centro del tile focal).
pub fn world_to_scene_xz(world: Vec3, center_tile_x: i32, center_tile_z: i32) -> (f32, f32) {
    world_to_tile_local_centered(world, center_tile_x, center_tile_z)
}

/// XZ de escena → world Bevy (inverso de [`world_to_scene_xz`]).
pub fn scene_xz_to_world(x: f32, z: f32, center_tile_x: i32, center_tile_z: i32) -> (f32, f32) {
    const HALF: f32 = crate::track::TILE_SIZE_M * 0.5;
    let cx = center_tile_x as f32 * TILE_SIZE_M;
    let cz = -(center_tile_z as f32 * TILE_SIZE_M);
    (x + cx + HALF, z + cz + HALF)
}

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
    for (start, end, shape_idx) in shaped_chords {
        if *shape_idx == 0 || tsection.is_road_shape(*shape_idx) {
            continue;
        }
        if tsection.shape_file_name(*shape_idx).is_none() {
            continue;
        }
        let dx = end.x - start.x;
        let dz = end.z - start.z;
        let chord_len = (dx * dx + dz * dz).sqrt();
        if chord_len < 0.5 {
            continue;
        }
        let dir = Vec3::new(dx / chord_len, 0.0, dz / chord_len);
        let heading = dx.atan2(dz);
        let rot = Quat::from_rotation_y(heading);
        let section_len = tsection
            .procedural_dims(*shape_idx)
            .map(|d| d.length_m as f32)
            .filter(|l| *l > 0.5)
            .unwrap_or(25.0);
        let mut dist = 0.0f32;
        while dist + 0.25 <= chord_len {
            let t = dist / chord_len;
            let wx = start.x + dir.x * dist;
            let wy = start.y + (end.y - start.y) * t;
            let wz = start.z + dir.z * dist;
            let (sx, sz) = world_to_scene_xz(Vec3::new(wx, 0.0, wz), center_tile.0, center_tile.1);
            let y = heights.rail_y_on_chord(Vec3::new(wx, wy, wz), *start, *end, center_tile);
            out.push(TdbUkfsInstance {
                section_idx: *shape_idx,
                position: Vec3::new(sx, y, sz),
                rotation: rot,
            });
            dist += section_len;
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
