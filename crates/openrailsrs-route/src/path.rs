//! Switch-aware edge pathfinding on a [`TrackGraph`].

use std::collections::{HashMap, VecDeque};

use openrailsrs_track::{NodeKind, SwitchPosition, TrackGraph};

use crate::RouteError;

/// Outgoing edge ids traversable from a node, respecting switch position.
///
/// At a 3-pin junction the blades select between `stem_edge` and `diverging_edge`;
/// any other outgoing edge is the trailing leg and stays open so pathfinding can
/// leave toward the third pin (needed for MSTS PAT corridors that reverse a vector).
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
            let stem = stem_edge.0.as_str();
            let div = diverging_edge.0.as_str();
            let filtered: Vec<String> = all
                .iter()
                .filter(|e| {
                    let id = e.as_str();
                    id == chosen || (id != stem && id != div)
                })
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
    edge_path_with(graph, start, destination, true)
}

/// Like [`edge_path`] but traverses every outgoing edge, ignoring switch position.
///
/// Used when deriving a player `.pat` corridor: the path file defines the route;
/// switch positions are computed afterwards to match that corridor.
pub fn edge_path_ignoring_switches(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, RouteError> {
    edge_path_with(graph, start, destination, false)
}

fn edge_path_with(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
    switch_aware: bool,
) -> Result<Vec<String>, RouteError> {
    if start == destination {
        return Ok(Vec::new());
    }
    let mut q = VecDeque::new();
    let mut parent: HashMap<String, (String, String)> = HashMap::new();
    q.push_back(start.to_string());
    parent.insert(start.to_string(), (String::new(), String::new()));

    while let Some(node) = q.pop_front() {
        let outgoing = if switch_aware {
            allowed_outgoing_edges(graph, &node)
        } else {
            graph.outgoing_edges(&node).to_vec()
        };
        for eid in outgoing {
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

/// Direct outgoing edge from `from` to `to`, if any.
pub fn direct_edge(graph: &TrackGraph, from: &str, to: &str) -> Option<String> {
    graph
        .outgoing_edges(from)
        .iter()
        .find(|eid| graph.edge(eid).is_some_and(|edge| edge.to.0.as_str() == to))
        .cloned()
}

/// Chain edges through an ordered list of graph node ids (PAT waypoints).
///
/// Uses a direct edge when `waypoints[i]` → `waypoints[i+1]` exists; otherwise
/// switch-aware BFS for that hop only.
pub fn edge_path_via_waypoints(
    graph: &TrackGraph,
    waypoints: &[String],
) -> Result<Vec<String>, RouteError> {
    if waypoints.len() < 2 {
        return Err(RouteError::Msg(
            "need at least 2 waypoints for PAT path".into(),
        ));
    }
    let mut out = Vec::new();
    for pair in waypoints.windows(2) {
        let a = pair[0].as_str();
        let b = pair[1].as_str();
        if a == b {
            continue;
        }
        if let Some(eid) = direct_edge(graph, a, b) {
            out.push(eid);
        } else {
            let hop = edge_path(graph, a, b)?;
            out.extend(hop);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, SwitchPosition, TrackGraph};

    fn fork_graph(switch_pos: SwitchPosition) -> TrackGraph {
        let mut g = TrackGraph::new();
        for (id, x) in [
            ("start", 0.0),
            ("junction", 1000.0),
            ("dest_a", 2000.0),
            ("dest_b", 2000.0),
        ] {
            let kind = if id == "junction" {
                NodeKind::Switch {
                    stem_edge: EdgeId("e2".into()),
                    diverging_edge: EdgeId("e3".into()),
                }
            } else {
                NodeKind::Plain
            };
            g.insert_node(Node {
                id: NodeId(id.into()),
                kind,
                x_m: x,
                y_m: 0.0,
            })
            .unwrap();
        }
        for (id, from, to) in [
            ("e1", "start", "junction"),
            ("e2", "junction", "dest_a"),
            ("e3", "junction", "dest_b"),
        ] {
            g.insert_edge(Edge {
                id: EdgeId(id.into()),
                from: NodeId(from.into()),
                to: NodeId(to.into()),
                length_m: 1000.0,
                speed_limit_mps: 20.0,
                grade_percent: 0.0,
            })
            .unwrap();
        }
        g.set_switch("junction", switch_pos).unwrap();
        g
    }

    #[test]
    fn via_waypoints_matches_bfs_on_fork() {
        let g = fork_graph(SwitchPosition::Diverging);
        let wps = [
            "start".to_string(),
            "junction".to_string(),
            "dest_b".to_string(),
        ];
        let via = edge_path_via_waypoints(&g, &wps).expect("via waypoints");
        let bfs = edge_path(&g, "start", "dest_b").expect("bfs");
        assert_eq!(via, bfs);
        assert_eq!(via, vec!["e1", "e3"]);
    }

    /// 3-pin junction: stem/div blades + trailing reverse leg (MSTS PAT outbound).
    fn trailing_junction_graph(switch_pos: SwitchPosition) -> TrackGraph {
        let mut g = TrackGraph::new();
        for (id, x) in [
            ("approach", 0.0),
            ("junction", 1000.0),
            ("stem_end", 2000.0),
            ("div_end", 2000.0),
        ] {
            let kind = if id == "junction" {
                NodeKind::Switch {
                    stem_edge: EdgeId("e_stem".into()),
                    diverging_edge: EdgeId("e_div".into()),
                }
            } else {
                NodeKind::Plain
            };
            g.insert_node(Node {
                id: NodeId(id.into()),
                kind,
                x_m: x,
                y_m: 0.0,
            })
            .unwrap();
        }
        for (id, from, to) in [
            ("e_app", "approach", "junction"),
            ("e_stem", "junction", "stem_end"),
            ("e_div", "junction", "div_end"),
            // Trailing reverse: leave junction back toward a PAT continuation.
            ("e_trail_r", "junction", "approach"),
        ] {
            g.insert_edge(Edge {
                id: EdgeId(id.into()),
                from: NodeId(from.into()),
                to: NodeId(to.into()),
                length_m: 1000.0,
                speed_limit_mps: 20.0,
                grade_percent: 0.0,
            })
            .unwrap();
        }
        g.set_switch("junction", switch_pos).unwrap();
        g
    }

    #[test]
    fn switch_keeps_trailing_leg_open() {
        let g = trailing_junction_graph(SwitchPosition::Straight);
        let out = allowed_outgoing_edges(&g, "junction");
        assert!(out.contains(&"e_stem".to_string()));
        assert!(out.contains(&"e_trail_r".to_string()));
        assert!(!out.contains(&"e_div".to_string()));
    }

    #[test]
    fn reverse_edge_ids_do_not_parse_as_plain_e_prefix() {
        // Document contract for MSTS import: `e{N}_r` must not parse as TDB id N.
        assert!("17466_r".parse::<u32>().is_err());
    }
}
