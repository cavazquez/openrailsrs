use openrailsrs_sim::{LiveDriveSession, engine_equipment::MotorOperation};
fn session(example: &str, file: &str) -> LiveDriveSession {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(example);
    let s = openrailsrs_scenarios::load_scenario(dir.join(file)).unwrap();
    LiveDriveSession::from_scenario(&dir, &s).unwrap()
}
#[test]
fn independent_motors_change_actual_traction_without_burning_a_second_tank() {
    let mut s = session("traction_operation", "scenario_equipment.toml");
    s.driver_brake = 0.;
    s.driver_direction = 1.;
    s.driver_throttle = 1.;
    s.gameplay.next_stop_idx = 1;
    s.gameplay.departure_hold = false;
    s.exterior.door = openrailsrs_sim::DoorState::Closed;
    s.step_realtime(6., |_| {});
    let before = s.state.vehicle_systems[0].motors.clone();
    assert!(
        s.velocity_mps() > 0.,
        "{} · {} · {} · {} · {:?} · {:?}",
        s.traction_diagnosis(),
        s.state.throttle,
        s.state.brake,
        s.gameplay.departure_hold,
        s.exterior.door,
        s.state.motor_force_n
    );
    assert!(before.iter().all(|m| m.power_w > 100000.));
    let fuel = s.state.fuel_consumption_g
        - s.state
            .diesel
            .cars
            .iter()
            .filter(|c| c.vehicle != 0)
            .map(|c| c.consumed_l * 850.8)
            .sum::<f64>();
    assert_eq!(
        s.state
            .diesel
            .cars
            .iter()
            .find(|c| c.vehicle == 0)
            .unwrap()
            .consumed_l,
        0.
    );
    assert!((fuel - before.iter().map(|m| m.consumed_l).sum::<f64>() * 850.8).abs() < 0.001);
    s.operate_motor(0, 0, MotorOperation::Isolate).unwrap();
    s.step_realtime(1., |_| {});
    let motors = &s.state.vehicle_systems[0].motors;
    assert_eq!(motors[0].power_w, 0.);
    assert!(motors[1].power_w > 100000.);
    assert!(motors[1].fuel_l < before[1].fuel_l);
    let mut restored = session("traction_operation", "scenario_equipment.toml");
    restored.restore_snapshot(s.snapshot()).unwrap();
    s.step_realtime(1., |_| {});
    restored.step_realtime(1., |_| {});
    assert_eq!(s.state.vehicle_systems, restored.state.vehicle_systems);
}
#[test]
fn physical_fault_is_optional_specific_and_cannot_be_reset_before_recovery() {
    let mut s = session("traction_operation", "scenario_equipment.toml");
    s.state.vehicle_systems[0].motors[0].oil_l = 0.;
    s.step_realtime(0.1, |_| {});
    assert_eq!(
        s.state.vehicle_systems[0].motors[0].fault,
        Some(openrailsrs_sim::engine_equipment::MotorFault::OilLevel)
    );
    assert!(s.state.vehicle_systems[0].motors[1].fault.is_none());
    assert!(s.operate_motor(0, 0, MotorOperation::Reset).is_err());
    s.state.vehicle_systems[0].motors[0].oil_l = 200.;
    s.operate_motor(0, 0, MotorOperation::Reset).unwrap();
    assert!(s.state.vehicle_systems[0].motors[0].fault.is_none());
}
#[test]
fn battery_crosses_dead_line_draws_once_and_recovers_from_line_after_restore() {
    use openrailsrs_scenarios::vehicle_systems::TractionSource;
    let mut s = session("electric_supply", "scenario_battery.toml");
    s.select_traction_source(0, TractionSource::Battery)
        .unwrap();
    s.driver_brake = 0.;
    s.driver_direction = 1.;
    s.driver_throttle = 1.;
    s.gameplay.next_stop_idx = 1;
    s.gameplay.departure_hold = false;
    s.exterior.door = openrailsrs_sim::DoorState::Closed;
    s.state.pos_on_edge_m = 180.;
    s.step_realtime(2., |_| {});
    let e = &s.state.vehicle_systems[0].energy;
    assert_eq!(e.active, Some(TractionSource::Battery));
    assert!(
        s.velocity_mps() > 0.,
        "{} · {} · {} · {} · {:?} · {:?}",
        s.traction_diagnosis(),
        s.state.throttle,
        s.state.brake,
        s.gameplay.departure_hold,
        s.exterior.door,
        s.state.motor_force_n
    );
    assert!(
        e.discharged_j > 0.,
        "battery motor {:?}, rail {:?}, max {}, curves {:?}",
        s.state.motor_force_n,
        s.state.rail_force_n,
        s.physics.max_tractive_effort_n,
        s.physics.electric.fallback_cars
    );
    assert!(e.valid(s.state.vehicle_systems[0].definition.battery.as_ref()));
    let mut restored = session("electric_supply", "scenario_battery.toml");
    restored.restore_snapshot(s.snapshot()).unwrap();
    s.step_realtime(0.5, |_| {});
    restored.step_realtime(0.5, |_| {});
    assert_eq!(s.state.vehicle_systems, restored.state.vehicle_systems);
    s.driver_throttle = 0.;
    s.state.pos_on_edge_m = 10.;
    s.select_traction_source(0, TractionSource::Auto).unwrap();
    s.step_realtime(3., |_| {});
    assert_eq!(
        s.state.vehicle_systems[0].energy.active,
        Some(TractionSource::Line)
    );
    assert!(s.state.vehicle_systems[0].energy.charged_j > 0.);
}

#[test]
fn regenerative_receptivity_rejection_and_rheostat_preserve_total_brake_force() {
    let mut s = session("electric_supply", "scenario_battery.toml");
    s.state.throttle = 0.;
    s.state.brake = 1.;
    s.state.electric_brake_command = 1.;
    openrailsrs_sim::electric::advance(&mut s.state, &s.path_data, &s.physics.electric, 0.);
    openrailsrs_sim::power_supply::advance(&mut s.state, &s.physics.power_supply);
    for _ in 0..25 {
        openrailsrs_sim::brake_equipment::forces(
            &mut s.state,
            &s.physics,
            20.,
            0.05,
            vec![100000.],
        );
        s.state.time.0 += 0.05;
    }
    let b = s.state.vehicle_systems[0].brakes.clone();
    assert!(b.regenerative_w > 0. && b.regenerative_w <= 100000.001);
    assert!(b.rheostatic_w > 0.);
    assert!((b.friction_force_n + b.electric_force_n - 100000.).abs() < 1e-6);
    assert!((b.regenerative_w + b.rheostatic_w - b.electric_force_n * 20.).abs() < 1e-6);
    s.state.electric.cars[0].main_power = false;
    let forces = openrailsrs_sim::brake_equipment::forces(
        &mut s.state,
        &s.physics,
        20.,
        0.05,
        vec![100000.],
    );
    assert_eq!(s.state.vehicle_systems[0].brakes.regenerative_w, 0.);
    assert!((forces[0] - 100000.).abs() < 1e-6);
}
#[test]
fn hot_brakes_conserve_heat_and_recover_on_cooling_instead_of_resetting() {
    let mut s = session("traction_operation", "scenario_equipment.toml");
    let capacity = s.state.vehicle_systems[0]
        .definition
        .brake_heat
        .as_ref()
        .unwrap()
        .heat_capacity_j_k;
    for _ in 0..400 {
        s.state.vehicle_systems[0].brakes.friction_force_n = 100000.;
        openrailsrs_sim::brake_equipment::account(&mut s.state, 25., 0.05);
    }
    let b = s.state.vehicle_systems[0].brakes.clone();
    assert!(b.temperature_c > 150.);
    assert!((b.heat_j - b.cooled_j - capacity * (b.temperature_c - 20.)).abs() < 1e-5);
    let hot = openrailsrs_sim::brake_equipment::forces(
        &mut s.state,
        &s.physics,
        0.1,
        0.05,
        vec![100000., 0.],
    )[0];
    s.state.vehicle_systems[0].brakes.friction_force_n = 0.;
    for _ in 0..30000 {
        openrailsrs_sim::brake_equipment::account(&mut s.state, 0., 0.05);
    }
    let cool = openrailsrs_sim::brake_equipment::forces(
        &mut s.state,
        &s.physics,
        0.1,
        0.05,
        vec![100000., 0.],
    )[0];
    assert!(cool > hot);
    assert!(s.state.vehicle_systems[0].valid());
}
#[test]
fn wheel_slide_vents_pressure_and_traction_control_recovers_smoothly() {
    let mut s = session("traction_operation", "scenario_equipment.toml");
    s.set_rail_weather(openrailsrs_sim::adhesion::RailWeather::Snow);
    for c in &mut s.state.rail_adhesion.as_mut().unwrap().cars {
        c.factor = 0.05;
    }
    for _ in 0..100 {
        openrailsrs_sim::brake_equipment::forces(
            &mut s.state,
            &s.physics,
            20.,
            0.05,
            vec![500000., 0.],
        );
    }
    assert!(s.state.vehicle_systems[0].brakes.pressure_fraction < 0.95);
    let rail = s.state.rail_adhesion.as_mut().unwrap();
    rail.cars[0].wheel_speed_mps = 40.;
    for _ in 0..15 {
        openrailsrs_sim::brake_equipment::prepare_traction(&mut s.state, 2, 20., 0.05);
    }
    let cut = s.state.vehicle_systems[0].brakes.traction_fraction;
    assert!(cut < 0.1);
    s.state.rail_adhesion.as_mut().unwrap().cars[0].wheel_speed_mps = 20.;
    openrailsrs_sim::brake_equipment::prepare_traction(&mut s.state, 2, 20., 0.05);
    assert!(s.state.vehicle_systems[0].brakes.traction_fraction > cut);
    assert!(s.state.vehicle_systems[0].brakes.traction_fraction < cut + 0.02);
}

#[test]
fn native_multi_motor_profiles_keep_different_governors_and_independent_shutdown() {
    let mut s = session("traction_operation", "scenario_native_motors.toml");
    assert_eq!(s.state.vehicle_systems[0].motors.len(), 2);
    assert_eq!(s.state.vehicle_systems[0].motors[0].rpm, 300.);
    assert_eq!(s.state.vehicle_systems[0].motors[1].rpm, 400.);
    s.driver_brake = 0.;
    s.driver_throttle = 1.;
    s.driver_direction = 1.;
    s.gameplay.next_stop_idx = 1;
    s.gameplay.departure_hold = false;
    s.exterior.door = openrailsrs_sim::DoorState::Closed;
    s.step_realtime(4., |_| {});
    assert!(s.velocity_mps() > 0.);
    s.toggle_diesel_engine(0).unwrap();
    s.step_realtime(0.2, |_| {});
    assert!(!s.state.vehicle_systems[0].motors[0].running);
    assert!(s.state.vehicle_systems[0].motors[1].running);
    let mut restored = session("traction_operation", "scenario_native_motors.toml");
    restored.restore_snapshot(s.snapshot()).unwrap();
    s.step_realtime(0.2, |_| {});
    restored.step_realtime(0.2, |_| {});
    assert_eq!(s.state.vehicle_systems, restored.state.vehicle_systems);
}
#[test]
fn multiple_unit_delays_survive_save_and_incompatible_protocols_never_drive_remote_unit() {
    let mut s = session("traction_operation", "scenario_multiple_unit.toml");
    s.state.throttle = 0.8;
    s.state.driver_reverser = 1.;
    openrailsrs_sim::multiple_unit::advance(&mut s.state, &s.physics, 0.05);
    assert_eq!(s.state.vehicle_systems[0].mu.throttle, 0.8);
    assert_eq!(s.state.vehicle_systems[1].mu.throttle, 0.);
    s.state.time.0 = 0.5;
    let mut r = session("traction_operation", "scenario_multiple_unit.toml");
    r.restore_snapshot(s.snapshot()).unwrap();
    for state in [&mut s, &mut r] {
        state.state.time.0 = 1.1;
        openrailsrs_sim::multiple_unit::advance(&mut state.state, &state.physics, 0.05);
    }
    assert_eq!(s.state.vehicle_systems, r.state.vehicle_systems);
    assert_eq!(s.state.vehicle_systems[1].mu.throttle, 0.8);
    assert_eq!(s.state.vehicle_systems[1].mu.reverser, 1.);
    s.state.vehicle_systems[1]
        .definition
        .multiple_unit
        .as_mut()
        .unwrap()
        .compatible_protocols
        .clear();
    s.state.vehicle_systems[1]
        .definition
        .multiple_unit
        .as_mut()
        .unwrap()
        .protocol = "isolated".into();
    openrailsrs_sim::multiple_unit::advance(&mut s.state, &s.physics, 0.05);
    assert_eq!(s.state.vehicle_systems[1].mu.throttle, 0.);
    assert!(!s.state.vehicle_systems[1].mu.compatible);
}
#[test]
fn separate_steam_boilers_and_injectors_keep_mass_and_cab_ownership_after_save() {
    let mut s = session("traction_operation", "scenario_steam_equipment.toml");
    assert_eq!(s.state.steam_boilers.len(), 2);
    s.state
        .boiler_state
        .as_mut()
        .unwrap()
        .controls
        .automatic_fireman = false;
    s.state.boiler_state.as_mut().unwrap().controls.injector1 = true;
    s.state.boiler_state.as_mut().unwrap().water_kg = 2500.;
    let tender = s.state.boiler_state.as_ref().unwrap().tender_water_kg;
    s.step_realtime(1., |_| {});
    assert!(s.state.boiler_state.as_ref().unwrap().tender_water_kg < tender);
    assert!(s.state.vehicle_systems[0].aux.injector_water_kg > 0.);
    assert!(
        !s.state.steam_boilers[1].boiler.controls.injector1
            || s.state.steam_boilers[1].boiler.controls.automatic_fireman
    );
    s.driver_throttle = 0.;
    s.driver_direction = 0.5;
    s.state.velocity_mps = 0.;
    s.select_cab(1).unwrap();
    assert_eq!(
        s.cab_telemetry().boiler_bar,
        Some(s.state.steam_boilers[1].boiler.pressure_bar)
    );
    let mut restored = session("traction_operation", "scenario_steam_equipment.toml");
    restored.restore_snapshot(s.snapshot()).unwrap();
    s.step_realtime(0.1, |_| {});
    restored.step_realtime(0.1, |_| {});
    assert_eq!(s.state.steam_boilers, restored.state.steam_boilers);
}
#[test]
fn freight_payload_changes_inertia_and_service_limit_without_changing_empty_author_mass() {
    let s = session("traction_operation", "scenario_freight.toml");
    assert_eq!(s.formation.cars[1].mass_kg, 45000.);
    assert_eq!(s.physics.mass_kg, 85000.);
    assert!(s.effective_speed_limit_mps() <= 12.5);
    let mut restored = session("traction_operation", "scenario_freight.toml");
    restored.restore_snapshot(s.snapshot()).unwrap();
    assert_eq!(restored.physics.mass_kg, s.physics.mass_kg);
}
