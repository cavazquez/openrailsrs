//! Import railway topology from an Overpass API JSON response.
//!
//! # Overpass query template
//!
//! ```text
//! [out:json];
//! (
//!   way[railway=rail]({{bbox}});
//!   node(w);
//! );
//! out body;
//! ```
//!
//! Download the result from <https://overpass-turbo.eu/> and pass the file to
//! [`import_osm_file`] or [`import_osm_str`].

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ImportError;
use crate::geo::{equirectangular_m, haversine_m};

// ── Overpass JSON input types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct OverpassResponse {
    pub elements: Vec<OsmElement>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum OsmElement {
    Node(OsmNode),
    Way(OsmWay),
    /// Relations are ignored.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OsmNode {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OsmWay {
    pub id: i64,
    /// Ordered list of node ids forming the way's geometry.
    pub nodes: Vec<i64>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ── track.toml output types (mirrors openrailsrs-route/src/load.rs) ──────────

/// Top-level structure serialised to `track.toml`.
#[derive(Debug, Serialize)]
pub struct TrackToml {
    pub route: RouteMeta,
    pub nodes: Vec<NodeDef>,
    pub edges: Vec<EdgeDef>,
}

#[derive(Debug, Serialize)]
pub struct RouteMeta {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct NodeDef {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<NodeKindDef>,
    pub x_m: f64,
    pub y_m: f64,
}

/// Only the Station variant needs serialisation (Plain is the default when
/// `kind` is absent).
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKindDef {
    Station { name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct EdgeDef {
    pub id: String,
    pub from: String,
    pub to: String,
    pub length_m: f64,
    pub speed_limit_kmh: f64,
    pub grade_percent: f64,
}

// ── Import options ────────────────────────────────────────────────────────────

/// Options controlling the import behaviour.
#[derive(Debug, Clone)]
pub struct OsmImportOptions {
    /// Route id written into `[route] id`.
    pub route_id: String,
    /// Default speed limit (km/h) when the way has no `maxspeed` tag.
    pub default_speed_kmh: f64,
    /// Add reverse edges for each segment (railways are normally bidirectional).
    /// Defaults to `true`.
    pub bidirectional: bool,
}

impl Default for OsmImportOptions {
    fn default() -> Self {
        Self {
            route_id: "imported".into(),
            default_speed_kmh: 80.0,
            bidirectional: true,
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Import a `track.toml` layout from an Overpass JSON **file**.
///
/// Returns the serialised TOML string, ready to write to `track.toml`.
pub fn import_osm_file(
    path: impl AsRef<Path>,
    opts: &OsmImportOptions,
) -> Result<String, ImportError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| ImportError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    import_osm_str(&text, opts)
}

/// Import a `track.toml` layout from an Overpass JSON **string**.
pub fn import_osm_str(json: &str, opts: &OsmImportOptions) -> Result<String, ImportError> {
    let layout = build_layout(json, opts)?;
    Ok(toml::to_string_pretty(&layout)?)
}

/// Parse the Overpass JSON and build the `TrackToml` in-memory layout.
pub fn build_layout(json: &str, opts: &OsmImportOptions) -> Result<TrackToml, ImportError> {
    let resp: OverpassResponse = serde_json::from_str(json)?;

    // Separate nodes and railway ways.
    let mut node_map: HashMap<i64, OsmNode> = HashMap::new();
    let mut ways: Vec<OsmWay> = Vec::new();

    for elem in resp.elements {
        match elem {
            OsmElement::Node(n) => {
                node_map.insert(n.id, n);
            }
            OsmElement::Way(w) => {
                let is_rail = w
                    .tags
                    .get("railway")
                    .map(|v| matches!(v.as_str(), "rail" | "light_rail" | "subway" | "tram"))
                    .unwrap_or(false);
                if is_rail {
                    ways.push(w);
                }
            }
            OsmElement::Other => {}
        }
    }

    if ways.is_empty() {
        return Err(ImportError::NoRailwayWays);
    }

    // Count how many ways each node participates in → junction detection.
    let mut node_way_count: HashMap<i64, u32> = HashMap::new();
    for w in &ways {
        // First and last nodes are always graph endpoints regardless of count.
        for &nid in &w.nodes {
            *node_way_count.entry(nid).or_insert(0) += 1;
        }
    }

    // A node is a "graph node" (endpoint) if it:
    //   - is the first or last node of any way, OR
    //   - appears in more than one way (junction), OR
    //   - has a station/halt tag (even if mid-way).
    let is_graph_node = |nid: i64, is_endpoint: bool| -> bool {
        if is_endpoint {
            return true;
        }
        let count = node_way_count.get(&nid).copied().unwrap_or(0);
        if count > 1 {
            return true;
        }
        if let Some(n) = node_map.get(&nid) {
            let railway = n.tags.get("railway").map(|s| s.as_str()).unwrap_or("");
            if matches!(railway, "station" | "halt" | "stop") {
                return true;
            }
        }
        false
    };

    // Collect the set of all graph-node ids (to project + emit).
    let mut graph_node_ids: HashSet<i64> = HashSet::new();
    for w in &ways {
        let n = w.nodes.len();
        for (i, &nid) in w.nodes.iter().enumerate() {
            if is_graph_node(nid, i == 0 || i == n - 1) {
                graph_node_ids.insert(nid);
            }
        }
    }

    // Compute reference point (centroid of all graph nodes) for projection.
    let ref_lat = graph_node_ids
        .iter()
        .filter_map(|id| node_map.get(id))
        .map(|n| n.lat)
        .sum::<f64>()
        / graph_node_ids.len() as f64;
    let ref_lon = graph_node_ids
        .iter()
        .filter_map(|id| node_map.get(id))
        .map(|n| n.lon)
        .sum::<f64>()
        / graph_node_ids.len() as f64;

    // Build NodeDef list.
    let mut node_defs: Vec<NodeDef> = Vec::new();
    let mut seen_node_ids: HashSet<i64> = HashSet::new();

    for &nid in &graph_node_ids {
        if seen_node_ids.contains(&nid) {
            continue;
        }
        seen_node_ids.insert(nid);
        let osm_node = node_map.get(&nid).ok_or(ImportError::MissingNode(nid))?;
        let (x_m, y_m) = equirectangular_m(osm_node.lat, osm_node.lon, ref_lat, ref_lon);

        let railway_tag = osm_node
            .tags
            .get("railway")
            .map(|s| s.as_str())
            .unwrap_or("");
        let kind = match railway_tag {
            "station" | "halt" | "stop" => {
                let name = osm_node
                    .tags
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| format!("n{nid}"));
                Some(NodeKindDef::Station { name })
            }
            _ => None,
        };

        node_defs.push(NodeDef {
            id: format!("n{nid}"),
            kind,
            x_m: (x_m * 10.0).round() / 10.0,
            y_m: (y_m * 10.0).round() / 10.0,
        });
    }

    // Build EdgeDef list by segmenting each way at graph nodes.
    let mut edge_defs: Vec<EdgeDef> = Vec::new();

    for w in &ways {
        let speed_kmh = w
            .tags
            .get("maxspeed")
            .and_then(|s| {
                // maxspeed may be "120", "120 mph", "120 km/h", etc.
                s.split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
            })
            .unwrap_or(opts.default_speed_kmh);

        // Split the way at graph nodes; accumulate segment lengths.
        let n = w.nodes.len();
        let mut seg_start_idx = 0;
        let mut seg_start_nid = w.nodes[0];
        let mut accumulated_m = 0.0;
        let mut seg_idx = 0_u32;

        for i in 1..n {
            let prev_nid = w.nodes[i - 1];
            let curr_nid = w.nodes[i];

            let prev_node = node_map
                .get(&prev_nid)
                .ok_or(ImportError::MissingNode(prev_nid))?;
            let curr_node = node_map
                .get(&curr_nid)
                .ok_or(ImportError::MissingNode(curr_nid))?;
            accumulated_m +=
                haversine_m(prev_node.lat, prev_node.lon, curr_node.lat, curr_node.lon);

            let is_end = i == n - 1;
            let is_junction = is_graph_node(curr_nid, is_end);

            if is_junction && curr_nid != seg_start_nid {
                edge_defs.push(EdgeDef {
                    id: format!("w{}_{}", w.id, seg_idx),
                    from: format!("n{seg_start_nid}"),
                    to: format!("n{curr_nid}"),
                    length_m: (accumulated_m * 10.0).round() / 10.0,
                    speed_limit_kmh: speed_kmh,
                    grade_percent: 0.0,
                });
                seg_start_idx = i;
                seg_start_nid = curr_nid;
                accumulated_m = 0.0;
                seg_idx += 1;
            }
            let _ = seg_start_idx; // used to track start, suppress warning
        }
    }

    // Add reverse edges for bidirectional operation (railways run both ways).
    if opts.bidirectional {
        let forward = edge_defs.clone();
        for e in forward {
            edge_defs.push(EdgeDef {
                id: format!("{}_r", e.id),
                from: e.to.clone(),
                to: e.from.clone(),
                length_m: e.length_m,
                speed_limit_kmh: e.speed_limit_kmh,
                grade_percent: -e.grade_percent,
            });
        }
    }

    // Deduplicate edges with same from/to (parallel tracks).
    edge_defs.dedup_by(|a, b| a.from == b.from && a.to == b.to);

    Ok(TrackToml {
        route: RouteMeta {
            id: opts.route_id.clone(),
        },
        nodes: node_defs,
        edges: edge_defs,
    })
}
