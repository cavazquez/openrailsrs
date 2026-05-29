//! Chiltern route path sanity (skipped when `examples/chiltern/track.toml` is absent).

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_sim::path::edge_path;
use openrailsrs_track::SwitchPosition;

/// Minimum edges on n3 → n10770 with Birmingham Pullman switch overrides.
const MIN_PATH_EDGES: usize = 6;

#[test]
fn chiltern_path_reaches_beyond_local_switch_back() {
    let route_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !route_dir.join("track.toml").exists() {
        return;
    }
    let mut g = load_track_graph_from_route_dir(&route_dir).unwrap();
    g.set_switch("n10770", SwitchPosition::Diverging).unwrap();
    g.set_switch("n10780", SwitchPosition::Straight).unwrap();
    let path = edge_path(&g, "n3", "n10770").expect("path");
    assert!(
        path.len() >= MIN_PATH_EDGES,
        "expected at least {MIN_PATH_EDGES} edges on Birmingham path, got {}: {path:?}",
        path.len()
    );
    assert!(
        path.contains(&"e10783".to_string()),
        "path must start via Paddington edge e10783"
    );
    assert!(
        path.contains(&"e10771".to_string()),
        "path must reach destination approach e10771, got: {path:?}"
    );
}

#[test]
fn chiltern_birmingham_stop_nodes_on_path() {
    let route_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !route_dir.join("track.toml").exists() {
        return;
    }
    let mut g = load_track_graph_from_route_dir(&route_dir).unwrap();
    g.set_switch("n10770", SwitchPosition::Diverging).unwrap();
    g.set_switch("n10780", SwitchPosition::Straight).unwrap();
    let path = edge_path(&g, "n3", "n10770").expect("path");
    let mut nodes_on_path = std::collections::HashSet::new();
    for eid in &path {
        if let Some(e) = g.edge(eid) {
            nodes_on_path.insert(e.to.0.clone());
        }
    }
    for stop in ["n10778", "n10770"] {
        assert!(
            nodes_on_path.contains(stop),
            "stop node {stop} must lie on Birmingham path"
        );
    }
}
