//! Switch-aware pathfinding — delegates to [`openrailsrs_route::path`].

use openrailsrs_route::path::edge_path as route_edge_path;
use openrailsrs_scenarios::model::RouteSection;
use openrailsrs_track::TrackGraph;

use crate::SimError;

pub fn edge_path(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, SimError> {
    route_edge_path(graph, start, destination).map_err(|e| SimError::Msg(e.to_string()))
}

/// Resolve the edge sequence for a scenario route.
///
/// When `route.waypoints` has ≥2 entries (from MSTS `.pat` import), follows that
/// ordered node list; if the last waypoint is not `destination`, appends a
/// switch-aware BFS hop to the destination.
pub fn resolve_route_edges(
    graph: &TrackGraph,
    route: &RouteSection,
) -> Result<Vec<String>, SimError> {
    if route.waypoints.len() >= 2 {
        let mut edges = openrailsrs_route::path::edge_path_via_waypoints(graph, &route.waypoints)
            .map_err(|e| SimError::Msg(e.to_string()))?;
        if route.waypoints.last().map(String::as_str) != Some(route.destination.as_str()) {
            let tail_from = route
                .waypoints
                .last()
                .ok_or_else(|| SimError::Msg("empty waypoints".into()))?;
            let tail = edge_path(graph, tail_from, &route.destination)?;
            edges.extend(tail);
        }
        Ok(edges)
    } else {
        edge_path(graph, &route.start, &route.destination)
    }
}

#[cfg(test)]
mod chiltern_path_smoke {
    use super::*;
    use openrailsrs_route::load_track_graph_from_route_dir;

    #[test]
    fn chiltern_paths_from_paddington_area() {
        let route =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route.join("track.toml").exists() {
            return;
        }
        let g = load_track_graph_from_route_dir(&route).expect("graph");
        assert!(edge_path(&g, "n5", "n10780").is_ok());
        assert!(edge_path(&g, "n5", "n3").is_ok());
    }
}
