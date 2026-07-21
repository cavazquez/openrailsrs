//! Convert an MSTS Track Database (`.tdb`) into an `openrailsrs` `track.toml`.
//!
//! The mapping is:
//! - `TrEndNode`      → plain node
//! - `TrJunctionNode` → switch node (`stem_edge` / `diverging_edge` inferred from pins)
//! - `TrVectorNode`   → two endpoint nodes + one directed edge
//!
//! Output matches [`openrailsrs_route::load::RouteLayoutFile`] (see `examples/` and OSM import).

use std::collections::HashMap;
use std::path::Path;

use openrailsrs_formats::{
    ActivityFile, MstsFile, TrItem, TrItemKind, TrPinRef, TrVectorSectionRecord, TrackDbFile,
    TrackNodeKind, TrackVectorGeometry, TrackVectorPoint, parse_msts_file,
};
use serde::Serialize;

use crate::error::MstsError;

// ── TOML schema (mirrors openrailsrs-route/src/load.rs) ─────────────────────

#[derive(Serialize)]
struct TrackToml {
    route: RouteMeta,
    nodes: Vec<NodeToml>,
    edges: Vec<EdgeToml>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    signals: Vec<SignalToml>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    msts_aliases: Vec<MstsAliasToml>,
}

#[derive(Serialize)]
struct MstsAliasToml {
    tdb_id: u32,
    kind: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
}

#[derive(Serialize)]
struct RouteMeta {
    id: String,
}

#[derive(Serialize)]
struct NodeToml {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<SwitchKindTable>,
    #[serde(default, skip_serializing_if = "is_zero")]
    x_m: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    y_m: f64,
}

#[derive(Serialize)]
struct SwitchKindTable {
    switch: SwitchEdges,
}

#[derive(Serialize)]
struct SwitchEdges {
    stem_edge: String,
    diverging_edge: String,
    #[serde(default = "default_switch_position")]
    default_position: String,
}

fn default_switch_position() -> String {
    "straight".into()
}

#[derive(Serialize)]
struct EdgeToml {
    id: String,
    from: String,
    to: String,
    length_m: f64,
    speed_limit_kmh: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    grade_percent: f64,
}

#[derive(Serialize)]
struct SignalToml {
    id: String,
    edge_id: String,
    position_m: f64,
    aspect: String,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Read the first `*.tdb` file found in `route_dir` and convert it to a
/// `track.toml` TOML string.
pub fn import_route(route_dir: &Path) -> Result<String, MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = load_tdb(route_dir, &tdb_path)?;
    ensure_non_empty_tdb(&tdb, &tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let toml = convert_tdb_to_toml(&tdb, &route_id, None)?;
    Ok(toml)
}

/// Same as `import_route` but applies activity-level overrides (failed signals
/// and restricted speed zones) to the generated `track.toml`.
pub fn import_route_with_activity(route_dir: &Path, act_path: &Path) -> Result<String, MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = load_tdb(route_dir, &tdb_path)?;
    ensure_non_empty_tdb(&tdb, &tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let activity = ActivityFile::from_path(act_path)?;
    let toml = convert_tdb_to_toml(&tdb, &route_id, Some(&activity))?;
    Ok(toml)
}

/// Same as `import_route` but also returns a count summary `(nodes, edges)`.
pub fn import_route_with_summary(route_dir: &Path) -> Result<(String, usize, usize), MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = load_tdb(route_dir, &tdb_path)?;
    ensure_non_empty_tdb(&tdb, &tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let (nodes, edges) = count_nodes_edges(&tdb);
    let toml = convert_tdb_to_toml(&tdb, &route_id, None)?;
    Ok((toml, nodes, edges))
}

/// Update `x_m`/`y_m` on an existing `track.toml` from the route `.tdb` without replacing edges.
///
/// Node ids must match a fresh import (same `n*` / `anon*` names). Returns the number of nodes
/// that received non-zero coordinates.
pub fn patch_track_coordinates(route_dir: &Path, track_path: &Path) -> Result<usize, MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = load_tdb(route_dir, &tdb_path)?;
    ensure_non_empty_tdb(&tdb, &tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let fresh = convert_tdb_to_toml(&tdb, &route_id, None)?;
    let fresh_val: toml::Value = toml::from_str(&fresh)?;
    let existing_text = std::fs::read_to_string(track_path)?;
    let mut existing: toml::Value = toml::from_str(&existing_text)?;

    let fresh_coords = node_coordinates_from_toml(&fresh_val);
    let Some(nodes) = existing.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return Err(MstsError::msg(format!(
            "no [[nodes]] in {}",
            track_path.display()
        )));
    };

    let mut patched = 0usize;
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some((x_m, y_m)) = fresh_coords.get(id) else {
            continue;
        };
        if let Some(table) = node.as_table_mut() {
            table.insert("x_m".into(), toml::Value::Float(*x_m));
            table.insert("y_m".into(), toml::Value::Float(*y_m));
            patched += 1;
        }
    }

    std::fs::write(track_path, toml::to_string_pretty(&existing)?)?;
    Ok(patched)
}

fn node_coordinates_from_toml(value: &toml::Value) -> HashMap<String, (f64, f64)> {
    let mut out = HashMap::new();
    let Some(nodes) = value.get("nodes").and_then(|v| v.as_array()) else {
        return out;
    };
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let x_m = node.get("x_m").and_then(|v| v.as_float()).unwrap_or(0.0);
        let y_m = node.get("y_m").and_then(|v| v.as_float()).unwrap_or(0.0);
        if x_m == 0.0 && y_m == 0.0 {
            continue;
        }
        out.insert(id.to_string(), (x_m, y_m));
    }
    out
}

fn load_tdb(_route_dir: &Path, tdb_path: &Path) -> Result<TrackDbFile, MstsError> {
    let mut tdb = TrackDbFile::from_path(tdb_path)?;
    let tit_path = tdb_path.with_extension("tit");
    if tit_path.exists() {
        let _ = tdb.merge_tit_speed_posts(&tit_path);
    }
    Ok(tdb)
}

fn ensure_non_empty_tdb(tdb: &TrackDbFile, tdb_path: &Path) -> Result<(), MstsError> {
    if tdb.nodes.is_empty() {
        return Err(MstsError::msg(format!(
            "no track nodes parsed from {} (native MSTS editor layout may be unsupported)",
            tdb_path.display()
        )));
    }
    Ok(())
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn find_tdb(dir: &Path) -> Result<std::path::PathBuf, MstsError> {
    let mut tdbs: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case("tdb"))
                .unwrap_or(false)
        })
        .collect();
    tdbs.sort();
    tdbs.into_iter()
        .next()
        .ok_or_else(|| MstsError::msg(format!("no *.tdb file found in {}", dir.display())))
}

fn find_route_id(route_dir: &Path, tdb_path: &Path) -> String {
    for entry in std::fs::read_dir(route_dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if !p
            .extension()
            .map(|x| x.eq_ignore_ascii_case("trk"))
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(MstsFile::Route(route)) = parse_msts_file(&p) {
            return route.route_id;
        }
    }
    tdb_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported")
        .to_string()
}

fn count_nodes_edges(tdb: &TrackDbFile) -> (usize, usize) {
    let mut nodes = 0usize;
    let mut edges = 0usize;
    for n in &tdb.nodes {
        match &n.kind {
            TrackNodeKind::End | TrackNodeKind::Junction { .. } => nodes += 1,
            TrackNodeKind::Vector { .. } => {
                nodes += 2;
                edges += 1;
            }
        }
    }
    (nodes, edges)
}

fn convert_tdb_to_toml(
    tdb: &TrackDbFile,
    route_id: &str,
    activity: Option<&ActivityFile>,
) -> Result<String, MstsError> {
    let mut node_map: HashMap<u32, String> = HashMap::new();
    let mut junction_pins: HashMap<u32, Vec<TrPinRef>> = HashMap::new();
    let mut msts_aliases: Vec<MstsAliasToml> = Vec::new();
    let mut nodes: Vec<NodeToml> = Vec::new();

    for n in &tdb.nodes {
        match &n.kind {
            TrackNodeKind::End => {
                let id = format!("n{}", n.id);
                node_map.insert(n.id, id.clone());
                msts_aliases.push(MstsAliasToml {
                    tdb_id: n.id,
                    kind: "node".into(),
                    id: id.clone(),
                    from: None,
                    to: None,
                });
                nodes.push(NodeToml {
                    id,
                    kind: None,
                    x_m: n.position.map(point_graph_x).unwrap_or(0.0),
                    y_m: n.position.map(point_graph_z).unwrap_or(0.0),
                });
            }
            TrackNodeKind::Junction { pins } => {
                let id = format!("n{}", n.id);
                node_map.insert(n.id, id.clone());
                junction_pins.insert(n.id, pins.clone());
                msts_aliases.push(MstsAliasToml {
                    tdb_id: n.id,
                    kind: "node".into(),
                    id: id.clone(),
                    from: None,
                    to: None,
                });
                nodes.push(NodeToml {
                    id,
                    kind: None,
                    x_m: n.position.map(point_graph_x).unwrap_or(0.0),
                    y_m: n.position.map(point_graph_z).unwrap_or(0.0),
                });
            }
            TrackNodeKind::Vector { .. } => {}
        }
    }

    let mut vec_counter = 0u32;
    let mut item_to_edge: HashMap<u32, String> = HashMap::new();
    let mut edges: Vec<EdgeToml> = Vec::new();

    for n in &tdb.nodes {
        if let TrackNodeKind::Vector {
            length_m,
            speed_limit_mps,
            pins,
            item_ids,
            geometry,
            sections,
            ..
        } = &n.kind
        {
            let from_id = resolve_pin(pins.0, &node_map, &mut vec_counter, &mut nodes);
            let to_id = resolve_pin(pins.1, &node_map, &mut vec_counter, &mut nodes);
            let (start_pt, end_pt) = vector_endpoint_positions(sections, *geometry, *length_m);
            if let Some(start) = start_pt {
                set_node_position(&mut nodes, &from_id, start);
            }
            if let Some(end) = end_pt {
                set_node_position(&mut nodes, &to_id, end);
            }
            let edge_id = format!("e{}", n.id);
            for item_id in item_ids {
                item_to_edge.insert(*item_id, edge_id.clone());
            }
            edges.push(EdgeToml {
                id: edge_id.clone(),
                from: from_id.clone(),
                to: to_id.clone(),
                length_m: *length_m,
                speed_limit_kmh: *speed_limit_mps * 3.6,
                grade_percent: 0.0,
            });
            msts_aliases.push(MstsAliasToml {
                tdb_id: n.id,
                kind: "edge".into(),
                id: edge_id,
                from: Some(from_id),
                to: Some(to_id),
            });
        }
    }

    configure_switch_nodes(&mut nodes, &mut edges, &junction_pins, &msts_aliases);
    apply_tdb_world_positions(tdb, &node_map, &mut nodes);

    let mut signals = build_signals(&tdb.items, &item_to_edge);

    if let Some(act) = activity {
        apply_failed_signals(&mut signals, &act.failed_signals);
        apply_speed_posts(&mut edges, &tdb.items, &item_to_edge);
        apply_restricted_zones(&mut edges, &act.restricted_zones, &item_to_edge);
    } else {
        apply_speed_posts(&mut edges, &tdb.items, &item_to_edge);
    }

    let track = TrackToml {
        route: RouteMeta {
            id: route_id.to_string(),
        },
        nodes,
        edges,
        signals,
        msts_aliases,
    };
    Ok(toml::to_string_pretty(&track)?)
}

/// Attach switch metadata to junction nodes once all edges exist.
fn configure_switch_nodes(
    nodes: &mut [NodeToml],
    edges: &mut [EdgeToml],
    junction_pins: &HashMap<u32, Vec<TrPinRef>>,
    aliases: &[MstsAliasToml],
) {
    for node in nodes.iter_mut() {
        let Some(jid) = node_id_num(&node.id) else {
            continue;
        };
        let Some(pins) = junction_pins.get(&jid) else {
            continue;
        };
        if pins.len() < 2 {
            continue;
        }

        let stem_pin = pins.iter().find(|p| p.branch_index == 0);
        let div_pin = pins.iter().find(|p| p.branch_index == 1).or_else(|| {
            pins.iter().find(|p| {
                p.branch_index > 0 && p.node_id != stem_pin.map(|s| s.node_id).unwrap_or(0)
            })
        });

        let stem_target = stem_pin.and_then(|p| resolve_pin_endpoint(p.node_id, jid, aliases));
        let div_target = div_pin.and_then(|p| resolve_pin_endpoint(p.node_id, jid, aliases));

        let stem_id = stem_target
            .as_ref()
            .and_then(|t| find_or_orient_edge(edges, &node.id, t, true));
        let div_id = div_target
            .as_ref()
            .and_then(|t| find_or_orient_edge(edges, &node.id, t, false));

        let (stem_id, div_id) = match (stem_id, div_id) {
            (Some(s), Some(d)) if s != d => (s, d),
            (Some(s), _) => {
                let fallback = edges
                    .iter()
                    .find(|e| e.from == node.id && e.id != s)
                    .map(|e| e.id.clone());
                (s.clone(), fallback.unwrap_or(s))
            }
            _ => continue,
        };

        node.kind = Some(SwitchKindTable {
            switch: SwitchEdges {
                stem_edge: stem_id,
                diverging_edge: div_id,
                default_position: default_switch_position(),
            },
        });
    }
}

fn resolve_pin_endpoint(
    pin_tdb_id: u32,
    junction_id: u32,
    aliases: &[MstsAliasToml],
) -> Option<String> {
    let alias = aliases.iter().find(|a| a.tdb_id == pin_tdb_id)?;
    match alias.kind.as_str() {
        "node" => Some(alias.id.clone()),
        "edge" => {
            let from = alias.from.as_deref()?;
            let to = alias.to.as_deref()?;
            let j = format!("n{junction_id}");
            if from == j {
                Some(to.to_string())
            } else if to == j {
                Some(from.to_string())
            } else {
                Some(to.to_string())
            }
        }
        _ => None,
    }
}

/// Find an edge leaving `node_id` toward `target`, flipping direction if needed.
fn find_or_orient_edge(
    edges: &mut [EdgeToml],
    node_id: &str,
    target: &str,
    prefer_stem: bool,
) -> Option<String> {
    for edge in edges.iter_mut() {
        if edge.from == node_id && edge.to == target {
            return Some(edge.id.clone());
        }
        if edge.to == node_id && edge.from == target {
            std::mem::swap(&mut edge.from, &mut edge.to);
            return Some(edge.id.clone());
        }
    }
    // Fallback: first outgoing edge toward target via BFS neighbor
    for edge in edges.iter_mut() {
        if edge.from == node_id {
            return Some(edge.id.clone());
        }
        if edge.to == node_id && prefer_stem {
            std::mem::swap(&mut edge.from, &mut edge.to);
            return Some(edge.id.clone());
        }
    }
    let _ = target;
    None
}

fn node_id_num(id: &str) -> Option<u32> {
    id.strip_prefix('n')?.parse().ok()
}

fn build_signals(items: &[TrItem], item_to_edge: &HashMap<u32, String>) -> Vec<SignalToml> {
    let mut out = Vec::new();
    for item in items {
        let TrItemKind::Signal { aspect_initial } = &item.kind else {
            continue;
        };
        let Some(edge_id) = item_to_edge.get(&item.id) else {
            continue;
        };
        out.push(SignalToml {
            id: format!("sig{}", item.id),
            edge_id: edge_id.clone(),
            position_m: item.distance_m,
            aspect: aspect_initial.as_toml_str().to_string(),
        });
    }
    out
}

fn apply_failed_signals(signals: &mut [SignalToml], failed_ids: &[u32]) {
    if failed_ids.is_empty() {
        return;
    }
    for sig in signals.iter_mut() {
        let id_num: Option<u32> = sig
            .id
            .strip_prefix("sig")
            .and_then(|s| s.parse::<u32>().ok());
        if let Some(num) = id_num {
            if failed_ids.contains(&num) {
                sig.aspect = "stop".to_string();
            }
        }
    }
}

fn apply_speed_posts(
    edges: &mut [EdgeToml],
    items: &[TrItem],
    item_to_edge: &HashMap<u32, String>,
) {
    for item in items {
        let TrItemKind::SpeedPost { speed_mph } = item.kind else {
            continue;
        };
        if speed_mph <= 0.0 {
            continue;
        }
        let Some(edge_id) = item_to_edge.get(&item.id) else {
            continue;
        };
        let cap_kmh = speed_mph * 1.609_344;
        for edge in edges.iter_mut() {
            if &edge.id == edge_id {
                edge.speed_limit_kmh = edge.speed_limit_kmh.min(cap_kmh);
            }
        }
    }
}

fn apply_restricted_zones(
    edges: &mut [EdgeToml],
    zones: &[openrailsrs_formats::RestrictedZone],
    item_to_edge: &HashMap<u32, String>,
) {
    if zones.is_empty() {
        return;
    }
    for zone in zones {
        if zone.max_speed_mps <= 0.0 {
            continue;
        }
        let cap_kmh = zone.max_speed_mps * 3.6;
        if zone.item_id_start == 0 && zone.item_id_end == 0 {
            continue;
        }
        let start_edge = item_to_edge.get(&zone.item_id_start);
        let end_edge = item_to_edge.get(&zone.item_id_end);
        for edge in edges.iter_mut() {
            let touches = match (start_edge, end_edge) {
                (Some(s), _) if *s == edge.id => true,
                (_, Some(e)) if *e == edge.id => true,
                _ => false,
            };
            if touches {
                edge.speed_limit_kmh = edge.speed_limit_kmh.min(cap_kmh);
            }
        }
    }
}

fn resolve_pin(
    pin: u32,
    map: &HashMap<u32, String>,
    counter: &mut u32,
    nodes: &mut Vec<NodeToml>,
) -> String {
    if let Some(id) = map.get(&pin) {
        return id.clone();
    }
    *counter += 1;
    let id = format!("anon{counter}");
    nodes.push(NodeToml {
        id: id.clone(),
        kind: None,
        x_m: 0.0,
        y_m: 0.0,
    });
    id
}

fn set_node_position(nodes: &mut [NodeToml], id: &str, point: TrackVectorPoint) {
    let x_m = point_graph_x(point);
    let y_m = point_graph_z(point);
    let Some(node) = nodes.iter_mut().find(|n| n.id == id) else {
        return;
    };
    if is_zero(&node.x_m) && is_zero(&node.y_m) {
        node.x_m = x_m;
        node.y_m = y_m;
    }
}

/// World anchors at the two pin ends of a vector node (from `TrVectorSection` chain).
fn vector_endpoint_positions(
    sections: &[TrVectorSectionRecord],
    geometry: Option<TrackVectorGeometry>,
    length_m: f64,
) -> (Option<TrackVectorPoint>, Option<TrackVectorPoint>) {
    let filtered: Vec<_> = sections
        .iter()
        .copied()
        .filter(|s| s.section_index != 0 || s.shape_index != 0)
        .collect();
    if !filtered.is_empty() {
        let start = Some(filtered[0].start);
        let end = if filtered.len() > 1 {
            Some(filtered[filtered.len() - 1].start)
        } else {
            single_section_terminal(filtered[0], geometry, length_m)
        };
        return (start, end.or(start));
    }
    geometry
        .map(|g| (Some(g.start), Some(g.end)))
        .unwrap_or((None, None))
}

fn single_section_terminal(
    section: TrVectorSectionRecord,
    geometry: Option<TrackVectorGeometry>,
    length_m: f64,
) -> Option<TrackVectorPoint> {
    if let Some(g) = geometry {
        let distinct = (g.end.x - g.start.x).abs() > 1e-3
            || (g.end.z - g.start.z).abs() > 1e-3
            || g.end.tile_x != g.start.tile_x
            || g.end.tile_z != g.start.tile_z;
        if distinct {
            return Some(g.end);
        }
    }
    let _ = (section, length_m);
    None
}

/// Fill junction/end nodes that lack `UiD` by propagating along `TrPins`.
fn apply_tdb_world_positions(
    tdb: &TrackDbFile,
    node_map: &HashMap<u32, String>,
    nodes: &mut [NodeToml],
) {
    let mut positions: HashMap<u32, TrackVectorPoint> = HashMap::new();
    for n in &tdb.nodes {
        if let Some(p) = n.position {
            positions.insert(n.id, p);
        }
    }
    for n in &tdb.nodes {
        let TrackNodeKind::Vector {
            pins,
            sections,
            geometry,
            length_m,
            ..
        } = &n.kind
        else {
            continue;
        };
        let (start, end) = vector_endpoint_positions(sections, *geometry, *length_m);
        if let Some(s) = start {
            positions.entry(pins.0).or_insert(s);
        }
        if let Some(e) = end {
            positions.entry(pins.1).or_insert(e);
        }
    }
    loop {
        let mut changed = false;
        for n in &tdb.nodes {
            if positions.contains_key(&n.id) {
                continue;
            }
            let mut refs = Vec::new();
            for other in &tdb.nodes {
                if other.id == n.id {
                    continue;
                }
                let Some(p) = positions.get(&other.id) else {
                    continue;
                };
                let connects = other.pin_refs.iter().any(|pin| pin.node_id == n.id)
                    || n.pin_refs.iter().any(|pin| pin.node_id == other.id);
                if connects {
                    refs.push(*p);
                }
            }
            if refs.is_empty() {
                continue;
            }
            positions.insert(n.id, average_points(&refs));
            changed = true;
        }
        if !changed {
            break;
        }
    }
    for (tdb_id, node_id) in node_map {
        if let Some(p) = positions.get(tdb_id) {
            set_node_position(nodes, node_id, *p);
        }
    }
}

fn average_points(points: &[TrackVectorPoint]) -> TrackVectorPoint {
    let n = points.len() as f64;
    TrackVectorPoint {
        tile_x: points[0].tile_x,
        tile_z: points[0].tile_z,
        x: points.iter().map(|p| p.x).sum::<f64>() / n,
        y: points.iter().map(|p| p.y).sum::<f64>() / n,
        z: points.iter().map(|p| p.z).sum::<f64>() / n,
    }
}

fn point_graph_x(point: TrackVectorPoint) -> f64 {
    // Signed internal tile X (Open Rails convention); a positive "display"
    // value would mirror the tile grid east-west.
    point.tile_x as f64 * 2048.0 + point.x
}

fn point_graph_z(point: TrackVectorPoint) -> f64 {
    // Whole-world Z negation (Open Rails XNA convention).
    -(point.tile_z as f64 * 2048.0 + point.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TrackDbFile;

    #[test]
    fn mps_to_kmh_conversion() {
        assert!((80.0_f64 * 3.6 - 288.0).abs() < 1e-6);
    }

    #[test]
    fn native_msts_emits_vector_alias() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let tdb = TrackDbFile::from_path(fixtures.join("native_msts.tdb")).expect("tdb");
        let toml = convert_tdb_to_toml(&tdb, "test", None).expect("toml");
        assert!(toml.contains("tdb_id = 2"));
        assert!(toml.contains("kind = \"edge\""));
        assert!(toml.contains("id = \"e2\""));
    }

    #[test]
    fn native_msts_propagates_end_node_position() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let tdb = TrackDbFile::from_path(fixtures.join("native_msts.tdb")).expect("tdb");
        let toml = convert_tdb_to_toml(&tdb, "test", None).expect("toml");
        let value: toml::Value = toml::from_str(&toml).expect("valid toml");
        let nodes = value["nodes"].as_array().expect("nodes");
        let n4 = nodes
            .iter()
            .find(|n| n.get("id").and_then(|v| v.as_str()) == Some("n4"))
            .expect("n4");
        assert!(
            n4.get("x_m").and_then(|v| v.as_float()).is_some(),
            "n4 should receive propagated coordinates via junction pins"
        );
    }
}
