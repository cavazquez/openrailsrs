//! Verify that `TrackGraph::evaluate_signals` correctly updates signal aspects
//! based on block occupancy.
//!
//! Graph layout (linear, 3 edges):
//!
//!   A ──[e1]──> B ──[e2]──> C ──[e3]──> D
//!
//! Signal `s1` sits on `e1` (just before node B).  Its script says:
//!   - on_block_ahead (e2 occupied)   → Stop
//!   - on_second_block_ahead (e3 occ) → Caution
//!   - default                         → Clear

use std::collections::HashMap;

use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_track::{
    Edge, Node, NodeKind, SignalAspect, SignalScript, TrackGraph, TrackSignal,
};

fn build_graph() -> TrackGraph {
    let mut g = TrackGraph::new();
    for id in ["A", "B", "C", "D"] {
        g.insert_node(Node {
            id: NodeId(id.to_string()),
            kind: NodeKind::Plain,
            x_m: 0.0,
            y_m: 0.0,
        })
        .unwrap();
    }
    for (id, from, to) in [("e1", "A", "B"), ("e2", "B", "C"), ("e3", "C", "D")] {
        g.insert_edge(Edge {
            id: EdgeId(id.to_string()),
            from: NodeId(from.to_string()),
            to: NodeId(to.to_string()),
            length_m: 500.0,
            speed_limit_mps: 30.0,
            grade_percent: 0.0,
        })
        .unwrap();
    }
    // Signal on e1, scripted.
    g.insert_signal(TrackSignal {
        id: "s1".to_string(),
        edge_id: "e1".to_string(),
        position_m: 400.0,
        aspect: SignalAspect::Clear,
        clear_after_s: None,
        script: Some(SignalScript {
            on_block_ahead: Some(SignalAspect::Stop),
            on_second_block_ahead: Some(SignalAspect::Caution),
            default: Some(SignalAspect::Clear),
        }),
    })
    .unwrap();
    g
}

#[test]
fn all_clear_when_no_trains() {
    let mut g = build_graph();
    let block_map = HashMap::new();
    g.evaluate_signals(&block_map);
    assert_eq!(g.signal("s1").unwrap().aspect, SignalAspect::Clear);
}

#[test]
fn stop_when_block_ahead_occupied() {
    let mut g = build_graph();
    let mut block_map = HashMap::new();
    block_map.insert("e2".to_string(), "train-1".to_string()); // block immediately ahead
    g.evaluate_signals(&block_map);
    assert_eq!(
        g.signal("s1").unwrap().aspect,
        SignalAspect::Stop,
        "signal should show Stop when block ahead (e2) is occupied"
    );
}

#[test]
fn caution_when_second_block_occupied() {
    let mut g = build_graph();
    let mut block_map = HashMap::new();
    block_map.insert("e3".to_string(), "train-2".to_string()); // second block ahead
    g.evaluate_signals(&block_map);
    assert_eq!(
        g.signal("s1").unwrap().aspect,
        SignalAspect::Caution,
        "signal should show Caution when second block (e3) is occupied"
    );
}

#[test]
fn stop_takes_priority_over_caution() {
    let mut g = build_graph();
    let mut block_map = HashMap::new();
    block_map.insert("e2".to_string(), "train-1".to_string()); // first block
    block_map.insert("e3".to_string(), "train-2".to_string()); // second block
    g.evaluate_signals(&block_map);
    assert_eq!(
        g.signal("s1").unwrap().aspect,
        SignalAspect::Stop,
        "Stop rule has higher priority than Caution rule"
    );
}
