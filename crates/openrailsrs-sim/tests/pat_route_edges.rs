//! PAT-derived route edges vs switch-aware BFS (Chiltern Birmingham).

use openrailsrs_formats::PathFile;
use openrailsrs_msts::path_placement::{pat_edge_path_with_offset, pat_waypoints_with_offset};
use openrailsrs_route::load_route_from_dir;
use openrailsrs_scenarios::model::RouteSection;
use openrailsrs_sim::path::{edge_path, resolve_route_edges};
use openrailsrs_track::SwitchPosition;

const MIN_PATH_EDGES: usize = 6;

fn birmingham_graph_and_pat() -> Option<(openrailsrs_track::TrackGraph, PathFile)> {
    let route_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !route_dir.join("track.toml").exists() {
        return None;
    }
    let pat = std::path::Path::new(
        "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/PATHS/RS_Let's go to Birmingham.pat",
    );
    if !pat.exists() {
        return None;
    }
    let loaded = load_route_from_dir(&route_dir).ok()?;
    let mut graph = loaded.graph;
    graph.set_switch("n10770", SwitchPosition::Diverging).ok()?;
    graph.set_switch("n10780", SwitchPosition::Straight).ok()?;
    let path_file = PathFile::from_path(pat).ok()?;
    Some((graph, path_file))
}

#[test]
fn chiltern_pat_waypoints_build_connected_chain() {
    let Some((graph, pat)) = birmingham_graph_and_pat() else {
        return;
    };
    let loaded = load_route_from_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern"),
    )
    .expect("graph");
    let wps =
        pat_waypoints_with_offset(&graph, &loaded.msts_aliases, &pat, "n3", "n10770", 305.576)
            .expect("waypoints");
    assert!(wps.len() >= 2);
    assert_eq!(wps.first().map(String::as_str), Some("n3"));
}

#[test]
fn chiltern_pat_edges_match_bfs_with_switches() {
    let Some((graph, pat)) = birmingham_graph_and_pat() else {
        return;
    };
    let loaded = load_route_from_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern"),
    )
    .expect("graph");
    let pat_edges =
        pat_edge_path_with_offset(&graph, &loaded.msts_aliases, &pat, "n3", "n10770", 305.576)
            .expect("pat path");
    let bfs = edge_path(&graph, "n3", "n10770").expect("bfs");
    assert_eq!(pat_edges, bfs);
    assert!(pat_edges.len() >= MIN_PATH_EDGES);
    assert!(pat_edges.contains(&"e10783".to_string()));
}

#[test]
fn resolve_route_edges_uses_waypoints_when_present() {
    let Some((graph, pat)) = birmingham_graph_and_pat() else {
        return;
    };
    let loaded = load_route_from_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern"),
    )
    .expect("graph");
    let waypoints =
        pat_waypoints_with_offset(&graph, &loaded.msts_aliases, &pat, "n3", "n10770", 305.576)
            .expect("wps");
    let route = RouteSection {
        path: ".".into(),
        start: "n3".into(),
        destination: "n10770".into(),
        start_offset_m: Some(305.576),
        stops: vec![],
        switches: vec![],
        waypoints,
        assume_signals_clear: false,
        edge_speed_limits: vec![],
    };
    let via = resolve_route_edges(&graph, &route).expect("resolve");
    let bfs = edge_path(&graph, "n3", "n10770").expect("bfs");
    assert_eq!(via, bfs);
}

#[test]
fn sim_runtime_path_has_birmingham_edges() {
    use openrailsrs_route::load_track_graph_from_route_dir;
    use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
    use openrailsrs_sim::path::resolve_route_edges;
    use openrailsrs_track::SwitchPosition;

    let chiltern = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }
    let scenario_path = chiltern.join("scenario.toml");
    let mut scenario = load_scenario(&scenario_path).expect("scenario");
    apply_scenario_runtime_overlay_dir(&mut scenario, &chiltern).expect("overlay");
    let mut graph = load_track_graph_from_route_dir(&chiltern).expect("graph");
    for sw in &scenario.route.switches {
        let pos = match sw.position {
            openrailsrs_scenarios::model::SwitchPositionDef::Straight => SwitchPosition::Straight,
            openrailsrs_scenarios::model::SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        graph.set_switch(&sw.node, pos).expect("switch");
    }
    let path = resolve_route_edges(&graph, &scenario.route).expect("path");
    eprintln!("sim runtime path: {} edges: {path:?}", path.len());
    assert!(path.len() >= 6, "expected full Birmingham path");
    assert!(path.contains(&"e10771".to_string()));
}
