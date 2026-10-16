//! Composition test: rack, converter, vertical contact and braking share one
//! driven formation and one transactional save, rather than isolated models.
use openrailsrs_sim::LiveDriveSession;
use std::path::PathBuf;

fn fixture(name: &str) -> LiveDriveSession {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/physics_lab");
    let scenario = openrailsrs_scenarios::load_scenario(directory.join(name)).unwrap();
    LiveDriveSession::from_scenario(&directory, &scenario).unwrap()
}

#[test]
fn rack_entry_and_exit_follow_a_train_accelerating_from_rest() {
    use openrailsrs_sim::rack::RackPhase;
    let mut session = fixture("scenario_rack_transition.toml");
    assert_eq!(session.velocity_mps(), 0.);
    session.driver_direction = 1.;
    session.driver_throttle = 0.12;
    session.driver_brake = 0.;
    let mut engaging = false;
    let mut engaged = false;
    let mut exited = false;
    for _ in 0..1200 {
        session.step_realtime(0.05, |_| {});
        let rack = &session.state.rack.cars[0];
        assert_ne!(rack.phase, RackPhase::Fault);
        engaging |= rack.phase == RackPhase::Engaging;
        engaged |= rack.phase == RackPhase::Engaged;
        if engaged && rack.phase == RackPhase::Disengaged {
            assert!(rack.section.is_none());
            assert_eq!(rack.transmitted_force_n, 0.);
            assert!(session.state.rail_force_n[0] > 0.);
            exited = true;
            break;
        }
    }
    assert!(
        engaging && engaged && exited,
        "entry/exit did not complete: {:?}",
        session.state.rack
    );
    assert!(session.state.odometer_m > 70.);
}

#[test]
fn optional_derailment_is_reachable_by_driving_from_rest() {
    let mut active = fixture("scenario_derailment_drive.toml");
    let mut disabled = fixture("scenario_derailment_drive.toml");
    disabled.set_derailment_enabled(false);
    assert_eq!(active.velocity_mps(), 0.);
    for session in [&mut active, &mut disabled] {
        session.driver_direction = 1.;
        session.driver_throttle = 1.;
        session.driver_brake = 0.;
    }
    let mut peak_speed: f64 = 0.;
    for _ in 0..3600 {
        active.step_realtime(0.05, |_| {});
        disabled.step_realtime(0.05, |_| {});
        peak_speed = peak_speed.max(active.velocity_mps());
        if !active.derailment_events().is_empty() {
            break;
        }
    }
    let events = active.derailment_events();
    assert!(
        !events.is_empty(),
        "no incident after driving: speed={}, distance={}, peak={peak_speed}, failure={:?}",
        active.velocity_mps(),
        active.state.odometer_m,
        active.gameplay.failure
    );
    println!(
        "Driven incident: time={}s, distance={}m, peak={}m/s, events={events:?}",
        active.time_s(),
        active.state.odometer_m,
        peak_speed
    );
    assert!(active.state.odometer_m > 1.);
    assert!(
        events
            .iter()
            .all(|(_, event)| event.event_time_s.unwrap() > 0. && event.event_speed_mps > 1.)
    );
    assert!(disabled.derailment_events().is_empty());
    assert!(active.state.brake_system.emergency);
    assert!(active.state.motor_force_n.iter().all(|force| *force == 0.));
    let saved = active.snapshot();
    let mut resumed = fixture("scenario_derailment_drive.toml");
    resumed.restore_snapshot(saved).unwrap();
    assert_eq!(active.state.bogies, resumed.state.bogies);
    resumed.set_derailment_enabled(false);
    assert_eq!(
        active.derailment_events().len(),
        resumed.derailment_events().len()
    );
}

#[test]
fn converter_rack_vertical_contact_emergency_and_restore_run_together() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let directory = root.join("examples/physics_lab");
    let mut scenario =
        openrailsrs_scenarios::load_scenario(directory.join("scenario_rack.toml")).unwrap();
    let vertical =
        openrailsrs_scenarios::load_scenario(directory.join("scenario_vertical.toml")).unwrap();
    scenario.train.bogie_dynamics = vertical.train.bogie_dynamics;
    let create = || LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    let mut original = create();
    original.driver_direction = 1.;
    original.driver_throttle = 0.65;
    original.driver_brake = 0.;
    let mut rack_worked = false;
    let mut vertical_loaded = false;
    for _ in 0..200 {
        original.step_realtime(0.05, |_| {});
        rack_worked |= original
            .state
            .rack
            .cars
            .iter()
            .any(|s| s.transmitted_force_n > 100.);
        vertical_loaded |= original
            .state
            .bogies
            .iter()
            .filter_map(|s| s.vertical.as_ref())
            .flat_map(|s| &s.supports)
            .flat_map(|s| &s.wheels)
            .any(|w| w.reference_load_n > 0. && (w.normal_force_n - w.reference_load_n).abs() > 1.);
    }
    assert!(rack_worked && vertical_loaded);
    assert!(
        original
            .state
            .diesel
            .cars
            .iter()
            .any(|s| s.hydraulic.is_some())
    );
    original.driver_throttle = 0.;
    original.driver_brake = 1.;
    original.step_realtime(0.5, |_| {});
    let encoded = serde_json::to_vec(&original.snapshot()).unwrap();
    let mut resumed = create();
    resumed
        .restore_snapshot(serde_json::from_slice(&encoded).unwrap())
        .unwrap();
    for _ in 0..40 {
        original.step_realtime(0.05, |_| {});
        resumed.step_realtime(0.05, |_| {});
    }
    assert_eq!(original.state.velocity_mps, resumed.state.velocity_mps);
    assert_eq!(original.state.rack, resumed.state.rack);
    assert_eq!(original.state.diesel, resumed.state.diesel);
    assert_eq!(original.state.bogies, resumed.state.bogies);
    assert!(original.state.rack.cars.iter().any(|s| s.brake_work_j > 0.));
    let before = serde_json::to_value(resumed.snapshot()).unwrap();
    let mut invalid = resumed.snapshot();
    invalid.state.bogies[0].vertical.as_mut().unwrap().supports[0].wheels[0].normal_force_n = -1.;
    assert!(resumed.restore_snapshot(invalid).is_err());
    assert_eq!(before, serde_json::to_value(resumed.snapshot()).unwrap());
}
