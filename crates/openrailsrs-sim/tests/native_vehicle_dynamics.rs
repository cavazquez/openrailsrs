//! Regressions for authored hardware, rather than the old converted defaults.
use openrailsrs_scenarios::NativePhysicsEnvironment;
use openrailsrs_sim::native_dynamics::{NativeAxleState, NativeTrainPhysics};
use openrailsrs_train::{Consist, Vehicle, load_consist_with_asset_root};
use std::path::PathBuf;

fn native_stock() -> (Consist, NativeTrainPhysics) {
    let base =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern_local/physics");
    let con = base.join("consists/birmingham_pullman.con");
    let consist = load_consist_with_asset_root(&con, &base).unwrap();
    let env = NativePhysicsEnvironment {
        ambient_c: 18.61272,
        adhesion_factor: 0.5,
        ..Default::default()
    };
    let native = NativeTrainPhysics::load(&con, &base, &consist, Some(&env))
        .unwrap()
        .unwrap();
    (consist, native)
}

#[test]
fn original_formation_keeps_both_governors_axles_units_and_rigid_connections() {
    let (consist, native) = native_stock();
    assert!((consist.total_mass_kg() - 440906.4).abs() < 1.);
    let engines = consist.diesel_traction_models();
    assert_eq!(engines.len(), 2);
    assert_eq!(engines[0].idle_rpm(), 650.);
    assert_eq!(engines[1].idle_rpm(), 315.);
    assert_eq!(native.vehicles[0].drive_axles, 4.);
    assert_eq!(native.vehicles[7].drive_axles, 1.);
    assert!((native.vehicles[7].wheel_radius_m - 0.5334).abs() < 1e-8);
    assert!((native.vehicles[1].pitch_span_m.unwrap() - 14.1732).abs() < 1e-8);
    assert!(native.vehicles.iter().all(|v| v.rigid_connection));
}

#[test]
fn native_notches_and_residual_pressure_control_traction_independently_of_shoes() {
    let (_, native) = native_stock();
    assert!((native.throttle(0.75, 0.) - 0.7).abs() < 1e-9);
    // Below the 5 PSI shoe spring, pressure still trips the 4 PSI power interlock.
    assert_eq!(native.throttle(0.75, 4.6 * 0.0689475729), 0.);
    assert!((native.throttle(0.75, 3.9 * 0.0689475729) - 0.7).abs() < 1e-9);
}

#[test]
fn bearing_temperature_and_legacy_tail_match_independent_native_static_forces() {
    let (consist, native) = native_stock();
    let davis = consist.per_vehicle_davis(None);
    let masses: Vec<_> = consist
        .vehicles
        .iter()
        .map(|v| match v {
            Vehicle::Loco(l) => l.mass_kg,
            Vehicle::Wagon(w) => w.mass_kg,
        })
        .collect();
    let mut state = native.initial_state(2);
    // Read from the unmodified OR 1.6.1 DLL diagnostic at rest, 18.61272 °C.
    for (i, expected) in [(0, 2182.38477), (1, 1385.13953), (7, 1886.14014)] {
        assert!((native.resistance(i, masses[i], &davis[i], 0., &state) - expected).abs() < 1.);
    }
    state.bearing_c.fill(30.8581657);
    assert!((native.resistance(0, masses[0], &davis[0], 0., &state) - 1704.8916).abs() < 1.);
}

#[test]
fn wheel_speed_and_transmitted_force_remain_distinct_during_slip() {
    let (_, native) = native_stock();
    let mut axle = NativeAxleState { speed_mps: 10. };
    let mut transmitted = 0.;
    for _ in 0..100 {
        transmitted = axle.step(
            &native.vehicles[7],
            67000.,
            67000.,
            10.,
            100000.,
            0.,
            0.5,
            0.05,
        );
    }
    assert!(axle.speed_mps > 15., "wheel speed {}", axle.speed_mps);
    assert!(
        (10000.0..90000.).contains(&transmitted),
        "rail force {transmitted}"
    );
}

fn native_session() -> openrailsrs_sim::LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern_local/scenario_oracle.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    openrailsrs_sim::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}

#[test]
fn native_rigid_and_single_body_share_per_car_resistance_and_grades() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern_local/scenario_oracle.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    let mut rigid =
        openrailsrs_sim::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)
            .unwrap();
    scenario.simulation.multi_body = false;
    let mut single =
        openrailsrs_sim::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)
            .unwrap();
    for session in [&mut rigid, &mut single] {
        session.driver_direction = 1.;
        session.driver_throttle = 0.75;
        session.driver_brake = 0.;
    }
    for _ in 0..400 {
        rigid.step_realtime(0.05, |_| {});
        single.step_realtime(0.05, |_| {});
        assert!((rigid.velocity_mps() - single.velocity_mps()).abs() < 1e-8);
        assert!((rigid.state.odometer_m - single.state.odometer_m).abs() < 1e-8);
    }
}

#[test]
fn native_save_restores_ep_temperature_and_slip_without_divergence() {
    let mut original = native_session();
    original.driver_direction = 1.;
    original.driver_brake = 0.;
    original.driver_throttle = 0.75;
    original.step_realtime(1., |_| {});
    let serialized = serde_json::to_vec(&original.snapshot()).unwrap();
    let mut resumed = native_session();
    resumed
        .restore_snapshot(serde_json::from_slice(&serialized).unwrap())
        .unwrap();
    original.step_realtime(1., |_| {});
    resumed.step_realtime(1., |_| {});
    assert!((original.velocity_mps() - resumed.velocity_mps()).abs() < 1e-9);
    assert!((original.state.odometer_m - resumed.state.odometer_m).abs() < 1e-9);
    assert_eq!(original.state.diesel_rpm, resumed.state.diesel_rpm);
    assert_eq!(
        serde_json::to_value(&original.state.native_dynamics).unwrap(),
        serde_json::to_value(&resumed.state.native_dynamics).unwrap()
    );
    let before = original.state.odometer_m;
    let mut corrupt = original.snapshot();
    corrupt
        .state
        .native_dynamics
        .as_mut()
        .unwrap()
        .axles
        .clear();
    assert!(original.restore_snapshot(corrupt).is_err());
    assert_eq!(original.state.odometer_m, before);
    let mut corrupt = original.snapshot();
    corrupt.state.native_dynamics.as_mut().unwrap().bearing_c[0] = f64::NAN;
    assert!(original.restore_snapshot(corrupt).is_err());
    assert_eq!(original.state.odometer_m, before);
}

#[test]
fn uncoupling_preserves_retained_bearings_and_rebuilds_motor_indices() {
    let mut session = native_session();
    session.step_realtime(0.1, |_| {});
    let state = session.state.native_dynamics.as_mut().unwrap();
    state.bearing_c.fill(35.);
    state.axles[0].speed_mps = 2.;
    for index in 3..8 {
        session
            .operate_car(index, openrailsrs_sim::CarOperation::Handbrake)
            .unwrap();
    }
    session.uncouple_after(2).unwrap();
    let dynamics = session.state.native_dynamics.as_ref().unwrap();
    assert_eq!(dynamics.bearing_c, vec![35.; 3]);
    assert_eq!(dynamics.axles.len(), 1);
    assert_eq!(dynamics.axles[0].speed_mps, 2.);
    let mut resumed = native_session();
    resumed.restore_snapshot(session.snapshot()).unwrap();
    resumed.recouple().unwrap();
    let dynamics = resumed.state.native_dynamics.as_ref().unwrap();
    assert_eq!(&dynamics.bearing_c[..3], &[35.; 3]);
    assert_eq!(dynamics.axles.len(), 2);
}
