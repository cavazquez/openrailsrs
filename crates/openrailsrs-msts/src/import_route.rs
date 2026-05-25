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
    ActivityFile, MstsFile, TrItem, TrItemKind, TrackDbFile, TrackNodeKind, parse_msts_file,
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
    let tdb = TrackDbFile::from_path(&tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let toml = convert_tdb_to_toml(&tdb, &route_id, None)?;
    Ok(toml)
}

/// Same as `import_route` but applies activity-level overrides (failed signals
/// and restricted speed zones) to the generated `track.toml`.
pub fn import_route_with_activity(route_dir: &Path, act_path: &Path) -> Result<String, MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = TrackDbFile::from_path(&tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let activity = ActivityFile::from_path(act_path)?;
    let toml = convert_tdb_to_toml(&tdb, &route_id, Some(&activity))?;
    Ok(toml)
}

/// Same as `import_route` but also returns a count summary `(nodes, edges)`.
pub fn import_route_with_summary(route_dir: &Path) -> Result<(String, usize, usize), MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = TrackDbFile::from_path(&tdb_path)?;
    let route_id = find_route_id(route_dir, &tdb_path);
    let (nodes, edges) = count_nodes_edges(&tdb);
    let toml = convert_tdb_to_toml(&tdb, &route_id, None)?;
    Ok((toml, nodes, edges))
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn find_tdb(dir: &Path) -> Result<std::path::PathBuf, MstsError> {
    for entry in std::fs::read_dir(dir)? {
        let e = entry?;
        let p = e.path();
        if p.extension()
            .map(|x| x.eq_ignore_ascii_case("tdb"))
            .unwrap_or(false)
        {
            return Ok(p);
        }
    }
    Err(MstsError::msg(format!(
        "no *.tdb file found in {}",
        dir.display()
    )))
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
    let mut junction_pins: HashMap<u32, (u32, u32)> = HashMap::new();
    let mut nodes: Vec<NodeToml> = Vec::new();

    for n in &tdb.nodes {
        match &n.kind {
            TrackNodeKind::End => {
                let id = format!("n{}", n.id);
                node_map.insert(n.id, id.clone());
                nodes.push(NodeToml {
                    id,
                    kind: None,
                    x_m: 0.0,
                    y_m: 0.0,
                });
            }
            TrackNodeKind::Junction { pin1, pin2 } => {
                let id = format!("n{}", n.id);
                node_map.insert(n.id, id.clone());
                junction_pins.insert(n.id, (*pin1, *pin2));
                nodes.push(NodeToml {
                    id,
                    kind: None,
                    x_m: 0.0,
                    y_m: 0.0,
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
        } = &n.kind
        {
            let from_id = resolve_pin(pins.0, &node_map, &mut vec_counter, &mut nodes);
            let to_id = resolve_pin(pins.1, &node_map, &mut vec_counter, &mut nodes);
            let edge_id = format!("e{}", n.id);
            for item_id in item_ids {
                item_to_edge.insert(*item_id, edge_id.clone());
            }
            edges.push(EdgeToml {
                id: edge_id,
                from: from_id,
                to: to_id,
                length_m: *length_m,
                speed_limit_kmh: *speed_limit_mps * 3.6,
                grade_percent: 0.0,
            });
        }
    }

    configure_switch_nodes(&mut nodes, &edges, &junction_pins);

    let mut signals = build_signals(&tdb.items, &item_to_edge);

    if let Some(act) = activity {
        apply_failed_signals(&mut signals, &act.failed_signals);
        apply_restricted_zones(&mut edges, &act.restricted_zones, &item_to_edge);
    }

    let track = TrackToml {
        route: RouteMeta {
            id: route_id.to_string(),
        },
        nodes,
        edges,
        signals,
    };
    Ok(toml::to_string_pretty(&track)?)
}

/// Attach switch metadata to junction nodes once all edges exist.
fn configure_switch_nodes(
    nodes: &mut [NodeToml],
    edges: &[EdgeToml],
    junction_pins: &HashMap<u32, (u32, u32)>,
) {
    for node in nodes.iter_mut() {
        let Some(jid) = node_id_num(&node.id) else {
            continue;
        };
        let Some((pin1, pin2)) = junction_pins.get(&jid) else {
            continue;
        };

        let incident: Vec<&EdgeToml> = edges
            .iter()
            .filter(|e| e.from == node.id || e.to == node.id)
            .collect();

        if incident.len() < 2 {
            continue;
        }

        let pin1_node = format!("n{pin1}");
        let pin2_node = format!("n{pin2}");

        let diverging = incident
            .iter()
            .find(|e| edge_other_end(e, &node.id) == pin1_node)
            .or_else(|| {
                incident
                    .iter()
                    .find(|e| edge_other_end(e, &node.id) == pin2_node)
            })
            .or_else(|| {
                incident
                    .iter()
                    .min_by(|a, b| a.length_m.partial_cmp(&b.length_m).unwrap())
            });

        let Some(diverging) = diverging else {
            continue;
        };

        let stem = incident
            .iter()
            .find(|e| e.id != diverging.id)
            .copied()
            .unwrap_or(*diverging);

        node.kind = Some(SwitchKindTable {
            switch: SwitchEdges {
                stem_edge: stem.id.clone(),
                diverging_edge: diverging.id.clone(),
                default_position: default_switch_position(),
            },
        });
    }
}

fn node_id_num(id: &str) -> Option<u32> {
    id.strip_prefix('n')?.parse().ok()
}

fn edge_other_end(edge: &EdgeToml, node_id: &str) -> String {
    if edge.from == node_id {
        edge.to.clone()
    } else {
        edge.from.clone()
    }
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

fn apply_restricted_zones(
    edges: &mut [EdgeToml],
    zones: &[openrailsrs_formats::RestrictedZone],
    item_to_edge: &HashMap<u32, String>,
) {
    if zones.is_empty() {
        return;
    }
    for zone in zones {
        let start_edge = item_to_edge.get(&zone.item_id_start);
        let end_edge = item_to_edge.get(&zone.item_id_end);
        let cap_kmh = zone.max_speed_mps * 3.6;
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

#[cfg(test)]
mod tests {
    #[test]
    fn mps_to_kmh_conversion() {
        assert!((80.0_f64 * 3.6 - 288.0).abs() < 1e-6);
    }
}
