use std::path::{Path, PathBuf};

use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_track::{
    Edge, Node, NodeKind, SignalAspect, SwitchPosition, TrackGraph, TrackSignal,
};
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
        /// Initial runtime position of the switch.  Defaults to `straight` when omitted.
        #[serde(default)]
        default_position: SwitchPositionDef,
    },
}

/// TOML representation of a switch's initial position.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwitchPositionDef {
    #[default]
    Straight,
    Diverging,
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
    #[serde(default)]
    pub script: Option<SignalScriptDef>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SignalScriptDef {
    #[serde(default)]
    pub on_block_ahead: Option<SignalAspectDef>,
    #[serde(default)]
    pub on_second_block_ahead: Option<SignalAspectDef>,
    #[serde(default)]
    pub default: Option<SignalAspectDef>,
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
        let switch_pos: Option<SwitchPosition> = match &n.kind {
            NodeKindDef::Switch {
                default_position, ..
            } => Some(match default_position {
                SwitchPositionDef::Straight => SwitchPosition::Straight,
                SwitchPositionDef::Diverging => SwitchPosition::Diverging,
            }),
            _ => None,
        };
        let kind = match n.kind {
            NodeKindDef::Plain => NodeKind::Plain,
            NodeKindDef::Station { name } => NodeKind::Station { name },
            NodeKindDef::Switch {
                diverging_edge,
                stem_edge,
                ..
            } => NodeKind::Switch {
                diverging_edge: EdgeId(diverging_edge),
                stem_edge: EdgeId(stem_edge),
            },
        };
        g.insert_node(Node {
            id: NodeId(n.id.clone()),
            kind,
            x_m: n.x_m,
            y_m: n.y_m,
        })?;
        if let Some(pos) = switch_pos {
            g.set_switch(&n.id, pos)?;
        }
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
            script: s.script.map(|sc| openrailsrs_track::SignalScript {
                on_block_ahead: sc.on_block_ahead.map(|a| match a {
                    SignalAspectDef::Clear => SignalAspect::Clear,
                    SignalAspectDef::Caution => SignalAspect::Caution,
                    SignalAspectDef::Stop => SignalAspect::Stop,
                }),
                on_second_block_ahead: sc.on_second_block_ahead.map(|a| match a {
                    SignalAspectDef::Clear => SignalAspect::Clear,
                    SignalAspectDef::Caution => SignalAspect::Caution,
                    SignalAspectDef::Stop => SignalAspect::Stop,
                }),
                default: sc.default.map(|a| match a {
                    SignalAspectDef::Clear => SignalAspect::Clear,
                    SignalAspectDef::Caution => SignalAspect::Caution,
                    SignalAspectDef::Stop => SignalAspect::Stop,
                }),
            }),
        })?;
    }
    Ok(g)
}
