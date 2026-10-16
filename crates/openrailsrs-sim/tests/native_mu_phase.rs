//! Causal governor/controller ordering from OR 1.6.1. No observed RPM, force
//! or trajectory is supplied to the running simulation.
use openrailsrs_sim::LiveDriveSession;
use std::path::PathBuf;

fn session(cab: usize) -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario_two_diesel.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario.route.start_offset_m = Some(100.);
    scenario.route.stops.clear();
    scenario.simulation.native_physics = Some(Default::default());
    let mut session = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    session.driver_direction = 0.5;
    session.driver_throttle = 0.;
    session.select_cab(cab).unwrap();
    session.driver_direction = 1.;
    session.driver_brake = 0.;
    session
}

#[test]
fn lead_governor_reads_old_mu_and_following_engine_reads_new_mu_on_rise_and_fall() {
    let mut s = session(0);
    let idle = s.physics.diesel.cars[0].governor.idle_rpm;
    let target = openrailsrs_sim::diesel_operation::native_governor_target_rpm(
        &s.physics.diesel.cars[0].governor,
        0.8,
    );
    s.driver_throttle = 0.8;
    s.step_realtime(0.05, |_| {});
    let lead = s.state.diesel.car(0).unwrap();
    let following = s.state.diesel.car(1).unwrap();
    assert_eq!(lead.demanded_rpm, idle);
    assert_eq!(lead.rpm, idle);
    assert_eq!(following.demanded_rpm, target);
    assert!(following.rpm > idle);
    assert_eq!(s.state.throttle, 0.8);
    assert_eq!(s.state.native_dynamics.as_ref().unwrap().mu_throttle, 0.8);

    // Each subsequent tick is integrated from its own RPM. No captured RPM
    // is used to establish the high-power starting state for the falling edge.
    for _ in 0..40 {
        s.step_realtime(0.05, |_| {});
    }
    let before_lead = s.state.diesel.car(0).unwrap().rpm;
    let before_following = s.state.diesel.car(1).unwrap().rpm;
    s.driver_throttle = 0.;
    s.step_realtime(0.05, |_| {});
    let lead = s.state.diesel.car(0).unwrap();
    let following = s.state.diesel.car(1).unwrap();
    assert_eq!(lead.demanded_rpm, target);
    assert!(lead.rpm >= before_lead);
    assert_eq!(following.demanded_rpm, idle);
    assert!(following.rpm < before_following);
    // The governor's one-tick MU history must not delay cutting traction.
    assert_eq!(s.state.throttle, 0.);
    assert!(s.state.diesel_apparent_throttle.iter().all(|v| *v == 0.));
    assert_eq!(s.state.native_dynamics.as_ref().unwrap().mu_throttle, 0.);
    s.step_realtime(0.05, |_| {});
    assert_eq!(s.state.diesel.car(0).unwrap().demanded_rpm, idle);
}

#[test]
fn engine_order_uses_the_selected_cab_as_the_mu_publication_boundary() {
    let mut front_cab = session(0);
    let mut rear_cab = session(1);
    for s in [&mut front_cab, &mut rear_cab] {
        s.driver_throttle = 0.8;
        s.step_realtime(0.05, |_| {});
    }
    for vehicle in [0, 1] {
        let idle = rear_cab.physics.diesel.cars[vehicle].governor.idle_rpm;
        // With the lead last in car order, neither diesel governor has seen
        // its publication yet, including the locomotive physically before it.
        assert_eq!(rear_cab.state.diesel.car(vehicle).unwrap().rpm, idle);
    }
    assert!(front_cab.state.diesel.car(1).unwrap().rpm > rear_cab.state.diesel.car(1).unwrap().rpm);
    rear_cab.step_realtime(0.05, |_| {});
    for vehicle in [0, 1] {
        assert!(
            rear_cab.state.diesel.car(vehicle).unwrap().rpm
                > rear_cab.physics.diesel.cars[vehicle].governor.idle_rpm
        );
    }
}

#[test]
fn saved_mu_phase_survives_a_pending_controller_change_and_rejects_invalid_history() {
    let mut original = session(0);
    original.driver_throttle = 0.8;
    original.step_realtime(0.05, |_| {});
    original.driver_throttle = 0.;
    let saved = original.snapshot();
    let mut resumed = session(0);
    resumed
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    assert_eq!(
        resumed.state.native_dynamics.as_ref().unwrap().mu_throttle,
        0.8
    );
    for _ in 0..20 {
        original.step_realtime(0.05, |_| {});
        resumed.step_realtime(0.05, |_| {});
        assert_eq!(original.state.diesel_rpm, resumed.state.diesel_rpm);
        assert_eq!(original.state.diesel, resumed.state.diesel);
        assert_eq!(original.state.odometer_m, resumed.state.odometer_m);
        assert_eq!(
            original.state.native_dynamics.as_ref().unwrap().mu_throttle,
            resumed.state.native_dynamics.as_ref().unwrap().mu_throttle
        );
    }
    let before = serde_json::to_value(resumed.snapshot()).unwrap();
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut invalid = resumed.snapshot();
        invalid.state.native_dynamics.as_mut().unwrap().mu_throttle = value;
        assert!(resumed.restore_snapshot(invalid).is_err());
        assert_eq!(serde_json::to_value(resumed.snapshot()).unwrap(), before);
    }
}

#[test]
fn native_governor_keeps_single_arithmetic_and_equal_target_rate_history() {
    use openrailsrs_sim::diesel_operation::{
        advance_native_governor_rpm, native_governor_target_rpm,
    };
    let p = openrailsrs_train::DieselEngineParams::from_msts_defaults(3263000., 315., 900., 40.);
    let throttle = 0.552_f64;
    let x = throttle as f32 * 100.;
    let expected = (100. - x) / 100. * 315. + x / 100. * 900.;
    let target = native_governor_target_rpm(&p, throttle);
    assert_eq!(target, f64::from(expected));
    let mut previous_rate = 0.;
    let rising = advance_native_governor_rpm(&p, 315., target, 0.05, &mut previous_rate);
    assert_eq!(rising, f64::from(315_f32 + 40_f32 * 0.05_f32));
    assert_eq!(previous_rate, 40.);
    // Native dRPM has no assignment in the RealRPM == DemandedRPM branch.
    let equal = advance_native_governor_rpm(&p, target, target, 0.05, &mut previous_rate);
    assert_eq!(equal, f64::from(target as f32 + 40_f32 * 0.05_f32));
    let falling = advance_native_governor_rpm(&p, equal, 315., 0.05, &mut previous_rate);
    // Native Math.Min operates on negative values on the falling branch;
    // ChangeDownRPMpS is therefore not a magnitude cap on this legacy law.
    let square = 2_f32 * 40_f32 * (equal as f32 - 315_f32);
    let expected_rate = (-f64::from(square).sqrt()).min(-40.) as f32;
    assert_eq!(falling, f64::from(equal as f32 + expected_rate * 0.05_f32));
    assert_eq!(previous_rate, f64::from(expected_rate));
    assert!(previous_rate < -40.);
}

#[test]
fn pending_native_rpm_rate_is_restorable_and_invalid_rate_is_transactionally_rejected() {
    let mut original = session(0);
    let mut legacy_json = serde_json::to_value(original.snapshot()).unwrap();
    for car in legacy_json["state"]["diesel"]["cars"]
        .as_array_mut()
        .unwrap()
    {
        car.as_object_mut().unwrap().remove("native_rpm_rate");
    }
    let mut legacy = session(0);
    legacy
        .restore_snapshot(serde_json::from_value(legacy_json).unwrap())
        .unwrap();
    assert!(
        legacy
            .state
            .diesel
            .cars
            .iter()
            .all(|c| c.native_rpm_rate == 0.)
    );
    original.state.diesel.cars[0].native_rpm_rate = 2.;
    let saved = original.snapshot();
    let mut resumed = session(0);
    resumed
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    original.step_realtime(0.05, |_| {});
    resumed.step_realtime(0.05, |_| {});
    assert_eq!(original.state.diesel, resumed.state.diesel);
    let before = serde_json::to_value(resumed.snapshot()).unwrap();
    for rate in [f64::NAN, f64::INFINITY, -1e9, 1e9] {
        let mut invalid = resumed.snapshot();
        invalid.state.diesel.cars[0].native_rpm_rate = rate;
        assert!(resumed.restore_snapshot(invalid).is_err());
        assert_eq!(serde_json::to_value(resumed.snapshot()).unwrap(), before);
    }
}

#[test]
fn native_scalar_distance_and_odometer_follow_final_single_speed() {
    let mut native = session(0);
    assert!(native.state.vehicles.is_empty());
    native.state.velocity_mps = 10.;
    // A large existing odometer exposes Single accumulation independently of
    // the physical edge position; no reference trajectory is prescribed.
    native.state.odometer_m = 10000.125;
    let old_odometer = native.state.odometer_m;
    let old_position = native.state.pos_on_edge_m;
    native.step_realtime(0.05, |_| {});
    let speed = native.state.velocity_mps;
    let distance = speed as f32 * 0.05_f32;
    assert_eq!(speed, f64::from(speed as f32));
    assert_eq!(
        native.state.odometer_m,
        f64::from(old_odometer as f32 + distance)
    );
    assert!((native.state.pos_on_edge_m - old_position - f64::from(distance)).abs() < 1e-12);
}

#[test]
fn native_rigid_bodies_share_head_distance_and_survive_save_continuation() {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/traction_operation");
    let content = tempfile::tempdir().unwrap();
    for directory in ["route", "consists", "vehicles"] {
        std::fs::create_dir(content.path().join(directory)).unwrap();
    }
    for file in ["route/track.toml", "consists/two_diesel.con"] {
        std::fs::copy(source.join(file), content.path().join(file)).unwrap();
    }
    // Declare the hardware in the same content used before and after restore.
    // Mutating only session.physics would leave original_physics unchanged,
    // so restore would correctly rebuild the original elastic definition.
    let engine = std::fs::read_to_string(source.join("vehicles/diesel.eng")).unwrap();
    assert_eq!(engine.matches("(Engine").count(), 1);
    let engine = engine.replacen(
        "(Engine",
        "(Engine (Coupling (CouplingHasRigidConnection 1))",
        1,
    );
    std::fs::write(content.path().join("vehicles/diesel.eng"), engine).unwrap();
    let mut scenario =
        openrailsrs_scenarios::load_scenario(source.join("scenario_two_diesel.toml")).unwrap();
    scenario.route.start_offset_m = Some(100.);
    scenario.route.stops.clear();
    scenario.simulation.native_physics = Some(Default::default());
    scenario.simulation.multi_body = true;
    let fixture = || {
        let mut s = LiveDriveSession::from_scenario(content.path(), &scenario).unwrap();
        assert!(
            s.physics
                .native
                .as_ref()
                .unwrap()
                .vehicles
                .iter()
                .all(|p| p.rigid_connection)
        );
        s.driver_direction = 1.;
        s.driver_throttle = 0.;
        s.driver_brake = 0.;
        s.state.velocity_mps = 10.;
        for body in &mut s.state.vehicles {
            body.velocity_mps = 10.;
        }
        s
    };
    let mut s = fixture();
    assert!(s.state.vehicles.len() >= 2);
    let positions = s
        .state
        .vehicles
        .iter()
        .map(|v| v.position_m)
        .collect::<Vec<_>>();
    let head = s.state.pos_on_edge_m;
    s.step_realtime(0.05, |_| {});
    let distance = f64::from(s.state.velocity_mps as f32 * 0.05_f32);
    assert!((s.state.pos_on_edge_m - head - distance).abs() < 1e-12);
    for (old, body) in positions.iter().zip(&s.state.vehicles) {
        assert_eq!(body.velocity_mps, s.state.velocity_mps);
        assert!((body.position_m - old - distance).abs() < 1e-12);
    }
    let saved = s.snapshot();
    let mut resumed = fixture();
    resumed
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    for _ in 0..20 {
        s.step_realtime(0.05, |_| {});
        resumed.step_realtime(0.05, |_| {});
        for (actual, restored) in s.state.vehicles.iter().zip(&resumed.state.vehicles) {
            assert_eq!(actual.velocity_mps, restored.velocity_mps);
            assert_eq!(actual.position_m, restored.position_m);
        }
        assert_eq!(s.state.odometer_m, resumed.state.odometer_m);
        assert_eq!(s.state.pos_on_edge_m, resumed.state.pos_on_edge_m);
    }
}
