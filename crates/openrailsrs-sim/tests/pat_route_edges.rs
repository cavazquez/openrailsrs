//! PAT-derived route edges (Chiltern Birmingham — TrackPDP world placement).

use openrailsrs_formats::PathFile;
use openrailsrs_msts::path_placement::{
    pat_edge_path_with_offset, pat_waypoints_with_offset, placement_from_imported_route,
};
use openrailsrs_route::load_route_from_dir;
use openrailsrs_scenarios::model::RouteSection;
use openrailsrs_sim::path::{edge_path, resolve_route_edges};
use openrailsrs_track::SwitchPosition;

fn birmingham_graph_pat_and_hints() -> Option<(
    openrailsrs_track::TrackGraph,
    PathFile,
    openrailsrs_msts::path_placement::RouteHints,
    std::collections::HashMap<u32, openrailsrs_route::MstsAlias>,
)> {
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
    let hints = placement_from_imported_route(&route_dir, pat, 194.424).ok()?;
    let loaded = load_route_from_dir(&route_dir).ok()?;
    let mut graph = loaded.graph;
    for sw in &hints.switches {
        let pos = match sw.position {
            openrailsrs_scenarios::model::SwitchPositionDef::Straight => SwitchPosition::Straight,
            openrailsrs_scenarios::model::SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        let _ = graph.set_switch(&sw.node, pos);
    }
    let path_file = PathFile::from_path(pat).ok()?;
    Some((graph, path_file, hints, loaded.msts_aliases))
}

#[test]
fn chiltern_pat_waypoints_build_connected_chain() {
    let Some((graph, pat, hints, aliases)) = birmingham_graph_pat_and_hints() else {
        return;
    };
    let wps = pat_waypoints_with_offset(
        &graph,
        &aliases,
        &pat,
        &hints.start,
        &hints.destination,
        hints.start_offset_m,
    )
    .expect("waypoints");
    assert!(
        wps.len() >= 10,
        "expected long PAT waypoint chain after reverse edges, got {}",
        wps.len()
    );
    assert_eq!(wps.first().map(String::as_str), Some(hints.start.as_str()));
    assert_eq!(hints.start, "n17368");
    assert_ne!(hints.destination, "n17381", "must not stop at the 3-node stub sink");
}

#[test]
fn chiltern_pat_edges_leave_platform_via_reverse() {
    let Some((graph, pat, hints, aliases)) = birmingham_graph_pat_and_hints() else {
        return;
    };
    let pat_edges = pat_edge_path_with_offset(
        &graph,
        &aliases,
        &pat,
        &hints.start,
        &hints.destination,
        hints.start_offset_m,
    )
    .expect("pat path");
    assert!(
        pat_edges.len() > 5,
        "expected ≫ stub path, got {} edges",
        pat_edges.len()
    );
    let dist: f64 = pat_edges
        .iter()
        .filter_map(|e| graph.edge(e).map(|ed| ed.length_m))
        .sum();
    assert!(
        dist > 5_000.0,
        "expected ≥5 km PAT corridor, got {dist:.0} m"
    );
    assert!(
        pat_edges.iter().any(|e| e == "e17466_r"),
        "outbound must continue via reverse of e17466, got {:?}",
        &pat_edges[..pat_edges.len().min(8)]
    );
    // Global BFS may shortcut the PAT; waypoint resolution must stay connected.
    assert!(edge_path(&graph, &hints.start, &hints.destination).is_ok());
}

#[test]
fn resolve_route_edges_uses_waypoints_when_present() {
    let Some((graph, pat, hints, aliases)) = birmingham_graph_pat_and_hints() else {
        return;
    };
    let waypoints = pat_waypoints_with_offset(
        &graph,
        &aliases,
        &pat,
        &hints.start,
        &hints.destination,
        hints.start_offset_m,
    )
    .expect("wps");
    let route = RouteSection {
        path: ".".into(),
        start: hints.start.clone(),
        destination: hints.destination.clone(),
        start_offset_m: Some(hints.start_offset_m),
        stops: vec![],
        switches: hints.switches.clone(),
        waypoints,
        assume_signals_clear: false,
        edge_speed_limits: vec![],
    };
    let via = resolve_route_edges(&graph, &route).expect("resolve");
    assert!(via.len() > 5);
    assert!(via.iter().any(|e| e == "e17466_r"));
    let pat_edges = pat_edge_path_with_offset(
        &graph,
        &aliases,
        &pat,
        &hints.start,
        &hints.destination,
        hints.start_offset_m,
    )
    .expect("pat edges");
    assert_eq!(via, pat_edges);
}

#[test]
fn sim_runtime_path_uses_scenario_spawn() {
    use openrailsrs_route::load_track_graph_from_route_dir;
    use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
    use openrailsrs_sim::path::resolve_route_edges;

    let chiltern = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }
    let scenario_path = chiltern.join("scenario.toml");
    let mut scenario = load_scenario(&scenario_path).expect("scenario");
    apply_scenario_runtime_overlay_dir(&mut scenario, &chiltern).expect("overlay");
    assert_eq!(scenario.route.start, "n17368");
    let mut graph = load_track_graph_from_route_dir(&chiltern).expect("graph");
    for sw in &scenario.route.switches {
        let pos = match sw.position {
            openrailsrs_scenarios::model::SwitchPositionDef::Straight => SwitchPosition::Straight,
            openrailsrs_scenarios::model::SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        graph.set_switch(&sw.node, pos).expect("switch");
    }
    let path = resolve_route_edges(&graph, &scenario.route).expect("path");
    eprintln!("sim runtime path: {} edges", path.len());
    assert!(
        path.len() > 5,
        "expected long path from corrected spawn, got {} edges",
        path.len()
    );
    assert!(path.contains(&"e17369".to_string()));
    assert!(
        path.contains(&"e17466_r".to_string()),
        "live-drive must leave platform via e17466_r"
    );
}

#[test]
fn live_drive_session_from_chiltern_scenario() {
    use openrailsrs_sim::LiveDriveSession;
    let scenario_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/scenario.toml");
    let scenario_dir = scenario_path.parent().unwrap();
    let scenario = openrailsrs_scenarios::load_scenario(&scenario_path).expect("scenario");
    assert!(
        scenario.route.waypoints.len() >= 2,
        "waypoints missing: {}",
        scenario.route.waypoints.len()
    );
    LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
}

/// Viewer `graph_start_position` must not call bare BFS: default switch layout
/// yields `no path from n17368 to n5158` even though reverse edges exist.
#[test]
fn resolve_scenario_route_edges_works_on_raw_graph() {
    use openrailsrs_route::load_track_graph_from_route_dir;
    use openrailsrs_scenarios::load_scenario;
    use openrailsrs_sim::path::{edge_path, resolve_scenario_route_edges};

    let chiltern = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }
    let scenario = load_scenario(chiltern.join("scenario.toml")).expect("scenario");
    let graph = load_track_graph_from_route_dir(&chiltern).expect("graph");
    assert!(
        edge_path(&graph, &scenario.route.start, &scenario.route.destination).is_err(),
        "bare BFS should fail without applying scenario switches"
    );
    let path = resolve_scenario_route_edges(&graph, &scenario.route).expect("scenario path");
    assert!(path.len() > 5);
    assert!(path.contains(&"e17466_r".to_string()));
}
