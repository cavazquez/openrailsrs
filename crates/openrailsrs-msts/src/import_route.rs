//! Convert an MSTS Track Database (`.tdb`) into an `openrailsrs` `track.toml`.
//!
//! The mapping is:
//! - `TrEndNode`      → plain node (`kind = "plain"`)
//! - `TrJunctionNode` → switch node (`kind = "switch"`)
//! - `TrVectorNode`   → two implicit endpoint nodes + one directed edge
//!
//! The generated TOML uses the same schema as the hand-authored `track.toml`
//! files found under `examples/`, so it can be loaded without modification by
//! `openrailsrs-route`.

use std::collections::HashMap;
use std::path::Path;

use openrailsrs_formats::{ActivityFile, TrItem, TrItemKind, TrackDbFile, TrackNodeKind};
use serde::Serialize;

use crate::error::MstsError;

// ── TOML schema mirrors ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct TrackToml {
    nodes: Vec<NodeToml>,
    edges: Vec<EdgeToml>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    signals: Vec<SignalToml>,
}

#[derive(Serialize)]
struct NodeToml {
    id: String,
    kind: String,
}

#[derive(Serialize)]
struct EdgeToml {
    id: String,
    from: String,
    to: String,
    length_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed_limit_mps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    grade_percent: Option<f64>,
}

#[derive(Serialize)]
struct SignalToml {
    id: String,
    edge_id: String,
    position_m: f64,
    aspect: String,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Read the first `*.tdb` file found in `route_dir` and convert it to a
/// `track.toml` TOML string.
///
/// Returns `Ok(toml_string)` on success.
pub fn import_route(route_dir: &Path) -> Result<String, MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = TrackDbFile::from_path(&tdb_path)?;
    let toml = convert_tdb_to_toml(&tdb, None)?;
    Ok(toml)
}

/// Same as `import_route` but applies activity-level overrides (failed signals
/// and restricted speed zones) to the generated `track.toml`.
pub fn import_route_with_activity(route_dir: &Path, act_path: &Path) -> Result<String, MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = TrackDbFile::from_path(&tdb_path)?;
    let activity = ActivityFile::from_path(act_path)?;
    let toml = convert_tdb_to_toml(&tdb, Some(&activity))?;
    Ok(toml)
}

/// Same as `import_route` but also returns a count summary `(nodes, edges)`.
pub fn import_route_with_summary(route_dir: &Path) -> Result<(String, usize, usize), MstsError> {
    let tdb_path = find_tdb(route_dir)?;
    let tdb = TrackDbFile::from_path(&tdb_path)?;
    let (nodes, edges) = count_nodes_edges(&tdb);
    let toml = convert_tdb_to_toml(&tdb, None)?;
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
    activity: Option<&ActivityFile>,
) -> Result<String, MstsError> {
    // Track which node IDs from TrEndNode / TrJunctionNode we've already emitted.
    let mut node_map: HashMap<u32, String> = HashMap::new();
    let mut nodes: Vec<NodeToml> = Vec::new();
    let mut edges: Vec<EdgeToml> = Vec::new();

    // First pass: collect named (End/Junction) nodes.
    for n in &tdb.nodes {
        let (kind_str, include) = match &n.kind {
            TrackNodeKind::End => ("plain", true),
            TrackNodeKind::Junction { .. } => ("switch", true),
            TrackNodeKind::Vector { .. } => ("plain", false),
        };
        if include {
            let id = format!("n{}", n.id);
            node_map.insert(n.id, id.clone());
            nodes.push(NodeToml {
                id,
                kind: kind_str.to_string(),
            });
        }
    }

    // Second pass: generate edges from vector nodes and remember which `TrItemId`
    // belongs to each edge so signals can be projected onto them later.
    let mut vec_counter = 0u32;
    let mut item_to_edge: HashMap<u32, String> = HashMap::new();
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
                speed_limit_mps: Some(*speed_limit_mps),
                grade_percent: None,
            });
        }
    }

    let mut signals = build_signals(&tdb.items, &item_to_edge);

    if let Some(act) = activity {
        apply_failed_signals(&mut signals, &act.failed_signals);
        apply_restricted_zones(&mut edges, &act.restricted_zones, &item_to_edge);
    }

    let track = TrackToml {
        nodes,
        edges,
        signals,
    };
    Ok(toml::to_string_pretty(&track)?)
}

/// Convert every `SignalItem` into a `[[signals]]` TOML entry. Items whose parent
/// vector node was never emitted as an edge (e.g. orphan refs) are silently skipped.
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

/// Force `aspect = "stop"` on every signal whose underlying `TrItemId` appears
/// in the activity's `FailedSignals` list.
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

/// Reduce the `speed_limit_mps` of every edge that contains a restricted-zone
/// endpoint. The restriction is conservative — applied to the whole edge — so
/// users can refine it manually after import.
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
        for edge in edges.iter_mut() {
            let touches = match (start_edge, end_edge) {
                (Some(s), _) if *s == edge.id => true,
                (_, Some(e)) if *e == edge.id => true,
                _ => false,
            };
            if !touches {
                continue;
            }
            edge.speed_limit_mps = Some(match edge.speed_limit_mps {
                Some(curr) => curr.min(zone.max_speed_mps),
                None => zone.max_speed_mps,
            });
        }
    }
}

/// Look up the node ID in the map; if pin is 0 or unknown, create an anonymous
/// plain node so the edge can still reference something.
fn resolve_pin(
    pin: u32,
    map: &HashMap<u32, String>,
    counter: &mut u32,
    nodes: &mut Vec<NodeToml>,
) -> String {
    if let Some(id) = map.get(&pin) {
        return id.clone();
    }
    // Create an anonymous node for unresolved pins.
    *counter += 1;
    let id = format!("anon{}", counter);
    nodes.push(NodeToml {
        id: id.clone(),
        kind: "plain".to_string(),
    });
    id
}
