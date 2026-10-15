use openrailsrs_sim::LiveDriveSession;
use std::path::PathBuf;

fn session() -> LiveDriveSession {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/traction_operation");
    let scenario =
        openrailsrs_scenarios::load_scenario(root.join("scenario_gearbox.toml")).unwrap();
    LiveDriveSession::from_scenario(&root, &scenario).unwrap()
}

#[test]
fn manual_gears_drive_the_live_train_and_resume_pending_shifts_without_refilling() {
    let mut s = session();
    assert_eq!(s.cab_telemetry().gear_indication, Some(0.));
    assert!(s.traction_status().unwrap().contains("caja N"));
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 1.;
    s.step_realtime(5., |_| {});
    assert_eq!(
        s.velocity_mps(),
        0.,
        "neutral must not use the ungeared P/v fallback"
    );
    assert!(s.traction_diagnosis().contains("caja en neutro"));
    s.shift_gear(1).unwrap();
    s.step_realtime(25., |_| {});
    assert!(
        s.velocity_mps() > 1.,
        "first gear must transmit native torque"
    );
    assert!(s.state.diesel.cars[0].consumed_l > 0.);
    assert_eq!(s.cab_telemetry().gear_indication, Some(1.));
    s.shift_gear(1).unwrap();
    let checkpoint = s.snapshot();
    let mut resumed = session();
    resumed.restore_snapshot(checkpoint.clone()).unwrap();
    for _ in 0..100 {
        s.step_realtime(0.05, |_| {});
        resumed.step_realtime(0.05, |_| {});
    }
    assert_eq!(s.state.diesel, resumed.state.diesel);
    assert_eq!(s.state.velocity_mps, resumed.state.velocity_mps);
    s.driver_throttle = 0.;
    s.step_realtime(3., |_| {});
    assert_eq!(
        s.state.diesel.cars[0]
            .gearbox
            .as_ref()
            .unwrap()
            .current_gear_index,
        1
    );
    for _ in 0..10 {
        s.shift_gear(1).unwrap();
    }
    assert_eq!(
        s.state.diesel.cars[0]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index,
        3
    );
    let mut bad = checkpoint.clone();
    bad.state.diesel.cars[0]
        .gearbox
        .as_mut()
        .unwrap()
        .next_gear_index = 999;
    assert!(resumed.restore_snapshot(bad).is_err());
    let mut bad = checkpoint;
    bad.state.diesel.cars[0].gearbox = None;
    assert!(resumed.restore_snapshot(bad).is_err());
}

#[test]
fn a_mixed_clutch_group_rejects_the_whole_request_without_partial_changes() {
    let mut s = session();
    let mut remote = s.physics.diesel.cars[0].clone();
    remote.vehicle = 1;
    let p = remote.params.gearbox.as_mut().unwrap();
    p.mechanical = true;
    p.kind = openrailsrs_formats::GearboxKind::C;
    s.physics.diesel.cars.push(remote);
    let mut state = s.state.diesel.cars[0].clone();
    state.vehicle = 1;
    s.state.diesel.cars.push(state);
    s.driver_throttle = 0.5;
    let before = s.state.diesel.clone();
    assert!(s.shift_gear(1).unwrap_err().contains("regulador"));
    assert_eq!(
        s.state.diesel, before,
        "a refused remote clutch must not shift the leading engine"
    );
}

#[test]
fn gear_commands_follow_declared_mu_compatibility_and_isolation() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/traction_operation");
    let scenario =
        openrailsrs_scenarios::load_scenario(root.join("scenario_multiple_unit.toml")).unwrap();
    let mut s = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let profile = session().physics.diesel.cars[0].params.gearbox.clone();
    for c in &mut s.physics.diesel.cars {
        c.params.gearbox = profile.clone();
    }
    for c in &mut s.state.diesel.cars {
        c.gearbox = Some(Default::default());
    }
    s.state.vehicle_systems[1]
        .definition
        .multiple_unit
        .as_mut()
        .unwrap()
        .protocol = "incompatible".into();
    s.shift_gear(1).unwrap();
    assert_eq!(
        s.state.diesel.cars[0]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index,
        0
    );
    assert_eq!(
        s.state.diesel.cars[1]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index,
        -1
    );
    s.state.vehicle_systems[1]
        .definition
        .multiple_unit
        .as_mut()
        .unwrap()
        .protocol = "mu-b".into();
    s.shift_gear(1).unwrap();
    assert_eq!(
        s.state.diesel.cars[1]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index,
        1
    );
    s.physics.diesel.cars[1].connected = false;
    s.shift_gear(1).unwrap();
    assert_eq!(
        s.state.diesel.cars[0]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index,
        2
    );
    assert_eq!(
        s.state.diesel.cars[1]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index,
        1
    );
}
