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
            // A legacy PAT may finish immediately after a reversal with its
            // endpoint already under the formation. Its front has passed that
            // endpoint; preserve the physical train rather than moving it back.
            if index > 0 && reversed && last + 1 == file.pdps.len() {
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
