//! Map MSTS `.pat` / `.srv` hints onto an imported `track.toml` graph.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use openrailsrs_formats::{PathFile, TrackDbFile, TrackNodeKind};
use openrailsrs_route::{
    MstsAlias, load_route_from_dir,
    path::{edge_path, edge_path_ignoring_switches, edge_path_via_waypoints},
};
use openrailsrs_scenarios::model::{SwitchDef, SwitchPositionDef};
use openrailsrs_track::TrackGraph;

use crate::error::MstsError;

type NativePathPolylines = HashMap<String, Vec<(f64, f64)>>;

/// Route start/destination hints derived from a player path and service file.
#[derive(Debug, Clone)]
pub struct RouteHints {
    pub start: String,
    pub destination: String,
    /// Path metres from [`Self::start`] to the **consist head** (#132).
    pub start_offset_m: f64,
    pub switches: Vec<SwitchDef>,
}

/// Convert an OR rear-traveller snap into a head `start_offset_m` (#132).
///
/// `rear_snap_offset_m` is the graph offset of TrackPDP[0]; `consist_length_m` is the
/// sum of vehicle lengths (head origin → rear of last car).
#[inline]
pub fn head_offset_from_rear_snap(rear_snap_offset_m: f64, consist_length_m: f64) -> f64 {
    (rear_snap_offset_m + consist_length_m.max(0.0)).max(0.0)
}

/// Sum of vehicle lengths for PAT rear→head conversion (#132).
#[inline]
pub fn consist_length_from_vehicle_lengths(lengths_m: &[f64]) -> f64 {
    lengths_m.iter().copied().map(|l| l.max(0.0)).sum()
}

/// Read the first `DistanceDownPath` from `SERVICES/<id>.srv`.
///
/// `PathID` values often carry a `(player)` suffix absent from the actual
/// `.srv` filename.  Both the full id and the base name are tried.
pub fn read_distance_down_path(route_dir: &Path, service_id: &str) -> Option<f64> {
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
            && let Some(dist) = parse_first_distance_down_path(&text)
        {
            return Some(dist);
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
    // Whole-file fallback for UTF-16 / wrapped layouts.
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

/// Resolve player start/destination on an imported graph using `.pat` + offset.
///
/// For TrackPDP (world) paths, `start_offset_m` / `DistanceDownPath` is **ignored** —
/// spawn snaps PDP[0]. Pass [`Some`]`consist_length_m` to convert that rear snap into a
/// head offset (#132). Non-world PAT sequences still walk `start_offset_m` as head metres.
pub fn placement_for_pat(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    pat_path: &Path,
    start_offset_m: f64,
) -> Result<RouteHints, MstsError> {
    placement_for_pat_with_consist(graph, aliases, pat_path, start_offset_m, None)
}

/// Like [`placement_for_pat`], optionally converting TrackPDP rear snap → head (#132).
pub fn placement_for_pat_with_consist(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    pat_path: &Path,
    start_offset_m: f64,
    consist_length_m: Option<f64>,
) -> Result<RouteHints, MstsError> {
    let path_file = PathFile::from_path(pat_path)?;
    if path_file.has_world_pdps() {
        let polylines = native_path_polylines(graph, aliases, pat_path)?;
        return placement_from_world(
            graph,
            aliases,
            &path_file,
            consist_length_m,
            polylines.as_ref(),
        );
    }

    let resolved = resolve_pat_sequence(graph, aliases, &path_file)?;
    let (start, offset) = placement_from_distance(graph, &resolved, start_offset_m)?;
    let destination = pick_destination_node(graph, &start, &resolved)?;
    let switches = switches_from_pat(&path_file, graph, aliases, &start, &destination, offset)?;

    Ok(RouteHints {
        start,
        destination,
        start_offset_m: offset,
        switches,
    })
}

/// Place using native `TrackPDP` world coordinates (Open Rails `PathFile` semantics).
///
/// Snaps TrackPDP[0] (OR rear traveller). `DistanceDownPath` is never used as spawn offset.
/// When `consist_length_m` is set, writes **head** offset = rear snap + length (#132).
fn placement_from_world(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
    consist_length_m: Option<f64>,
    polylines: Option<&NativePathPolylines>,
) -> Result<RouteHints, MstsError> {
    let world_pdps = world_pdps_for_placement(path_file);
    if world_pdps.is_empty() {
        return Err(MstsError::Msg(
            "PAT has no usable TrackPDP world positions".into(),
        ));
    }

    let sample = world_pdps[0];
    let path_dir = world_pdps.get(1).map(|next| {
        (
            next.graph_x_m() - sample.graph_x_m(),
            next.graph_z_m() - sample.graph_z_m(),
        )
    });
    let (start, rear_offset, _) = snap_world_to_edge(
        graph,
        sample.graph_x_m(),
        sample.graph_z_m(),
        path_dir,
        polylines,
    )?;
    // Default: keep rear snap as written offset (Chiltern scenario.toml is calibrated).
    // With consist length: convert to head for OR rear-traveller parity (#132).
    let start_offset_m = match consist_length_m {
        Some(len) if len > 0.0 => head_offset_from_rear_snap(rear_offset, len),
        _ => rear_offset,
    };

    let destination = destination_from_world_pdps(graph, &start, &world_pdps, polylines)?;

    let switches = switches_from_pat(
        path_file,
        graph,
        aliases,
        &start,
        &destination,
        start_offset_m,
    )?;

    Ok(RouteHints {
        start,
        destination,
        start_offset_m,
        switches,
    })
}

/// Native section anchors distinguish curved/parallel tracks which share an
/// inaccurate junction-to-junction chord. Keep synthetic graph-only PAT support.
fn native_path_polylines(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    pat: &Path,
) -> Result<Option<NativePathPolylines>, MstsError> {
    let Some(paths) = pat.parent().filter(|p| {
        p.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("PATHS"))
    }) else {
        return Ok(None);
    };
    let Some(route) = paths.parent() else {
        return Ok(None);
    };
    let mut files = std::fs::read_dir(route)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tdb")))
        .collect::<Vec<_>>();
    files.sort();
    let Some(tdb) = files.first() else {
        return Ok(None);
    };
    if std::fs::metadata(tdb)?.len() > 64 * 1024 * 1024 {
        return Err(MstsError::msg("Native path track database exceeds 64 MiB"));
    }
    let tdb = TrackDbFile::from_path(tdb)?;
    let mut out = HashMap::new();
    for node in &tdb.nodes {
        let TrackNodeKind::Vector { sections, .. } = &node.kind else {
            continue;
        };
        let Some(alias) = aliases.get(&node.id) else {
            continue;
        };
        let Some(edge) = graph.edge(&alias.id) else {
            continue;
        };
        let (Some(from), Some(to)) = (graph.node(&edge.from.0), graph.node(&edge.to.0)) else {
            continue;
        };
        if sections.is_empty() {
            continue;
        }
        let mut points = vec![(from.x_m, from.y_m)];
        points.extend(
            sections
                .iter()
                .map(|s| (s.start.graph_x_m(), s.start.graph_z_m())),
        );
        points.push((to.x_m, to.y_m));
        out.insert(alias.id.clone(), points.clone());
        let reverse = format!("{}_r", alias.id);
        if graph.edge(&reverse).is_some() {
            points.reverse();
            out.insert(reverse, points);
        }
    }
    Ok((!out.is_empty()).then_some(out))
}

fn world_pdps_for_placement(path_file: &PathFile) -> Vec<openrailsrs_formats::TrackVectorPoint> {
    path_file
        .pdps
        .iter()
        .filter(|p| !p.is_invalid())
        .filter_map(|p| p.world)
        .collect()
}

/// Pick the last world-PDP node still reachable along the directed graph.
///
/// Walks TrackPDP order (PAT polyline), not global graph distance — otherwise reverse
/// edges can invent multi-thousand-km detours to unrelated snaps. Switch positions are
/// ignored here; [`switches_for_route`] aligns them to the chosen corridor afterwards.
fn destination_from_world_pdps(
    graph: &TrackGraph,
    start: &str,
    world_pdps: &[openrailsrs_formats::TrackVectorPoint],
    polylines: Option<&NativePathPolylines>,
) -> Result<String, MstsError> {
    if let Some(polylines) = polylines
        && let Some(last) = world_pdps.last()
    {
        // An endpoint inside a station vector can be closer to a node on
        // another track. Keep the native vector containing the authored end.
        let nearest = nearest_node_id(graph, last).filter(|id| {
            graph
                .node(id)
                .is_some_and(|node| sq_dist_node(last.graph_x_m(), last.graph_z_m(), node) < 0.0025)
        });
        let destination = if let Some(node) = nearest {
            node
        } else {
            let direction = world_pdps.iter().rev().nth(1).map(|previous| {
                (
                    last.graph_x_m() - previous.graph_x_m(),
                    last.graph_z_m() - previous.graph_z_m(),
                )
            });
            let (_, _, end) = snap_world_to_edge(
                graph,
                last.graph_x_m(),
                last.graph_z_m(),
                direction,
                Some(polylines),
            )?;
            end
        };
        if destination != start && edge_path_ignoring_switches(graph, start, &destination).is_ok() {
            return Ok(destination);
        }
    }
    let mut last_ok: Option<(String, f64)> = None;
    let mut useful: Option<(String, f64)> = None;
    const USEFUL_PATH_M: f64 = 5_000.0;
    for w in world_pdps {
        let Some(cand) = nearest_node_id(graph, w) else {
            continue;
        };
        if cand == start {
            continue;
        }
        let Ok(edges) = edge_path_ignoring_switches(graph, start, &cand) else {
            continue;
        };
        let dist: f64 = edges
            .iter()
            .filter_map(|eid| graph.edge(eid).map(|e| e.length_m))
            .sum();
        // Prefer the furthest PDP along the file that remains on a directed path.
        if last_ok.as_ref().is_none_or(|(n, _)| n != &cand) {
            last_ok = Some((cand.clone(), dist));
        }
        if dist >= USEFUL_PATH_M {
            useful = Some((cand, dist));
        }
    }
    // Prefer a ≥5 km corridor when the PAT reaches that far; else the last reachable PDP.
    if let Some((node, _)) = useful.or(last_ok) {
        return Ok(node);
    }
    bfs_far_node(graph, start).ok_or_else(|| {
        MstsError::Msg(format!(
            "no reachable destination from world-snapped start {start}"
        ))
    })
}

/// Interpolate a point along TrackPDP world positions at `distance_m` from the start.
pub fn point_along_world_polyline(
    world_pdps: &[openrailsrs_formats::TrackVectorPoint],
    distance_m: f64,
) -> Option<(f32, f32, f32)> {
    if world_pdps.is_empty() {
        return None;
    }
    if distance_m <= 0.0 {
        return Some(world_pdps[0].bevy_position());
    }
    let mut walked = 0.0f64;
    for i in 0..world_pdps.len().saturating_sub(1) {
        let (ax, ay, az) = world_pdps[i].bevy_position();
        let (bx, by, bz) = world_pdps[i + 1].bevy_position();
        let dx = (bx - ax) as f64;
        let dz = (bz - az) as f64;
        let seg = (dx * dx + dz * dz).sqrt();
        if seg <= 0.01 {
            continue;
        }
        if walked + seg >= distance_m {
            let t = ((distance_m - walked) / seg).clamp(0.0, 1.0) as f32;
            return Some((ax + t * (bx - ax), ay + t * (by - ay), az + t * (bz - az)));
        }
        walked += seg;
    }
    Some(world_pdps.last().unwrap().bevy_position())
}

/// Snap a Bevy XZ point to the nearest graph edge; return start node + offset along travel.
///
/// Uses `f64` throughout — Bevy/graph coords are ~1e7 m, where `f32` loses sub-metre precision.
fn snap_world_to_edge(
    graph: &TrackGraph,
    px: f64,
    pz: f64,
    path_dir: Option<(f64, f64)>,
    polylines: Option<&NativePathPolylines>,
) -> Result<(String, f64, String), MstsError> {
    let mut best: Option<(f64, String, String, f64, f64, f64)> = None;
    for (eid, edge) in graph.edges_iter() {
        let Some(from) = graph.node(&edge.from.0) else {
            continue;
        };
        let Some(to) = graph.node(&edge.to.0) else {
            continue;
        };
        let chord = [(from.x_m, from.y_m), (to.x_m, to.y_m)];
        let points = polylines
            .and_then(|p| p.get(eid))
            .map(Vec::as_slice)
            .unwrap_or(&chord);
        let total = points
            .windows(2)
            .map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1))
            .sum::<f64>();
        if total <= 1e-6 {
            continue;
        }
        let mut before = 0.0;
        let mut closest = (f64::INFINITY, 0.0, 0.0, 0.0);
        for pair in points.windows(2) {
            let (ax, az) = pair[0];
            let (dx, dz) = (pair[1].0 - ax, pair[1].1 - az);
            let length = dx.hypot(dz);
            if length <= 1e-6 {
                continue;
            }
            let t = (((px - ax) * dx + (pz - az) * dz) / (length * length)).clamp(0.0, 1.0);
            let dist2 = (px - ax - t * dx).powi(2) + (pz - az - t * dz).powi(2);
            if dist2 < closest.0 {
                closest = (
                    dist2,
                    (before + t * length) / total,
                    dx / length,
                    dz / length,
                );
            }
            before += length;
        }
        let (dist2, t, dx, dz) = closest;
        let align = path_dir
            .map(|(pdx, pdz)| {
                let pl = (pdx * pdx + pdz * pdz).sqrt().max(1e-9);
                (dx * pdx + dz * pdz) / pl
            })
            .unwrap_or(1.0);
        let better = match &best {
            None => true,
            Some((best_d2, _, _, _, _, best_align)) => {
                dist2 + 1e-6 < *best_d2
                    || ((dist2 - *best_d2).abs() < 1e-6 && align.abs() > best_align.abs())
            }
        };
        if better {
            best = Some((
                dist2,
                edge.from.0.clone(),
                edge.to.0.clone(),
                t,
                edge.length_m,
                align,
            ));
        }
    }
    let Some((_, from, to, t, len, align)) = best else {
        return Err(MstsError::Msg(
            "could not snap PAT world point to any graph edge".into(),
        ));
    };
    // Prefer the endpoint that still has a way forward on the graph.
    let (cand_start, cand_offset, cand_other) = if align >= 0.0 {
        (from, (t * len).clamp(0.0, len), to)
    } else {
        (to, ((1.0 - t) * len).clamp(0.0, len), from)
    };
    if !graph.outgoing_edges(&cand_start).is_empty() {
        return Ok((cand_start, cand_offset, cand_other));
    }
    if !graph.outgoing_edges(&cand_other).is_empty() {
        return Ok((cand_other, (len - cand_offset).clamp(0.0, len), cand_start));
    }
    Ok((cand_start, cand_offset, cand_other))
}

/// Convenience: load `track.toml` from `route_dir` and compute hints.
pub fn placement_from_imported_route(
    route_dir: &Path,
    pat_path: &Path,
    start_offset_m: f64,
) -> Result<RouteHints, MstsError> {
    placement_from_imported_route_with_consist(route_dir, pat_path, start_offset_m, None)
}

/// Like [`placement_from_imported_route`] with optional rear→head conversion (#132).
pub fn placement_from_imported_route_with_consist(
    route_dir: &Path,
    pat_path: &Path,
    start_offset_m: f64,
    consist_length_m: Option<f64>,
) -> Result<RouteHints, MstsError> {
    let loaded = load_route_from_dir(route_dir).map_err(|e| MstsError::Msg(e.to_string()))?;
    placement_for_pat_with_consist(
        &loaded.graph,
        &loaded.msts_aliases,
        pat_path,
        start_offset_m,
        consist_length_m,
    )
}

/// Compress consecutive duplicate graph node ids from a resolved PAT sequence.
pub fn compress_pat_waypoints(nodes: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for node in nodes {
        if out.last().map(String::as_str) != Some(node.as_str()) {
            out.push(node.clone());
        }
    }
    out
}

/// Graph node ids along the player `.pat` from `start` to `destination` (outbound leg).
///
/// Native MSTS `TrackPDP` lines carry world coordinates; when present, waypoints are
/// derived by snapping each PDP to the nearest graph node. Otherwise falls back to
/// resolved TDB node ids (see [`TrPathPDP`] / `minimal.pat`).
pub fn pat_waypoints(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, MstsError> {
    pat_waypoints_with_offset(graph, aliases, path_file, start, destination, 0.0)
}

/// Like [`pat_waypoints`] but skips the first `start_offset_m` metres along the PAT
/// world polyline before building the connected node chain.
pub fn pat_waypoints_with_offset(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
    start: &str,
    destination: &str,
    start_offset_m: f64,
) -> Result<Vec<String>, MstsError> {
    if path_file.pdps.iter().any(|p| p.world.is_some()) {
        return pat_waypoints_from_world(graph, path_file, start, destination, start_offset_m);
    }
    let compressed = compressed_pat_nodes(graph, aliases, path_file)?;
    let start_idx = compressed
        .iter()
        .position(|n| n == start)
        .ok_or_else(|| MstsError::Msg(format!("PAT does not visit start node {start}")))?;
    if let Some(dest_offset) = compressed[start_idx..]
        .iter()
        .position(|n| n == destination)
    {
        let dest_idx = start_idx + dest_offset;
        return Ok(compressed[start_idx..=dest_idx].to_vec());
    }
    pat_outbound_waypoints_from_compressed(&compressed, start_idx)
}

/// Snap native `TrackPDP` world positions to graph nodes and extract the outbound leg.
///
/// Only appends a snapped node when it is reachable from the previous waypoint
/// (switch-aware BFS), so noisy nearest-node snaps off the route are skipped.
pub fn pat_waypoints_from_world(
    graph: &TrackGraph,
    path_file: &PathFile,
    start: &str,
    destination: &str,
    start_offset_m: f64,
) -> Result<Vec<String>, MstsError> {
    let mut waypoints = vec![start.to_string()];
    let mut past_offset = start_offset_m <= 0.0;
    let mut walked = 0.0f64;
    let world_pdps: Vec<_> = path_file.pdps.iter().filter_map(|p| p.world).collect();
    for (i, w) in world_pdps.iter().enumerate() {
        if i > 0 {
            let dx = w.graph_x_m() - world_pdps[i - 1].graph_x_m();
            let dz = w.graph_z_m() - world_pdps[i - 1].graph_z_m();
            walked += (dx * dx + dz * dz).sqrt();
        }
        if !past_offset {
            if walked >= start_offset_m {
                past_offset = true;
            } else {
                continue;
            }
        }
        // The last PDP may be inside the destination vector. Its nearest
        // junction can belong to another parallel track; placement already
        // resolved the actual native endpoint.
        let Some(nid) = (if i + 1 == world_pdps.len() {
            Some(destination.to_string())
        } else {
            nearest_node_id(graph, w)
        }) else {
            continue;
        };
        if waypoints.last().map(String::as_str) == Some(nid.as_str()) {
            continue;
        }
        let tail = waypoints.last().expect("non-empty");
        // Ignore switches while chaining PAT snaps; runtime switches are derived later.
        if edge_path_ignoring_switches(graph, tail, &nid).is_ok() {
            waypoints.push(nid);
        }
    }
    if !waypoints.iter().any(|n| n == destination)
        && let Some(tail) = waypoints.last()
        && edge_path_ignoring_switches(graph, tail, destination).is_ok()
        && tail != destination
    {
        waypoints.push(destination.to_string());
    }
    let start_idx = waypoints.iter().position(|n| n == start).unwrap_or(0);
    if let Some(dest_offset) = waypoints[start_idx..].iter().position(|n| n == destination) {
        let dest_idx = start_idx + dest_offset;
        return Ok(waypoints[start_idx..=dest_idx].to_vec());
    }
    pat_outbound_waypoints_from_compressed(&waypoints, start_idx)
}

fn nearest_node_id(
    graph: &TrackGraph,
    world: &openrailsrs_formats::TrackVectorPoint,
) -> Option<String> {
    let px = world.graph_x_m();
    let pz = world.graph_z_m();
    graph
        .nodes_iter()
        .min_by(|(_, a), (_, b)| {
            let da = sq_dist_node(px, pz, a);
            let db = sq_dist_node(px, pz, b);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, _)| id.to_string())
}

fn sq_dist_node(px: f64, pz: f64, node: &openrailsrs_track::Node) -> f64 {
    let dx = node.x_m - px;
    let dz = node.y_m - pz;
    dx * dx + dz * dz
}

/// Outbound PAT leg: from `start_idx` until the first return to `n1` (exclusive).
pub fn pat_outbound_waypoints_from_compressed(
    compressed: &[String],
    start_idx: usize,
) -> Result<Vec<String>, MstsError> {
    if start_idx >= compressed.len() {
        return Err(MstsError::Msg("start_idx past end of PAT".into()));
    }
    let mut end_idx = compressed.len();
    for (i, node) in compressed.iter().enumerate().skip(start_idx + 1) {
        if node == "n1" {
            end_idx = i;
            break;
        }
    }
    Ok(compressed[start_idx..end_idx].to_vec())
}

/// Outbound PAT graph nodes from `start` (see [`pat_outbound_waypoints_from_compressed`]).
pub fn pat_outbound_waypoints(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
    start: &str,
) -> Result<Vec<String>, MstsError> {
    let compressed = compressed_pat_nodes(graph, aliases, path_file)?;
    let start_idx = compressed
        .iter()
        .position(|n| n == start)
        .ok_or_else(|| MstsError::Msg(format!("PAT does not visit start node {start}")))?;
    pat_outbound_waypoints_from_compressed(&compressed, start_idx)
}

fn compressed_pat_nodes(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
) -> Result<Vec<String>, MstsError> {
    let resolved = resolve_pat_sequence(graph, aliases, path_file)?;
    Ok(compress_pat_waypoints(
        &resolved
            .iter()
            .map(|r| r.graph_node.clone())
            .collect::<Vec<_>>(),
    ))
}

/// Edge ids following the player `.pat` between `start` and `destination`.
pub fn pat_edge_path(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, MstsError> {
    pat_edge_path_with_offset(graph, aliases, path_file, start, destination, 0.0)
}

pub fn pat_edge_path_with_offset(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
    start: &str,
    destination: &str,
    start_offset_m: f64,
) -> Result<Vec<String>, MstsError> {
    let waypoints = pat_waypoints_with_offset(
        graph,
        aliases,
        path_file,
        start,
        destination,
        start_offset_m,
    )?;
    let mut edges =
        edge_path_via_waypoints(graph, &waypoints).map_err(|e| MstsError::Msg(e.to_string()))?;
    if waypoints.last().map(String::as_str) != Some(destination) {
        let tail_from = waypoints
            .last()
            .ok_or_else(|| MstsError::Msg("empty PAT".into()))?;
        let tail =
            edge_path(graph, tail_from, destination).map_err(|e| MstsError::Msg(e.to_string()))?;
        edges.extend(tail);
    }
    Ok(edges)
}

#[derive(Debug, Clone)]
struct ResolvedPatNode {
    graph_node: String,
}

fn resolve_pat_sequence(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    path_file: &PathFile,
) -> Result<Vec<ResolvedPatNode>, MstsError> {
    let indexed: Vec<(usize, u32)> = path_file
        .pdps
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.node_id.map(|id| (i, id)))
        .collect();
    if indexed.is_empty() {
        return Err(MstsError::Msg(
            "PAT has no TDB node ids (TrPathPDP); use TrackPDP world placement".into(),
        ));
    }
    let mut out = Vec::new();
    for (pos, (i, tdb_id)) in indexed.iter().enumerate() {
        let next_id = indexed.get(pos + 1).map(|(_, id)| *id);
        let graph_node = resolve_pat_graph_node(graph, aliases, *tdb_id, next_id, *i > 0)?;
        out.push(ResolvedPatNode { graph_node });
    }
    Ok(out)
}

fn resolve_pat_graph_node(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    tdb_id: u32,
    next_tdb_id: Option<u32>,
    _has_prev: bool,
) -> Result<String, MstsError> {
    let key = format!("n{tdb_id}");
    if graph.node(&key).is_some() {
        return Ok(key);
    }
    if let Some(alias) = aliases.get(&tdb_id) {
        if alias.is_node() {
            return Ok(alias.id.clone());
        }
        if alias.is_edge() {
            let from = alias.from.as_deref().unwrap_or("");
            let to = alias.to.as_deref().unwrap_or("");
            if let Some(next) = next_tdb_id {
                let next_node = format!("n{next}");
                if next_node == from || aliases.get(&next).is_some_and(|a| a.id == from) {
                    return Ok(to.to_string());
                }
                if next_node == to || aliases.get(&next).is_some_and(|a| a.id == to) {
                    return Ok(from.to_string());
                }
            }
            return Ok(from.to_string());
        }
    }
    Err(MstsError::Msg(format!(
        "could not resolve PAT node tdb_id={tdb_id} on imported graph"
    )))
}

fn placement_from_distance(
    graph: &TrackGraph,
    pat: &[ResolvedPatNode],
    distance_m: f64,
) -> Result<(String, f64), MstsError> {
    if pat.is_empty() {
        return Err(MstsError::Msg("empty PAT".into()));
    }
    if distance_m <= 0.0 {
        return Ok((pat[0].graph_node.clone(), 0.0));
    }

    // Legacy TrPathPDP-only fixture: PAT starts at dead-end n1 with a real TDB id.
    // Never apply this when TrackPDP flags were mis-read as node ids (world PATs use
    // [`placement_from_world`] instead).
    if pat[0].graph_node == "n1" && pat.len() > 1 && pat[1].graph_node != "n1" {
        let has_n1_n3 = graph.edges_iter().any(|(_, e)| {
            (e.from.0 == "n3" && e.to.0 == "n1") || (e.from.0 == "n1" && e.to.0 == "n3")
        });
        if has_n1_n3 {
            let platform_len = graph
                .edges_iter()
                .find(|(_, e)| {
                    (e.from.0 == "n3" && e.to.0 == "n1") || (e.from.0 == "n1" && e.to.0 == "n3")
                })
                .map(|(_, e)| e.length_m)
                .unwrap_or(500.0);
            let start = pat[1].graph_node.clone();
            let offset = (platform_len - distance_m).clamp(0.0, platform_len);
            return Ok((start, offset));
        }
    }

    let mut remaining = distance_m;
    for i in 0..pat.len().saturating_sub(1) {
        let hop = hop_length(graph, &pat[i].graph_node, &pat[i + 1].graph_node);
        if remaining <= hop {
            return Ok((pat[i].graph_node.clone(), remaining));
        }
        remaining -= hop;
    }
    Ok((pat.last().unwrap().graph_node.clone(), 0.0))
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
    1000.0
}

fn pick_destination_node(
    graph: &TrackGraph,
    start: &str,
    pat: &[ResolvedPatNode],
) -> Result<String, MstsError> {
    let start_idx = pat.iter().position(|p| p.graph_node == start);
    let forward: Vec<&ResolvedPatNode> = pat
        .iter()
        .skip(start_idx.map(|i| i + 1).unwrap_or(0))
        .collect();

    let mut best: Option<(String, f64)> = None;
    let mut seen = HashSet::new();
    for p in forward.into_iter().chain(pat.iter()) {
        if !seen.insert(p.graph_node.as_str()) {
            continue;
        }
        if p.graph_node == start {
            continue;
        }
        if graph.outgoing_edges(&p.graph_node).is_empty() {
            continue;
        }
        let Ok(edges) = edge_path_ignoring_switches(graph, start, &p.graph_node) else {
            continue;
        };
        let dist: f64 = edges
            .iter()
            .filter_map(|eid| graph.edge(eid).map(|e| e.length_m))
            .sum();
        if best.as_ref().is_none_or(|(_, d)| dist > *d) {
            best = Some((p.graph_node.clone(), dist));
        }
    }
    if let Some((node, dist)) = &best
        && *dist > 1000.0
    {
        return Ok(node.clone());
    }
    if let Some(far) = bfs_far_node(graph, start) {
        return Ok(far);
    }
    best.map(|(n, _)| n)
        .ok_or_else(|| MstsError::Msg(format!("no reachable destination from start node {start}")))
}

fn bfs_far_node(graph: &TrackGraph, start: &str) -> Option<String> {
    let mut q = VecDeque::from([(start.to_string(), 0_usize)]);
    let mut seen = HashSet::from([start.to_string()]);
    let mut best = start.to_string();
    let mut best_depth = 0;
    while let Some((node, depth)) = q.pop_front() {
        if depth > best_depth {
            best_depth = depth;
            best = node.clone();
        }
        for eid in graph.outgoing_edges(&node) {
            let Some(edge) = graph.edge(eid) else {
                continue;
            };
            let next = edge.to.0.clone();
            if seen.insert(next.clone()) {
                q.push_back((next, depth + 1));
            }
        }
    }
    if best_depth > 0 { Some(best) } else { None }
}

fn switches_from_pat(
    path_file: &PathFile,
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    start: &str,
    destination: &str,
    start_offset_m: f64,
) -> Result<Vec<SwitchDef>, MstsError> {
    if path_file.has_world_pdps() {
        // A global shortest route may take another branch than the authored PAT.
        // Align switches to each ordered waypoint hop, before runtime traversal.
        let waypoints =
            pat_waypoints_from_world(graph, path_file, start, destination, start_offset_m)?;
        let mut edges = Vec::new();
        for pair in waypoints.windows(2) {
            edges.extend(
                edge_path_ignoring_switches(graph, &pair[0], &pair[1])
                    .map_err(|e| MstsError::msg(e.to_string()))?,
            );
        }
        return Ok(switches_for_edges(graph, &edges));
    }
    let mut out = Vec::new();
    for pdp in &path_file.pdps {
        // TrPathPDP junction_flag 1 = diverging. Native TrackPDP uses flag1==2 for junctions
        // without a TDB node id — those fall through to switches_for_route.
        let Some(tdb_id) = pdp.node_id else {
            continue;
        };
        if pdp.junction_flag == 0 {
            continue;
        }
        let node = resolve_pat_graph_node(graph, aliases, tdb_id, None, true).ok();
        let Some(node) = node else { continue };
        if !matches!(
            graph.node(&node).map(|n| &n.kind),
            Some(openrailsrs_track::NodeKind::Switch { .. })
        ) {
            continue;
        }
        out.push(SwitchDef {
            node,
            position: SwitchPositionDef::Diverging,
        });
    }
    if out.is_empty() {
        return switches_for_route(graph, start, destination);
    }
    Ok(out)
}

fn switches_for_route(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<SwitchDef>, MstsError> {
    let edge_ids = edge_path_ignoring_switches(graph, start, destination)
        .map_err(|e| MstsError::Msg(e.to_string()))?;
    Ok(switches_for_edges(graph, &edge_ids))
}

fn switches_for_edges(graph: &TrackGraph, edge_ids: &[String]) -> Vec<SwitchDef> {
    let mut out = Vec::new();
    for (node_id, node) in graph.nodes_iter() {
        let openrailsrs_track::NodeKind::Switch {
            stem_edge,
            diverging_edge,
        } = &node.kind
        else {
            continue;
        };
        let uses_stem = edge_ids.iter().any(|e| e == &stem_edge.0);
        let uses_div = edge_ids.iter().any(|e| e == &diverging_edge.0);
        if uses_stem && !uses_div {
            out.push(SwitchDef {
                node: node_id.to_string(),
                position: SwitchPositionDef::Straight,
            });
        } else if uses_div && !uses_stem {
            out.push(SwitchDef {
                node: node_id.to_string(),
                position: SwitchPositionDef::Diverging,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, SwitchPosition};

    #[test]
    fn native_anchors_choose_the_curved_platform_instead_of_a_neighbouring_chord() {
        let mut graph = TrackGraph::new();
        let (x, z) = (-12_450_000.125, -30_566_000.25);
        for (id, dx, dz) in [
            ("correct_start", -10.0, 0.0),
            ("correct_end", -10.0, 100.0),
            ("other_start", 0.2, 0.0),
            ("other_end", 0.2, 100.0),
        ] {
            graph
                .insert_node(Node {
                    id: NodeId(id.into()),
                    kind: NodeKind::Plain,
                    x_m: x + dx,
                    y_m: z + dz,
                })
                .unwrap();
        }
        let mut polylines = HashMap::new();
        for (id, start, end, dx) in [
            ("correct", "correct_start", "correct_end", 0.0),
            ("other", "other_start", "other_end", 10.0),
        ] {
            let from = graph.node(start).unwrap();
            let to = graph.node(end).unwrap();
            let mut points = vec![
                (from.x_m, from.y_m),
                (x + dx, z),
                (x + dx, z + 100.0),
                (to.x_m, to.y_m),
            ];
            polylines.insert(id.to_string(), points.clone());
            points.reverse();
            polylines.insert(format!("{id}_r"), points);
            for (edge, a, b) in [
                (id.to_string(), start, end),
                (format!("{id}_r"), end, start),
            ] {
                graph
                    .insert_edge(Edge {
                        id: EdgeId(edge),
                        from: NodeId(a.into()),
                        to: NodeId(b.into()),
                        length_m: 100.0,
                        speed_limit_mps: 20.0,
                        grade_percent: 0.0,
                    })
                    .unwrap();
            }
        }
        let chord = snap_world_to_edge(&graph, x, z + 50.0, Some((0.0, 1.0)), None).unwrap();
        assert_eq!(chord.0, "other_start");
        let native =
            snap_world_to_edge(&graph, x, z + 50.0, Some((0.0, 1.0)), Some(&polylines)).unwrap();
        assert_eq!(native.0, "correct_start");
        assert_eq!(native.2, "correct_end");
        assert!((native.1 - 50.0).abs() < 1e-6);
        let reverse =
            snap_world_to_edge(&graph, x, z + 50.0, Some((0.0, -1.0)), Some(&polylines)).unwrap();
        assert_eq!(reverse.0, "correct_end");
        assert_eq!(reverse.2, "correct_start");
        assert!((reverse.1 - 50.0).abs() < 1e-6);

        graph
            .insert_edge(Edge {
                id: EdgeId("other_track_connection".into()),
                from: NodeId("correct_end".into()),
                to: NodeId("other_start".into()),
                length_m: 100.0,
                speed_limit_mps: 20.0,
                grade_percent: 0.0,
            })
            .unwrap();
        let worlds = [z, z + 50.0].map(|z| openrailsrs_formats::TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 0.0,
            z: -z,
        });
        assert_ne!(
            destination_from_world_pdps(&graph, "correct_start", &worlds, None).unwrap(),
            "correct_end"
        );
        assert_eq!(
            destination_from_world_pdps(&graph, "correct_start", &worlds, Some(&polylines))
                .unwrap(),
            "correct_end"
        );
    }

    #[test]
    fn native_waypoints_align_switches_to_the_authored_branch() {
        let mut graph = TrackGraph::new();
        for (id, x, z) in [
            ("start", 0.0, 0.0),
            ("junction", 100.0, 0.0),
            ("via", 200.0, 100.0),
            ("end", 300.0, 0.0),
        ] {
            let kind = if id == "junction" {
                NodeKind::Switch {
                    stem_edge: EdgeId("direct".into()),
                    diverging_edge: EdgeId("scenic".into()),
                }
            } else {
                NodeKind::Plain
            };
            graph
                .insert_node(Node {
                    id: NodeId(id.into()),
                    kind,
                    x_m: x,
                    y_m: z,
                })
                .unwrap();
        }
        for (id, from, to) in [
            ("lead", "start", "junction"),
            ("direct", "junction", "end"),
            ("scenic", "junction", "via"),
            ("finish", "via", "end"),
        ] {
            graph
                .insert_edge(Edge {
                    id: EdgeId(id.into()),
                    from: NodeId(from.into()),
                    to: NodeId(to.into()),
                    length_m: 100.0,
                    speed_limit_mps: 20.0,
                    grade_percent: 0.0,
                })
                .unwrap();
        }
        let path = PathFile {
            name: "Original branch".into(),
            pdps: [(0.0, 0.0), (100.0, 0.0), (200.0, -100.0), (300.0, 0.0)]
                .into_iter()
                .map(|(x, z)| openrailsrs_formats::PathDataPoint {
                    node_id: None,
                    junction_flag: 2,
                    invalid_flag: 0,
                    world: Some(openrailsrs_formats::TrackVectorPoint {
                        tile_x: 0,
                        tile_z: 0,
                        x,
                        y: 0.0,
                        z,
                    }),
                })
                .collect(),
        };
        assert_eq!(
            edge_path(&graph, "start", "end").unwrap(),
            ["lead", "direct"]
        );
        let switches =
            switches_from_pat(&path, &graph, &HashMap::new(), "start", "end", 0.0).unwrap();
        assert_eq!(switches.len(), 1);
        assert!(matches!(switches[0].position, SwitchPositionDef::Diverging));
        graph
            .set_switch("junction", SwitchPosition::Diverging)
            .unwrap();
        assert_eq!(
            edge_path(&graph, "start", "end").unwrap(),
            ["lead", "scenic", "finish"]
        );
    }

    #[test]
    fn head_offset_from_rear_snap_adds_consist_length() {
        // #132: same rear snap + length → unique head offset.
        assert!((head_offset_from_rear_snap(100.0, 50.0) - 150.0).abs() < 1e-9);
        assert!((head_offset_from_rear_snap(166.735, 165.8) - 332.535).abs() < 1e-6);
        assert_eq!(
            consist_length_from_vehicle_lengths(&[20.0, 20.0, 18.5]),
            58.5
        );
        // Without consist length, world placement keeps rear snap (not DistanceDownPath).
        assert_eq!(head_offset_from_rear_snap(166.735, 0.0), 166.735);
    }

    #[test]
    fn chiltern_srv_distance_down_path() {
        let route_dir = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern",
        );
        if !route_dir.exists() {
            return;
        }
        let srv_path = route_dir
            .join("SERVICES")
            .join("RS_Let's go to Birmingham.srv");
        assert!(srv_path.exists(), "srv missing: {}", srv_path.display());
        let text =
            openrailsrs_formats::encoding::read_msts_file_to_string(&srv_path).expect("read srv");
        let dist = read_distance_down_path(route_dir, "RS_Let's go to Birmingham")
            .expect("DistanceDownPath from Birmingham srv");
        assert!((dist - 194.424).abs() < 0.01, "got {dist}");
        assert!(
            text.contains("DistanceDownPath"),
            "decoded srv should mention DistanceDownPath"
        );
    }

    #[test]
    fn chiltern_placement_snaps_near_pat_start() {
        let route_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("track.toml").exists() {
            return;
        }
        let pat = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/PATHS/RS_Let's go to Birmingham.pat",
        );
        if !pat.exists() {
            return;
        }
        let path_file = PathFile::from_path(pat).expect("parse pat");
        assert!(path_file.has_world_pdps());
        assert!(
            path_file.pdps[0].node_id.is_none(),
            "native TrackPDP must not invent node_id from flags"
        );
        assert_eq!(path_file.pdps[0].junction_flag, 1);

        let hints = placement_from_imported_route(&route_dir, pat, 194.424).expect("placement");
        assert!(hints.start_offset_m >= 0.0);
        assert_ne!(hints.start, hints.destination);

        let loaded = load_route_from_dir(&route_dir).expect("load");
        let spawn_dist = spawn_distance_to_pat_start(&loaded.graph, &hints, &path_file);
        assert!(
            spawn_dist < 100.0,
            "spawn should be near PAT start / Pfm 6, got {spawn_dist:.1} m (start={} offset={:.3})",
            hints.start,
            hints.start_offset_m
        );
        assert_ne!(
            hints.start, "n3",
            "must not use broken Paddington n1→n3+305 heuristic on TrackPDP flags"
        );
    }

    #[test]
    fn chiltern_birmingham_pat_edge_path() {
        let route_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("track.toml").exists() {
            return;
        }
        let pat = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/PATHS/RS_Let's go to Birmingham.pat",
        );
        if !pat.exists() {
            return;
        }
        let path_file = PathFile::from_path(pat).expect("parse pat");
        let loaded = load_route_from_dir(&route_dir).expect("load chiltern");
        let hints = placement_from_imported_route(&route_dir, pat, 194.424).expect("placement");
        let mut graph = loaded.graph.clone();
        for sw in &hints.switches {
            let pos = match sw.position {
                SwitchPositionDef::Straight => openrailsrs_track::SwitchPosition::Straight,
                SwitchPositionDef::Diverging => openrailsrs_track::SwitchPosition::Diverging,
            };
            let _ = graph.set_switch(&sw.node, pos);
        }

        let pat_path = pat_edge_path_with_offset(
            &graph,
            &loaded.msts_aliases,
            &path_file,
            &hints.start,
            &hints.destination,
            hints.start_offset_m,
        )
        .expect("pat edges");
        let wps = pat_waypoints_with_offset(
            &graph,
            &loaded.msts_aliases,
            &path_file,
            &hints.start,
            &hints.destination,
            hints.start_offset_m,
        )
        .expect("waypoints");
        eprintln!(
            "PAT waypoints (start={} offset={:.3}): {} nodes",
            hints.start,
            hints.start_offset_m,
            wps.len()
        );

        assert!(
            pat_path.len() > 5,
            "expected long PAT path after reverse edges, got {} edges",
            pat_path.len()
        );
        assert!(
            pat_path.first().is_some_and(|e| e == "e17431_r"),
            "native platform 6 must use e17431_r instead of a detour through the neighbouring platform, got {:?}",
            &pat_path[..pat_path.len().min(8)]
        );
        assert!(
            wps.len() >= 10,
            "expected many PAT waypoints, got {}",
            wps.len()
        );
        assert_eq!(wps.first().map(String::as_str), Some(hints.start.as_str()));
        assert_eq!(hints.start, "n17361");
        let spawn_dist = spawn_distance_to_pat_start(&graph, &hints, &path_file);
        assert!(
            spawn_dist < 150.0,
            "path start should stay near PAT start, got {spawn_dist:.1} m"
        );
    }

    fn spawn_distance_to_pat_start(
        graph: &TrackGraph,
        hints: &RouteHints,
        path_file: &PathFile,
    ) -> f64 {
        let w = path_file
            .pdps
            .iter()
            .find_map(|p| p.world)
            .expect("PAT world start");
        let px = w.graph_x_m();
        let pz = w.graph_z_m();
        let start = graph.node(&hints.start).expect("start node");
        // Approximate spawn as start + offset toward the first outgoing edge used by route.
        let mut best = f64::INFINITY;
        for eid in openrailsrs_route::path::allowed_outgoing_edges(graph, &hints.start) {
            let Some(edge) = graph.edge(&eid) else {
                continue;
            };
            let Some(to) = graph.node(&edge.to.0) else {
                continue;
            };
            let t = if edge.length_m > 0.0 {
                (hints.start_offset_m / edge.length_m).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let sx = start.x_m + t * (to.x_m - start.x_m);
            let sz = start.y_m + t * (to.y_m - start.y_m);
            let d = ((sx - px).powi(2) + (sz - pz).powi(2)).sqrt();
            best = best.min(d);
        }
        best
    }
}
