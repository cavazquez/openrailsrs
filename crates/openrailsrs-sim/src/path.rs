use std::collections::{HashMap, VecDeque};

use openrailsrs_track::{NodeKind, SwitchPosition, TrackGraph};

use crate::SimError;

/// Ordered edge IDs from `start` node to `destination` node (BFS over edges).
///
/// Switch nodes are respected: the BFS only expands the edge corresponding to the switch's
/// current runtime position (`stem_edge` for `Straight`, `diverging_edge` for `Diverging`).
/// Plain nodes expand all outgoing edges as before.
pub fn edge_path(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, SimError> {
    if start == destination {
        return Ok(Vec::new());
    }
    let mut q = VecDeque::new();
    let mut parent: HashMap<String, (String, String)> = HashMap::new();
    q.push_back(start.to_string());
    parent.insert(start.to_string(), (String::new(), String::new()));

    while let Some(node) = q.pop_front() {
        // Determine which edges are traversable from this node.
        let allowed: Vec<String> = {
            let all = graph.outgoing_edges(&node);
            match graph.node(&node).map(|n| &n.kind) {
                Some(NodeKind::Switch {
                    stem_edge,
                    diverging_edge,
                }) => {
                    let pos = graph
                        .switch_position(&node)
                        .unwrap_or(SwitchPosition::Straight);
                    let chosen = match pos {
                        SwitchPosition::Straight => stem_edge.0.as_str(),
                        SwitchPosition::Diverging => diverging_edge.0.as_str(),
                    };
                    all.iter()
                        .filter(|e| e.as_str() == chosen)
                        .cloned()
                        .collect()
                }
                _ => all.to_vec(),
            }
        };

        for eid in &allowed {
            let edge = graph
                .edge(eid)
                .ok_or_else(|| SimError::Msg(format!("missing edge {eid}")))?;
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
    Err(SimError::Msg(format!(
        "no path from {start} to {destination}"
    )))
}
