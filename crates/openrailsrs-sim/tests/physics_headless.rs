use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_sim::{physics::step, state::TrainSimState};
use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

fn flat_line_graph() -> TrackGraph {
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
        kind: NodeKind::Plain,
        x_m: 1.0,
        y_m: 0.0,
    })
    .unwrap();
    g.insert_edge(Edge {
        id: EdgeId("e1".into()),
        from: NodeId("a".into()),
        to: NodeId("b".into()),
        length_m: 100_000.0,
        speed_limit_mps: 100.0,
        grade_percent: 0.0,
    })
    .unwrap();
    g
}

#[test]
fn acceleration_increases_speed_on_flat_track() {
    let g = flat_line_graph();
    let mut st = TrainSimState::new(vec!["e1".into()]);
    st.throttle = 1.0;
    st.brake = 0.0;
    let v0 = st.velocity_mps;
    for _ in 0..200 {
        let _ = step(
            &mut st,
            &g,
            100_000.0,
            2_000_000.0,
            350_000.0,
            400_000.0,
            0.1,
        );
    }
    assert!(
        st.velocity_mps > v0 + 1.0,
        "expected acceleration, got v={}",
        st.velocity_mps
    );
}

#[test]
fn braking_reduces_speed() {
    let g = flat_line_graph();
    let mut st = TrainSimState::new(vec!["e1".into()]);
    st.velocity_mps = 25.0;
    st.throttle = 0.0;
    st.brake = 1.0;
    let v0 = st.velocity_mps;
    for _ in 0..500 {
        let _ = step(
            &mut st,
            &g,
            100_000.0,
            2_000_000.0,
            350_000.0,
            400_000.0,
            0.05,
        );
    }
    assert!(
        st.velocity_mps < v0 - 5.0,
        "expected braking, got v={}",
        st.velocity_mps
    );
}
