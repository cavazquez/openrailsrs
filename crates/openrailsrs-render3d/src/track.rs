//! Capa 3: vía desde `track.toml` (import) o grafo `.tdb` (rutas MSTS/OR nativas).

use std::path::Path;

use bevy::math::Vec3;
use openrailsrs_formats::{msts_tile_x_index_for_coord, msts_tile_z_index_for_coord};
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_track::TrackGraph;

use crate::tdb_track;
use crate::terrain::TileHeight;

pub use crate::tdb_track::{collect_tdb_chords, load_tdb_context};

/// Lado del tile MSTS (m).
pub const TILE_SIZE_M: f32 = 2048.0;
/// Semiancho de la cinta de vía (m); ~3.2 m de ancho total (balasto).
const TRACK_HALF_WIDTH_M: f32 = 1.6;
/// Altura del riel sobre el terreno (m).
pub(crate) const RAIL_LIFT_M: f32 = 0.4;
/// Margen alrededor del tile para incluir aristas que lo cruzan (m).
const TILE_MARGIN_M: f32 = 64.0;
/// Paso de subdivisión de cada segmento (m): la cinta sigue el relieve en vez
/// de hundirse bajo las lomas en los tramos largos.
const SAMPLE_STEP_M: f32 = 4.0;

/// Cinta de vía: triángulos planos apoyados sobre el terreno, en coords locales.
#[derive(Default, Clone, Debug)]
pub struct TrackRibbon {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl TrackRibbon {
    /// Cantidad de quads de la cinta (2 triángulos = 6 índices cada uno).
    pub fn segment_count(&self) -> usize {
        self.indices.len() / 6
    }
}

/// Carga el grafo de `track.toml`, si existe.
pub fn load_graph(route_dir: &Path) -> Option<TrackGraph> {
    load_track_graph_from_route_dir(route_dir).ok()
}

/// Tile `(x, z)` del centroide de los nodos del grafo (cae sobre la línea).
pub fn graph_centroid_tile(graph: &TrackGraph) -> Option<(i32, i32)> {
    let mut sx = 0.0f64;
    let mut sz = 0.0f64;
    let mut n = 0u64;
    for (_id, node) in graph.nodes_iter() {
        sx += node.x_m;
        sz += node.y_m;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let rx = (sx / n as f64) as f32;
    let rz = (sz / n as f64) as f32;
    Some((
        msts_tile_x_index_for_coord(rx),
        msts_tile_z_index_for_coord(rz),
    ))
}

/// Centro del tile en espacio render (Bevy): X = tile_x·2048, Z = −tile_z·2048.
fn tile_center(tile_x: i32, tile_z: i32) -> (f32, f32) {
    (tile_x as f32 * TILE_SIZE_M, -(tile_z as f32) * TILE_SIZE_M)
}

/// Construye la cinta de vía dentro del tile, en coords locales centradas y
/// apoyada sobre el terreno.
pub fn build_track_ribbon(
    graph: &TrackGraph,
    tile_x: i32,
    tile_z: i32,
    height: &TileHeight,
) -> TrackRibbon {
    let mut ribbon = TrackRibbon::default();
    let (cx, cz) = tile_center(tile_x, tile_z);
    let bound = TILE_SIZE_M * 0.5 + TILE_MARGIN_M;
    let in_tile = |lx: f32, lz: f32| lx.abs() <= bound && lz.abs() <= bound;

    for (_id, edge) in graph.edges_iter() {
        let (Some(a), Some(b)) = (graph.node(&edge.from.0), graph.node(&edge.to.0)) else {
            continue;
        };
        let (ax, az) = (a.x_m as f32 - cx, a.y_m as f32 - cz);
        let (bx, bz) = (b.x_m as f32 - cx, b.y_m as f32 - cz);

        if !(in_tile(ax, az) || in_tile(bx, bz)) {
            continue;
        }
        // Recortamos a la caja del tile para que la cinta no se dispare fuera.
        if let Some((cax, caz, cbx, cbz)) = clip_segment_to_box(ax, az, bx, bz, bound) {
            push_segment(&mut ribbon, cax, caz, cbx, cbz, height);
        }
    }
    ribbon
}

/// Cinta de vía desde acordes `.tdb` en espacio de escena (tile central en origen).
pub fn build_tdb_track_ribbon_scene(
    chords: &[(Vec3, Vec3)],
    center_tile_x: i32,
    center_tile_z: i32,
    grid_radius: u32,
    heights: &crate::tdb_track::TileHeightIndex<'_>,
) -> TrackRibbon {
    let mut ribbon = TrackRibbon::default();
    let bound = TILE_SIZE_M * 0.5 + TILE_MARGIN_M + grid_radius as f32 * TILE_SIZE_M;

    for &(start, end) in chords {
        let (ax, az) = tdb_track::world_to_scene_xz(start, center_tile_x, center_tile_z);
        let (bx, bz) = tdb_track::world_to_scene_xz(end, center_tile_x, center_tile_z);
        if ax.abs() > bound && az.abs() > bound && bx.abs() > bound && bz.abs() > bound {
            continue;
        }
        if let Some((cax, caz, cbx, cbz)) = clip_segment_to_box(ax, az, bx, bz, bound) {
            push_segment_scene(
                &mut ribbon,
                cax,
                caz,
                cbx,
                cbz,
                start,
                end,
                heights,
                (center_tile_x, center_tile_z),
            );
        }
    }
    ribbon
}

/// Cinta de vía desde acordes `.tdb` (coords Bevy world) recortada al tile.
pub fn build_tdb_track_ribbon(
    chords: &[(Vec3, Vec3)],
    center_tile_x: i32,
    center_tile_z: i32,
    heights: &crate::tdb_track::TileHeightIndex<'_>,
    grid_radius: u32,
) -> TrackRibbon {
    build_tdb_track_ribbon_scene(chords, center_tile_x, center_tile_z, grid_radius, heights)
}

/// Recorta el segmento (a→b) a la caja `[-bound, bound]²` en XZ (Liang-Barsky).
/// Devuelve `None` si queda totalmente fuera.
fn clip_segment_to_box(
    ax: f32,
    az: f32,
    bx: f32,
    bz: f32,
    bound: f32,
) -> Option<(f32, f32, f32, f32)> {
    let dx = bx - ax;
    let dz = bz - az;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;
    let edges = [
        (-dx, ax + bound),
        (dx, bound - ax),
        (-dz, az + bound),
        (dz, bound - az),
    ];
    for (p, q) in edges {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                if r > t0 {
                    t0 = r;
                }
            } else {
                if r < t0 {
                    return None;
                }
                if r < t1 {
                    t1 = r;
                }
            }
        }
    }
    Some((ax + dx * t0, az + dz * t0, ax + dx * t1, az + dz * t1))
}

#[allow(clippy::too_many_arguments)]
fn push_segment_scene(
    ribbon: &mut TrackRibbon,
    ax: f32,
    az: f32,
    bx: f32,
    bz: f32,
    world_start: Vec3,
    world_end: Vec3,
    heights: &crate::tdb_track::TileHeightIndex<'_>,
    center_tile: (i32, i32),
) {
    let dx = bx - ax;
    let dz = bz - az;
    let len = (dx * dx + dz * dz).sqrt();
    if len < 1e-3 {
        return;
    }
    let px = -dz / len * TRACK_HALF_WIDTH_M;
    let pz = dx / len * TRACK_HALF_WIDTH_M;

    let steps = (len / SAMPLE_STEP_M).ceil().max(1.0) as usize;
    let mut prev: Option<u32> = None;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let x = ax + dx * t;
        let z = az + dz * t;
        let msl_y = world_start.y + (world_end.y - world_start.y) * t;
        let y = heights.rail_y_at_scene(x, z, msl_y, center_tile, world_start, world_end);

        let idx = ribbon.positions.len() as u32;
        ribbon.positions.push([x + px, y, z + pz]);
        ribbon.positions.push([x - px, y, z - pz]);
        ribbon.normals.push([0.0, 1.0, 0.0]);
        ribbon.normals.push([0.0, 1.0, 0.0]);

        if let Some(p) = prev {
            ribbon
                .indices
                .extend_from_slice(&[p, idx, p + 1, p + 1, idx, idx + 1]);
        }
        prev = Some(idx);
    }
}

fn push_segment(ribbon: &mut TrackRibbon, ax: f32, az: f32, bx: f32, bz: f32, height: &TileHeight) {
    let dx = bx - ax;
    let dz = bz - az;
    let len = (dx * dx + dz * dz).sqrt();
    if len < 1e-3 {
        return;
    }
    // Perpendicular en XZ, escalada al semiancho.
    let px = -dz / len * TRACK_HALF_WIDTH_M;
    let pz = dx / len * TRACK_HALF_WIDTH_M;

    // Subdividimos el segmento en pasos cortos, muestreando la altura en cada
    // sección transversal, y unimos secciones consecutivas en una tira continua.
    let steps = (len / SAMPLE_STEP_M).ceil().max(1.0) as usize;
    let mut prev: Option<u32> = None;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let x = ax + dx * t;
        let z = az + dz * t;
        let y = height.local_y(x, z) + RAIL_LIFT_M;

        let idx = ribbon.positions.len() as u32;
        ribbon.positions.push([x + px, y, z + pz]);
        ribbon.positions.push([x - px, y, z - pz]);
        ribbon.normals.push([0.0, 1.0, 0.0]);
        ribbon.normals.push([0.0, 1.0, 0.0]);

        if let Some(p) = prev {
            ribbon
                .indices
                .extend_from_slice(&[p, idx, p + 1, p + 1, idx, idx + 1]);
        }
        prev = Some(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::load_tile_geometry;
    use std::path::PathBuf;

    fn chiltern_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    #[test]
    fn chiltern_track_falls_inside_centroid_tile() {
        let dir = chiltern_dir();
        let Some(graph) = load_graph(&dir) else {
            eprintln!("skip: track.toml de Chiltern no disponible");
            return;
        };
        let (tx, tz) = graph_centroid_tile(&graph).expect("centroide de nodos");
        let tile = load_tile_geometry(&dir, tx, tz).expect("tile del centroide");
        let ribbon = build_track_ribbon(&graph, tx, tz, &tile.height);

        assert!(
            ribbon.segment_count() > 0,
            "el tile del centroide del grafo debería tener vía, tuvo {} segmentos",
            ribbon.segment_count()
        );
        // Todos los vértices dentro del tile + margen y con altura finita.
        let bound = TILE_SIZE_M * 0.5 + TILE_MARGIN_M;
        for v in &ribbon.positions {
            assert!(v.iter().all(|c| c.is_finite()));
            assert!(v[0].abs() <= bound + 2.0 && v[2].abs() <= bound + 2.0);
        }
    }
}
