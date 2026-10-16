use openrailsrs_scenarios::vehicle_systems::TractionSource;
use openrailsrs_sim::{CarOperation, LiveDriveSession};
fn load(example: &str, file: &str) -> (std::path::PathBuf, openrailsrs_scenarios::ScenarioFile) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(example);
    let s = openrailsrs_scenarios::load_scenario(dir.join(file)).unwrap();
    (dir, s)
}
fn drive(s: &mut LiveDriveSession) {
    s.driver_brake = 0.;
    s.driver_direction = 1.;
    s.driver_throttle = 1.;
    s.gameplay.next_stop_idx = 1;
    s.gameplay.departure_hold = false;
    s.exterior.door = openrailsrs_sim::DoorState::Closed;
}
#[test]
fn dual_propulsion_uses_exactly_one_active_source_and_diesel_drives_off_the_wire() {
    let (dir, p) = load("electric_supply", "scenario_dual.toml");
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    s.state.pos_on_edge_m = 180.;
    s.select_traction_source(0, TractionSource::Diesel).unwrap();
    drive(&mut s);
    s.step_realtime(4., |_| {});
    assert_eq!(
        s.state.vehicle_systems[0].energy.active,
        Some(TractionSource::Diesel)
    );
    assert!(s.velocity_mps() > 0.5);
    assert_eq!(s.state.vehicle_systems[0].energy.discharged_j, 0.);
    assert!(s.state.vehicle_systems[0].motors[0].power_w > 0.);
    s.driver_throttle = 0.;
    s.select_traction_source(0, TractionSource::Battery)
        .unwrap();
    s.driver_throttle = 1.;
    s.step_realtime(3., |_| {});
    assert_eq!(
        s.state.vehicle_systems[0].energy.active,
        Some(TractionSource::Battery)
    );
    assert_eq!(s.state.vehicle_systems[0].motors[0].power_w, 0.);
    assert!(s.state.vehicle_systems[0].energy.discharged_j > 0.);
    let mut restored = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    restored.restore_snapshot(s.snapshot()).unwrap();
    s.step_realtime(0.2, |_| {});
    restored.step_realtime(0.2, |_| {});
    assert_eq!(s.state.vehicle_systems, restored.state.vehicle_systems);
    assert_eq!(s.state.velocity_mps, restored.state.velocity_mps);
}
#[test]
fn finite_battery_reaches_its_reserve_then_coasts_without_unaccounted_traction() {
    let (dir, mut p) = load("electric_supply", "scenario_battery.toml");
    let b = p.train.systems[0].battery.as_mut().unwrap();
    b.capacity_kwh = 0.01;
    b.initial_fraction = 0.11;
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    s.state.pos_on_edge_m = 180.;
    s.select_traction_source(0, TractionSource::Battery)
        .unwrap();
    drive(&mut s);
    s.state.velocity_mps = 10.;
    for v in &mut s.state.vehicles {
        v.velocity_mps = 10.;
    }
    let mut coast = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    coast.state.pos_on_edge_m = 180.;
    coast
        .select_traction_source(0, TractionSource::Line)
        .unwrap();
    drive(&mut coast);
    coast.state.velocity_mps = 10.;
    for _ in 0..60 {
        s.step_realtime(0.05, |_| {});
        coast.step_realtime(0.05, |_| {});
    }
    let c = &s.state.vehicle_systems[0];
    let def = c.definition.battery.as_ref().unwrap();
    assert!(c.energy.valid(Some(def)));
    assert!(c.energy.energy_j >= def.capacity_kwh * 3_600_000. * def.minimum_fraction - 1e-6);
    assert!(s.state.rail_force_n[0] < 0.001);
    assert!(s.velocity_mps() > 1. && s.state.odometer_m > 10.);
    assert!(
        (s.velocity_mps() - coast.velocity_mps()).abs() < 0.02,
        "empty storage must follow the same inertial coasting trajectory"
    );
    assert!(c.energy.mechanical_j <= 0.01 * 3_600_000. * 0.01 * def.efficiency + 1e-5);
}
#[test]
fn hotel_bus_detachment_and_reconnection_keep_source_delays_and_save_state() {
    let (dir, p) = load("traction_operation", "scenario_hotel.toml");
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    s.step_realtime(1., |_| {});
    assert!(s.state.power_supply.vehicle_supplied(2));
    s.operate_car(1, CarOperation::Handbrake).unwrap();
    s.operate_car(2, CarOperation::Handbrake).unwrap();
    s.uncouple_after(0).unwrap();
    s.step_realtime(0.1, |_| {});
    assert!(!s.state.power_supply.vehicle_supplied(1));
    assert_eq!(s.state.power_supply.hotel_w, 0.);
    let mut restored = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    restored.restore_snapshot(s.snapshot()).unwrap();
    s.recouple().unwrap();
    restored.recouple().unwrap();
    // Manual coupling leaves the new train-bus connection disconnected until
    // the crew connects it; waiting alone must not energize the hotel cars.
    s.step_realtime(1., |_| {});
    restored.step_realtime(1., |_| {});
    assert_eq!(s.state.power_supply, restored.state.power_supply);
    assert!(!s.state.power_supply.vehicle_supplied(2));
    assert_eq!(s.state.power_supply.hotel_w, 0.);
    s.operate_car(1, CarOperation::TrainBus).unwrap();
    restored.operate_car(1, CarOperation::TrainBus).unwrap();
    s.step_realtime(1., |_| {});
    restored.step_realtime(1., |_| {});
    assert_eq!(s.state.power_supply, restored.state.power_supply);
    assert!(s.state.power_supply.vehicle_supplied(2));
}
#[test]
fn diesel_thermal_energy_is_conserved_and_faults_can_be_disabled_explicitly() {
    let (dir, p) = load("traction_operation", "scenario_equipment.toml");
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    let c = &mut s.state.vehicle_systems[0];
    let p = &c.definition.motors[0];
    let mut params = c.definition.diesel_circuits.clone().unwrap();
    params.failures_enabled = false;
    let m = &mut c.motors[0];
    m.oil_l = 0.;
    m.oil_c = 180.;
    m.coolant_c = 100.;
    let initial =
        m.oil_c * params.oil_heat_capacity_j_k + m.coolant_c * params.coolant_heat_capacity_j_k;
    for _ in 0..200 {
        m.advance(p, Some(&params), 1., 20., 0.05)
    }
    let final_energy =
        m.oil_c * params.oil_heat_capacity_j_k + m.coolant_c * params.coolant_heat_capacity_j_k;
    assert!((final_energy - initial - m.generated_heat_j + m.rejected_heat_j).abs() < 1e-5);
    assert!(m.fault.is_none());
    assert!(m.running);
    assert!(m.fan);
    let mut grouped = c.motors[1].clone();
    let mut short = grouped.clone();
    grouped.advance(p, Some(&params), 0., 20., 1.);
    for _ in 0..20 {
        short.advance(p, Some(&params), 0., 20., 0.05)
    }
    assert!((grouped.oil_c - short.oil_c).abs() < 1e-6);
    assert!((grouped.coolant_c - short.coolant_c).abs() < 1e-6);
}
#[test]
fn metered_air_compressor_cannot_charge_after_its_auxiliary_battery_is_empty() {
    let (dir, mut p) = load("traction_operation", "scenario_equipment.toml");
    p.train.systems[0].auxiliaries = Some(openrailsrs_scenarios::vehicle_systems::AuxiliaryDef {
        native_injector_mm: None,
        compressor_efficiency: 0.7,
        compressor_idle_w: 100.,
        battery_kwh: 0.0001,
        leak_bar_s: 0.02,
        steam_compressor_kg_j: 0.,
        ejector_steam_kg_s: 0.,
        injector_steam_kg_per_water_kg: 0.05,
        injector_max_pressure_bar: 20.,
        feedwater_c: 20.,
    });
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    s.operate_motor(
        0,
        0,
        openrailsrs_sim::engine_equipment::MotorOperation::StartStop,
    )
    .unwrap();
    s.operate_motor(
        0,
        1,
        openrailsrs_sim::engine_equipment::MotorOperation::StartStop,
    )
    .unwrap();
    s.operate_car(0, CarOperation::Power).unwrap();
    s.state.brake_system.circuits[0].reservoir_bar = 1.;
    s.step_realtime(10., |_| {});
    let c = &s.state.vehicle_systems[0];
    assert!(c.aux.valid());
    assert_eq!(c.aux.battery_j, 0.);
    let pressure = s.state.brake_system.circuits[0].reservoir_bar;
    s.step_realtime(2., |_| {});
    assert!(s.state.brake_system.circuits[0].reservoir_bar < pressure);
}
#[test]
fn wheel_slide_control_changes_real_cylinder_pressure_and_wet_stopping_distance() {
    fn stop(enabled: bool) -> (f64, usize, f64) {
        let (dir, mut p) = load("traction_operation", "scenario_equipment.toml");
        let def = &mut p.train.systems[0];
        def.electric_brake = None;
        def.brake_heat = None;
        def.wheel_slide.as_mut().unwrap().enabled = enabled;
        let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
        drive(&mut s);
        s.driver_throttle = 0.;
        s.driver_brake = 1.;
        s.state.velocity_mps = 20.;
        s.gameplay.stop_targets.clear();
        s.set_rail_weather(openrailsrs_sim::adhesion::RailWeather::Snow);
        let mut vents = 0;
        let mut minimum = 1_f64;
        for _ in 0..6000 {
            s.step_realtime(0.05, |_| {});
            let b = &s.state.vehicle_systems[0].brakes;
            minimum = minimum.min(b.pressure_fraction);
            vents += usize::from(b.pressure_fraction < 0.99);
            if s.velocity_mps() < 0.1 {
                break;
            }
        }
        assert!(
            s.velocity_mps() < 0.1,
            "wet stopping run did not stop: {}",
            s.velocity_mps()
        );
        (s.state.odometer_m, vents, minimum)
    }
    let protected = stop(true);
    let locked = stop(false);
    assert!(protected.1 > 0);
    assert!(protected.2 < 0.95);
    assert_eq!(locked.1, 0);
    assert_eq!(locked.2, 1.);
    assert!(protected.0.is_finite() && locked.0.is_finite());
    assert!((protected.0 - locked.0).abs() > 0.1);
    eprintln!(
        "WSP stopping metres: enabled {}, disabled {}; pressure minimum {}",
        protected.0, locked.0, protected.2
    );
}

#[test]
fn freight_speed_remains_on_alternate_paths_after_restoring() {
    let (dir, p) = load("traction_operation", "scenario_freight.toml");
    let s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    let rebuilt = openrailsrs_sim::path_data::PathData::from_path(&s.state.path_edges, &s.graph);
    assert!(rebuilt.edges.iter().all(|e| e.speed_limit_mps <= 12.5));
    let mut restored = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    restored.restore_snapshot(s.snapshot()).unwrap();
    assert!(
        restored
            .path_data
            .edges
            .iter()
            .all(|e| e.speed_limit_mps <= 12.5)
    );
}
#[test]
fn waiting_traffic_and_player_share_freezing_and_thawing_and_restore_the_same_surface() {
    let (dir, mut p) = load("player_lab", "scenario.toml");
    p.simulation.weather_track=Some(serde_json::from_str(r#"{"schema":1,"seed":42,"phases":[{"start_s":0,"transition_s":0,"kind":"rain","intensity":0.8,"temperature_c":-0.5,"wind_mps":[0,0]},{"start_s":12,"transition_s":0,"kind":"rain","intensity":0.1,"temperature_c":15,"wind_mps":[0,0]}]}"#).unwrap());
    p.extra_trains=vec![serde_json::from_value(serde_json::json!({"id":"waiting","consist":p.train.consist,"start":"yard_a","destination":"yard_b","start_time_s":100.0,"output_csv":"unused.csv"})).unwrap()];
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    let mut traffic = openrailsrs_sim::LiveTraffic::from_scenario(&dir, &p).unwrap();
    for _ in 0..200 {
        traffic.advance(&mut s, 0.05, None, |_| {});
    }
    assert!(!traffic.services[0].departed);
    let surface = s.state.surface_conditions.as_ref().unwrap();
    assert!(surface.ice_mm > 0.);
    assert!(surface.grip_multiplier() < 1.);
    assert_eq!(
        s.state.surface_conditions,
        traffic.services[0].session.state.surface_conditions
    );
    let mut restored = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    restored.restore_snapshot(s.snapshot()).unwrap();
    let mut restored_traffic = openrailsrs_sim::LiveTraffic::from_scenario(&dir, &p).unwrap();
    restored_traffic
        .restore_snapshot(traffic.snapshot())
        .unwrap();
    for _ in 0..400 {
        traffic.advance(&mut s, 0.05, None, |_| {});
        restored_traffic.advance(&mut restored, 0.05, None, |_| {});
    }
    assert_eq!(
        s.state.surface_conditions,
        restored.state.surface_conditions
    );
    assert_eq!(
        s.state.surface_conditions,
        traffic.services[0].session.state.surface_conditions
    );
    assert!(s.state.surface_conditions.as_ref().unwrap().valid());
}
#[test]
fn loaded_freight_completes_its_stations_and_passes_a_scheduled_service() {
    let (dir, p) = load("traction_operation", "scenario_freight_traffic.toml");
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    let mut traffic = openrailsrs_sim::LiveTraffic::from_scenario(&dir, &p).unwrap();
    let mut max_speed = 0_f64;
    for _ in 0..4000 {
        traffic.advance(&mut s, 0.2, Some(0.6), |_| {});
        max_speed = max_speed.max(s.velocity_mps());
        if s.arrived {
            break;
        }
    }
    assert!(
        s.arrived,
        "freight did not finish: {} m, {:?}",
        s.state.odometer_m, s.gameplay
    );
    assert!(traffic.services[0].departed);
    assert!(traffic.services[0].session.state.odometer_m > 100.);
    assert!(
        max_speed <= 12.5 + 0.5,
        "maximum freight speed: {max_speed}"
    );
    assert!(
        s.gameplay.stop_results.len() == 3
            && s.gameplay
                .stop_results
                .iter()
                .all(|r| r.arrival_speed_mps < 0.5 && r.position_error_m.abs() <= 10.)
    );
    assert_eq!(traffic.services[0].session.physics.mass_kg, 85000.);
}
#[test]
fn continuous_mandatory_unit_ramp_keeps_a_transport_delay_instead_of_debouncing() {
    let (dir, p) = load("traction_operation", "scenario_multiple_unit.toml");
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    s.state.driver_reverser = 1.;
    for i in 0..80 {
        s.state.time.0 = i as f64 * 0.05;
        s.state.throttle = i as f64 / 100.;
        openrailsrs_sim::multiple_unit::advance(&mut s.state, &s.physics, 0.05);
        let expected = if i >= 20 { (i - 20) as f64 / 100. } else { 0. };
        assert!((s.state.vehicle_systems[1].mu.throttle - expected).abs() < 1e-9);
        assert!(s.state.vehicle_systems[1].mu.valid());
    }
}

#[test]
fn short_thermal_demo_has_a_recoverable_fault_only_in_the_uncooled_motor() {
    use openrailsrs_sim::engine_equipment::{MotorFault, MotorOperation};
    let (dir, p) = load("traction_operation", "scenario_motor_fault.toml");
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    drive(&mut s);
    s.gameplay.stop_targets.clear();
    s.operate_motor(0, 0, MotorOperation::Fan).unwrap();
    for _ in 0..2400 {
        s.step_realtime(0.05, |_| {});
        if s.state.vehicle_systems[0].motors[0].fault.is_some() {
            break;
        }
    }
    let motors = &s.state.vehicle_systems[0].motors;
    assert!(matches!(
        motors[0].fault,
        Some(MotorFault::OilTemperature | MotorFault::CoolantTemperature)
    ));
    assert!(motors[1].fault.is_none());
    assert!(motors[1].power_w > 0.);
    assert!(s.operate_motor(0, 0, MotorOperation::Reset).is_err());
    s.driver_throttle = 0.;
    s.operate_motor(0, 0, MotorOperation::Fan).unwrap();
    for _ in 0..2400 {
        s.step_realtime(0.05, |_| {});
        let c = &s.state.vehicle_systems[0];
        if c.motors[0]
            .cause(c.definition.diesel_circuits.as_ref().unwrap())
            .is_none()
        {
            break;
        }
    }
    s.operate_motor(0, 0, MotorOperation::Reset).unwrap();
    assert!(s.state.vehicle_systems[0].motors[0].fault.is_none());
}

#[test]
fn prolonged_downhill_braking_heats_and_recovers_with_a_conserved_thermal_ledger() {
    let (dir, mut p) = load("traction_operation", "scenario_equipment.toml");
    p.train.systems[0].electric_brake = None;
    let route = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(dir.join("route/track.toml"))
        .unwrap()
        .replace(
            "length_m = 500.0",
            "length_m = 100000.0\ngrade_percent = -8.0",
        );
    std::fs::write(route.path().join("track.toml"), text).unwrap();
    p.route.path = route.path().to_str().unwrap().to_string();
    p.route.stops.clear();
    let mut s = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    drive(&mut s);
    s.gameplay.next_stop_idx = 0;
    s.driver_throttle = 0.;
    s.state.velocity_mps = 20.;
    for i in 0..2400 {
        s.driver_brake = if i % 200 < 150 { 0.4 } else { 0.0 };
        s.step_realtime(0.05, |_| {});
    }
    let c = &s.state.vehicle_systems[0];
    let b = &c.brakes;
    let capacity = c.definition.brake_heat.as_ref().unwrap().heat_capacity_j_k;
    assert!(
        b.temperature_c > 100.,
        "downhill temperature {}",
        b.temperature_c
    );
    assert!((b.heat_j - b.cooled_j - capacity * (b.temperature_c - 20.)).abs() < 0.01);
    let hot = b.temperature_c;
    let mut restored = LiveDriveSession::from_scenario(&dir, &p).unwrap();
    restored.restore_snapshot(s.snapshot()).unwrap();
    assert_eq!(restored.state.vehicle_systems, s.state.vehicle_systems);
    s.driver_brake = 0.;
    s.state.velocity_mps = 0.;
    s.state.vehicle_systems[0].brakes.friction_force_n = 0.;
    for _ in 0..2000 {
        openrailsrs_sim::brake_equipment::account(&mut s.state, 0., 0.05);
    }
    assert!(s.state.vehicle_systems[0].brakes.temperature_c < hot);
}
