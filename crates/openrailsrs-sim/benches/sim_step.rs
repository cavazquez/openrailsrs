use criterion::{Criterion, black_box, criterion_group, criterion_main};
use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_sim::{physics::step, state::TrainSimState};
use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

fn build_line_graph() -> TrackGraph {
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
        x_m: 10000.0,
        y_m: 0.0,
    })
    .unwrap();
    g.insert_edge(Edge {
        id: EdgeId("e1".into()),
        from: NodeId("a".into()),
        to: NodeId("b".into()),
        length_m: 10_000.0,
        speed_limit_mps: 30.0,
        grade_percent: 0.0,
    })
    .unwrap();
    g
}

fn bench_physics_step(c: &mut Criterion) {
    let g = build_line_graph();
    c.bench_function("physics_step", |b| {
        b.iter(|| {
            let mut st = TrainSimState::new(vec!["e1".into()]);
            st.throttle = 1.0;
            st.velocity_mps = 10.0;
            for _ in 0..100 {
                step(
                    black_box(&mut st),
                    black_box(&g),
                    black_box(100_000.0),
                    black_box(2_000_000.0),
                    black_box(350_000.0),
                    black_box(300_000.0),
                    black_box(0.05),
                );
            }
            black_box(st.velocity_mps);
        });
    });
}

criterion_group!(benches, bench_physics_step);
criterion_main!(benches);
