//! Integration tests for switch-aware BFS pathfinding.
//!
//! Graph topology:
//!
//! ```
//!   start --e1--> junction --e2(straight)--> dest_a
//!                          \-e3(diverging)--> dest_b
//! ```

use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_sim::path::edge_path;
use openrailsrs_track::{Edge, Node, NodeKind, SwitchPosition, TrackGraph};

/// Build the forked test graph with a switch node at `junction`.
///
/// `junction` stem_edge = "e2", diverging_edge = "e3".
fn fork_graph(switch_pos: SwitchPosition) -> TrackGraph {
    let mut g = TrackGraph::new();

    for (id, x) in [
        ("start", 0.0),
        ("junction", 1000.0),
        ("dest_a", 2000.0),
        ("dest_b", 2000.0),
    ] {
        let kind = if id == "junction" {
            NodeKind::Switch {
                stem_edge: EdgeId("e2".into()),
                diverging_edge: EdgeId("e3".into()),
            }
        } else {
            NodeKind::Plain
        };
        g.insert_node(Node {
            id: NodeId(id.into()),
            kind,
            x_m: x,
            y_m: 0.0,
        })
        .unwrap();
    }

    for (id, from, to) in [
        ("e1", "start", "junction"),
        ("e2", "junction", "dest_a"),
        ("e3", "junction", "dest_b"),
    ] {
        g.insert_edge(Edge {
            id: EdgeId(id.into()),
            from: NodeId(from.into()),
            to: NodeId(to.into()),
            length_m: 1000.0,
            speed_limit_mps: 20.0,
            grade_percent: 0.0,
        })
        .unwrap();
    }

    g.set_switch("junction", switch_pos).unwrap();
    g
}

#[test]
fn straight_routes_to_dest_a() {
    let g = fork_graph(SwitchPosition::Straight);
    let path = edge_path(&g, "start", "dest_a").expect("path found");
    assert_eq!(path, vec!["e1", "e2"]);
}

#[test]
fn diverging_routes_to_dest_b() {
    let g = fork_graph(SwitchPosition::Diverging);
    let path = edge_path(&g, "start", "dest_b").expect("path found");
    assert_eq!(path, vec!["e1", "e3"]);
}

#[test]
fn straight_cannot_reach_dest_b() {
    let g = fork_graph(SwitchPosition::Straight);
    let result = edge_path(&g, "start", "dest_b");
    assert!(result.is_err(), "straight switch should not reach dest_b");
}

#[test]
fn diverging_cannot_reach_dest_a() {
    let g = fork_graph(SwitchPosition::Diverging);
    let result = edge_path(&g, "start", "dest_a");
    assert!(result.is_err(), "diverging switch should not reach dest_a");
}

#[test]
fn smoke_route_with_switch_reaches_yard_b() {
    // Verify the smoke track.toml (with junction switch in straight) still routes to yard_b.
    use openrailsrs_route::load_track_graph_from_route_dir;
    let route_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
    let g = load_track_graph_from_route_dir(&route_dir).expect("load route");
    let path = edge_path(&g, "yard_a", "yard_b").expect("path to yard_b");
    // Should go through e1 -> e2 -> e3 (junction straight branch).
    assert_eq!(path, vec!["e1", "e2", "e3"]);
}

#[test]
fn smoke_route_diverging_reaches_siding_c() {
    // When we set junction to diverging, the path leads to siding_c instead.
    use openrailsrs_route::load_track_graph_from_route_dir;
    let route_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
    let mut g = load_track_graph_from_route_dir(&route_dir).expect("load route");
    g.set_switch("junction", SwitchPosition::Diverging).unwrap();
    let path = edge_path(&g, "yard_a", "siding_c").expect("path to siding_c");
    assert_eq!(path, vec!["e1", "e2", "e4"]);
}
