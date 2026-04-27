use std::path::{Path, PathBuf};

use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_track::{Edge, Node, NodeKind, SignalAspect, TrackGraph, TrackSignal};
use serde::Deserialize;

use crate::RouteError;

/// Declarative route layout for tests and minimal routes (`track.toml`).
#[derive(Debug, Deserialize)]
pub struct RouteLayoutFile {
    pub route: RouteMeta,
    #[serde(default)]
    pub nodes: Vec<NodeDef>,
    #[serde(default)]
    pub edges: Vec<EdgeDef>,
    #[serde(default)]
    pub signals: Vec<SignalDef>,
}

#[derive(Debug, Deserialize)]
pub struct RouteMeta {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct NodeDef {
    pub id: String,
    #[serde(default)]
    pub kind: NodeKindDef,
    #[serde(default)]
    pub x_m: f64,
    #[serde(default)]
    pub y_m: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKindDef {
    #[default]
    Plain,
    Station {
        name: String,
    },
    Switch {
        diverging_edge: String,
        stem_edge: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct EdgeDef {
    pub id: String,
    pub from: String,
    pub to: String,
    pub length_m: f64,
    #[serde(default = "default_speed")]
    pub speed_limit_kmh: f64,
    #[serde(default)]
    pub grade_percent: f64,
}

fn default_speed() -> f64 {
    80.0
}

#[derive(Debug, Deserialize)]
pub struct SignalDef {
    pub id: String,
    pub edge_id: String,
    #[serde(default)]
    pub position_m: f64,
    #[serde(default)]
    pub aspect: SignalAspectDef,
    #[serde(default)]
    pub clear_after_s: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalAspectDef {
    #[default]
    Clear,
    Caution,
    Stop,
}

/// Load `track.toml` from a route directory (folder containing `track.toml`).
pub fn load_track_graph_from_route_dir(dir: impl AsRef<Path>) -> Result<TrackGraph, RouteError> {
    let dir = dir.as_ref();
    let layout_path: PathBuf = dir.join("track.toml");
    let text = std::fs::read_to_string(&layout_path).map_err(|e| RouteError::Io {
        path: layout_path.display().to_string(),
        source: e,
    })?;
    let layout: RouteLayoutFile = toml::from_str(&text)?;
    layout_to_graph(layout)
}

fn layout_to_graph(layout: RouteLayoutFile) -> Result<TrackGraph, RouteError> {
    let mut g = TrackGraph::new();
    for n in layout.nodes {
        let kind = match n.kind {
            NodeKindDef::Plain => NodeKind::Plain,
            NodeKindDef::Station { name } => NodeKind::Station { name },
            NodeKindDef::Switch {
                diverging_edge,
                stem_edge,
            } => NodeKind::Switch {
                diverging_edge: EdgeId(diverging_edge),
                stem_edge: EdgeId(stem_edge),
            },
        };
        g.insert_node(Node {
            id: NodeId(n.id),
            kind,
            x_m: n.x_m,
            y_m: n.y_m,
        })?;
    }
    for e in layout.edges {
        let lim_mps = e.speed_limit_kmh / 3.6;
        g.insert_edge(Edge {
            id: EdgeId(e.id),
            from: NodeId(e.from),
            to: NodeId(e.to),
            length_m: e.length_m,
            speed_limit_mps: lim_mps,
            grade_percent: e.grade_percent,
        })?;
    }
    for s in layout.signals {
        let aspect = match s.aspect {
            SignalAspectDef::Clear => SignalAspect::Clear,
            SignalAspectDef::Caution => SignalAspect::Caution,
            SignalAspectDef::Stop => SignalAspect::Stop,
        };
        g.insert_signal(TrackSignal {
            id: s.id,
            edge_id: s.edge_id,
            position_m: s.position_m,
            aspect,
            clear_after_s: s.clear_after_s,
        })?;
    }
    Ok(g)
}
