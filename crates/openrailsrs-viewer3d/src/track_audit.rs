//! Objective quality metrics for `--track-dev` TDB procedural track.
//!
//! Enable full detail: `OPENRAILSRS_TRACK_AUDIT=1`
//! Write JSON report: `OPENRAILSRS_TRACK_AUDIT=/tmp/track-audit.json`

use bevy::prelude::*;
use openrailsrs_formats::{TrackDbFile, TrackNodeKind};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::tdb_track::TdbChord;
use crate::track::{TrackScene, graph_to_world_with_offset, point_segment_distance_xz};
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};

/// Endpoint snap tolerance when comparing `.tdb` chords to `track.toml` edges (metres).
pub const GRAPH_ENDPOINT_TOLERANCE_M: f32 = 12.0;
const GRAPH_MIDPOINT_MATCH_TOLERANCE_M: f32 = 25.0;

pub fn track_audit_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_TRACK_AUDIT").is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackDevVerdict {
    Good,
    Partial,
    Poor,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StatSummary {
    pub count: usize,
    pub mean_m: f64,
    pub p50_m: f64,
    pub p95_m: f64,
    pub max_m: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrackDevAuditReport {
    pub radius_m: f32,
    pub vector_nodes_in_radius: usize,
    pub tdb_chords: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    /// `false` when imported `track.toml` nodes lack `x_m`/`y_m` (Chiltern) — graph match is skipped.
    pub graph_positions_available: bool,
    /// Share of `track.toml` edges whose endpoints snap to some TDB chord (either direction).
    pub graph_edge_match_pct: Option<f64>,
    /// Share of TDB vector terminal endpoints within tolerance of a graph node.
    pub chord_endpoint_snap_pct: Option<f64>,
    /// Perpendicular distance from graph edge midpoints to the nearest TDB chord.
    pub graph_midpoint_to_chord_m: StatSummary,
    /// Gap between consecutive chord endpoints inside the same vector node (should be ~0).
    pub intra_node_chain_gap_m: StatSummary,
    /// Gap between chord endpoints on adjacent vector nodes connected via `TrPins`.
    pub inter_node_chain_gap_m: StatSummary,
    pub verdict: TrackDevVerdict,
}

impl TrackDevAuditReport {
    pub fn log_summary(&self) {
        let graph_match = self
            .graph_edge_match_pct
            .map(|v| format!("{v:.0}%"))
            .unwrap_or_else(|| "n/a".into());
        let endpoint_snap = self
            .chord_endpoint_snap_pct
            .map(|v| format!("{v:.0}%"))
            .unwrap_or_else(|| "n/a".into());
        let mid_p95 = if self.graph_positions_available {
            format!("{:.1}m", self.graph_midpoint_to_chord_m.p95_m)
        } else {
            "n/a".into()
        };
        viewer_log!(
            "openrailsrs-viewer3d: track-audit — verdict={:?} | \
             {} chord(s) / {} graph edge(s) in {:.0}m | \
             chain gap mean {:.2}m | inter-node gap mean {:.2}m | graph match {graph_match} | endpoint snap {endpoint_snap} | \
             mid→chord p95 {mid_p95}",
            self.verdict,
            self.tdb_chords,
            self.graph_edges,
            self.radius_m,
            self.intra_node_chain_gap_m.mean_m,
            self.inter_node_chain_gap_m.mean_m,
        );
    }

    pub fn log_detail(&self) {
        self.log_summary();
        if !self.graph_positions_available {
            viewer_log!(
                "openrailsrs-viewer3d: track-audit   track.toml sin x_m/y_m — comparación con grafo omitida"
            );
        }
        viewer_log!(
            "openrailsrs-viewer3d: track-audit   vectors in radius={} graph_nodes={}",
            self.vector_nodes_in_radius,
            self.graph_nodes
        );
        viewer_log!(
            "openrailsrs-viewer3d: track-audit   intra-node chain gap: mean {:.2}m p95 {:.2}m max {:.2}m",
            self.intra_node_chain_gap_m.mean_m,
            self.intra_node_chain_gap_m.p95_m,
            self.intra_node_chain_gap_m.max_m
        );
        viewer_log!(
            "openrailsrs-viewer3d: track-audit   inter-node chain gap: mean {:.2}m p95 {:.2}m max {:.2}m",
            self.inter_node_chain_gap_m.mean_m,
            self.inter_node_chain_gap_m.p95_m,
            self.inter_node_chain_gap_m.max_m
        );
        viewer_log!(
            "openrailsrs-viewer3d: track-audit   graph midpoint→chord: mean {:.1}m p50 {:.1}m p95 {:.1}m",
            self.graph_midpoint_to_chord_m.mean_m,
            self.graph_midpoint_to_chord_m.p50_m,
            self.graph_midpoint_to_chord_m.p95_m
        );
        match self.verdict {
            TrackDevVerdict::Good => viewer_log!(
                "openrailsrs-viewer3d: track-audit   → grafo y TDB alineados (vista dev usable)"
            ),
            TrackDevVerdict::Partial => viewer_log!(
                "openrailsrs-viewer3d: track-audit   → TDB parcialmente encadenado; puede haber huecos"
            ),
            TrackDevVerdict::Poor => viewer_log!(
                "openrailsrs-viewer3d: track-audit   → TDB fragmentado o sin posiciones de grafo para validar"
            ),
        }
    }

    pub fn write_json_if_requested(&self) {
        let Some(path) = track_audit_json_path() else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(err) = std::fs::write(&path, json) {
                    viewer_log!(
                        "openrailsrs-viewer3d: track-audit — failed to write {}: {err}",
                        path.display()
                    );
                } else {
                    viewer_log!(
                        "openrailsrs-viewer3d: track-audit — wrote {}",
                        path.display()
                    );
                }
            }
            Err(err) => viewer_log!("openrailsrs-viewer3d: track-audit — JSON error: {err}"),
        }
    }
}

fn track_audit_json_path() -> Option<std::path::PathBuf> {
    let value = std::env::var_os("OPENRAILSRS_TRACK_AUDIT")?;
    let s = value.to_string_lossy();
    if s == "1" || s.eq_ignore_ascii_case("true") {
        return None;
    }
    Some(std::path::PathBuf::from(s.as_ref()))
}

pub struct TrackDevAuditInput<'a> {
    pub tdb: &'a TrackDbFile,
    pub scene: &'a TrackScene,
    pub focus: &'a RouteFocus,
    pub offset: RouteWorldOffset,
    pub radius_m: f32,
    pub chords: &'a [TdbChord],
}

pub fn audit_track_dev(input: TrackDevAuditInput<'_>) -> TrackDevAuditReport {
    let TrackDevAuditInput {
        tdb,
        scene,
        focus,
        offset,
        radius_m,
        chords,
    } = input;
    let graph_nodes = collect_graph_node_positions(scene, offset, focus, radius_m);
    let graph_edges = collect_graph_edges(scene, offset, focus, radius_m);

    let graph_positions_available = graph_has_world_positions(scene);
    let (graph_edge_match_pct, chord_endpoint_snap_pct, graph_midpoint_to_chord_m) =
        if graph_positions_available {
            (
                Some(graph_edge_match_fraction(&graph_edges, chords)),
                Some(chord_endpoint_snap_fraction(chords, &graph_nodes)),
                summarize(&graph_midpoint_distances(&graph_edges, chords)),
            )
        } else {
            (None, None, StatSummary::default())
        };
    let intra_node_chain_gap_m = summarize(&intra_node_chain_gaps(chords));
    let inter_node_chain_gap_m = summarize(&inter_node_chain_gaps(tdb, chords));

    let vector_nodes_in_radius = tdb
        .nodes
        .iter()
        .filter(|n| {
            matches!(n.kind, openrailsrs_formats::TrackNodeKind::Vector { .. })
                && chord_belongs_to_node(chords, n.id)
        })
        .count();

    let verdict = classify_verdict(
        graph_positions_available,
        graph_edge_match_pct,
        chord_endpoint_snap_pct,
        graph_midpoint_to_chord_m.p95_m,
        intra_node_chain_gap_m.p95_m,
        intra_node_chain_gap_m.mean_m,
        inter_node_chain_gap_m.p95_m,
    );

    TrackDevAuditReport {
        radius_m,
        vector_nodes_in_radius,
        tdb_chords: chords.len(),
        graph_nodes: graph_nodes.len(),
        graph_edges: graph_edges.len(),
        graph_positions_available,
        graph_edge_match_pct,
        chord_endpoint_snap_pct,
        graph_midpoint_to_chord_m,
        intra_node_chain_gap_m,
        inter_node_chain_gap_m,
        verdict,
    }
}

fn graph_has_world_positions(scene: &TrackScene) -> bool {
    scene
        .graph
        .nodes_iter()
        .any(|(_, n)| n.x_m.abs() > 1.0 || n.y_m.abs() > 1.0)
}

fn chord_belongs_to_node(chords: &[TdbChord], node_id: u32) -> bool {
    chords.iter().any(|c| c.node_id == node_id)
}

#[derive(Clone, Copy, Debug)]
struct GraphNodePos {
    world: Vec3,
}

#[derive(Clone, Copy, Debug)]
struct GraphEdgeSeg {
    tdb_node_id: Option<u32>,
    a: Vec3,
    b: Vec3,
}

fn collect_graph_node_positions(
    scene: &TrackScene,
    offset: RouteWorldOffset,
    focus: &RouteFocus,
    radius_m: f32,
) -> Vec<GraphNodePos> {
    scene
        .graph
        .nodes_iter()
        .filter_map(|(_, node)| {
            let world = graph_to_world_with_offset(offset.delta, node.x_m, node.y_m);
            if focus.horizontal_distance(world) <= radius_m {
                Some(GraphNodePos { world })
            } else {
                None
            }
        })
        .collect()
}

fn collect_graph_edges(
    scene: &TrackScene,
    offset: RouteWorldOffset,
    focus: &RouteFocus,
    radius_m: f32,
) -> Vec<GraphEdgeSeg> {
    let mut out = Vec::new();
    for (_, edge) in scene.graph.edges_iter() {
        let Some(from) = scene.graph.node(&edge.from.0) else {
            continue;
        };
        let Some(to) = scene.graph.node(&edge.to.0) else {
            continue;
        };
        let a = graph_to_world_with_offset(offset.delta, from.x_m, from.y_m);
        let b = graph_to_world_with_offset(offset.delta, to.x_m, to.y_m);
        if focus.horizontal_distance(a) > radius_m && focus.horizontal_distance(b) > radius_m {
            continue;
        }
        out.push(GraphEdgeSeg {
            tdb_node_id: graph_edge_tdb_node_id(&edge.id.0),
            a,
            b,
        });
    }
    out
}

fn graph_edge_tdb_node_id(edge_id: &str) -> Option<u32> {
    edge_id.strip_prefix('e')?.parse().ok()
}

fn graph_edge_match_fraction(edges: &[GraphEdgeSeg], chords: &[TdbChord]) -> f64 {
    if edges.is_empty() {
        return 100.0;
    }
    let terminals = collect_chord_terminals(chords);
    let matched = edges
        .iter()
        .filter(|edge| edge_matches_chord(edge, &terminals, chords))
        .count();
    matched as f64 / edges.len() as f64 * 100.0
}

fn edge_matches_chord(
    edge: &GraphEdgeSeg,
    terminals: &[ChordTerminals],
    chords: &[TdbChord],
) -> bool {
    if let Some(node_id) = edge.tdb_node_id {
        if let Some(terminal) = terminals
            .iter()
            .find(|terminal| terminal.node_id == node_id)
        {
            if endpoints_snap(edge.a, edge.b, terminal.start_world, terminal.end_world)
                || endpoints_snap(edge.a, edge.b, terminal.end_world, terminal.start_world)
            {
                return true;
            }
        }
    }
    let mid = edge.a.lerp(edge.b, 0.5);
    if min_distance_point_to_chords_xz(mid.x, mid.z, chords) <= GRAPH_MIDPOINT_MATCH_TOLERANCE_M {
        return true;
    }
    point_snaps_to_any_terminal(edge.a, terminals) && point_snaps_to_any_terminal(edge.b, terminals)
}

fn endpoints_snap(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> bool {
    distance_xz(a0, b0) <= GRAPH_ENDPOINT_TOLERANCE_M
        && distance_xz(a1, b1) <= GRAPH_ENDPOINT_TOLERANCE_M
}

fn chord_endpoint_snap_fraction(chords: &[TdbChord], nodes: &[GraphNodePos]) -> f64 {
    let terminals = collect_chord_terminals(chords);
    if terminals.is_empty() {
        return 0.0;
    }
    let mut endpoints = 0usize;
    let mut snapped = 0usize;
    for terminal in terminals {
        for point in [terminal.start_world, terminal.end_world] {
            endpoints += 1;
            if nodes
                .iter()
                .any(|n| distance_xz(n.world, point) <= GRAPH_ENDPOINT_TOLERANCE_M)
            {
                snapped += 1;
            }
        }
    }
    snapped as f64 / endpoints as f64 * 100.0
}

#[derive(Clone, Copy, Debug)]
struct ChordTerminals {
    node_id: u32,
    start_world: Vec3,
    end_world: Vec3,
}

fn collect_chord_terminals(chords: &[TdbChord]) -> Vec<ChordTerminals> {
    let mut by_node: HashMap<u32, Vec<&TdbChord>> = HashMap::new();
    for chord in chords {
        by_node.entry(chord.node_id).or_default().push(chord);
    }
    let mut out = Vec::with_capacity(by_node.len());
    for (node_id, mut node_chords) in by_node {
        node_chords.sort_by_key(|c| c.section_index);
        let Some(first) = node_chords.first() else {
            continue;
        };
        let Some(last) = node_chords.last() else {
            continue;
        };
        out.push(ChordTerminals {
            node_id,
            start_world: first.start_world,
            end_world: last.end_world,
        });
    }
    out
}

fn point_snaps_to_any_terminal(point: Vec3, terminals: &[ChordTerminals]) -> bool {
    terminals.iter().any(|terminal| {
        distance_xz(point, terminal.start_world) <= GRAPH_ENDPOINT_TOLERANCE_M
            || distance_xz(point, terminal.end_world) <= GRAPH_ENDPOINT_TOLERANCE_M
    })
}

fn graph_midpoint_distances(edges: &[GraphEdgeSeg], chords: &[TdbChord]) -> Vec<f32> {
    edges
        .iter()
        .map(|edge| {
            let mid = edge.a.lerp(edge.b, 0.5);
            min_distance_point_to_chords_xz(mid.x, mid.z, chords)
        })
        .collect()
}

fn min_distance_point_to_chords_xz(px: f32, pz: f32, chords: &[TdbChord]) -> f32 {
    chords
        .iter()
        .map(|chord| {
            point_segment_distance_xz(
                px,
                pz,
                chord.start_world.x,
                chord.start_world.z,
                chord.end_world.x,
                chord.end_world.z,
            )
        })
        .fold(f32::INFINITY, f32::min)
}

fn intra_node_chain_gaps(chords: &[TdbChord]) -> Vec<f32> {
    let mut by_node: HashMap<u32, Vec<&TdbChord>> = HashMap::new();
    for chord in chords {
        by_node.entry(chord.node_id).or_default().push(chord);
    }
    let mut gaps = Vec::new();
    for mut node_chords in by_node.into_values() {
        if node_chords.len() < 2 {
            continue;
        }
        node_chords.sort_by_key(|c| c.section_index);
        for pair in node_chords.windows(2) {
            gaps.push(distance_xz(pair[0].end_world, pair[1].start_world));
        }
    }
    gaps
}

fn inter_node_chain_gaps(tdb: &TrackDbFile, chords: &[TdbChord]) -> Vec<f32> {
    let vector_ids: HashSet<u32> = chords
        .iter()
        .map(|c| c.node_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|id| {
            tdb.node_by_id(*id)
                .is_some_and(|n| matches!(n.kind, TrackNodeKind::Vector { .. }))
        })
        .collect();
    let nodes_by_id: HashMap<u32, &openrailsrs_formats::TrackDbNode> =
        tdb.nodes.iter().map(|n| (n.id, n)).collect();
    let mut seen_pairs = HashSet::new();
    let mut gaps = Vec::new();
    for &a in &vector_ids {
        for b in connected_vector_neighbors(a, &vector_ids, &nodes_by_id) {
            let pair = if a < b { (a, b) } else { (b, a) };
            if !seen_pairs.insert(pair) {
                continue;
            }
            if let Some(gap) = min_chord_endpoint_gap(chords, a, b) {
                gaps.push(gap);
            }
        }
    }
    gaps
}

fn connected_vector_neighbors(
    vector_id: u32,
    vector_ids: &HashSet<u32>,
    nodes_by_id: &HashMap<u32, &openrailsrs_formats::TrackDbNode>,
) -> Vec<u32> {
    let Some(node) = nodes_by_id.get(&vector_id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pin in &node.pin_refs {
        if pin.node_id != vector_id && vector_ids.contains(&pin.node_id) {
            out.push(pin.node_id);
        }
        let Some(pin_node) = nodes_by_id.get(&pin.node_id) else {
            continue;
        };
        for next in &pin_node.pin_refs {
            if next.node_id != vector_id && vector_ids.contains(&next.node_id) {
                out.push(next.node_id);
            }
        }
    }
    out
}

fn min_chord_endpoint_gap(chords: &[TdbChord], a: u32, b: u32) -> Option<f32> {
    let mut best = f32::INFINITY;
    for ca in chords.iter().filter(|c| c.node_id == a) {
        for cb in chords.iter().filter(|c| c.node_id == b) {
            for pa in [ca.start_world, ca.end_world] {
                for pb in [cb.start_world, cb.end_world] {
                    best = best.min(distance_xz(pa, pb));
                }
            }
        }
    }
    best.is_finite().then_some(best)
}

fn classify_verdict(
    graph_positions_available: bool,
    graph_edge_match_pct: Option<f64>,
    chord_endpoint_snap_pct: Option<f64>,
    midpoint_p95_m: f64,
    chain_gap_p95_m: f64,
    chain_gap_mean_m: f64,
    inter_node_gap_p95_m: f64,
) -> TrackDevVerdict {
    if !graph_positions_available {
        if chain_gap_p95_m <= 1.0 && chain_gap_mean_m <= 2.0 && inter_node_gap_p95_m <= 5.0 {
            return TrackDevVerdict::Good;
        }
        if chain_gap_p95_m <= 5.0 && chain_gap_mean_m <= 10.0 {
            return TrackDevVerdict::Partial;
        }
        return TrackDevVerdict::Poor;
    }
    let graph_edge_match_pct = graph_edge_match_pct.unwrap_or(0.0);
    let chord_endpoint_snap_pct = chord_endpoint_snap_pct.unwrap_or(0.0);
    if graph_edge_match_pct >= 70.0
        && chord_endpoint_snap_pct >= 80.0
        && midpoint_p95_m <= f64::from(GRAPH_MIDPOINT_MATCH_TOLERANCE_M)
        && chain_gap_p95_m <= 1.0
    {
        TrackDevVerdict::Good
    } else if graph_edge_match_pct >= 35.0 && chord_endpoint_snap_pct >= 50.0 {
        TrackDevVerdict::Partial
    } else {
        TrackDevVerdict::Poor
    }
}

fn summarize(values: &[f32]) -> StatSummary {
    if values.is_empty() {
        return StatSummary::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let count = sorted.len();
    let sum: f32 = sorted.iter().sum();
    StatSummary {
        count,
        mean_m: f64::from(sum) / count as f64,
        p50_m: f64::from(percentile(&sorted, 0.50)),
        p95_m: f64::from(percentile(&sorted, 0.95)),
        max_m: f64::from(*sorted.last().unwrap_or(&0.0)),
    }
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f32 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn distance_xz(a: Vec3, b: Vec3) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

pub fn run_track_dev_audit(
    tdb: &TrackDbFile,
    scene: &TrackScene,
    focus: &RouteFocus,
    offset: RouteWorldOffset,
    radius_m: f32,
    chords: &[TdbChord],
) {
    let report = audit_track_dev(TrackDevAuditInput {
        tdb,
        scene,
        focus,
        offset,
        radius_m,
        chords,
    });
    if track_audit_enabled() {
        report.log_detail();
    } else {
        report.log_summary();
    }
    report.write_json_if_requested();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tdb_track::collect_tdb_chords;
    use crate::track::TrackScene;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_formats::{
        TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind, TrackVectorGeometry,
        TrackVectorPoint,
    };
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};
    use std::path::PathBuf;

    fn section_at(x: f64, z: f64) -> TrVectorSectionRecord {
        TrVectorSectionRecord {
            shape_idx: 1,
            aux_shape_idx: 0,
            start: TrackVectorPoint {
                tile_x: 0,
                tile_z: 0,
                x,
                y: 0.0,
                z,
            },
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
        }
    }

    #[test]
    fn aligned_synthetic_route_scores_good() {
        let s0 = section_at(0.0, 0.0);
        let s1 = section_at(100.0, 0.0);
        let tdb = TrackDbFile {
            nodes: vec![TrackDbNode {
                id: 2,
                position: None,
                pin_refs: Vec::new(),
                kind: TrackNodeKind::Vector {
                    length_m: 100.0,
                    speed_limit_mps: 0.0,
                    pins: (1, 3),
                    item_ids: Vec::new(),
                    sections: vec![s0, s1],
                    geometry: Some(TrackVectorGeometry {
                        start: s0.start,
                        end: s1.start,
                    }),
                },
            }],
            items: Vec::new(),
        };
        let mut graph = TrackGraph::new();
        graph
            .insert_node(Node {
                id: NodeId("n1".into()),
                kind: NodeKind::Plain,
                x_m: 0.0,
                y_m: 0.0,
            })
            .unwrap();
        graph
            .insert_node(Node {
                id: NodeId("n2".into()),
                kind: NodeKind::Plain,
                x_m: 100.0,
                y_m: 0.0,
            })
            .unwrap();
        graph
            .insert_edge(Edge {
                id: EdgeId("e1".into()),
                from: NodeId("n1".into()),
                to: NodeId("n2".into()),
                length_m: 100.0,
                speed_limit_mps: 30.0,
                grade_percent: 0.0,
            })
            .unwrap();
        let scene = TrackScene::from_graph(graph);
        let focus = RouteFocus {
            center: Vec3::new(50.0, 0.0, 0.0),
            height_origin: 0.0,
        };
        let chords = collect_tdb_chords(&tdb, &focus, 500.0);
        let report = audit_track_dev(TrackDevAuditInput {
            tdb: &tdb,
            scene: &scene,
            focus: &focus,
            offset: RouteWorldOffset::default(),
            radius_m: 500.0,
            chords: &chords,
        });
        assert_eq!(report.verdict, TrackDevVerdict::Good);
        assert_eq!(report.graph_edge_match_pct, Some(100.0));
    }

    /// Requires `OPENRAILSRS_MSTS_CONTENT` with Chiltern route installed.
    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn chiltern_track_dev_audit_smoke() {
        use openrailsrs_route::load_track_graph_from_route_dir;
        use openrailsrs_scenarios::load_scenario;

        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern/scenario.toml");
        let route_dir = scenario_path.parent().expect("dir");
        if std::env::var_os("OPENRAILSRS_MSTS_CONTENT").is_none() {
            eprintln!("skip: set OPENRAILSRS_MSTS_CONTENT");
            return;
        }
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let graph = load_track_graph_from_route_dir(route_dir).expect("track.toml");
        let scene = TrackScene::from_graph(graph);
        let anchor = crate::world::msts_to_bevy(
            6080,
            14925,
            openrailsrs_formats::Vec3 {
                x: 891.831,
                y: 35.7818,
                z: 582.756,
            },
        );
        let focus = RouteFocus {
            center: anchor,
            height_origin: anchor.y,
        };
        let _world = crate::world::load_world_from_route_dir_near(route_dir, Some(anchor), 8000.0);
        let graph_start = {
            use crate::track::graph_to_world;
            use openrailsrs_route::edge_path;
            let path_edges = edge_path(
                &scene.graph,
                &scenario.route.start,
                &scenario.route.destination,
            )
            .expect("path");
            let mut remaining = scenario.route.start_offset_m.unwrap_or(0.0).max(0.0);
            let mut pos = graph_to_world(0.0, 0.0);
            for edge_id in path_edges {
                let edge = scene.graph.edge(&edge_id).expect("edge");
                let from = scene.graph.node(&edge.from.0).expect("from");
                let to = scene.graph.node(&edge.to.0).expect("to");
                if remaining <= edge.length_m || edge.length_m <= 0.0 {
                    let frac = if edge.length_m > 0.0 {
                        (remaining / edge.length_m).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    pos = graph_to_world(
                        from.x_m + frac * (to.x_m - from.x_m),
                        from.y_m + frac * (to.y_m - from.y_m),
                    );
                    break;
                }
                remaining -= edge.length_m;
            }
            pos
        };
        let delta = Vec3::new(anchor.x - graph_start.x, 0.0, anchor.z - graph_start.z);
        let offset = if Vec2::new(delta.x, delta.z).length() <= 100_000.0 {
            RouteWorldOffset::default()
        } else {
            RouteWorldOffset { delta }
        };
        let assets = crate::shapes::RouteAssets::new(route_dir);
        let tdb = assets.track_db().expect("Chiltern .tdb");
        let radius = crate::launch::TRACK_DEV_TDB_RADIUS_M;
        let chords = collect_tdb_chords(tdb, &focus, radius);
        let report = audit_track_dev(TrackDevAuditInput {
            tdb,
            scene: &scene,
            focus: &focus,
            offset,
            radius_m: radius,
            chords: &chords,
        });
        report.log_detail();
        assert!(report.tdb_chords > 1000, "chords near Birmingham anchor");
    }
}
