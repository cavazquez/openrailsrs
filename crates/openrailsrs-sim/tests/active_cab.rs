use openrailsrs_sim::LiveDriveSession;
use std::path::PathBuf;

fn session() -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario_two_diesel.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}

#[test]
fn selected_cab_reads_its_own_engine_pressure_and_supply_and_restores() {
    let mut s = session();
    s.driver_direction = 0.5;
    s.driver_throttle = 0.;
    openrailsrs_sim::power_supply::advance(&mut s.state, &s.physics.power_supply);
    let indices = s
        .state
        .diesel
        .cars
        .iter()
        .map(|c| c.vehicle)
        .collect::<Vec<_>>();
    assert_eq!(indices.len(), 2);
    s.state.diesel.cars[0].rpm = 450.;
    s.state.diesel.cars[1].rpm = 900.;
    s.state.power_supply.cars[0].supply.low_voltage = true;
    s.operate_car(indices[1], openrailsrs_sim::CarOperation::Battery)
        .unwrap();
    openrailsrs_sim::power_supply::advance(&mut s.state, &s.physics.power_supply);
    s.headlights = 2;
    let positions = s.state.vehicles.clone();
    let odometer = s.state.odometer_m;
    s.select_cab(indices[1]).unwrap();
    assert_eq!(s.cab_telemetry().diesel_rpm, Some(900.));
    assert_eq!(s.effective_headlights(), 0);
    assert_eq!(s.state.odometer_m, odometer);
    assert_eq!(s.state.vehicles.len(), positions.len());
    for (a, b) in s.state.vehicles.iter().zip(positions) {
        assert_eq!(a.position_m, b.position_m);
    }
    let saved = s.snapshot();
    let mut restored = session();
    restored.restore_snapshot(saved).unwrap();
    assert_eq!(restored.cab_vehicle(), indices[1]);
    assert_eq!(restored.cab_telemetry().diesel_rpm, Some(900.));
    assert_eq!(restored.effective_headlights(), 0);
}

#[test]
fn cab_change_rejects_motion_throttle_drive_and_trailers_without_mutation() {
    let mut s = session();
    let original = s.cab_vehicle();
    s.state.velocity_mps = 1.;
    assert!(s.cycle_cab().is_err());
    s.state.velocity_mps = 0.;
    s.driver_direction = 0.5;
    s.driver_throttle = 0.1;
    assert!(s.cycle_cab().is_err());
    s.driver_throttle = 0.;
    assert!(s.select_cab(999).is_err());
    assert_eq!(s.cab_vehicle(), original);
    let next = s.cycle_cab().unwrap();
    assert_ne!(next, original);
    let mut saved = s.snapshot();
    saved.state.active_cab_vehicle = Some(999);
    assert!(s.restore_snapshot(saved).is_err());
    assert_eq!(s.cab_vehicle(), next);
}
