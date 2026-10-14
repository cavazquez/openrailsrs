use openrailsrs_sim::{CarOperation, LiveDriveSession};
use std::path::PathBuf;
#[test]
fn shared_bogie_constraints_conserve_articulated_train_during_drive_braking_restore_and_reverse() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/player_lab/scenario_jacobs_curve.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    let mut session = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    assert_eq!(session.state.articulated_joints, vec![[0, 1]]);
    assert!(session.uncouple_after(0).is_err());
    session.driver_direction = 1.;
    session.driver_throttle = 0.6;
    session.driver_brake = 0.;
    for turn in 0..800 {
        session.step_realtime(0.05, |_| {});
        let a = &session.state.vehicles[0];
        let b = &session.state.vehicles[1];
        assert!((a.velocity_mps - b.velocity_mps).abs() < 1e-10);
        assert!((a.position_m - b.position_m).abs() < 1e-10);
        assert!(
            session
                .state
                .couplers
                .iter()
                .all(|c| c.extension_m.abs() < 1e-10 && !c.broken)
        );
        if turn == 400 {
            let save = session.snapshot();
            let mut reload =
                LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
            reload.restore_snapshot(save).unwrap();
            session = reload;
        }
    }
    assert!(session.state.odometer_m > 10.);
    session.driver_throttle = 0.;
    session.driver_brake = 1.;
    for _ in 0..600 {
        session.step_realtime(0.05, |_| {});
    }
    assert!(session.velocity_mps() < 0.1);
    let head = session.head_chainage_m();
    session.set_direction(0.).unwrap();
    session.driver_brake = 0.;
    session.driver_throttle = 0.4;
    for _ in 0..200 {
        session.step_realtime(0.05, |_| {});
    }
    assert!(session.head_chainage_m() < head - 1.);
    assert!(
        (session.state.vehicles[0].velocity_mps - session.state.vehicles[1].velocity_mps).abs()
            < 1e-10
    );
    session.driver_throttle = 0.;
    session.driver_brake = 1.;
    for _ in 0..600 {
        session.step_realtime(0.05, |_| {});
    }
    session.operate_car(1, CarOperation::Handbrake).unwrap();
    assert!(session.uncouple_after(0).is_err());
}
