use std::path::PathBuf;

use openrailsrs_sim::LiveDriveSession;

#[test]
fn lateral_forces_shared_supports_visual_independence_and_save_resume_during_drive() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/player_lab/scenario_bogie_mechanics.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    let mut visual_variant = scenario.clone();
    for p in &mut visual_variant.train.presentation {
        p.bogie_yaw_limit_rad = Some(0.005);
        p.jacobs = None;
        p.suspension = None;
    }
    let mut a = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    let mut b = LiveDriveSession::from_scenario(path.parent().unwrap(), &visual_variant).unwrap();
    // Physical cant has its own table: a different visual roll must not change
    // lateral loading when that physical table exists.
    for edge in &mut b.path_data.edges {
        for sample in &mut edge.profile.curves {
            sample.cant_rad *= -3.;
        }
    }
    assert_eq!(a.state.articulated_joints, vec![[0, 1]]);
    assert_eq!(b.state.articulated_joints, a.state.articulated_joints);
    a.driver_throttle = 0.7;
    b.driver_throttle = 0.7;
    a.driver_direction = 1.;
    b.driver_direction = 1.;
    a.driver_brake = 0.;
    b.driver_brake = 0.;
    let mut lateral_seen = false;
    for tick in 0..500 {
        a.step_realtime(0.05, |_| {});
        b.step_realtime(0.05, |_| {});
        assert_eq!(
            a.state.bogies, b.state.bogies,
            "visual model changed mechanics at tick {tick}"
        );
        assert_eq!(a.state.velocity_mps, b.state.velocity_mps);
        assert_eq!(
            a.state.bogies[0].supports[1].lateral_offset_m,
            a.state.bogies[1].supports[0].lateral_offset_m
        );
        lateral_seen |= a
            .state
            .bogies
            .iter()
            .any(|s| s.supports.iter().any(|s| s.requested_force_n.abs() > 100.));
        assert!(
            a.state
                .bogies
                .iter()
                .zip(&a.physics.bogies)
                .all(|(s, p)| s.valid_for(p))
        );
        if tick == 250 {
            let saved = serde_json::to_vec(&a.snapshot()).unwrap();
            let mut resumed =
                LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
            resumed
                .restore_snapshot(serde_json::from_slice(&saved).unwrap())
                .unwrap();
            assert_eq!(resumed.state.bogies, a.state.bogies);
            a = resumed;
            a.driver_throttle = 0.7;
            a.driver_brake = 0.;
        }
    }
    assert!(
        lateral_seen && a.state.odometer_m > 1.,
        "lateral_seen={lateral_seen}, distance={} speed={}",
        a.state.odometer_m,
        a.state.velocity_mps
    );
    let before = a.state.bogies.clone();
    let mut invalid = a.snapshot();
    invalid.state.bogies[0].supports[0].lateral_offset_m = 100.;
    assert!(a.restore_snapshot(invalid).is_err());
    assert_eq!(a.state.bogies, before);
    let mut erased = a.snapshot();
    erased.state.bogies.clear();
    assert!(a.restore_snapshot(erased).is_err());
    assert_eq!(a.state.bogies, before);

    let mut legacy = serde_json::to_value(a.snapshot()).unwrap();
    legacy["state"]
        .as_object_mut()
        .unwrap()
        .remove("bogie_state_version");
    legacy["state"].as_object_mut().unwrap().remove("bogies");
    legacy["formation"]
        .as_object_mut()
        .unwrap()
        .remove("parked_bogies");
    a.restore_snapshot(serde_json::from_value(legacy).unwrap())
        .unwrap();
    assert_eq!(a.state.bogie_state_version, Some(1));
    assert!(a.state.bogies.iter().all(|s| *s == Default::default()));
    a.step_realtime(0.05, |_| {});
    assert!(
        a.state
            .bogies
            .iter()
            .any(|s| s.supports.iter().any(|s| s.requested_force_n.abs() > 100.))
    );
    a.driver_throttle = 0.;
    a.driver_brake = 1.;
    for _ in 0..600 {
        a.step_realtime(0.05, |_| {});
        if a.velocity_mps() < 0.001 {
            break;
        }
    }
    assert!(a.velocity_mps() < 0.01);
    let head = a.head_chainage_m();
    a.set_direction(0.).unwrap();
    a.driver_brake = 0.;
    a.driver_throttle = 0.5;
    // The full service application needs pneumatic release time before the
    // reverse effort moves the consist.
    for _ in 0..400 {
        a.step_realtime(0.05, |_| {});
        if a.head_chainage_m() < head - 0.5 {
            break;
        }
    }
    assert!(
        a.head_chainage_m() < head,
        "reverse did not move: head_before={head}, head_after={}, speed={}, time={}, phase={:?}",
        a.head_chainage_m(),
        a.velocity_mps(),
        a.time_s(),
        a.gameplay.phase
    );
    assert!(
        a.state
            .bogies
            .iter()
            .zip(&a.physics.bogies)
            .all(|(s, p)| s.valid_for(p))
    );
}

#[test]
fn visual_jacobs_alone_does_not_create_a_mechanical_constraint() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/player_lab/scenario_bogie_mechanics.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario.train.bogie_dynamics.clear();
    let session = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    assert!(
        scenario
            .train
            .presentation
            .iter()
            .any(|p| p.jacobs.is_some())
    );
    assert!(session.state.articulated_joints.is_empty());
}
