use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_sim::{
    path_data::PathData,
    physics::{TrainPhysics, step},
    run_from_scenario_file, run_scenario_multi_train,
    state::TrainSimState,
};
use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};
use openrailsrs_train::{DavisCoefficients, TractiveCurve};

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

fn default_train() -> TrainPhysics {
    TrainPhysics {
        mass_kg: 100_000.0,
        max_power_w: 2_000_000.0,
        max_tractive_effort_n: 350_000.0,
        max_brake_n: 300_000.0,
        davis: DavisCoefficients::default(),
        tractive: TractiveCurve::from_power_and_effort(2_000_000.0, 350_000.0),
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
    }
}

/// Micro-benchmark: 100 physics steps in a tight loop (hot-path).
fn bench_physics_step(c: &mut Criterion) {
    let g = build_line_graph();
    let path_edges = vec!["e1".to_string()];
    let pd = PathData::from_path(&path_edges, &g);
    let train = default_train();

    c.bench_function("physics_step_100", |b| {
        b.iter(|| {
            let mut st = TrainSimState::new(path_edges.clone());
            st.throttle = 1.0;
            st.velocity_mps = 10.0;
            for _ in 0..100 {
                step(
                    black_box(&mut st),
                    black_box(&pd),
                    black_box(&train),
                    black_box(0.05),
                );
            }
            black_box(st.velocity_mps);
        });
    });
}

/// Full scenario benchmark: run the smoke single-train scenario end-to-end.
fn bench_full_scenario(c: &mut Criterion) {
    let scenario =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
    if !scenario.exists() {
        return; // skip if assets not present
    }
    c.bench_function("full_scenario_smoke", |b| {
        b.iter(|| {
            let _ = run_from_scenario_file(black_box(&scenario));
        });
    });
}

/// Full scenario benchmark: run the smoke multi-train scenario end-to-end.
fn bench_multi_train_scenario(c: &mut Criterion) {
    use openrailsrs_scenarios::load_scenario;

    let scenario_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/smoke/scenario_multi.toml");
    if !scenario_path.exists() {
        return;
    }
    let scenario_dir = scenario_path.parent().unwrap();
    let scenario = match load_scenario(&scenario_path) {
        Ok(s) => s,
        Err(_) => return,
    };

    c.bench_function("full_scenario_multi_train", |b| {
        b.iter(|| {
            let _ = run_scenario_multi_train(black_box(scenario_dir), black_box(&scenario));
        });
    });
}

criterion_group!(
    benches,
    bench_physics_step,
    bench_full_scenario,
    bench_multi_train_scenario
);
criterion_main!(benches);
