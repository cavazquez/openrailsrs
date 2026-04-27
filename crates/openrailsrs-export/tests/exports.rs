use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_export::{
    textual_replay_from_csv, track_graph_to_ascii, track_graph_to_dot, track_graph_to_geojson,
};
use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

fn graph() -> TrackGraph {
    let mut g = TrackGraph::new();
    g.insert_node(Node {
        id: NodeId("a".into()),
        kind: NodeKind::Plain,
        x_m: 0.0,
        y_m: 0.0,
    })
    .unwrap();
    g.insert_node(Node {
        id: NodeId("b".into()),
        kind: NodeKind::Station { name: "B".into() },
        x_m: 10.0,
        y_m: 0.0,
    })
    .unwrap();
    g.insert_edge(Edge {
        id: EdgeId("e1".into()),
        from: NodeId("a".into()),
        to: NodeId("b".into()),
        length_m: 10.0,
        speed_limit_mps: 20.0,
        grade_percent: 0.0,
    })
    .unwrap();
    g
}

#[test]
fn dot_geojson_ascii_are_non_empty() {
    let g = graph();
    let dot = track_graph_to_dot(&g);
    assert!(dot.contains("digraph"));
    assert!(dot.contains("e1"));

    let geo = track_graph_to_geojson(&g);
    assert_eq!(geo["type"], "FeatureCollection");
    assert_eq!(geo["features"].as_array().unwrap().len(), 1);

    let ascii = track_graph_to_ascii(&g, 12, 4);
    assert!(ascii.contains('#'));
    assert!(ascii.contains('O'));
}

#[test]
fn textual_replay_reads_csv() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("run.csv");
    std::fs::write(
        &p,
        "time_s,edge_id,pos_on_edge_m,velocity_mps,odometer_m,cumulative_energy_kwh,throttle,brake\n0.0,e1,0,0,0,0,0,0\n",
    )
    .unwrap();
    let s = textual_replay_from_csv(&p, 5).unwrap();
    assert!(s.contains("textual replay"));
    assert!(s.contains("e1"));
}
