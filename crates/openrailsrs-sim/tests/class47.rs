//! The second native formation feeds actual engine and pneumatic state to the cab.
use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::LiveDriveSession;
use std::path::PathBuf;

#[test]
fn class47_supply_and_air_distributor_feed_cab_instruments() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/class47_reference/scenario.toml");
    let scenario = load_scenario(&path).unwrap();
    let mut session = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    assert_eq!(session.physics.vehicle_lengths_m.len(), 7);
    assert!((session.physics.mass_kg - 320_674.640625).abs() < 0.1);
    session.driver_direction = 1.;
    session.driver_throttle = 0.;
    session.driver_brake = 1.;
    session.state.brake_system.precharge(1.);
    for _ in 0..400 {
        session.step_realtime(0.05, |_| {});
    }
    let full = session.cab_telemetry();
    assert!((full.diesel_rpm.unwrap() - 450.).abs() < 0.1);
    assert!((full.brake_pipe_bar - 3.5).abs() < 0.01);
    assert!((full.brake_cyl_bar - 4.82633).abs() < 0.01);
    assert_eq!(
        session.velocity_mps(),
        0.,
        "supply RPM must not produce traction"
    );
    session.driver_brake = 0.;
    for _ in 0..600 {
        session.step_realtime(0.05, |_| {});
    }
    let released = session.cab_telemetry();
    assert!((released.brake_pipe_bar - 5.).abs() < 0.01);
    assert!(released.brake_cyl_bar < 0.01);
    assert!((released.diesel_rpm.unwrap() - 450.).abs() < 0.1);
    session.driver_brake = 0.6;
    for _ in 0..600 {
        session.step_realtime(0.05, |_| {});
    }
    let partial = session.cab_telemetry();
    assert!((partial.brake_pipe_bar - 4.1).abs() < 0.01);
    assert!((partial.brake_cyl_bar - 2.9219).abs() < 0.01);
}
