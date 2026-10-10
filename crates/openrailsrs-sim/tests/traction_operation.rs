use openrailsrs_sim::steam::{BoilerState, SteamCommand, steam_step};
use openrailsrs_sim::{CarOperation, LiveDriveSession, diesel_operation::EnginePhase};
use std::path::PathBuf;

fn session(file: &str) -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation")
        .join(file);
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}
fn advance(s: &mut LiveDriveSession, seconds: f64) {
    s.step_realtime(seconds, |_| {});
}
fn steam_params() -> openrailsrs_train::SteamParams {
    session("scenario_steam.toml").physics.steam_params.unwrap()
}

#[test]
fn native_supply_delays_survive_save_and_battery_gates_cab_lights() {
    let mut s = session("scenario.toml");
    s.headlights = 2;
    s.cab_light = true;
    advance(&mut s, 0.1);
    assert!(s.cab_telemetry().main_power);
    assert_eq!(s.effective_headlights(), 2);
    s.toggle_diesel_engine(0).unwrap();
    s.operate_car(0, CarOperation::Battery).unwrap();
    advance(&mut s, 0.1);
    assert_eq!(s.effective_headlights(), 0);
    assert!(!s.effective_cab_light());
    assert!(!s.cab_telemetry().main_power);
    advance(&mut s, 10.);
    s.operate_car(0, CarOperation::Battery).unwrap();
    s.toggle_diesel_engine(0).unwrap();
    advance(&mut s, 8.);
    let saved = s.snapshot();
    let mut resumed = session("scenario.toml");
    resumed.restore_snapshot(saved).unwrap();
    advance(&mut s, 3.);
    advance(&mut resumed, 3.);
    assert_eq!(s.state.power_supply, resumed.state.power_supply);
    assert!(resumed.cab_telemetry().main_power);
    assert!(resumed.cab_telemetry().auxiliary_power);
    assert_eq!(resumed.effective_headlights(), 2);
    assert!(resumed.effective_cab_light());
    let mut invalid = resumed.snapshot();
    invalid.state.power_supply.cars[0].supply.main_started_s = Some(f64::INFINITY);
    assert!(resumed.restore_snapshot(invalid).is_err());
}

#[test]
fn steam_mechanics_and_boiler_continue_with_battery_off() {
    let mut s = session("scenario_steam.toml");
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 0.5;
    s.headlights = 2;
    s.cab_light = true;
    s.wiper_active = true;
    s.operate_car(0, CarOperation::Battery).unwrap();
    advance(&mut s, 8.);
    assert!(s.physics.steam_params.is_some());
    assert!(s.cab_telemetry().main_power);
    assert!(s.velocity_mps() > 0.1);
    assert!(s.state.boiler_state.as_ref().unwrap().tractive_force_n > 0.);
    assert_eq!(s.effective_headlights(), 0);
    assert!(!s.effective_cab_light());
    assert!(!s.effective_wiper_active());
    let mut resumed = session("scenario_steam.toml");
    resumed.restore_snapshot(s.snapshot()).unwrap();
    advance(&mut s, 2.);
    advance(&mut resumed, 2.);
    assert_eq!(s.state.power_supply, resumed.state.power_supply);
    assert_eq!(s.state.boiler_state, resumed.state.boiler_state);
}

#[test]
fn diesel_idle_burns_authored_fuel_and_stopping_cuts_traction() {
    let mut s = session("scenario.toml");
    advance(&mut s, 10.);
    assert!((s.state.diesel.cars[0].fuel_l - 24.95).abs() < 1e-8);
    assert!((s.state.fuel_consumption_g - 0.05 * 850.8).abs() < 1e-6);
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 0.5;
    advance(&mut s, 8.);
    assert!(s.velocity_mps() > 1.);
    s.toggle_diesel_engine(0).unwrap();
    assert!(!s.state.diesel.power_available(0));
    let distance = s.state.odometer_m;
    advance(&mut s, 2.);
    assert!(
        s.state.odometer_m > distance,
        "stopping the engine must allow coasting"
    );
    assert!(s.state.diesel_traction_force_n.iter().all(|f| *f == 0.));
    advance(&mut s, 8.);
    assert_eq!(s.state.diesel.cars[0].phase, EnginePhase::Stopped);
    let fuel = s.state.diesel.cars[0].fuel_l;
    advance(&mut s, 5.);
    assert_eq!(s.state.diesel.cars[0].fuel_l, fuel);
}

#[test]
fn default_chiltern_player_formation_exposes_both_diesel_motors() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern_extended/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    let mut s = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    assert_eq!(
        s.physics
            .diesel
            .cars
            .iter()
            .map(|c| c.vehicle)
            .collect::<Vec<_>>(),
        vec![0, 7]
    );
    assert!((s.state.diesel.cars[0].fuel_l - 250. * 4.54609).abs() < 1e-6);
    assert!((s.state.diesel.cars[1].fuel_l - 830. * 3.785411784).abs() < 1e-6);
    s.toggle_diesel_engine(0).unwrap();
    advance(&mut s, 20.);
    assert_eq!(s.state.diesel.cars[0].phase, EnginePhase::Stopped);
    assert_eq!(s.state.diesel.cars[1].phase, EnginePhase::Running);
    assert!(s.state.diesel.cars[1].consumed_l > 0.);
}

#[test]
fn diesel_start_needs_closed_regulator_battery_and_fuel_and_takes_time() {
    let mut s = session("scenario.toml");
    s.toggle_diesel_engine(0).unwrap();
    advance(&mut s, 10.);
    s.driver_throttle = 0.5;
    assert!(s.toggle_diesel_engine(0).unwrap_err().contains("regulador"));
    s.driver_throttle = 0.;
    s.operate_car(0, CarOperation::Battery).unwrap();
    assert!(s.toggle_diesel_engine(0).unwrap_err().contains("batería"));
    s.operate_car(0, CarOperation::Battery).unwrap();
    s.toggle_diesel_engine(0).unwrap();
    assert_eq!(s.state.diesel.cars[0].phase, EnginePhase::Starting);
    advance(&mut s, 0.5);
    assert_eq!(s.state.diesel.cars[0].phase, EnginePhase::Starting);
    assert!(!s.state.diesel.power_available(0));
    advance(&mut s, 8.);
    assert_eq!(s.state.diesel.cars[0].phase, EnginePhase::Running);
    assert!(s.state.diesel.power_available(0));
}

#[test]
fn empty_diesel_tank_is_finite_and_cannot_be_restarted_or_reset_in_a_save() {
    let mut s = session("scenario_low_fuel.toml");
    advance(&mut s, 22.);
    let d = &s.state.diesel.cars[0];
    assert_eq!(d.fuel_l, 0.);
    assert!((d.consumed_l - 0.2).abs() < 1e-10);
    assert!(!s.state.diesel.power_available(0));
    assert!(s.toggle_diesel_engine(0).unwrap_err().contains("vacío"));
    let saved = s.snapshot();
    let mut fresh = session("scenario_low_fuel.toml");
    fresh.restore_snapshot(saved.clone()).unwrap();
    assert_eq!(fresh.state.diesel.cars[0].fuel_l, 0.);
    let mut invalid = saved.clone();
    invalid.state.diesel = Default::default();
    assert!(
        fresh.restore_snapshot(invalid).is_err(),
        "removing resource state must not refill a tank"
    );
    let mut invalid = saved;
    invalid.state.diesel.cars[0].fuel_l = 0.2;
    assert!(
        fresh.restore_snapshot(invalid).is_err(),
        "fuel balance must be validated"
    );
}

#[test]
fn each_diesel_keeps_identity_and_fuel_when_another_motor_is_isolated() {
    let mut s = session("scenario_two_diesel.toml");
    s.toggle_diesel_engine(1).unwrap();
    advance(&mut s, 10.);
    assert_eq!(s.state.diesel.cars[1].phase, EnginePhase::Stopped);
    s.operate_car(0, CarOperation::Power).unwrap();
    assert_eq!(s.physics.diesel_vehicle_indices, vec![1]);
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 0.8;
    advance(&mut s, 2.);
    assert_eq!(
        s.velocity_mps(),
        0.,
        "stopped trail motor must not inherit lead RPM/power"
    );
    assert_eq!(s.state.diesel_rpm, vec![0.]);
    let remaining = s.state.diesel.cars[1].fuel_l;
    s.driver_throttle = 0.;
    s.toggle_diesel_engine(1).unwrap();
    advance(&mut s, 8.);
    s.driver_throttle = 0.8;
    advance(&mut s, 5.);
    assert!(s.velocity_mps() > 1.);
    assert!(s.state.diesel.cars[1].fuel_l < remaining);
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    advance(&mut s, 10.);
    assert!(s.velocity_mps() < 0.01);
    s.operate_car(0, CarOperation::Power).unwrap();
    advance(&mut s, 0.1);
    assert!(
        s.state
            .diesel
            .cars
            .iter()
            .all(|c| c.phase == EnginePhase::Running)
    );
    assert!(s.state.diesel.cars[1].fuel_l < remaining);
}

#[test]
fn save_mid_start_and_manual_steam_controls_continue_deterministically() {
    let mut diesel = session("scenario.toml");
    diesel.toggle_diesel_engine(0).unwrap();
    advance(&mut diesel, 10.);
    diesel.toggle_diesel_engine(0).unwrap();
    advance(&mut diesel, 1.5);
    let mut resumed = session("scenario.toml");
    let save = serde_json::to_vec(&diesel.snapshot()).unwrap();
    resumed
        .restore_snapshot(serde_json::from_slice(&save).unwrap())
        .unwrap();
    advance(&mut diesel, 8.);
    advance(&mut resumed, 8.);
    assert_eq!(diesel.state.diesel, resumed.state.diesel);

    let mut steam = session("scenario_steam.toml");
    steam.steam_command(SteamCommand::Injector1).unwrap();
    steam.steam_command(SteamCommand::Blower).unwrap();
    steam.steam_command(SteamCommand::Cutoff(-0.2)).unwrap();
    advance(&mut steam, 2.);
    let mut resumed = session("scenario_steam.toml");
    let save = serde_json::to_vec(&steam.snapshot()).unwrap();
    resumed
        .restore_snapshot(serde_json::from_slice(&save).unwrap())
        .unwrap();
    advance(&mut steam, 6.);
    advance(&mut resumed, 6.);
    assert_eq!(steam.state.boiler_state, resumed.state.boiler_state);
    let mut invalid = resumed.snapshot();
    invalid.state.boiler_state.as_mut().unwrap().tender_water_kg = 1e30;
    assert!(resumed.restore_snapshot(invalid).is_err());
}

#[test]
fn steam_injection_conserves_water_and_firing_conserves_coal() {
    let p = steam_params();
    let mut b = BoilerState::from_params(&p);
    b.water_kg = p.operation.boiler_water_capacity_kg * 0.6;
    b.command(SteamCommand::Injector1);
    b.command(SteamCommand::Firing(0.1));
    let water_before = b.water_kg + b.tender_water_kg;
    let coal_before = b.coal_kg + b.fire_mass_kg;
    let mut steam_lost = 0.;
    let mut coal_burned = 0.;
    for _ in 0..200 {
        steam_step(&mut b, &p, 0.2, 4., 0.05);
        steam_lost += b.steam_usage_kg_s * 0.05;
        coal_burned += b.coal_burn_kg_s * 0.05;
    }
    assert!((water_before - (b.water_kg + b.tender_water_kg) - steam_lost).abs() < 1e-7);
    assert!((coal_before - (b.coal_kg + b.fire_mass_kg) - coal_burned).abs() < 1e-7);
    assert!(b.tender_water_kg < p.initial_water_kg);
    assert!(b.valid_for(&p));
}

#[test]
fn steam_cutoff_is_independent_and_reduces_effort_and_consumption() {
    let p = steam_params();
    let mut full = BoilerState::from_params(&p);
    let mut short = full.clone();
    short.command(SteamCommand::Cutoff(-0.5));
    let a = steam_step(&mut full, &p, 0.8, 10., 0.05);
    let b = steam_step(&mut short, &p, 0.8, 10., 0.05);
    assert!((b / a - 1. / 3.).abs() < 1e-12);
    assert!(short.steam_usage_kg_s < full.steam_usage_kg_s);
    assert!(short.controls.automatic_fireman);
}

#[test]
fn finite_tender_never_refills_itself_and_dry_boiler_failure_latches() {
    let mut p = steam_params();
    p.initial_water_kg = 2.;
    p.initial_coal_kg = 0.1;
    let mut b = BoilerState::from_params(&p);
    b.water_kg = p.operation.boiler_water_capacity_kg * 0.2;
    b.command(SteamCommand::Injector1);
    b.command(SteamCommand::Firing(0.5));
    for _ in 0..10 {
        steam_step(&mut b, &p, 1., 10., 0.05);
    }
    assert_eq!(b.tender_water_kg, 0.);
    assert_eq!(b.coal_kg, 0.);
    b.water_kg = p.operation.boiler_water_capacity_kg * 0.1;
    assert_eq!(steam_step(&mut b, &p, 1., 10., 0.05), 0.);
    assert!(b.low_water_failure);
    b.water_kg = p.operation.boiler_water_capacity_kg * 0.8;
    b.command(SteamCommand::AutomaticFireman);
    assert_eq!(steam_step(&mut b, &p, 1., 10., 0.05), 0.);
    assert!(
        b.low_water_failure,
        "damage cannot be cleared by changing controls"
    );
}

#[test]
fn fire_out_stops_heat_and_pressure_can_fall_below_two_bar() {
    let mut p = steam_params();
    p.initial_coal_kg = 0.;
    let mut b = BoilerState::from_params(&p);
    b.fire_mass_kg = 0.;
    b.pressure_bar = 0.5;
    let initial = b.pressure_bar;
    for _ in 0..200 {
        steam_step(&mut b, &p, 1., 10., 0.05);
    }
    assert_eq!(b.evaporation_kg_s, 0.);
    assert!(b.pressure_bar < initial && b.pressure_bar >= 0.);
    assert_eq!(b.coal_kg, 0.);
    assert_eq!(b.fire_mass_kg, 0.);
}

#[test]
fn short_steam_fixture_exhausts_reserves_and_keeps_damage_when_restored() {
    let mut s = session("scenario_steam_low_water.toml");
    s.steam_command(SteamCommand::Injector1).unwrap();
    advance(&mut s, 20.);
    let b = s.state.boiler_state.as_ref().unwrap();
    assert_eq!(b.tender_water_kg, 0.);
    assert_eq!(b.coal_kg, 0.);
    assert!(
        b.low_water_failure,
        "small boiler must expose the low-water failure promptly"
    );
    assert_eq!(b.tractive_force_n, 0.);
    assert!(b.valid_for(s.physics.steam_params.as_ref().unwrap()));
    let saved = s.snapshot();
    let mut restored = session("scenario_steam_low_water.toml");
    restored.restore_snapshot(saved).unwrap();
    assert!(
        restored
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .low_water_failure
    );
    assert_eq!(
        restored
            .state
            .boiler_state
            .as_ref()
            .unwrap()
            .tender_water_kg,
        0.
    );
}

#[test]
fn steam_safety_valve_releases_mass_and_injectors_cool_the_boiler() {
    let p = steam_params();
    let mut idle = BoilerState::from_params(&p);
    idle.pressure_bar *= 1.05;
    idle.command(SteamCommand::Damper(0.5));
    let total = idle.water_kg + idle.tender_water_kg;
    steam_step(&mut idle, &p, 0., 0., 0.05);
    assert!(idle.safety_valve);
    assert!(idle.water_kg + idle.tender_water_kg < total);
    assert!(idle.steam_usage_kg_s > 0.02);
    let mut dry = BoilerState::from_params(&p);
    dry.water_kg *= 0.8;
    dry.command(SteamCommand::Damper(0.));
    let mut wet = dry.clone();
    wet.command(SteamCommand::Injector1);
    steam_step(&mut dry, &p, 0., 0., 0.05);
    steam_step(&mut wet, &p, 0., 0., 0.05);
    assert!(wet.pressure_bar < dry.pressure_bar);
    assert!(wet.water_kg > dry.water_kg);
}

#[test]
fn diesel_operation_matches_the_pinned_original_dll_checkpoints() {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../oracles/openrails-traction-operation.json"
    ))
    .unwrap();
    assert_eq!(reference["checkpoints"].as_array().unwrap().len(), 40);
    let mut s = session("scenario.toml");
    s.physics.diesel.cars[0].params.consumption_lph = vec![(0., 0.), (300., 18.), (900., 180.)];
    let mut max_rpm_error = 0_f64;
    let mut max_flow_error = 0_f64;
    let mut max_fuel_error = 0_f64;
    for tick in 0..400 {
        if tick == 20 || tick == 340 {
            s.state.diesel.cars[0].command_running = false;
        }
        if tick == 60 {
            s.state.diesel.cars[0].command_running = true;
        }
        let throttle = if (180..280).contains(&tick) { 0.6 } else { 0. };
        openrailsrs_sim::diesel_operation::advance(
            &mut s.state.diesel,
            &s.physics.diesel,
            throttle,
            0.05,
            true,
        );
        if tick % 10 == 9 {
            let expected = &reference["checkpoints"][tick / 10];
            let state = &s.state.diesel.cars[0];
            assert_eq!(
                format!("{:?}", state.phase),
                expected["state"].as_str().unwrap(),
                "tick {}",
                tick + 1
            );
            max_rpm_error =
                max_rpm_error.max((state.rpm - expected["rpm"].as_f64().unwrap()).abs());
            max_flow_error =
                max_flow_error.max((state.flow_lps - expected["flow_lps"].as_f64().unwrap()).abs());
            max_fuel_error = max_fuel_error
                .max((state.consumed_l - expected["consumed_l"].as_f64().unwrap()).abs());
        }
    }
    println!(
        "native diesel operation: RPM error {max_rpm_error:.6}, flow error {max_flow_error:.9} L/s, fuel error {max_fuel_error:.9} L"
    );
    // Fixed bounds allow original float rounding, independently of the
    // full-service speed/position acceptance limits.
    assert!(max_rpm_error < 0.1, "RPM error {max_rpm_error}");
    assert!(max_flow_error < 0.00001, "flow error {max_flow_error}");
    assert!(max_fuel_error < 0.0001, "fuel error {max_fuel_error}");
}

#[test]
fn resource_units_match_native_stf_without_changing_frozen_references() {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../oracles/openrails-traction-operation.json"
    ))
    .unwrap();
    for q in reference["quantities"].as_array().unwrap() {
        let key = q["key"].as_str().unwrap();
        let raw = q["raw"].as_str().unwrap();
        let body = if key == "MaxDieselLevel" {
            format!("Type ( Diesel ) {key} ( {raw} )")
        } else {
            format!(
                "Type ( Steam ) NumCylinders ( 2 ) CylinderDiameter ( 0.47m ) CylinderStroke ( 0.66m ) WheelRadius ( 0.97m ) {key} ( {raw} )"
            )
        };
        let ast =
            openrailsrs_formats::parse_vehicle_text(&format!("Engine ( e Mass ( 82000 ) {body} )"))
                .unwrap();
        let e = openrailsrs_formats::EngineFile::from_ast(&ast).unwrap();
        let actual = match key {
            "MaxDieselLevel" => e.diesel_operation.unwrap().capacity_l,
            "MaxBoilerPressure" => e.steam.unwrap().working_pressure_bar,
            "BoilerVolume" => e.steam.unwrap().operation.boiler_water_capacity_kg,
            "MaxTenderWaterMass" => e.steam.unwrap().initial_water_kg,
            "MaxTenderCoalMass" => e.steam.unwrap().initial_coal_kg,
            _ => panic!("unrecognized frozen quantity {key}"),
        };
        assert!(
            (actual - q["value"].as_f64().unwrap()).abs() < 0.002,
            "{key}: {actual}"
        );
    }
}
