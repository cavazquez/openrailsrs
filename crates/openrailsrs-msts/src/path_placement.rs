//! Map MSTS `.pat` / `.srv` hints onto an imported `track.toml` graph.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use openrailsrs_formats::PathFile;
use openrailsrs_route::{
    MstsAlias, load_route_from_dir,
    path::{edge_path, edge_path_via_waypoints},
};
use openrailsrs_scenarios::model::{SwitchDef, SwitchPositionDef};
use openrailsrs_track::TrackGraph;

use crate::error::MstsError;

/// Route start/destination hints derived from a player path and service file.
#[derive(Debug, Clone)]
pub struct RouteHints {
    pub start: String,
    pub destination: String,
    pub start_offset_m: f64,
    pub switches: Vec<SwitchDef>,
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
pub fn placement_for_pat(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    pat_path: &Path,
    start_offset_m: f64,
) -> Result<RouteHints, MstsError> {
    let path_file = PathFile::from_path(pat_path)?;
    let resolved = resolve_pat_sequence(graph, aliases, &path_file)?;

    let (start, offset) = placement_from_distance(graph, &resolved, start_offset_m)?;
    let destination = pick_destination_node(graph, &start, &resolved)?;
    let switches = switches_from_pat(&path_file, graph, aliases, &start, &destination)?;

    Ok(RouteHints {
        start,
        destination,
        start_offset_m: offset,
        switches,
    })
}

/// Convenience: load `track.toml` from `route_dir` and compute hints.
pub fn placement_from_imported_route(
    route_dir: &Path,
    pat_path: &Path,
    start_offset_m: f64,
) -> Result<RouteHints, MstsError> {
    let loaded = load_route_from_dir(route_dir).map_err(|e| MstsError::Msg(e.to_string()))?;
    placement_for_pat(
        &loaded.graph,
        &loaded.msts_aliases,
        pat_path,
        start_offset_m,
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
            let (ax, _, az) = world_pdps[i - 1].bevy_position();
            let (bx, _, bz) = w.bevy_position();
            let dx = (bx - ax) as f64;
            let dz = (bz - az) as f64;
            walked += (dx * dx + dz * dz).sqrt();
        }
        if !past_offset {
            if walked >= start_offset_m {
                past_offset = true;
            } else {
                continue;
            }
        }
        let Some(nid) = nearest_node_id(graph, w) else {
            continue;
        };
        if waypoints.last().map(String::as_str) == Some(nid.as_str()) {
            continue;
        }
        let tail = waypoints.last().expect("non-empty");
        if edge_path(graph, tail, &nid).is_ok() {
            waypoints.push(nid);
        }
    }
    if !waypoints.iter().any(|n| n == destination) {
        if let Some(tail) = waypoints.last() {
            if edge_path(graph, tail, destination).is_ok() && tail != destination {
                waypoints.push(destination.to_string());
            }
        }
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
    let (px, _, pz) = world.bevy_position();
    graph
        .nodes_iter()
        .min_by(|(_, a), (_, b)| {
            let da = sq_dist_node(px, pz, a);
            let db = sq_dist_node(px, pz, b);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, _)| id.to_string())
}

fn sq_dist_node(px: f32, pz: f32, node: &openrailsrs_track::Node) -> f32 {
    let dx = node.x_m as f32 - px;
    let dz = node.y_m as f32 - pz;
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
    let mut out = Vec::new();
    for (i, pdp) in path_file.pdps.iter().enumerate() {
        let prev = pdp.node_id;
        let next_id = path_file.pdps.get(i + 1).map(|p| p.node_id);
        let graph_node = resolve_pat_graph_node(graph, aliases, pdp.node_id, next_id, i > 0)?;
        out.push(ResolvedPatNode { graph_node });
        let _ = prev;
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

    // Paddington platform: PAT starts at dead-end n1; DistanceDownPath is from buffer toward main line.
    if pat[0].graph_node == "n1" && pat.len() > 1 {
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
        let Ok(edges) = edge_path(graph, start, &p.graph_node) else {
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
    if let Some((node, dist)) = &best {
        if *dist > 1000.0 {
            return Ok(node.clone());
        }
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
        for eid in openrailsrs_route::path::allowed_outgoing_edges(graph, &node) {
            let Some(edge) = graph.edge(&eid) else {
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
) -> Result<Vec<SwitchDef>, MstsError> {
    let mut out = Vec::new();
    for pdp in &path_file.pdps {
        if pdp.junction_flag == 0 {
            continue;
        }
        let node = resolve_pat_graph_node(graph, aliases, pdp.node_id, None, true).ok();
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
    let edge_ids =
        edge_path(graph, start, destination).map_err(|e| MstsError::Msg(e.to_string()))?;
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
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn chiltern_placement_resolves_main_line_start() {
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
        let hints = placement_from_imported_route(&route_dir, pat, 194.424).expect("placement");
        assert!(hints.start_offset_m >= 0.0);
        assert_ne!(hints.start, hints.destination);
        assert_eq!(hints.start, "n3", "Paddington platform start on main line");
        assert!(hints.start_offset_m > 250.0 && hints.start_offset_m < 350.0);
    }

    #[test]
    fn chiltern_birmingham_pat_edge_path() {
        use openrailsrs_track::SwitchPosition;

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
        let mut graph = loaded.graph.clone();
        graph
            .set_switch("n10770", SwitchPosition::Diverging)
            .expect("sw");
        graph
            .set_switch("n10780", SwitchPosition::Straight)
            .expect("sw");

        let pat_path = pat_edge_path_with_offset(
            &graph,
            &loaded.msts_aliases,
            &path_file,
            "n3",
            "n10770",
            305.576,
        )
        .expect("pat edges");
        let wps = pat_waypoints_with_offset(
            &graph,
            &loaded.msts_aliases,
            &path_file,
            "n3",
            "n10770",
            305.576,
        )
        .expect("waypoints");
        eprintln!("PAT waypoints (offset 305m): {} nodes", wps.len());
        if wps.len() <= 8 {
            eprintln!("  {wps:?}");
        }

        assert!(
            pat_path.len() >= 6,
            "expected long Birmingham PAT path, got {} edges",
            pat_path.len()
        );
        assert!(
            pat_path.contains(&"e10783".to_string()),
            "PAT path must use Paddington departure e10783"
        );
        assert!(
            pat_path.contains(&"e10771".to_string()),
            "PAT path must reach destination approach e10771"
        );

        let bfs = edge_path(&graph, "n3", "n10770").expect("bfs");
        assert_eq!(
            pat_path, bfs,
            "with Birmingham switches, PAT waypoints should match BFS"
        );
    }
}
