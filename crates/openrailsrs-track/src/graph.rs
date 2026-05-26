use std::collections::HashMap;

use indexmap::IndexMap;
use openrailsrs_core::{EdgeId, NodeId};
use serde::{Deserialize, Serialize};

use crate::TrackError;
use crate::signal::TrackSignal;

/// Runtime position of a switch node.
///
/// - `Straight` routes the train through the `stem_edge`.
/// - `Diverging` routes the train through the `diverging_edge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SwitchPosition {
    #[default]
    Straight,
    Diverging,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeKind {
    Plain,
    Switch {
        /// Branch taken when switch is in "diverging" position (logical label).
        diverging_edge: EdgeId,
        stem_edge: EdgeId,
    },
    Station {
        name: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Optional planar coordinates for export / debug.
    #[serde(default)]
    pub x_m: f64,
    #[serde(default)]
    pub y_m: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub length_m: f64,
    pub speed_limit_mps: f64,
    #[serde(default)]
    pub grade_percent: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TrackGraph {
    nodes: IndexMap<String, Node>,
    edges: IndexMap<String, Edge>,
    /// Keyed by signal id for O(1) lookup.
    signals: IndexMap<String, TrackSignal>,
    /// edge_id → list of signal ids on that edge (ordered by position_m).
    signals_by_edge: IndexMap<String, Vec<String>>,
    outgoing: IndexMap<String, Vec<String>>,
    /// Runtime switch positions for `NodeKind::Switch` nodes.
    switch_positions: HashMap<String, SwitchPosition>,
}

impl TrackGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_node(&mut self, node: Node) -> Result<(), TrackError> {
        let key = node.id.0.clone();
        if self.nodes.contains_key(&key) {
            return Err(TrackError::DuplicateId(key));
        }
        self.nodes.insert(key.clone(), node);
        Ok(())
    }

    pub fn insert_edge(&mut self, edge: Edge) -> Result<(), TrackError> {
        let from = edge.from.0.clone();
        let to = edge.to.0.clone();
        if !self.nodes.contains_key(&from) {
            return Err(TrackError::UnknownNode(from));
        }
        if !self.nodes.contains_key(&to) {
            return Err(TrackError::UnknownNode(to));
        }
        let eid = edge.id.0.clone();
        if self.edges.contains_key(&eid) {
            return Err(TrackError::DuplicateId(eid));
        }
        self.outgoing.entry(from).or_default().push(eid.clone());
        self.edges.insert(eid, edge);
        Ok(())
    }

    pub fn outgoing_edges(&self, node: &str) -> &[String] {
        self.outgoing.get(node).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn edge(&self, id: &str) -> Option<&Edge> {
        self.edges.get(id)
    }

    /// Lower the speed limit on an edge (km/h). No-op if the edge is unknown.
    pub fn cap_edge_speed_limit_kmh(&mut self, id: &str, speed_limit_kmh: f64) {
        if let Some(edge) = self.edges.get_mut(id) {
            let cap_mps = speed_limit_kmh / 3.6;
            edge.speed_limit_mps = edge.speed_limit_mps.min(cap_mps);
        }
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn nodes_iter(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.nodes.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn edges_iter(&self) -> impl Iterator<Item = (&str, &Edge)> {
        self.edges.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Insert a signal into the graph.
    /// Returns an error if the signal id is duplicate or the edge does not exist.
    pub fn insert_signal(&mut self, signal: TrackSignal) -> Result<(), TrackError> {
        if !self.edges.contains_key(&signal.edge_id) {
            return Err(TrackError::UnknownEdgeForSignal(signal.edge_id.clone()));
        }
        if self.signals.contains_key(&signal.id) {
            return Err(TrackError::DuplicateSignalId(signal.id.clone()));
        }
        self.signals_by_edge
            .entry(signal.edge_id.clone())
            .or_default()
            .push(signal.id.clone());
        self.signals.insert(signal.id.clone(), signal);
        Ok(())
    }

    /// All signals on a specific edge, in insertion order.
    pub fn signals_on_edge(&self, edge_id: &str) -> impl Iterator<Item = &TrackSignal> {
        self.signals_by_edge
            .get(edge_id)
            .into_iter()
            .flat_map(|ids| ids.iter().filter_map(|id| self.signals.get(id)))
    }

    /// All signals in the graph.
    pub fn signals(&self) -> impl Iterator<Item = &TrackSignal> {
        self.signals.values()
    }

    /// Look up a signal by id.
    pub fn signal(&self, id: &str) -> Option<&TrackSignal> {
        self.signals.get(id)
    }

    /// Set the runtime position of a switch node.
    ///
    /// Returns `Err(TrackError::NotASwitch)` if the node exists but is not a `NodeKind::Switch`,
    /// or `Err(TrackError::UnknownNode)` if the node does not exist at all.
    pub fn set_switch(&mut self, node: &str, pos: SwitchPosition) -> Result<(), TrackError> {
        match self.nodes.get(node) {
            None => Err(TrackError::UnknownNode(node.to_string())),
            Some(n) => match &n.kind {
                NodeKind::Switch { .. } => {
                    self.switch_positions.insert(node.to_string(), pos);
                    Ok(())
                }
                _ => Err(TrackError::NotASwitch(node.to_string())),
            },
        }
    }

    /// Current position of a switch node, or `None` if the node is not a switch.
    pub fn switch_position(&self, node: &str) -> Option<SwitchPosition> {
        self.switch_positions.get(node).copied()
    }

    /// Evaluate all scripted signals and update their aspects based on block occupancy.
    ///
    /// `block_map` maps edge IDs to the ID of the train currently occupying that edge.
    /// Only signals that have a [`crate::signal::SignalScript`] are updated; static signals
    /// are left unchanged.
    ///
    /// Algorithm for each scripted signal:
    /// 1. Find the edge the signal sits on (`signal.edge_id`).
    /// 2. Identify the *destination* node of that edge → call it `n1`.
    /// 3. First block ahead = any outgoing edge from `n1`.
    /// 4. Second block ahead = any outgoing edge from the destination node of each
    ///    first-block edge.
    /// 5. Apply the highest-priority rule whose condition is met.
    pub fn evaluate_signals(&mut self, block_map: &HashMap<String, String>) {
        use crate::signal::SignalAspect;

        // Collect signal updates separately to avoid borrowing `self` mutably while
        // iterating over it.
        let updates: Vec<(String, SignalAspect)> = self
            .signals
            .values()
            .filter_map(|sig| {
                let script = sig.script.as_ref()?;

                // Destination node of the signal's edge.
                let edge = self.edges.get(&sig.edge_id)?;
                let dest_node_str = edge.to.0.clone();

                // First-block edges: outgoing from dest_node.
                let first_edges = self.outgoing_edges(&dest_node_str).to_vec();
                let block1_occupied = first_edges.iter().any(|e| block_map.contains_key(e));

                if block1_occupied {
                    if let Some(aspect) = script.on_block_ahead {
                        return Some((sig.id.clone(), aspect));
                    }
                }

                // Second-block edges: outgoing from each first-edge destination.
                let block2_occupied = first_edges.iter().any(|e| {
                    let dest2 = self.edges.get(e).map(|ed| ed.to.0.clone());
                    dest2.is_some_and(|d| {
                        self.outgoing_edges(&d)
                            .iter()
                            .any(|e2| block_map.contains_key(e2))
                    })
                });

                if block2_occupied {
                    if let Some(aspect) = script.on_second_block_ahead {
                        return Some((sig.id.clone(), aspect));
                    }
                }

                // Default.
                script.default.map(|aspect| (sig.id.clone(), aspect))
            })
            .collect();

        for (id, aspect) in updates {
            if let Some(sig) = self.signals.get_mut(&id) {
                sig.aspect = aspect;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_node_graph() -> TrackGraph {
        let mut g = TrackGraph::new();
        g.insert_node(Node {
            id: NodeId("a".into()),
            kind: NodeKind::Plain,
            x_m: 0.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_node(Node {
            id: NodeId("b".into()),
            kind: NodeKind::Switch {
                stem_edge: EdgeId("e1".into()),
                diverging_edge: EdgeId("e2".into()),
            },
            x_m: 100.0,
            y_m: 0.0,
        })
        .unwrap();
        g
    }

    #[test]
    fn set_switch_on_plain_node_returns_not_a_switch() {
        let mut g = two_node_graph();
        let err = g.set_switch("a", SwitchPosition::Straight).unwrap_err();
        assert!(
            matches!(err, TrackError::NotASwitch(ref id) if id == "a"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn set_switch_on_unknown_node_returns_unknown_node() {
        let mut g = two_node_graph();
        let err = g
            .set_switch("missing", SwitchPosition::Diverging)
            .unwrap_err();
        assert!(matches!(err, TrackError::UnknownNode(_)));
    }

    #[test]
    fn set_switch_and_read_back() {
        let mut g = two_node_graph();
        assert_eq!(g.switch_position("b"), None, "no position set yet");
        g.set_switch("b", SwitchPosition::Diverging).unwrap();
        assert_eq!(g.switch_position("b"), Some(SwitchPosition::Diverging));
        g.set_switch("b", SwitchPosition::Straight).unwrap();
        assert_eq!(g.switch_position("b"), Some(SwitchPosition::Straight));
    }

    #[test]
    fn switch_position_for_plain_node_returns_none() {
        let g = two_node_graph();
        assert_eq!(g.switch_position("a"), None);
    }
}
