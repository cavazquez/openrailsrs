//! Switch-aware pathfinding — delegates to [`openrailsrs_route::path`].

use openrailsrs_track::TrackGraph;

use crate::SimError;

pub fn edge_path(
    graph: &TrackGraph,
    start: &str,
    destination: &str,
) -> Result<Vec<String>, SimError> {
    openrailsrs_route::path::edge_path(graph, start, destination)
        .map_err(|e| SimError::Msg(e.to_string()))
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
