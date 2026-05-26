//! Switch-aware edge pathfinding on a [`TrackGraph`].

use std::collections::{HashMap, VecDeque};

use openrailsrs_track::{NodeKind, SwitchPosition, TrackGraph};

use crate::RouteError;

/// Outgoing edge ids traversable from a node, respecting switch position.
pub fn allowed_outgoing_edges(graph: &TrackGraph, node: &str) -> Vec<String> {
    let all = graph.outgoing_edges(node).to_vec();
    match graph.node(node).map(|n| &n.kind) {
        Some(NodeKind::Switch {
            stem_edge,
            diverging_edge,
        }) => {
            let pos = graph
                .switch_position(node)
                .unwrap_or(SwitchPosition::Straight);
            let chosen = match pos {
                SwitchPosition::Straight => stem_edge.0.as_str(),
                SwitchPosition::Diverging => diverging_edge.0.as_str(),
            };
            let filtered: Vec<String> = all
                .iter()
                .filter(|e| e.as_str() == chosen)
                .cloned()
                .collect();
            if filtered.is_empty() { all } else { filtered }
        }
        _ => all,
    }
}

/// Ordered edge IDs from `start` to `destination` (BFS, switch-aware).
pub fn edge_path(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, RouteError> {
    if start == destination {
        return Ok(Vec::new());
    }
    let mut q = VecDeque::new();
    let mut parent: HashMap<String, (String, String)> = HashMap::new();
    q.push_back(start.to_string());
    parent.insert(start.to_string(), (String::new(), String::new()));

    while let Some(node) = q.pop_front() {
        for eid in allowed_outgoing_edges(graph, &node) {
            let edge = graph
                .edge(&eid)
                .ok_or_else(|| RouteError::Msg(format!("missing edge {eid}")))?;
            let next = edge.to.0.clone();
            if parent.contains_key(&next) {
                continue;
            }
            parent.insert(next.clone(), (node.clone(), eid.clone()));
            if next == destination {
                let mut out = Vec::new();
                let mut cur = destination.to_string();
                while cur != start {
                    let (prev, e) = parent.get(&cur).unwrap();
                    out.push(e.clone());
                    cur = prev.clone();
                }
                out.reverse();
                return Ok(out);
            }
            q.push_back(next);
        }
    }
    Err(RouteError::Msg(format!(
        "no path from {start} to {destination}"
    )))
}
