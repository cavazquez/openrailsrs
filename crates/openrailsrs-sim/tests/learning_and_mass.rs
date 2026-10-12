use openrailsrs_sim::{
    LiveDriveSession,
    learning::{LearningState, Lesson},
};
use std::path::PathBuf;

fn session(file: &str) -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation")
        .join(file);
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}

#[test]
fn consumed_and_refilled_fuel_changes_actual_inertia_and_survives_resume() {
    let mut s = session("scenario.toml");
    let initial = s.effective_mass_kg();
    s.step_realtime(10., |_| {});
    assert!((initial - s.effective_mass_kg() - 0.05 * 0.8508).abs() < 1e-6);
    let before_position = s.pos_on_edge_m();
    let before_speed = s.velocity_mps();
    let fuel = &mut s.state.diesel.cars[0];
    let add = 0.05;
    fuel.fuel_l += add;
    fuel.refilled_l += add;
    assert!((s.effective_mass_kg() - initial).abs() < 1e-6);
    assert_eq!(before_position, s.pos_on_edge_m());
    assert_eq!(before_speed, s.velocity_mps());
    let mut resumed = session("scenario.toml");
    resumed.restore_snapshot(s.snapshot()).unwrap();
    assert_eq!(resumed.effective_mass_kg(), s.effective_mass_kg());
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 0.5;
    resumed.driver_direction = 1.;
    resumed.driver_brake = 0.;
    resumed.driver_throttle = 0.5;
    s.step_realtime(5., |_| {});
    resumed.step_realtime(5., |_| {});
    assert_eq!(s.velocity_mps(), resumed.velocity_mps());
}

#[test]
fn lesson_reads_physical_doors_and_neutral_and_never_commands_train() {
    let mut s = session("scenario.toml");
    s.set_direction(0.5).unwrap();
    s.driver_throttle = 0.;
    let lesson = LearningState::new(Lesson::Basic, &s);
    s.state.learning = Some(lesson);
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 1);
    s.step_realtime(1., |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 1);
    assert_eq!(s.driver_throttle, 0.);
    assert_eq!(s.driver_direction, 0.5);
    s.exterior.toggle_door();
    s.step_realtime(1.1, |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 2);
    let saved = s.snapshot();
    let mut resumed = session("scenario.toml");
    resumed.restore_snapshot(saved).unwrap();
    resumed.exterior.toggle_door();
    resumed.step_realtime(1.1, |_| {});
    assert_eq!(resumed.state.learning.as_ref().unwrap().stage, 3);
    assert!(resumed.state.learning.as_ref().unwrap().valid());
}

#[test]
fn report_counts_short_brake_events_and_resume_preserves_graph_and_totals() {
    let mut s = session("scenario.toml");
    s.driver_brake = 0.;
    s.step_realtime(0.1, |_| {});
    s.driver_brake = 0.5;
    s.step_realtime(0.1, |_| {});
    s.driver_brake = 0.;
    s.step_realtime(0.1, |_| {});
    s.driver_brake = 0.6;
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.driving_report.braking_events, 2);
    assert!(s.state.driving_report.braking_s >= 0.15);
    assert_eq!(s.state.driving_report.samples.len(), 1);
    let mut resumed = session("scenario.toml");
    resumed.restore_snapshot(s.snapshot()).unwrap();
    resumed.step_realtime(1., |_| {});
    s.step_realtime(1., |_| {});
    assert_eq!(
        serde_json::to_string(&s.state.driving_report).unwrap(),
        serde_json::to_string(&resumed.state.driving_report).unwrap()
    );
    assert!(s.state.driving_report.valid());
}

fn equipped_session() -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario.train.cab_equipment = vec![openrailsrs_scenarios::cab_equipment::CabEquipmentDef {
        vehicle: 0,
        left_window: true,
        right_window: true,
        cab_door: true,
        washer_capacity_l: 0.1,
        washer_flow_lps: 0.02,
    }];
    LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}
#[test]
fn washer_conserves_reserve_stops_without_battery_and_resume_keeps_apertures() {
    use openrailsrs_sim::{CarOperation, cab_equipment::CabOperation};
    let mut s = equipped_session();
    s.operate_cab(CabOperation::LeftWindow).unwrap();
    s.operate_cab(CabOperation::Door).unwrap();
    s.operate_cab(CabOperation::Washer).unwrap();
    s.step_realtime(2., |_| {});
    let cab = s.cab_equipment().unwrap();
    assert_eq!(cab.windows[0], 1.);
    assert_eq!(cab.door, 1.);
    assert!((cab.water_l - 0.06).abs() < 1e-9);
    s.operate_car(0, CarOperation::Battery).unwrap();
    let before = s.cab_equipment().unwrap().water_l;
    s.step_realtime(1., |_| {});
    assert_eq!(s.cab_equipment().unwrap().water_l, before);
    assert!(!s.cab_equipment().unwrap().spraying);
    let mut resumed = equipped_session();
    resumed.restore_snapshot(s.snapshot()).unwrap();
    assert_eq!(resumed.cab_equipment().unwrap().windows, [1., 0.]);
    assert_eq!(resumed.cab_equipment().unwrap().water_l, before);
    resumed.operate_car(0, CarOperation::Battery).unwrap();
    resumed.step_realtime(5., |_| {});
    assert_eq!(resumed.cab_equipment().unwrap().water_l, 0.);
    assert!(!resumed.cab_equipment().unwrap().spraying);
    assert!((resumed.cab_equipment().unwrap().used_l - 0.1).abs() < 1e-9);
    let mut unsupported = session("scenario.toml");
    assert!(unsupported.operate_cab(CabOperation::Washer).is_err());
}

#[test]
fn fuel_reserve_changes_acceleration_under_the_same_controls_and_brake_force() {
    let mut full = session("scenario.toml");
    let mut light = session("scenario.toml");
    light.state.diesel.cars[0].fuel_l = 5.;
    let expected = (25. - 5.) * 0.8508;
    assert!((full.effective_mass_kg() - light.effective_mass_kg() - expected).abs() < 1e-6);
    for s in [&mut full, &mut light] {
        s.gameplay.stop_targets.clear();
        s.driver_direction = 1.;
        s.driver_brake = 0.;
        s.driver_throttle = 0.2;
    }
    full.step_realtime(10., |_| {});
    light.step_realtime(10., |_| {});
    assert!(
        light.velocity_mps() > full.velocity_mps(),
        "fuel mass must affect inertia: {} vs {}",
        light.velocity_mps(),
        full.velocity_mps()
    );
    assert!(light.pos_on_edge_m() > full.pos_on_edge_m());
    assert_eq!(light.physics.max_brake_n, full.physics.max_brake_n);
}

#[test]
fn electric_lesson_tracks_actual_pantograph_breaker_cut_and_recovery() {
    use openrailsrs_sim::electric::PowerLoss;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/electric_supply/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario.route.stops.clear();
    let mut s = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    s.driver_throttle = 0.;
    s.set_direction(0.5).unwrap();
    s.state.learning = Some(LearningState::new(Lesson::Electric, &s));
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 1);
    s.toggle_pantograph().unwrap();
    s.step_realtime(3., |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 2);
    s.toggle_pantograph().unwrap();
    s.step_realtime(3., |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 3);
    s.toggle_circuit_breaker().unwrap();
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 4);
    s.toggle_circuit_breaker().unwrap();
    s.step_realtime(2., |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 5);
    s.set_direction(1.).unwrap();
    s.driver_throttle = 0.6;
    s.driver_brake = 0.;
    let mut cut = false;
    let mut recovered = false;
    for _ in 0..1200 {
        s.step_realtime(0.1, |_| {});
        cut |= s.state.electric.cars[0].loss == PowerLoss::NoSupply;
        if cut && s.state.learning.as_ref().unwrap().stage == 8 {
            recovered = true;
            break;
        }
    }
    assert!(
        cut && recovered,
        "lesson must wait for the physical neutral section, stage {}",
        s.state.learning.as_ref().unwrap().stage
    );
    s.driver_throttle = 0.;
    s.driver_brake = 0.7;
    s.step_realtime(30., |_| {});
    assert!(s.state.learning.as_ref().unwrap().completed_s.is_some());
    let mut resumed = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    resumed.restore_snapshot(s.snapshot()).unwrap();
    assert!(
        resumed
            .state
            .learning
            .as_ref()
            .unwrap()
            .completed_s
            .is_some()
    );
}

#[test]
fn prepared_emergency_requires_real_stop_rearm_and_pipe_recharge() {
    let mut s = session("scenario.toml");
    s.gameplay.stop_targets.clear();
    s.state.velocity_mps = 12.;
    s.state.brake_system.emergency = true;
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    s.state.learning = Some(LearningState::new(Lesson::Emergency, &s));
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 1);
    assert!(s.reset_emergency().is_err());
    s.step_realtime(30., |_| {});
    assert_eq!(s.velocity_mps(), 0.);
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 2);
    s.reset_emergency().unwrap();
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 3);
    s.driver_brake = 0.;
    s.step_realtime(15., |_| {});
    assert_eq!(s.state.learning.as_ref().unwrap().stage, 4);
    s.set_direction(1.).unwrap();
    s.driver_throttle = 0.3;
    s.step_realtime(6., |_| {});
    assert!(s.state.learning.as_ref().unwrap().completed_s.is_some());
}

#[test]
fn destination_uses_declared_service_and_actual_direction() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario.route.stops.clear();
    scenario.route.legs = vec![openrailsrs_scenarios::model::RouteLeg {
        service_id: Some("OUTBOUND".into()),
        not_before_s: None,
        operations: vec![],
        start: "a".into(),
        destination: "mid".into(),
        edges: vec!["e1".into()],
        start_offset_m: 0.,
        end_chainage_m: 200.,
        reversed: false,
        wait_s: 0.,
        stops: vec![],
        switches: vec![],
    }];
    // Verify label identity independently of leg advancement, which has its own
    // full train location/return service regressions in operational_path.
    let mut s = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    assert!(s.destination_display().contains("OUTBOUND"));
    assert!(s.destination_display().contains("→"));
    s.state.path_reversed = true;
    assert!(s.destination_display().contains("←"));
}

#[test]
fn braking_coach_accounts_for_grade_grip_and_tuned_mass_without_driving() {
    use openrailsrs_sim::{
        adhesion::{RailAdhesionConfig, RailAdhesionState, RailWeather},
        learning::braking_advice,
    };
    let mut s = session("scenario.toml");
    s.gameplay.next_stop_idx = 1;
    s.state.velocity_mps = 15.;
    s.driver_throttle = 0.4;
    s.driver_brake = 0.2;
    let before = serde_json::to_string(&s.snapshot()).unwrap();
    let controls = (s.driver_throttle, s.driver_brake, s.driver_direction);
    let dry = braking_advice(&s).unwrap();
    assert_eq!(serde_json::to_string(&s.snapshot()).unwrap(), before);
    assert_eq!(
        (s.driver_throttle, s.driver_brake, s.driver_direction),
        controls
    );
    s.path_data.edges[0].grade_percent = -3.;
    let downhill = braking_advice(&s).unwrap();
    assert!(downhill.distance_m > dry.distance_m);
    s.path_data.edges[0].grade_percent = 3.;
    assert!(braking_advice(&s).unwrap().distance_m < dry.distance_m);
    s.path_data.edges[0].grade_percent = 0.;
    s.state.rail_adhesion = Some(RailAdhesionState::new(
        &RailAdhesionConfig::default(),
        RailWeather::Snow,
        15.,
    ));
    assert!(braking_advice(&s).unwrap().distance_m > dry.distance_m);

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario
        .train
        .vehicle_profiles
        .push(openrailsrs_core::vehicle_profile::VehicleProfile {
            source_sha256: openrailsrs_train::consist_audit::source_fingerprint(
                &path.parent().unwrap().join("vehicles/diesel.eng"),
            )
            .unwrap(),
            mass_factor: 2.,
            ..Default::default()
        });
    let mut heavy = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    heavy.gameplay.next_stop_idx = 1;
    heavy.state.velocity_mps = 15.;
    assert_eq!(heavy.physics.max_brake_n, s.physics.max_brake_n);
    assert!(heavy.effective_mass_kg() > s.effective_mass_kg());
    assert!(braking_advice(&heavy).unwrap().distance_m > dry.distance_m);
}

#[test]
fn braking_coach_announces_insufficient_braking_and_has_no_advice_without_target() {
    use openrailsrs_sim::learning::braking_advice;
    let mut s = session("scenario.toml");
    s.gameplay.next_stop_idx = 1;
    s.state.velocity_mps = 15.;
    s.path_data.edges[0].grade_percent = -30.;
    let advice = braking_advice(&s).unwrap();
    assert!(!advice.distance_m.is_finite());
    assert!(advice.message.contains("insuficiente"));
    s.gameplay.stop_targets.clear();
    assert!(braking_advice(&s).is_none());
    s.state.velocity_mps = 0.;
    assert!(braking_advice(&s).is_none());
}
