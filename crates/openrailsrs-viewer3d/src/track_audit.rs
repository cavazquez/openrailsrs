//! Objective quality metrics for `--track-dev` TDB procedural track.
//!
//! Enable full detail: `OPENRAILSRS_TRACK_AUDIT=1`
//! Write JSON report: `OPENRAILSRS_TRACK_AUDIT=/tmp/track-audit.json`

use bevy::prelude::*;
use openrailsrs_formats::{TSectionCatalog, TrackDbFile, TrackNodeKind};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::tdb_track::{TdbChord, inter_node_junction_gap_m};
use crate::track::{TrackScene, graph_to_world_with_offset, point_segment_distance_xz};
use crate::viewer_log;
use crate::world::{RouteFocus, RouteWorldOffset};

/// Endpoint snap tolerance when comparing `.tdb` chords to `track.toml` edges (metres).
pub const GRAPH_ENDPOINT_TOLERANCE_M: f32 = 12.0;
const GRAPH_MIDPOINT_MATCH_TOLERANCE_M: f32 = 25.0;
const INTER_NODE_WORST_PAIR_COUNT: usize = 10;
const INTER_NODE_WORST_LOG_MIN_M: f64 = 5.0;
const STATIC_TRACKOBJ_WORST_COUNT: usize = 15;
const STATIC_TRACKOBJ_WORST_LOG_MIN_M: f64 = 5.0;
const STATIC_TRACKOBJ_SHAPE_INDEX_MAX_DIST_M: f32 = 250.0;
/// Prefer `SectionIdx` match only among candidates within this margin of the spatial best (metres).
const STATIC_TRACKOBJ_SHAPE_TIE_BREAK_M: f32 = 5.0;

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
    /// Distance from static MSTS `TrackObj` anchors (`WORLD/*.w`) to nearest TDB chord.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_trackobj_to_chord_m: Option<StatSummary>,
    /// Same as above but only chords whose `shape_idx` matches `TrackObj` `SectionIdx`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_trackobj_to_matching_chord_m: Option<StatSummary>,
    /// Number of static `TrackObj` anchors included in `static_trackobj_to_chord_m`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub static_trackobj_count: Option<usize>,
    /// Largest inter-node gaps (vector id pairs), for debugging outliers.
    pub worst_inter_node_gaps: Vec<InterNodeGapPair>,
    /// Static `TrackObj` anchors farthest from any TDB chord (MSTS reference outliers).
    pub worst_static_trackobj: Vec<StaticTrackObjOutlier>,
    pub verdict: TrackDevVerdict,
}

#[derive(Clone, Debug, Serialize)]
pub struct StaticTrackObjOutlier {
    pub uid: u32,
    pub tile_x: i32,
    pub tile_z: i32,
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_idx: Option<u32>,
    /// Distance to nearest chord (any shape).
    pub dist_any_m: f64,
    /// Distance to nearest chord with matching `shape_idx`; absent when no `SectionIdx` or no chord.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dist_matching_m: Option<f64>,
    /// TDB vector node for matching shape nearest to the TrackObj (from chord or `.tdb` index).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_id: Option<u32>,
    /// Primary sort key: matching distance when available, else any-chord distance.
    pub dist_m: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct InterNodeGapPair {
    pub vector_a: u32,
    pub vector_b: u32,
    /// Shared junction node id when vectors meet through a switch; absent on direct pin links.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_junction: Option<u32>,
    /// Minimum of junction-face geometry gap and culled-chord endpoint gap (metres).
    pub gap_m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub junction_face_gap_m: Option<f64>,
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
        if let (Some(stats), Some(count)) =
            (&self.static_trackobj_to_chord_m, self.static_trackobj_count)
        {
            viewer_log!(
                "openrailsrs-viewer3d: track-audit   static TrackObj→chord (any): mean {:.2}m p95 {:.2}m max {:.2}m (n={count})",
                stats.mean_m,
                stats.p95_m,
                stats.max_m
            );
        }
        if let Some(stats) = &self.static_trackobj_to_matching_chord_m {
            viewer_log!(
                "openrailsrs-viewer3d: track-audit   static TrackObj→matching chord: mean {:.2}m p95 {:.2}m max {:.2}m (n={})",
                stats.mean_m,
                stats.p95_m,
                stats.max_m,
                stats.count
            );
        }
        if !self.worst_static_trackobj.is_empty() {
            viewer_log!("openrailsrs-viewer3d: track-audit   worst static TrackObj→chord:");
            for o in &self.worst_static_trackobj {
                let section = o
                    .section_idx
                    .map(|s| format!(" sec={s}"))
                    .unwrap_or_default();
                let vector = o.vector_id.map(|v| format!(" V{v}")).unwrap_or_default();
                let matching = o
                    .dist_matching_m
                    .map(|d| format!(" match={d:.1}m"))
                    .unwrap_or_default();
                viewer_log!(
                    "openrailsrs-viewer3d: track-audit     uid={} tile ({},{}) pos ({:.1},{:.1},{:.1}){section}{vector}: any={:.1}m{matching}",
                    o.uid,
                    o.tile_x,
                    o.tile_z,
                    o.x_m,
                    o.y_m,
                    o.z_m,
                    o.dist_any_m,
                );
            }
        }
        if !self.worst_inter_node_gaps.is_empty() {
            viewer_log!("openrailsrs-viewer3d: track-audit   worst inter-node gaps:");
            for pair in &self.worst_inter_node_gaps {
                let via = pair
                    .via_junction
                    .map(|j| format!(" via J{j}"))
                    .unwrap_or_default();
                viewer_log!(
                    "openrailsrs-viewer3d: track-audit     V{}↔V{}{via}: {:.1}m{}",
                    pair.vector_a,
                    pair.vector_b,
                    pair.gap_m,
                    pair.junction_face_gap_m
                        .filter(|j| (*j - pair.gap_m).abs() > 0.5)
                        .map(|j| format!(" (junction face {j:.1}m)"))
                        .unwrap_or_default()
                );
            }
        }
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
    pub route_dir: Option<&'a std::path::Path>,
}

pub fn audit_track_dev(input: TrackDevAuditInput<'_>) -> TrackDevAuditReport {
    let TrackDevAuditInput {
        tdb,
        scene,
        focus,
        offset,
        radius_m,
        chords,
        route_dir,
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
    let inter_node_pairs = inter_node_gap_pairs(tdb, chords);
    let inter_node_chain_gap_m = summarize(
        &inter_node_pairs
            .iter()
            .map(|p| p.gap_m as f32)
            .collect::<Vec<_>>(),
    );
    let worst_inter_node_gaps = worst_inter_node_gap_pairs(&inter_node_pairs);
    let (
        static_trackobj_to_chord_m,
        static_trackobj_to_matching_chord_m,
        static_trackobj_count,
        worst_static_trackobj,
    ) = static_trackobj_to_chord_summary(route_dir, tdb, focus, radius_m, chords).unwrap_or((
        None,
        None,
        None,
        Vec::new(),
    ));

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
        static_trackobj_to_chord_m,
        static_trackobj_to_matching_chord_m,
        static_trackobj_count,
        worst_inter_node_gaps,
        worst_static_trackobj,
        verdict,
    }
}

struct StaticTrackObjSample {
    uid: u32,
    tile_x: i32,
    tile_z: i32,
    position: Vec3,
    section_idx: Option<u32>,
    dist_any_m: f32,
    dist_matching_m: Option<f32>,
    vector_id: Option<u32>,
}

type StaticTrackObjSummary = (
    Option<StatSummary>,
    Option<StatSummary>,
    Option<usize>,
    Vec<StaticTrackObjOutlier>,
);

fn static_trackobj_to_chord_summary(
    route_dir: Option<&std::path::Path>,
    tdb: &TrackDbFile,
    focus: &RouteFocus,
    radius_m: f32,
    chords: &[TdbChord],
) -> Option<StaticTrackObjSummary> {
    let route_dir = route_dir?;
    if chords.is_empty() {
        return Some((None, None, None, Vec::new()));
    }
    let shape_index = tdb.index_vector_sections_by_shape();
    let tsection = load_tsection_for_trackobj_audit(route_dir);
    let world =
        crate::world::load_world_from_route_dir_near(route_dir, Some(focus.center), radius_m);
    let mut samples = Vec::new();
    for obj in world.items.iter().filter(|o| o.kind == "TrackObj") {
        if is_road_trackobj_for_audit(obj, tsection.as_ref()) {
            continue;
        }
        if focus.horizontal_distance(obj.position) > radius_m {
            continue;
        }
        let Some(dist_any_m) = min_distance_to_chords(obj.position, chords) else {
            continue;
        };
        let (dist_matching_m, vector_id) = match obj.section_idx {
            Some(shape_idx) => matching_shape_distance_and_vector(
                obj.position,
                obj.tile_x,
                obj.tile_z,
                shape_idx,
                &shape_index,
                tdb,
                chords,
            ),
            None => (None, None),
        };
        samples.push(StaticTrackObjSample {
            uid: obj.uid.unwrap_or(0),
            tile_x: obj.tile_x,
            tile_z: obj.tile_z,
            position: obj.position,
            section_idx: obj.section_idx,
            dist_any_m,
            dist_matching_m,
            vector_id,
        });
    }
    if samples.is_empty() {
        return Some((None, None, None, Vec::new()));
    }
    let any_dists: Vec<f32> = samples.iter().map(|s| s.dist_any_m).collect();
    let matching_dists: Vec<f32> = samples.iter().filter_map(|s| s.dist_matching_m).collect();
    let count = any_dists.len();
    Some((
        Some(summarize(&any_dists)),
        if matching_dists.is_empty() {
            None
        } else {
            Some(summarize(&matching_dists))
        },
        Some(count),
        worst_static_trackobj_outliers(&samples),
    ))
}

fn load_tsection_for_trackobj_audit(route_dir: &std::path::Path) -> Option<TSectionCatalog> {
    if let Ok(catalog) = TSectionCatalog::load_for_route(route_dir) {
        if !catalog.shapes.is_empty() {
            return Some(catalog);
        }
    }
    let msts_route = crate::shapes::resolve_msts_route_dir(route_dir)?;
    TSectionCatalog::load_for_route(&msts_route)
        .ok()
        .filter(|catalog| !catalog.shapes.is_empty())
}

fn is_road_trackobj_for_audit(
    obj: &crate::world::WorldObject,
    tsection: Option<&TSectionCatalog>,
) -> bool {
    if obj
        .section_idx
        .is_some_and(|idx| tsection.is_some_and(|cat| cat.is_road_shape(idx)))
    {
        return true;
    }
    obj.shape_file
        .as_deref()
        .is_some_and(is_road_shape_file_name)
}

fn is_road_shape_file_name(name: &str) -> bool {
    let lower = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    lower.starts_with("hwy")
        || lower.starts_with("road")
        || lower.starts_with("rd")
        || lower.contains("road")
}

fn worst_static_trackobj_outliers(samples: &[StaticTrackObjSample]) -> Vec<StaticTrackObjOutlier> {
    let mut sorted: Vec<StaticTrackObjOutlier> = samples
        .iter()
        .filter(|s| {
            let key = s.dist_matching_m.unwrap_or(s.dist_any_m);
            f64::from(key) >= STATIC_TRACKOBJ_WORST_LOG_MIN_M
        })
        .map(|s| {
            let dist_any_m = f64::from(s.dist_any_m);
            let dist_matching_m = s.dist_matching_m.map(f64::from);
            let dist_m = dist_matching_m.unwrap_or(dist_any_m);
            StaticTrackObjOutlier {
                uid: s.uid,
                tile_x: s.tile_x,
                tile_z: s.tile_z,
                x_m: f64::from(s.position.x),
                y_m: f64::from(s.position.y),
                z_m: f64::from(s.position.z),
                section_idx: s.section_idx,
                dist_any_m,
                dist_matching_m,
                vector_id: s.vector_id,
                dist_m,
            }
        })
        .collect();
    sorted.sort_by(|a, b| {
        b.dist_m
            .partial_cmp(&a.dist_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.truncate(STATIC_TRACKOBJ_WORST_COUNT);
    sorted
}

fn tdb_ref_tile_from_world(tile_x: i32, tile_z: i32) -> (i32, i32) {
    // `.w` tile coords are parsed signed now, same convention as `.tdb`.
    (tile_x, tile_z)
}

fn matching_shape_distance_and_vector(
    point: Vec3,
    display_tile_x: i32,
    display_tile_z: i32,
    shape_idx: u32,
    shape_index: &HashMap<u32, Vec<openrailsrs_formats::IndexedTrVectorSection>>,
    tdb: &TrackDbFile,
    chords: &[TdbChord],
) -> (Option<f32>, Option<u32>) {
    let ref_tile = tdb_ref_tile_from_world(display_tile_x, display_tile_z);
    let mut raw_near: Vec<(f32, usize)> = Vec::new();
    for (i, chord) in chords.iter().enumerate() {
        if let Some(d) = min_distance_to_chords(point, std::slice::from_ref(chord)) {
            raw_near.push((d, i));
        }
    }
    if raw_near.is_empty() {
        return supplement_matching_from_shape_index(point, shape_idx, shape_index, tdb, ref_tile);
    }
    raw_near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let best_raw = raw_near[0].0;
    let rebase_cutoff = best_raw + STATIC_TRACKOBJ_SHAPE_TIE_BREAK_M + 1.0;

    let mut candidates: Vec<(f32, u32, bool)> = Vec::new();
    for (d, i) in raw_near.iter().take_while(|(d, _)| *d <= rebase_cutoff) {
        let chord = &chords[*i];
        candidates.push((*d, chord.node_id, chord.shape_idx == shape_idx));
    }

    if candidates.is_empty() {
        return supplement_matching_from_shape_index(point, shape_idx, shape_index, tdb, ref_tile);
    }
    select_spatial_match_with_shape_tie_break(candidates)
}

fn supplement_matching_from_shape_index(
    point: Vec3,
    shape_idx: u32,
    shape_index: &HashMap<u32, Vec<openrailsrs_formats::IndexedTrVectorSection>>,
    tdb: &TrackDbFile,
    ref_tile: (i32, i32),
) -> (Option<f32>, Option<u32>) {
    let Some(entries) = shape_index.get(&shape_idx) else {
        return (None, None);
    };
    let max_dist_sq = STATIC_TRACKOBJ_SHAPE_INDEX_MAX_DIST_M.powi(2);
    let mut candidates = Vec::new();
    for entry in entries {
        let (x, _, z) = entry
            .record
            .bevy_position_nearest_to(point.x, point.z, Some(ref_tile));
        let dx = point.x - x;
        let dz = point.z - z;
        if dx * dx + dz * dz > max_dist_sq {
            continue;
        }
        if let Some(d) = indexed_section_segment_distance(tdb, entry, point, ref_tile) {
            candidates.push((d, entry.node_id, true));
        }
    }
    select_spatial_match_with_shape_tie_break(candidates)
}

/// Pick the nearest segment; when several are within [`STATIC_TRACKOBJ_SHAPE_TIE_BREAK_M`], prefer matching `SectionIdx`.
fn select_spatial_match_with_shape_tie_break(
    mut candidates: Vec<(f32, u32, bool)>,
) -> (Option<f32>, Option<u32>) {
    if candidates.is_empty() {
        return (None, None);
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let best_dist = candidates[0].0;
    let chosen = candidates
        .iter()
        .filter(|(d, _, _)| *d <= best_dist + STATIC_TRACKOBJ_SHAPE_TIE_BREAK_M)
        .max_by(|a, b| a.2.cmp(&b.2).then_with(|| b.0.total_cmp(&a.0)))
        .copied()
        .unwrap_or(candidates[0]);
    (Some(chosen.0), Some(chosen.1))
}

fn indexed_section_segment_distance(
    tdb: &TrackDbFile,
    entry: &openrailsrs_formats::IndexedTrVectorSection,
    point: Vec3,
    ref_tile: (i32, i32),
) -> Option<f32> {
    let node = tdb.node_by_id(entry.node_id)?;
    let TrackNodeKind::Vector {
        sections,
        geometry,
        length_m,
        ..
    } = &node.kind
    else {
        return None;
    };
    let section_index = sections.iter().position(|s| {
        s.shape_idx == entry.record.shape_idx && section_records_match(s, &entry.record)
    })?;
    let section = sections[section_index];
    let (sx, _, sz) = section.bevy_position_nearest_to(point.x, point.z, Some(ref_tile));
    let start = Vec3::new(sx, 0.0, sz);
    let end = if section_index + 1 < sections.len() {
        let next = sections[section_index + 1];
        let (ex, _, ez) = next.bevy_position_nearest_to(point.x, point.z, Some(ref_tile));
        Vec3::new(ex, 0.0, ez)
    } else if let Some(geom) = geometry {
        let header = (section.header_tile_x, section.header_tile_z);
        let (ex, _, ez) =
            geom.end
                .bevy_position_nearest_to(point.x, point.z, Some(ref_tile), Some(header));
        Vec3::new(ex, 0.0, ez)
    } else if let Some(h) = section.heading_deg() {
        let len = if *length_m > 0.5 {
            *length_m as f32
        } else {
            10.0
        };
        let yaw = h.to_radians() as f32;
        start + Vec3::new(yaw.sin() * len, 0.0, yaw.cos() * len)
    } else {
        return None;
    };
    Some(point_segment_distance_xz(
        point.x, point.z, start.x, start.z, end.x, end.z,
    ))
}

fn section_records_match(
    a: &openrailsrs_formats::TrVectorSectionRecord,
    b: &openrailsrs_formats::TrVectorSectionRecord,
) -> bool {
    a.shape_idx == b.shape_idx
        && (a.start.x - b.start.x).abs() < 0.01
        && (a.start.z - b.start.z).abs() < 0.01
}

fn min_distance_to_chords(point: Vec3, chords: &[TdbChord]) -> Option<f32> {
    min_distance_to_chords_iter(point, chords.iter())
}

fn min_distance_to_chords_iter<'a, I>(point: Vec3, chords: I) -> Option<f32>
where
    I: Iterator<Item = &'a TdbChord>,
{
    let mut best = f32::INFINITY;
    for c in chords {
        best = best.min(point_segment_distance_xz(
            point.x,
            point.z,
            c.start_world.x,
            c.start_world.z,
            c.end_world.x,
            c.end_world.z,
        ));
    }
    best.is_finite().then_some(best)
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
        if chord.section_index == crate::tdb_track::TDB_JUNCTION_BRIDGE_SECTION {
            continue;
        }
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

fn inter_node_gap_pairs(tdb: &TrackDbFile, chords: &[TdbChord]) -> Vec<InterNodeGapPair> {
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
    let mut out = Vec::new();
    for &a in &vector_ids {
        for b in connected_vector_neighbors(a, &vector_ids, &nodes_by_id) {
            let pair = if a < b { (a, b) } else { (b, a) };
            if !seen_pairs.insert(pair) {
                continue;
            }
            let junction_face_gap_m = inter_node_junction_gap_m(tdb, pair.0, pair.1);
            let gap = match (
                junction_face_gap_m,
                min_chord_endpoint_gap(chords, pair.0, pair.1),
            ) {
                (Some(j), Some(c)) => j.min(c),
                (Some(j), None) => j,
                (None, Some(c)) => c,
                (None, None) => continue,
            };
            out.push(InterNodeGapPair {
                vector_a: pair.0,
                vector_b: pair.1,
                via_junction: inter_node_via_junction(pair.0, pair.1, &nodes_by_id),
                gap_m: f64::from(gap),
                junction_face_gap_m: junction_face_gap_m.map(f64::from),
            });
        }
    }
    out
}

fn inter_node_via_junction(
    a: u32,
    b: u32,
    nodes_by_id: &HashMap<u32, &openrailsrs_formats::TrackDbNode>,
) -> Option<u32> {
    let node_a = nodes_by_id.get(&a)?;
    let node_b = nodes_by_id.get(&b)?;
    if node_a.pin_refs.iter().any(|p| p.node_id == b) {
        return None;
    }
    for pin in &node_a.pin_refs {
        let Some(mid) = nodes_by_id.get(&pin.node_id) else {
            continue;
        };
        if !matches!(mid.kind, TrackNodeKind::Junction { .. }) {
            continue;
        }
        if node_b.pin_refs.iter().any(|p| p.node_id == pin.node_id) {
            return Some(pin.node_id);
        }
    }
    None
}

fn worst_inter_node_gap_pairs(pairs: &[InterNodeGapPair]) -> Vec<InterNodeGapPair> {
    let mut sorted: Vec<InterNodeGapPair> = pairs
        .iter()
        .filter(|p| p.gap_m >= INTER_NODE_WORST_LOG_MIN_M)
        .cloned()
        .collect();
    sorted.sort_by(|a, b| {
        b.gap_m
            .partial_cmp(&a.gap_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.truncate(INTER_NODE_WORST_PAIR_COUNT);
    sorted
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
    route_dir: Option<&std::path::Path>,
) {
    let report = audit_track_dev(TrackDevAuditInput {
        tdb,
        scene,
        focus,
        offset,
        radius_m,
        chords,
        route_dir,
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
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 0.0,
            z,
        };
        TrVectorSectionRecord {
            shape_idx: 1,
            aux_shape_idx: 0,
            header_tile_x: start.tile_x,
            header_tile_z: start.tile_z,
            start,
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
        let chords = collect_tdb_chords(&tdb, &focus, 500.0, None);
        let report = audit_track_dev(TrackDevAuditInput {
            tdb: &tdb,
            scene: &scene,
            focus: &focus,
            offset: RouteWorldOffset::default(),
            radius_m: 500.0,
            chords: &chords,
            route_dir: None,
        });
        assert_eq!(report.verdict, TrackDevVerdict::Good);
        assert_eq!(report.graph_edge_match_pct, Some(100.0));
    }

    #[test]
    fn spatial_match_prefers_shape_idx_within_tie_break() {
        let candidates = vec![(2.0, 100, false), (3.0, 200, true), (20.0, 300, true)];
        let (dist, vector) = select_spatial_match_with_shape_tie_break(candidates);
        assert_eq!(vector, Some(200));
        assert!((dist.unwrap() - 3.0).abs() < 0.01);
    }

    #[test]
    fn spatial_match_uses_nearest_when_shape_farther() {
        let candidates = vec![(1.0, 100, false), (50.0, 200, true)];
        let (dist, vector) = select_spatial_match_with_shape_tie_break(candidates);
        assert_eq!(vector, Some(100));
        assert!((dist.unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn road_shape_file_names_are_excluded_from_trackobj_audit() {
        assert!(is_road_shape_file_name("hwy2l2wnaStrt5mp.s"));
        assert!(is_road_shape_file_name("GLOBAL/SHAPES/RoadBridge.s"));
        assert!(!is_road_shape_file_name("A1t500r5d.s"));
        assert!(!is_road_shape_file_name("UKFS_R_1x200m.s"));
    }

    #[test]
    fn worst_static_trackobj_outliers_sorted_and_capped() {
        let samples = vec![
            StaticTrackObjSample {
                uid: 1,
                tile_x: 6080,
                tile_z: 14925,
                position: Vec3::new(100.0, 0.0, 200.0),
                section_idx: Some(42),
                dist_any_m: 141.0,
                dist_matching_m: Some(8.0),
                vector_id: Some(16839),
            },
            StaticTrackObjSample {
                uid: 2,
                tile_x: 6080,
                tile_z: 14925,
                position: Vec3::new(101.0, 0.0, 201.0),
                section_idx: None,
                dist_any_m: 2.0,
                dist_matching_m: None,
                vector_id: None,
            },
            StaticTrackObjSample {
                uid: 3,
                tile_x: 6081,
                tile_z: 14925,
                position: Vec3::new(102.0, 0.0, 202.0),
                section_idx: Some(7),
                dist_any_m: 55.0,
                dist_matching_m: Some(55.0),
                vector_id: Some(99),
            },
        ];
        let worst = worst_static_trackobj_outliers(&samples);
        assert_eq!(worst.len(), 2);
        assert_eq!(worst[0].vector_id, Some(99));
        assert!((worst[0].dist_m - 55.0).abs() < 0.1);
        assert_eq!(worst[1].vector_id, Some(16839));
        assert!((worst[1].dist_matching_m.unwrap() - 8.0).abs() < 0.1);
    }

    #[test]
    fn worst_inter_node_gaps_sorted_and_capped() {
        let pairs = vec![
            InterNodeGapPair {
                vector_a: 1,
                vector_b: 2,
                via_junction: Some(99),
                gap_m: 0.0,
                junction_face_gap_m: Some(12.0),
            },
            InterNodeGapPair {
                vector_a: 3,
                vector_b: 4,
                via_junction: None,
                gap_m: 343.0,
                junction_face_gap_m: Some(38.0),
            },
            InterNodeGapPair {
                vector_a: 5,
                vector_b: 6,
                via_junction: Some(100),
                gap_m: 2.0,
                junction_face_gap_m: None,
            },
        ];
        let worst = worst_inter_node_gap_pairs(&pairs);
        assert_eq!(worst.len(), 1, "solo cuenta gap_m real (>=5m)");
        assert_eq!(worst[0].vector_a, 3);
        assert!((worst[0].gap_m - 343.0).abs() < 0.1);
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
        let chords = collect_tdb_chords(tdb, &focus, radius, None);
        let report = audit_track_dev(TrackDevAuditInput {
            tdb,
            scene: &scene,
            focus: &focus,
            offset,
            radius_m: radius,
            chords: &chords,
            route_dir: Some(route_dir),
        });
        report.log_detail();
        assert!(report.tdb_chords > 1000, "chords near Birmingham anchor");
    }
}
