//! Preserve ordered native PAT subpaths and their reversal/wait markers.
use crate::{
    MstsError,
    path_placement::{native_path_polylines, snap_world_to_edge, switches_for_edges},
};
use openrailsrs_formats::PathFile;
use openrailsrs_route::{MstsAlias, path::edge_path_ignoring_switches};
use openrailsrs_scenarios::model::RouteLeg;
use openrailsrs_track::TrackGraph;
use std::{collections::HashMap, path::Path};
pub fn operational_legs(
    graph: &TrackGraph,
    aliases: &HashMap<u32, MstsAlias>,
    pat: &Path,
    train_length: f64,
) -> Result<Vec<RouteLeg>, MstsError> {
    let file = PathFile::from_path(pat)?;
    if !file.node_flags.iter().any(|flags| flags & 3 != 0) {
        return Ok(vec![]);
    }
    if !train_length.is_finite() || train_length <= 0. {
        return Err(MstsError::msg(
            "Operational PAT requires the formation length",
        ));
    }
    let polylines = native_path_polylines(graph, aliases, pat)?;
    let boundaries = file
        .node_flags
        .iter()
        .enumerate()
        .filter(|(i, flags)| **flags & 3 != 0 && *i + 1 < file.pdps.len())
        .map(|(i, _)| i)
        .chain([file.pdps.len() - 1])
        .collect::<Vec<_>>();
    let mut first = 0;
    let mut legs = vec![];
    let mut reversed = false;
    for (index, last) in boundaries.into_iter().enumerate() {
        let points = file.pdps[first..=last]
            .iter()
            .filter_map(|p| p.world)
            .collect::<Vec<_>>();
        if points.len() < 2 {
            return Err(MstsError::msg(
                "Operational PAT subpath needs native world anchors",
            ));
        }
        let direction = |a: usize, b: usize| {
            Some((
                points[b].graph_x_m() - points[a].graph_x_m(),
                points[b].graph_z_m() - points[a].graph_z_m(),
            ))
        };
        let (start, mut start_offset, _) = snap_world_to_edge(
            graph,
            points[0].graph_x_m(),
            points[0].graph_z_m(),
            direction(0, 1),
            polylines.as_ref(),
        )?;
        let (end_from, end_offset, destination) = snap_world_to_edge(
            graph,
            points[points.len() - 1].graph_x_m(),
            points[points.len() - 1].graph_z_m(),
            direction(points.len() - 2, points.len() - 1),
            polylines.as_ref(),
        )?;
        if index == 0 || file.node_flags[first] & 1 != 0 {
            start_offset += train_length;
        }
        let mut edges = vec![];
        let mut current = start.clone();
        for point in file.pdps[first..=last]
            .iter()
            .skip(1)
            .take(last - first - 1)
            .filter(|p| p.is_junction())
            .filter_map(|p| p.world.as_ref())
        {
            // Snap to the authored vector in its local direction, not a
            // neighbouring junction on a parallel track.
            let nearest = graph
                .nodes_iter()
                .min_by(|(_, a), (_, b)| {
                    let d = |n: &openrailsrs_track::Node| {
                        (n.x_m - point.graph_x_m()).powi(2) + (n.y_m - point.graph_z_m()).powi(2)
                    };
                    d(a).total_cmp(&d(b))
                })
                .map(|(id, _)| id.to_string())
                .ok_or_else(|| MstsError::msg("Empty graph"))?;
            if nearest != current
                && let Ok(hop) = edge_path_ignoring_switches(graph, &current, &nearest)
            {
                edges.extend(hop);
                current = nearest;
            }
        }
        if current != end_from {
            edges.extend(
                edge_path_ignoring_switches(graph, &current, &end_from)
                    .map_err(|e| MstsError::msg(e.to_string()))?,
            );
        }
        let end_edge = graph
            .outgoing_edges(&end_from)
            .iter()
            .find(|id| graph.edge(id).is_some_and(|e| e.to.0 == destination))
            .ok_or_else(|| MstsError::msg("PAT end vector disconnected"))?
            .clone();
        edges.push(end_edge);
        let mut end_chainage = edges[..edges.len() - 1]
            .iter()
            .filter_map(|e| graph.edge(e))
            .map(|e| e.length_m)
            .sum::<f64>()
            + end_offset;
        if end_chainage < start_offset {
            // A short following subpath can end at a marker already under the
            // formation. That leg has no remaining forward travel: retain the
            // occupied footprint and its wait, including intermediate legs.
            // The runtime still verifies every car before changing subpaths.
            if index > 0 && end_chainage + train_length >= start_offset {
                end_chainage = start_offset;
            } else {
                return Err(MstsError::msg(format!(
                    "Operational PAT leg {index} ends at {end_chainage:.1} m before its head at {start_offset:.1} m"
                )));
            }
        }
        let switches = switches_for_edges(graph, &edges);
        legs.push(RouteLeg {
            operations: vec![],
            service_id: None,
            not_before_s: None,
            start,
            destination,
            edges,
            start_offset_m: start_offset,
            end_chainage_m: end_chainage,
            reversed,
            wait_s: f64::from(file.node_flags.get(last).copied().unwrap_or(0) >> 16),
            stops: vec![],
            switches,
        });
        reversed ^= file.node_flags[last] & 1 != 0;
        first = last;
    }
    preserve_waiting_formation_prefixes(graph, &mut legs, train_length);
    // OR Train.CheckEndOfRoutePosition requires the rear traveller to clear
    // the last diverging junction before switching subpaths. A marker just
    // beyond that junction is insufficient for a long vehicle or formation.
    for i in 0..legs.len().saturating_sub(1) {
        if legs[i].reversed == legs[i + 1].reversed {
            continue;
        }
        let next = legs[i + 1]
            .edges
            .iter()
            .map(|id| id.trim_end_matches("_r"))
            .collect::<std::collections::HashSet<_>>();
        let mut common_start = legs[i]
            .edges
            .iter()
            .filter_map(|id| graph.edge(id))
            .map(|e| e.length_m)
            .sum::<f64>();
        let total = common_start;
        for id in legs[i].edges.iter().rev() {
            if !next.contains(id.trim_end_matches("_r")) {
                break;
            }
            common_start -= graph.edge(id).unwrap().length_m;
        }
        let cleared = common_start + train_length + 10.0;
        if common_start < total && cleared < total && cleared - legs[i].end_chainage_m <= 50.0 {
            legs[i].end_chainage_m = legs[i].end_chainage_m.max(cleared);
        }
    }
    Ok(legs)
}

/// Waiting points split an authored path without exchanging its travelling
/// ends. The next subpath must retain the vectors under the rear traveller,
/// even when the wait marker lies less than one formation length into its
/// first vector. Reuse only the preceding directed path; never invent a route
/// through a nearby or parallel track.
fn preserve_waiting_formation_prefixes(
    graph: &TrackGraph,
    legs: &mut [RouteLeg],
    train_length: f64,
) {
    for index in 1..legs.len() {
        let (previous, current) = legs.split_at_mut(index);
        let previous = &previous[index - 1];
        let current = &mut current[0];
        // Runtime accepts stopping up to 10 m before the authored wait marker.
        // Keep the rear traveller in the following path throughout that margin.
        let required_prefix = train_length + 10.;
        if previous.reversed != current.reversed || current.start_offset_m >= required_prefix {
            continue;
        }
        let Some(first) = current.edges.first() else {
            continue;
        };
        let Some(join) = previous.edges.iter().rposition(|id| id == first) else {
            continue;
        };
        let mut begin = join;
        let mut extra_length = 0.;
        while begin > 0 && current.start_offset_m + extra_length < required_prefix {
            begin -= 1;
            extra_length += graph.edge(&previous.edges[begin]).unwrap().length_m;
        }
        if begin == join {
            continue;
        }
        let mut edges = previous.edges[begin..join].to_vec();
        edges.append(&mut current.edges);
        current.start = graph.edge(&edges[0]).unwrap().from.0.clone();
        current.edges = edges;
        current.start_offset_m += extra_length;
        current.end_chainage_m += extra_length;
        current.switches = switches_for_edges(graph, &current.edges);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind};

    fn graph() -> TrackGraph {
        let mut graph = TrackGraph::new();
        for index in 0..5 {
            graph
                .insert_node(Node {
                    id: NodeId(format!("n{index}")),
                    kind: NodeKind::Plain,
                    x_m: index as f64 * 100.,
                    y_m: 0.,
                })
                .unwrap();
        }
        for index in 0..4 {
            graph
                .insert_edge(Edge {
                    id: EdgeId(format!("e{index}")),
                    from: NodeId(format!("n{index}")),
                    to: NodeId(format!("n{}", index + 1)),
                    length_m: 100.,
                    speed_limit_mps: 10.,
                    grade_percent: 0.,
                })
                .unwrap();
        }
        graph
    }
    fn leg(edges: &[&str], start_offset_m: f64, end_chainage_m: f64) -> RouteLeg {
        RouteLeg {
            start: "n2".into(),
            destination: "n4".into(),
            edges: edges.iter().map(|id| (*id).into()).collect(),
            start_offset_m,
            end_chainage_m,
            reversed: false,
            wait_s: 0.,
            stops: vec![],
            switches: vec![],
            operations: vec![],
            service_id: None,
            not_before_s: None,
        }
    }
    #[test]
    fn waiting_point_keeps_the_whole_long_formation_on_its_original_vectors() {
        let graph = graph();
        let mut legs = [
            leg(&["e0", "e1", "e2"], 180., 220.),
            leg(&["e2", "e3"], 20., 160.),
        ];
        preserve_waiting_formation_prefixes(&graph, &mut legs, 180.);
        assert_eq!(legs[1].edges, ["e0", "e1", "e2", "e3"]);
        assert_eq!(legs[1].start, "n0");
        assert_eq!(legs[1].start_offset_m, 220.);
        assert_eq!(legs[1].end_chainage_m, 360.);
        // Both travelling ends retain their absolute positions. Only the
        // coordinate origin of the following subpath has changed.
        assert_eq!(legs[0].end_chainage_m, legs[1].start_offset_m);
        assert_eq!(legs[0].end_chainage_m - 180., legs[1].start_offset_m - 180.);
    }
    #[test]
    fn wait_prefix_contains_the_rear_when_stopped_before_the_marker() {
        let graph = graph();
        let mut legs = [
            leg(&["e0", "e1", "e2"], 180., 280.),
            leg(&["e2", "e3"], 80., 160.),
        ];
        preserve_waiting_formation_prefixes(&graph, &mut legs, 180.);
        assert_eq!(legs[1].edges, ["e0", "e1", "e2", "e3"]);
        assert_eq!(legs[1].start_offset_m, 280.);
        assert!(legs[1].start_offset_m - 10. - 180. >= 0.);
    }

    #[test]
    fn a_reversal_or_a_disconnected_wait_path_never_borrows_a_directed_prefix() {
        let graph = graph();
        let previous = leg(&["e0", "e1", "e2"], 180., 220.);
        let mut reversed = leg(&["e2", "e3"], 20., 160.);
        reversed.reversed = true;
        let mut legs = [previous.clone(), reversed];
        preserve_waiting_formation_prefixes(&graph, &mut legs, 180.);
        assert_eq!(legs[1].edges, ["e2", "e3"]);
        let mut legs = [previous, leg(&["e3"], 20., 80.)];
        preserve_waiting_formation_prefixes(&graph, &mut legs, 180.);
        assert_eq!(legs[1].edges, ["e3"]);
    }
}
