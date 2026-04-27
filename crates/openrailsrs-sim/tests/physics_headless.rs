use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_sim::{
    path_data::PathData,
    physics::{TrainPhysics, step},
    state::TrainSimState,
};
use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};
use openrailsrs_train::{DavisCoefficients, TractiveCurve};

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

fn path_data_for(path: &[&str], g: &TrackGraph) -> PathData {
    let edges: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    PathData::from_path(&edges, g)
}

/// Train without an explicit traction curve — uses P/v fallback.
fn default_train_pv() -> TrainPhysics {
    TrainPhysics {
        mass_kg: 100_000.0,
        max_power_w: 2_000_000.0,
        max_tractive_effort_n: 350_000.0,
        max_brake_n: 400_000.0,
        davis: DavisCoefficients::default(),
        tractive: TractiveCurve::default(),
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
    }
}

/// Train with an explicit traction curve (two-segment).
fn default_train_with_curve() -> TrainPhysics {
    TrainPhysics {
        mass_kg: 100_000.0,
        max_power_w: 2_000_000.0,
        max_tractive_effort_n: 350_000.0,
        max_brake_n: 400_000.0,
        davis: DavisCoefficients::default(),
        tractive: TractiveCurve::from_power_and_effort(2_000_000.0, 350_000.0),
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
    }
}

// Keep original name as alias so existing call sites compile unchanged.
fn default_train() -> TrainPhysics {
    default_train_pv()
}

#[test]
fn acceleration_increases_speed_on_flat_track() {
    let g = flat_line_graph();
    let pd = path_data_for(&["e1"], &g);
    let train = default_train();
    let mut st = TrainSimState::new(vec!["e1".into()]);
    st.throttle = 1.0;
    st.brake = 0.0;
    let v0 = st.velocity_mps;
    for _ in 0..200 {
        let _ = step(&mut st, &pd, &train, 0.1);
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
    let pd = path_data_for(&["e1"], &g);
    let train = default_train();
    let mut st = TrainSimState::new(vec!["e1".into()]);
    st.velocity_mps = 25.0;
    st.throttle = 0.0;
    st.brake = 1.0;
    let v0 = st.velocity_mps;
    for _ in 0..500 {
        let _ = step(&mut st, &pd, &train, 0.05);
    }
    assert!(
        st.velocity_mps < v0 - 5.0,
        "expected braking, got v={}",
        st.velocity_mps
    );
}

#[test]
fn tractive_curve_accelerates_from_rest() {
    let g = flat_line_graph();
    let pd = path_data_for(&["e1"], &g);
    let train = default_train_with_curve();
    let mut st = TrainSimState::new(vec!["e1".into()]);
    st.throttle = 1.0;
    st.brake = 0.0;
    for _ in 0..200 {
        let _ = step(&mut st, &pd, &train, 0.1);
    }
    assert!(
        st.velocity_mps > 5.0,
        "curve-based train should accelerate, got v={}",
        st.velocity_mps
    );
}

#[test]
fn pv_fallback_and_curve_both_accelerate() {
    let g = flat_line_graph();
    let train_pv = default_train_pv();
    let train_curve = default_train_with_curve();

    let accel_after = |train: TrainPhysics| {
        let pd = path_data_for(&["e1"], &g);
        let mut st = TrainSimState::new(vec!["e1".into()]);
        st.throttle = 1.0;
        st.brake = 0.0;
        for _ in 0..100 {
            let _ = step(&mut st, &pd, &train, 0.1);
        }
        st.velocity_mps
    };

    let v_pv = accel_after(train_pv);
    let v_curve = accel_after(train_curve);
    assert!(v_pv > 2.0, "P/v fallback should accelerate, got {v_pv}");
    assert!(
        v_curve > 2.0,
        "curve-based should accelerate, got {v_curve}"
    );
}
