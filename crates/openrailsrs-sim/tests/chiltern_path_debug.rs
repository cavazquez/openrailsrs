//! Chiltern route path sanity (skipped when `examples/chiltern/track.toml` is absent).

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::model::{RouteSection, SwitchDef, SwitchPositionDef};
use openrailsrs_sim::path::resolve_route_edges;
use openrailsrs_track::SwitchPosition;

/// Minimum edges on the historic n3 → n10770 corridor (OR-P6 / brake-coast).
const MIN_PATH_EDGES: usize = 6;

fn brake_coast_route() -> RouteSection {
    RouteSection {
        path: ".".into(),
        start: "n3".into(),
        destination: "n10770".into(),
        start_offset_m: Some(305.576),
        stops: vec![],
        switches: vec![
            SwitchDef {
                node: "n10770".into(),
                position: SwitchPositionDef::Diverging,
            },
            SwitchDef {
                node: "n10780".into(),
                position: SwitchPositionDef::Straight,
            },
        ],
        // After #127 reverse edges, hop-count BFS prefers n3→n5 via e4_r.
        waypoints: vec![
            "n3".into(),
            "n10780".into(),
            "n10778".into(),
            "n10776".into(),
            "n10770".into(),
        ],
        assume_signals_clear: false,
        edge_speed_limits: vec![],
    }
}

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
    let path = resolve_route_edges(&g, &brake_coast_route()).expect("path");
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
    let path = resolve_route_edges(&g, &brake_coast_route()).expect("path");
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
