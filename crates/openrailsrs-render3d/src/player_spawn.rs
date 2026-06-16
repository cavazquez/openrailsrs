//! Posición inicial del jugador desde `.act` + `.pat` (+ `.srv` / `.tdb` / `track.toml`).

use std::path::{Path, PathBuf};

use bevy::math::Vec3;
use bevy::prelude::{Quat, Resource};
use openrailsrs_formats::{
    ActivityFile, PathDataPoint, PathFile, TrackDbFile, TrackDbNode, TrackNodeKind,
    msts_tile_x_index_for_coord, msts_tile_z_index_for_coord,
};
use openrailsrs_route::path::edge_path;
use openrailsrs_track::TrackGraph;

use crate::TilesToRender;
use crate::tdb_track;
use crate::track::TILE_SIZE_M;

const DEFAULT_HOP_LENGTH_M: f64 = 1000.0;
const CAMERA_EYE_ABOVE_RAIL_M: f32 = 2.2;
const RAIL_LIFT_M: f32 = 0.4;

/// Pose inicial del jugador en el espacio de escena (tile central en origen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerStartPose {
    pub position: Vec3,
    /// Yaw Bevy (radianes): forward local +Z alineado con la vía.
    pub yaw_rad: f32,
}

#[derive(Resource, Default)]
pub struct PlayerStartPoseResource(pub Option<PlayerStartPose>);

/// Resuelve posición desde un `.pat` (y offset opcional en metros).
pub fn resolve_pat_start_pose(
    route_dir: &Path,
    pat: &Path,
    distance_m: f64,
    graph: Option<&TrackGraph>,
    tdb: Option<&TrackDbFile>,
    center_tile: (i32, i32),
    tiles: &TilesToRender,
) -> Option<PlayerStartPose> {
    let pat_path = if pat.is_file() {
        pat.to_path_buf()
    } else {
        resolve_asset_path(route_dir, pat.to_string_lossy().as_ref())
    };
    let path = PathFile::from_path(&pat_path).ok()?;
    if path.pdps.is_empty() {
        return None;
    }

    let global = if let Some(g) = graph {
        global_from_graph(g, &path, distance_m)?
    } else if path.pdps.iter().any(|p| p.world.is_some()) {
        global_from_pat_world(&path, distance_m)?
    } else if let Some(tdb) = tdb {
        global_from_tdb(tdb, &path, distance_m)?
    } else {
        return None;
    };

    Some(scene_pose_from_global(
        global.0,
        global.1,
        center_tile,
        tiles,
    ))
}

/// Resuelve posición/orientación del jugador para la cámara fly (`.act` + `.srv`).
pub fn resolve_player_start_pose(
    route_dir: &Path,
    activity_path: &Path,
    graph: Option<&TrackGraph>,
    tdb: Option<&TrackDbFile>,
    center_tile: (i32, i32),
    tiles: &TilesToRender,
) -> Option<PlayerStartPose> {
    let act = ActivityFile::from_path(activity_path).ok()?;
    let pat_path = resolve_player_pat(route_dir, &act);
    let service_id = service_id_for_player(&act);
    let distance_m = service_id
        .as_deref()
        .and_then(|id| read_distance_down_path(route_dir, id))
        .unwrap_or(0.0)
        .max(0.0);
    resolve_pat_start_pose(
        route_dir,
        &pat_path,
        distance_m,
        graph,
        tdb,
        center_tile,
        tiles,
    )
}

/// Cámara a nivel de cabina sobre la vía más cercana al centro de la escena.
pub fn default_track_camera_pose(tiles: &TilesToRender) -> Option<PlayerStartPose> {
    let mut best_pose: Option<(f32, PlayerStartPose)> = None;

    for entry in &tiles.0 {
        let ribbon = &entry.track;
        if ribbon.positions.len() < 2 {
            continue;
        }

        let mut centers = Vec::new();
        for pair in ribbon.positions.chunks(2) {
            if pair.len() < 2 {
                break;
            }
            centers.push(Vec3::new(
                (pair[0][0] + pair[1][0]) * 0.5,
                (pair[0][1] + pair[1][1]) * 0.5,
                (pair[0][2] + pair[1][2]) * 0.5,
            ));
        }
        if centers.is_empty() {
            continue;
        }

        for (i, rail) in centers.iter().enumerate() {
            let d = rail.x * rail.x + rail.z * rail.z;
            if best_pose.is_some_and(|(best_d, _)| d >= best_d) {
                continue;
            }
            let yaw_rad = if i + 1 < centers.len() {
                let next = centers[i + 1];
                camera_yaw_from_track_direction(next.x - rail.x, next.z - rail.z)
            } else if i > 0 {
                let prev = centers[i - 1];
                camera_yaw_from_track_direction(rail.x - prev.x, rail.z - prev.z)
            } else {
                0.0
            };
            best_pose = Some((
                d,
                PlayerStartPose {
                    position: *rail + Vec3::Y * CAMERA_EYE_ABOVE_RAIL_M,
                    yaw_rad,
                },
            ));
        }
    }

    best_pose.map(|(_, pose)| pose)
}

/// Tiles con vía solo por `TrackObj` (sin cinta `.tdb`) no tienen ribbon: evita la
/// cámara cenital y coloca al jugador junto al túnel o al TrackObj más central.
pub fn default_trackobj_camera_pose(tiles: &TilesToRender) -> Option<PlayerStartPose> {
    use crate::objects::ObjectKind;

    let mut tunnel_mouth: Option<(Vec3, Quat)> = None;
    for entry in &tiles.0 {
        let offset = entry.world_offset;
        for obj in &entry.objects {
            if !obj
                .file_name
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case("IJ_tunnel_1bore.s"))
            {
                continue;
            }
            let mouth = obj.position + offset;
            if tunnel_mouth.is_some_and(|(best, _)| mouth.z >= best.z) {
                continue;
            }
            tunnel_mouth = Some((mouth, obj.rotation));
        }
    }
    if let Some((mouth, rot)) = tunnel_mouth {
        let along_cut = rot * Vec3::NEG_Z;
        let flat_along = Vec3::new(along_cut.x, 0.0, along_cut.z);
        // La boca del túnel mira perpendicular al eje de la vía en el corte Jinx.
        let mut portal = rot * Vec3::X;
        portal.y = 0.0;
        let flat_portal = if portal.length_squared() > 1e-6 {
            portal.normalize()
        } else if flat_along.length_squared() > 1e-6 {
            Vec3::new(-flat_along.z, 0.0, flat_along.x)
        } else {
            Vec3::X
        };
        let back = flat_portal * -40.0;
        return Some(PlayerStartPose {
            position: mouth + back + Vec3::Y * 12.0,
            yaw_rad: camera_yaw_from_track_direction(flat_portal.x, flat_portal.z),
        });
    }

    let mut best: Option<(f32, PlayerStartPose)> = None;
    for entry in &tiles.0 {
        let offset = entry.world_offset;
        for obj in &entry.objects {
            if obj.kind != ObjectKind::Track {
                continue;
            }
            let pos = obj.position + offset;
            let d = pos.x * pos.x + pos.z * pos.z;
            if best.is_some_and(|(best_d, _)| d >= best_d) {
                continue;
            }
            let forward = obj.rotation * Vec3::NEG_Z;
            best = Some((
                d,
                PlayerStartPose {
                    position: pos + Vec3::Y * CAMERA_EYE_ABOVE_RAIL_M,
                    yaw_rad: camera_yaw_from_track_direction(forward.x, forward.z),
                },
            ));
        }
    }
    best.map(|(_, pose)| pose)
}

fn global_from_graph(graph: &TrackGraph, path: &PathFile, distance_m: f64) -> Option<(Vec3, f32)> {
    let pat_nodes: Vec<String> = path
        .pdps
        .iter()
        .map(|p| format!("n{}", p.node_id))
        .collect();
    let (start_node, offset) = placement_from_pat_distance(graph, &pat_nodes, distance_m)?;
    let destination = pat_nodes.last()?.clone();
    let edge_ids = edge_path(graph, &start_node, &destination).ok()?;

    let mut remaining = offset.max(0.0);
    for edge_id in edge_ids {
        let edge = graph.edge(&edge_id)?;
        let from = graph.node(&edge.from.0)?;
        let to = graph.node(&edge.to.0)?;
        if remaining <= edge.length_m || edge.length_m <= 0.0 {
            let frac = if edge.length_m > 0.0 {
                (remaining / edge.length_m).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let x_m = from.x_m + frac * (to.x_m - from.x_m);
            let y_m = from.y_m + frac * (to.y_m - from.y_m);
            let dx = (to.x_m - from.x_m) as f32;
            let dz = (to.y_m - from.y_m) as f32;
            let yaw = camera_yaw_from_track_direction(dx, dz);
            return Some((Vec3::new(x_m as f32, 0.0, y_m as f32), yaw));
        }
        remaining -= edge.length_m;
    }

    let node = graph.node(&start_node)?;
    Some((Vec3::new(node.x_m as f32, 0.0, node.y_m as f32), 0.0))
}

fn global_from_pat_world(path: &PathFile, distance_m: f64) -> Option<(Vec3, f32)> {
    let points: Vec<(Vec3, f32)> = path
        .pdps
        .iter()
        .filter_map(|p| {
            let w = p.world?;
            let (x, y, z) = w.bevy_position();
            Some((Vec3::new(x, y, z), 0.0f32))
        })
        .collect();
    if points.is_empty() {
        return None;
    }
    if distance_m <= 0.0 {
        let yaw = heading_from_points(&points, 0);
        return Some((points[0].0, yaw));
    }

    let mut walked = 0.0f64;
    for i in 0..points.len().saturating_sub(1) {
        let (a, _) = points[i];
        let (b, _) = points[i + 1];
        let seg = chord_length_xz(a, b) as f64;
        if seg <= 0.01 {
            continue;
        }
        if walked + seg >= distance_m {
            let t = ((distance_m - walked) / seg).clamp(0.0, 1.0) as f32;
            let pos = a.lerp(b, t);
            let yaw = camera_yaw_from_track_direction(b.x - a.x, b.z - a.z);
            return Some((pos, yaw));
        }
        walked += seg;
    }
    let last = points.len() - 1;
    Some((points[last].0, heading_from_points(&points, last)))
}

fn global_from_tdb(tdb: &TrackDbFile, path: &PathFile, distance_m: f64) -> Option<(Vec3, f32)> {
    let (node_idx, offset) = pat_node_at_distance(path, distance_m)?;
    let node_id = path.pdps.get(node_idx)?.node_id;
    let node = tdb.node_by_id(node_id)?;
    let base = tdb_node_world(node)?;
    let yaw = tdb_node_heading(node).unwrap_or(0.0);
    if offset <= 0.01 {
        return Some((base, yaw));
    }
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    Some((base + forward * offset as f32, yaw))
}

fn scene_pose_from_global(
    global: Vec3,
    yaw_rad: f32,
    center_tile: (i32, i32),
    tiles: &TilesToRender,
) -> PlayerStartPose {
    let (center_tx, center_tz) = center_tile;
    let tile_x = msts_tile_x_index_for_coord(global.x);
    let tile_z = msts_tile_z_index_for_coord(global.z);
    let (centered_lx, centered_lz) =
        tdb_track::world_to_tile_local_centered(global, tile_x, tile_z);

    let terrain_y = tiles
        .0
        .iter()
        .find(|e| e.geometry.tile_x == tile_x && e.geometry.tile_z == tile_z)
        .map(|e| e.geometry.height.local_y(centered_lx, centered_lz))
        .unwrap_or(global.y);

    let tile_offset = Vec3::new(
        (tile_x - center_tx) as f32 * TILE_SIZE_M,
        0.0,
        (tile_z - center_tz) as f32 * TILE_SIZE_M,
    );
    let rail_y = terrain_y + RAIL_LIFT_M;
    let eye_y = rail_y + CAMERA_EYE_ABOVE_RAIL_M;
    let position = tile_offset + Vec3::new(centered_lx, eye_y, centered_lz);

    PlayerStartPose { position, yaw_rad }
}

fn placement_from_pat_distance(
    graph: &TrackGraph,
    pat: &[String],
    distance_m: f64,
) -> Option<(String, f64)> {
    if pat.is_empty() {
        return None;
    }
    if distance_m <= 0.0 {
        return Some((pat[0].clone(), 0.0));
    }

    if pat[0] == "n1" && pat.len() > 1 {
        let platform_len = graph
            .edges_iter()
            .find(|(_, e)| {
                (e.from.0 == "n3" && e.to.0 == "n1") || (e.from.0 == "n1" && e.to.0 == "n3")
            })
            .map(|(_, e)| e.length_m)
            .unwrap_or(500.0);
        let start = pat[1].clone();
        let offset = (platform_len - distance_m).clamp(0.0, platform_len);
        return Some((start, offset));
    }

    let mut remaining = distance_m;
    for i in 0..pat.len().saturating_sub(1) {
        let hop = hop_length(graph, &pat[i], &pat[i + 1]);
        if remaining <= hop {
            return Some((pat[i].clone(), remaining));
        }
        remaining -= hop;
    }
    Some((pat.last()?.clone(), 0.0))
}

fn pat_node_at_distance(path: &PathFile, distance_m: f64) -> Option<(usize, f64)> {
    if path.pdps.is_empty() {
        return None;
    }
    if distance_m <= 0.0 {
        return Some((0, 0.0));
    }

    let mut remaining = distance_m;
    for i in 0..path.pdps.len().saturating_sub(1) {
        let hop = pat_hop_length(&path.pdps[i], &path.pdps[i + 1]);
        if remaining <= hop {
            return Some((i, remaining));
        }
        remaining -= hop;
    }
    Some((path.pdps.len() - 1, 0.0))
}

fn pat_hop_length(a: &PathDataPoint, b: &PathDataPoint) -> f64 {
    if let (Some(wa), Some(wb)) = (a.world, b.world) {
        let (ax, _, az) = wa.bevy_position();
        let (bx, _, bz) = wb.bevy_position();
        let dx = bx - ax;
        let dz = bz - az;
        return ((dx * dx + dz * dz) as f64).sqrt();
    }
    DEFAULT_HOP_LENGTH_M
}

fn hop_length(graph: &TrackGraph, a: &str, b: &str) -> f64 {
    for (_, edge) in graph.edges_iter() {
        if edge.from.0 == a && edge.to.0 == b {
            return edge.length_m;
        }
        if edge.from.0 == b && edge.to.0 == a {
            return edge.length_m;
        }
    }
    DEFAULT_HOP_LENGTH_M
}

fn tdb_node_world(node: &TrackDbNode) -> Option<Vec3> {
    if let Some(p) = node.position {
        let (x, y, z) = p.bevy_position();
        return Some(Vec3::new(x, y, z));
    }
    let TrackNodeKind::Vector { sections, .. } = &node.kind else {
        return None;
    };
    let section = sections.first()?;
    let (x, y, z) = section.start.bevy_position();
    Some(Vec3::new(x, y, z))
}

fn tdb_node_heading(node: &TrackDbNode) -> Option<f32> {
    let TrackNodeKind::Vector { sections, .. } = &node.kind else {
        return None;
    };
    let section = sections.first()?;
    if let Some(h) = section.heading_deg() {
        return Some((h.to_radians() as f32).rem_euclid(std::f32::consts::TAU));
    }
    None
}

fn heading_from_points(points: &[(Vec3, f32)], idx: usize) -> f32 {
    if idx + 1 < points.len() {
        let a = points[idx].0;
        let b = points[idx + 1].0;
        return camera_yaw_from_track_direction(b.x - a.x, b.z - a.z);
    }
    if idx > 0 {
        let a = points[idx - 1].0;
        let b = points[idx].0;
        return camera_yaw_from_track_direction(b.x - a.x, b.z - a.z);
    }
    0.0
}

/// Yaw Bevy (Euler Y) para que la cámara mire **en el sentido** del raíl `(dx, dz)`,
/// no al revés (±π respecto al atan2 bruto).
pub fn camera_yaw_from_track_direction(dx: f32, dz: f32) -> f32 {
    if dx.abs() + dz.abs() < 1e-4 {
        return 0.0;
    }
    let yaw = track_yaw_from_direction(dx, dz);
    let fx = yaw.sin();
    let fz = -yaw.cos();
    if fx * dx + fz * dz < 0.0 {
        yaw + std::f32::consts::PI
    } else {
        yaw
    }
}

fn track_yaw_from_direction(dx: f32, dz: f32) -> f32 {
    if dx.abs() + dz.abs() < 1e-4 {
        return 0.0;
    }
    // Bevy forward = -Z; rotación Y alinea -Z con (dx, 0, dz) vía atan2(dx, -dz).
    dx.atan2(-dz)
}

fn chord_length_xz(a: Vec3, b: Vec3) -> f32 {
    let dx = b.x - a.x;
    let dz = b.z - a.z;
    (dx * dx + dz * dz).sqrt()
}

fn read_distance_down_path(route_dir: &Path, service_id: &str) -> Option<f64> {
    let trimmed = service_id
        .rfind('(')
        .map(|i| service_id[..i].trim())
        .unwrap_or(service_id);
    let candidates: &[&str] = if trimmed != service_id {
        &[service_id, trimmed]
    } else {
        &[service_id]
    };
    for &id in candidates {
        let srv_path = route_dir.join("SERVICES").join(format!("{id}.srv"));
        if let Ok(text) = openrailsrs_formats::encoding::read_msts_file_case_insensitive(&srv_path)
        {
            if let Some(dist) = parse_first_distance_down_path(&text) {
                return Some(dist);
            }
        }
    }
    None
}

fn parse_first_distance_down_path(text: &str) -> Option<f64> {
    for line in text.lines() {
        let line = line.trim().trim_matches('\0');
        if !line.contains("DistanceDownPath") {
            continue;
        }
        if let Some(v) = parse_distance_down_path_line(line) {
            return Some(v);
        }
    }
    for token in text.split("DistanceDownPath") {
        if let Some(v) = parse_distance_down_path_line(token) {
            return Some(v);
        }
    }
    None
}

fn parse_distance_down_path_line(line: &str) -> Option<f64> {
    let line = line.trim().trim_matches('\0');
    let open_paren = line.find('(')?;
    let inner = line[open_paren + 1..].trim().trim_end_matches(')').trim();
    inner.parse::<f64>().ok()
}

fn resolve_player_pat(route_dir: &Path, activity: &ActivityFile) -> PathBuf {
    if !activity.player_path.trim().is_empty() {
        return resolve_asset_path(route_dir, &activity.player_path);
    }
    if let Some(id) = service_id_for_player(activity) {
        return resolve_asset_path(route_dir, &format!("PATHS/{id}.pat"));
    }
    route_dir.to_path_buf()
}

fn service_id_for_player(activity: &ActivityFile) -> Option<String> {
    if let Some(id) = &activity.player_service_id {
        if !id.trim().is_empty() {
            return Some(id.trim().to_string());
        }
    }
    let path = activity.player_path.trim();
    if path.is_empty() {
        return None;
    }
    let normalized = path.replace('\\', "/");
    let stem = normalized
        .strip_prefix("PATHS/")
        .or_else(|| normalized.strip_prefix("paths/"))
        .unwrap_or(normalized.as_str());
    let stem = stem.strip_suffix(".pat").unwrap_or(stem);
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

fn resolve_asset_path(base: &Path, asset: &str) -> PathBuf {
    let normalized = asset.trim().replace('\\', "/");
    let candidate = base.join(&normalized);
    if candidate.exists() {
        return candidate;
    }
    openrailsrs_formats::encoding::resolve_path_case_insensitive(&candidate).unwrap_or(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TrackVectorPoint;

    #[test]
    fn pat_world_interpolates_distance() {
        let path = PathFile {
            name: String::new(),
            pdps: vec![
                PathDataPoint {
                    node_id: 1,
                    junction_flag: 0,
                    world: Some(TrackVectorPoint {
                        tile_x: 0,
                        tile_z: 0,
                        x: 0.0,
                        y: 10.0,
                        z: 0.0,
                    }),
                },
                PathDataPoint {
                    node_id: 2,
                    junction_flag: 0,
                    world: Some(TrackVectorPoint {
                        tile_x: 0,
                        tile_z: 0,
                        x: 100.0,
                        y: 10.0,
                        z: 0.0,
                    }),
                },
            ],
        };
        let (pos, yaw) = global_from_pat_world(&path, 50.0).expect("interp");
        assert!((pos.x - 50.0).abs() < 0.01);
        assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    }

    #[test]
    fn camera_yaw_faces_tangent_not_opposite() {
        let yaw = camera_yaw_from_track_direction(-1.0, 0.0);
        let fx = yaw.sin();
        let fz = -yaw.cos();
        assert!(
            -fx + fz * 0.0 > 0.9,
            "forward ({fx},{fz}) debe seguir (-1,0)"
        );
    }

    #[test]
    fn watersnake_tunnel_tile_spawns_at_tunnel_not_overview() {
        use crate::objects::load_objects;
        use crate::terrain::load_tile_geometry;
        use crate::{TileEntry, TilesToRender};

        let route = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("routes/NewForestRouteV3/Routes/Watersnake");
        if !route.is_dir() {
            return;
        }
        let (tx, tz) = (-6144, 14900);
        let tile = load_tile_geometry(&route, tx, tz).expect("tile");
        let base = tile.height.base_y();
        let objects = load_objects(&route, tx, tz, base);
        let entry = TileEntry {
            geometry: tile,
            world_offset: Vec3::ZERO,
            track: Default::default(),
            objects,
        };
        let tiles = TilesToRender(vec![entry]);
        assert!(
            default_track_camera_pose(&tiles).is_none(),
            "tile TrackObj-only no debe tener ribbon"
        );
        let pose = default_trackobj_camera_pose(&tiles).expect("pose junto al túnel");
        assert!(
            pose.position.y < 800.0,
            "no debe ser overview cenital (y={:.0})",
            pose.position.y
        );
        assert!(
            pose.position.x > -360.0 && pose.position.x < -280.0 && pose.position.z < -40.0,
            "cerca del túnel Jinx: ({:.0},{:.0})",
            pose.position.x,
            pose.position.z
        );
    }
}
