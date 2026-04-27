use indexmap::IndexMap;
use openrailsrs_core::{EdgeId, NodeId};
use serde::{Deserialize, Serialize};

use crate::TrackError;
use crate::signal::TrackSignal;

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
    signals: Vec<TrackSignal>,
    outgoing: IndexMap<String, Vec<String>>,
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

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn nodes_iter(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.nodes.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn edges_iter(&self) -> impl Iterator<Item = (&str, &Edge)> {
        self.edges.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn signals(&self) -> &[TrackSignal] {
        &self.signals
    }
}
