use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_track::{Edge, Node, NodeKind, TrackError, TrackGraph};

#[test]
fn rejects_duplicate_node_and_missing_edge_nodes() {
    let mut g = TrackGraph::new();
    g.insert_node(Node {
        id: NodeId("a".into()),
        kind: NodeKind::Plain,
        x_m: 0.0,
        y_m: 0.0,
    })
    .unwrap();

    let dup = g.insert_node(Node {
        id: NodeId("a".into()),
        kind: NodeKind::Plain,
        x_m: 1.0,
        y_m: 1.0,
    });
    assert!(matches!(dup, Err(TrackError::DuplicateId(_))));

    let bad_edge = g.insert_edge(Edge {
        id: EdgeId("e1".into()),
        from: NodeId("a".into()),
        to: NodeId("missing".into()),
        length_m: 10.0,
        speed_limit_mps: 10.0,
        grade_percent: 0.0,
    });
    assert!(matches!(bad_edge, Err(TrackError::UnknownNode(_))));
}

#[test]
fn outgoing_edges_are_recorded() {
    let mut g = TrackGraph::new();
    for id in ["a", "b", "c"] {
        g.insert_node(Node {
            id: NodeId(id.into()),
            kind: NodeKind::Plain,
            x_m: 0.0,
            y_m: 0.0,
        })
        .unwrap();
    }
    g.insert_edge(Edge {
        id: EdgeId("e1".into()),
        from: NodeId("a".into()),
        to: NodeId("b".into()),
        length_m: 10.0,
        speed_limit_mps: 10.0,
        grade_percent: 0.0,
    })
    .unwrap();
    g.insert_edge(Edge {
        id: EdgeId("e2".into()),
        from: NodeId("a".into()),
        to: NodeId("c".into()),
        length_m: 10.0,
        speed_limit_mps: 10.0,
        grade_percent: 0.0,
    })
    .unwrap();

    assert_eq!(g.outgoing_edges("a"), ["e1".to_string(), "e2".to_string()]);
}
