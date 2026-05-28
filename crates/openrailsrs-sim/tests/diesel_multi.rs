//! Multi-diesel traction: force summation and per-engine RPM state.

use openrailsrs_core::{EdgeId, NodeId};
use openrailsrs_sim::{
    path_data::PathData,
    physics::{TrainPhysics, step},
    state::TrainSimState,
};
use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};
use openrailsrs_train::{
    DavisCoefficients, DieselEngineParams, DieselTractionModel, TractiveCurve,
};

fn flat_graph() -> TrackGraph {
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
        length_m: 50_000.0,
        speed_limit_mps: 100.0,
        grade_percent: 0.0,
    })
    .unwrap();
    g
}

fn orts_engine(stall_n: f64) -> DieselTractionModel {
    DieselTractionModel::from_notch_curves(vec![
        (0.0, vec![(0.0, 0.0)]),
        (1.0, vec![(0.0, stall_n), (30.0, stall_n * 0.5)]),
    ])
}

fn legacy_engine(power_w: f64, force_n: f64) -> DieselTractionModel {
    DieselTractionModel::from_power_and_effort(power_w, force_n, 0.0)
}

#[test]
fn two_identical_engines_double_stall_force() {
    let e = orts_engine(80_000.0);
    let train = TrainPhysics {
        mass_kg: 200_000.0,
        max_power_w: 1_500_000.0,
        max_tractive_effort_n: 160_000.0,
        max_brake_n: 200_000.0,
        davis: DavisCoefficients::default(),
        vehicle_davis: Vec::new(),
        tractive: TractiveCurve::default(),
        diesel_engines: vec![e.clone(), e],
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
        multi_body_scalar_coast_below_v_mps: None,
        partial_throttle_run_up_time_s: None,
        orts_inherit_partial_run_up: false,
    };
    let g = flat_graph();
    let path = PathData::from_path(&["e1".to_string()], &g);
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.throttle = 1.0;
    state.diesel_rpm = vec![0.0, 0.0];
    step(&mut state, &path, &train, 1.0);
    assert!(
        state.velocity_mps > 0.35,
        "two engines should accelerate faster than one, v={}",
        state.velocity_mps
    );
}

#[test]
fn orts_plus_legacy_both_contribute() {
    let orts = orts_engine(70_000.0);
    let legacy = legacy_engine(1_000_000.0, 150_000.0);
    let f_orts = orts.force_at(0.0, 1.0);
    let f_legacy = legacy.force_at(0.0, 1.0);
    let train = TrainPhysics {
        mass_kg: 250_000.0,
        max_power_w: 1_750_000.0,
        max_tractive_effort_n: f_orts + f_legacy,
        max_brake_n: 200_000.0,
        davis: DavisCoefficients::default(),
        vehicle_davis: Vec::new(),
        tractive: TractiveCurve::default(),
        diesel_engines: vec![orts, legacy],
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
        multi_body_scalar_coast_below_v_mps: None,
        partial_throttle_run_up_time_s: None,
        orts_inherit_partial_run_up: false,
    };
    let g = flat_graph();
    let path = PathData::from_path(&["e1".to_string()], &g);
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.throttle = 1.0;
    state.diesel_rpm = vec![0.0, 0.0];
    step(&mut state, &path, &train, 1.0);
    assert!(state.velocity_mps > 0.25, "v={}", state.velocity_mps);
}

#[test]
fn per_engine_power_cap_limits_force_at_speed() {
    let orts = orts_engine(70_000.0);
    let legacy = legacy_engine(400_000.0, 150_000.0);
    let f_orts = orts.force_at(20.0, 1.0);
    let train = TrainPhysics {
        mass_kg: 250_000.0,
        max_power_w: 750_000.0,
        max_tractive_effort_n: 220_000.0,
        max_brake_n: 200_000.0,
        davis: DavisCoefficients::default(),
        vehicle_davis: Vec::new(),
        tractive: TractiveCurve::default(),
        diesel_engines: vec![orts, legacy],
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
        multi_body_scalar_coast_below_v_mps: None,
        partial_throttle_run_up_time_s: None,
        orts_inherit_partial_run_up: false,
    };
    let g = flat_graph();
    let path = PathData::from_path(&["e1".to_string()], &g);
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.throttle = 1.0;
    state.velocity_mps = 20.0;
    state.diesel_rpm = vec![750.0, 0.0];
    state.diesel_run_up = vec![0.0, 1.0];
    state.diesel_motor_heat = vec![0.0, 0.0];
    let v_before = state.velocity_mps;
    step(&mut state, &path, &train, 1.0);
    // Legacy engine capped at P/v = 400k/20 = 20 kN, not full 150 kN stall.
    let f_legacy_cap = 400_000.0 / 20.0;
    let expected_accel = (f_orts + f_legacy_cap) / train.mass_kg;
    let observed_accel = state.velocity_mps - v_before;
    assert!(
        (observed_accel - expected_accel).abs() < 0.05,
        "expected per-engine P/v cap: accel {observed_accel} vs {expected_accel}"
    );
}

#[test]
fn per_engine_rpm_independent() {
    let mut fast = orts_engine(50_000.0);
    let mut slow = orts_engine(50_000.0);
    let engine_params = |tau: f64| DieselEngineParams {
        power_tab: vec![(0.0, 0.0), (750.0, 500_000.0)],
        throttle_rpm_tab: vec![(0.0, 325.0), (1.0, 750.0)],
        idle_rpm: 325.0,
        max_rpm: 750.0,
        rpm_time_constant_s: tau,
        rate_of_change_up_rpm_pss: 0.0,
        rate_of_change_down_rpm_pss: 0.0,
        change_up_rpm_ps: 0.0,
        change_down_rpm_ps: 0.0,
        reverse_throttle_rpm_tab: openrailsrs_train::build_reverse_throttle_rpm_tab(&[
            (0.0, 325.0),
            (1.0, 750.0),
        ]),
    };
    fast.engine = Some(Box::new(engine_params(0.5)));
    slow.engine = Some(Box::new(engine_params(5.0)));
    let train = TrainPhysics {
        mass_kg: 100_000.0,
        max_power_w: 1_000_000.0,
        max_tractive_effort_n: 100_000.0,
        max_brake_n: 100_000.0,
        davis: DavisCoefficients::default(),
        vehicle_davis: Vec::new(),
        tractive: TractiveCurve::default(),
        diesel_engines: vec![fast, slow],
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam_params: None,
        brake_mapping: Default::default(),
        legacy_power_cap: true,
        brake_skid_limit: false,
        multi_body_scalar_coast_below_v_mps: None,
        partial_throttle_run_up_time_s: None,
        orts_inherit_partial_run_up: false,
    };
    let g = flat_graph();
    let path = PathData::from_path(&["e1".to_string()], &g);
    let mut state = TrainSimState::new(vec!["e1".into()]);
    state.throttle = 1.0;
    state.diesel_rpm = vec![325.0, 325.0];
    for _ in 0..20 {
        step(&mut state, &path, &train, 0.5);
    }
    assert_eq!(state.diesel_rpm.len(), 2);
    assert!(
        state.diesel_rpm[0] > state.diesel_rpm[1],
        "fast engine should spin up quicker: {:?}",
        state.diesel_rpm
    );
}
