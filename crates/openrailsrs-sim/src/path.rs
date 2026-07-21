//! Switch-aware pathfinding — delegates to [`openrailsrs_route::path`].

use openrailsrs_route::path::edge_path as route_edge_path;
use openrailsrs_scenarios::model::{RouteSection, SwitchPositionDef};
use openrailsrs_track::{SwitchPosition, TrackGraph};

use crate::SimError;

pub fn edge_path(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, SimError> {
    route_edge_path(graph, start, destination).map_err(|e| SimError::Msg(e.to_string()))
}

/// Apply `[route.switches]` positions onto `graph`.
pub fn apply_route_switches(graph: &mut TrackGraph, route: &RouteSection) -> Result<(), SimError> {
    for sw in &route.switches {
        let pos = match sw.position {
            SwitchPositionDef::Straight => SwitchPosition::Straight,
            SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        graph
            .set_switch(&sw.node, pos)
            .map_err(|e| SimError::Msg(e.to_string()))?;
    }
    Ok(())
}

/// Resolve the edge sequence for a scenario route.
///
/// When `route.waypoints` has ≥2 entries (from MSTS `.pat` import), follows that
/// ordered node list; if the last waypoint is not `destination`, appends a
/// switch-aware BFS hop to the destination.
///
/// Callers that have not yet applied [`apply_route_switches`] should use
/// [`resolve_scenario_route_edges`] instead — plain BFS on the default switch
/// layout often fails on MSTS corridors (e.g. Chiltern Paddington → Birmingham).
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

/// Clone `graph`, apply scenario switches, then [`resolve_route_edges`].
///
/// Safe for read-only viewers that must not mutate the shared track scene.
pub fn resolve_scenario_route_edges(
    graph: &TrackGraph,
    route: &RouteSection,
) -> Result<Vec<String>, SimError> {
    let mut g = graph.clone();
    apply_route_switches(&mut g, route)?;
    resolve_route_edges(&g, route)
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
        // Trailing leg e10783 stays open at n3 under default switch (stem e4_r).
        assert!(edge_path(&g, "n3", "n10780").is_ok());
        // Reverse of e4: n3 → n5 (stem when n3 is Straight).
        assert!(edge_path(&g, "n3", "n5").is_ok());
    }
}
