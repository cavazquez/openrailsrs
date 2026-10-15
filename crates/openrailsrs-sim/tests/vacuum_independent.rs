//! Physical integration: the independent handle is not a whole-train command.
use openrailsrs_sim::LiveDriveSession;

fn session() -> LiveDriveSession {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/vacuum_independent/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&file).unwrap();
    LiveDriveSession::from_scenario(file.parent().unwrap(), &scenario).unwrap()
}

#[test]
fn vacuum_independent_drives_brakes_only_the_engine_and_is_saved() {
    let mut released = session();
    let mut held = session();
    for s in [&mut released, &mut held] {
        s.driver_throttle = 0.35;
        s.driver_brake = 0.;
        s.set_direction(1.).unwrap();
    }
    held.set_independent_brake(1.).unwrap();
    for _ in 0..200 {
        released.step_realtime(0.05, |_| {});
        held.step_realtime(0.05, |_| {});
    }
    assert!(
        released.velocity_mps() > held.velocity_mps() + 0.2,
        "Released: {:.6} m/s, {}; independent: {:.6} m/s, {}; forces: {:?} / {:?}",
        released.velocity_mps(),
        released.traction_diagnosis(),
        held.velocity_mps(),
        held.traction_diagnosis(),
        released.state.brake_system.cylinder_forces_n(0.),
        held.state.brake_system.cylinder_forces_n(0.),
    );
    let free = released.state.brake_system.cylinder_forces_n(0.);
    let applied = held.state.brake_system.cylinder_forces_n(0.);
    assert!(applied[0] > 10_000. && free[0] < 1.);
    assert!(
        applied[1] < 1.,
        "Independent vacuum must leave the coach released"
    );
    assert!(held.cab_telemetry().vacuum_brake);
    let saved = held.snapshot();
    let encoded = serde_json::to_string(&saved).unwrap();
    let mut resumed = session();
    resumed
        .restore_snapshot(serde_json::from_str(&encoded).unwrap())
        .unwrap();
    for s in [&mut held, &mut resumed] {
        s.driver_throttle = 0.;
        s.driver_brake = 0.6;
        s.set_independent_brake(0.).unwrap();
    }
    for _ in 0..100 {
        held.step_realtime(0.05, |_| {});
        resumed.step_realtime(0.05, |_| {});
    }
    assert!((held.velocity_mps() - resumed.velocity_mps()).abs() < 1e-9);
    let train = held.state.brake_system.cylinder_forces_n(0.);
    assert!(
        train.iter().all(|f| *f > 5_000.),
        "Train braking must still apply every vehicle"
    );
    assert_eq!(
        serde_json::to_value(&held.state.brake_system).unwrap(),
        serde_json::to_value(&resumed.state.brake_system).unwrap()
    );
    let mut altered = serde_json::to_value(saved).unwrap();
    altered["state"]["brake_system"]["cylinders"][0]["native_vacuum"]["profile"]["independent"]["application_psi_s"] =
        999.0.into();
    assert!(
        session()
            .restore_snapshot(serde_json::from_value(altered).unwrap())
            .is_err()
    );
}

#[test]
fn vacuum_independent_controller_does_not_accept_nonfinite_inputs() {
    let mut s = session();
    assert!(s.set_independent_brake(f64::NAN).is_err());
    assert!(s.set_independent_brake(f64::INFINITY).is_err());
    s.set_independent_brake(2.).unwrap();
    assert_eq!(s.state.brake_system.independent_command, 1.);
}
