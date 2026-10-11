//! Playable integration checks: controls alter physics and yard goals cannot
//! pass just because their instruction was displayed.
use openrailsrs_sim::power_supply::HotelMode;
use openrailsrs_sim::{CarOperation, DoorState, LiveDriveSession};
use std::path::Path;

fn session(example: &str) -> LiveDriveSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(example);
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}
fn drive_to(s: &mut LiveDriveSession, target: f64) {
    s.driver_throttle = 0.0;
    s.set_direction(if target >= s.head_chainage_m() {
        1.0
    } else {
        0.0
    })
    .unwrap();
    for _ in 0..15000 {
        let remaining = (target - s.head_chainage_m()).abs();
        if remaining < 0.5 && s.velocity_mps() <= 0.1 {
            s.driver_throttle = 0.0;
            s.driver_brake = 1.0;
            s.step_realtime(0.2, |_| {});
            return;
        }
        let speed = if remaining > 3.0 { 1.0 } else { 0.25 };
        s.driver_throttle = if s.velocity_mps() < speed && remaining > 0.35 {
            0.2
        } else {
            0.0
        };
        s.driver_brake = if s.velocity_mps() > speed + 0.1 || remaining < 0.35 {
            0.3
        } else {
            0.0
        };
        s.step_realtime(0.05, |_| {});
    }
    panic!(
        "yard position {}, target {target}, speed {}, {}",
        s.head_chainage_m(),
        s.velocity_mps(),
        s.traction_diagnosis()
    );
}

#[test]
fn independent_emergency_and_main_reservoir_are_physical_and_saved() {
    let mut free = session("yard_training/scenario.toml");
    let mut held = session("yard_training/scenario.toml");
    for s in [&mut free, &mut held] {
        s.driver_direction = 1.0;
        s.driver_throttle = 0.3;
    }
    held.state.brake_system.independent_command = 1.0;
    free.step_realtime(5.0, |_| {});
    held.step_realtime(5.0, |_| {});
    assert!(held.velocity_mps() < free.velocity_mps());
    assert!(held.state.brake_system.cylinders[0].pressure_bar() > 3.0);
    assert_eq!(held.state.brake_system.cylinders[1].pressure_bar(), 0.0);
    assert!(
        held.state.brake_system.circuits[0].reservoir_bar
            < free.state.brake_system.circuits[0].reservoir_bar
    );
    assert!(held.cab_telemetry().main_res_bar > 7.0);
    free.state.brake_system.emergency = true;
    assert!(free.reset_emergency().is_err());
    free.step_realtime(10.0, |_| {});
    assert!(free.velocity_mps() <= 0.1);
    assert_eq!(free.state.throttle, 0.0);
    assert!(
        free.state
            .brake_system
            .cylinders
            .iter()
            .all(|c| c.pipe_pressure_bar().unwrap() < 0.01 && c.pressure_bar() > 1.0)
    );
    let snapshot = free.snapshot();
    let mut resumed = session("yard_training/scenario.toml");
    resumed.restore_snapshot(snapshot).unwrap();
    assert!(resumed.state.brake_system.emergency);
    free.driver_throttle = 0.0;
    free.reset_emergency().unwrap();
    free.step_realtime(20.0, |_| {});
    assert!(free.state.brake_system.cylinders[0].pressure_bar() < 0.1);
    let mut invalid = resumed.snapshot();
    invalid.state.brake_system.circuits[0].params.reservoir_m3 *= 2.0;
    assert!(resumed.restore_snapshot(invalid).is_err());
}

#[test]
fn passenger_power_uses_authored_watts_reduces_traction_and_counts_energy() {
    let mut cool = session("traction_operation/scenario_hotel.toml");
    let mut off = session("traction_operation/scenario_hotel.toml");
    cool.state.power_supply.hotel_mode = HotelMode::Cooling;
    off.state.power_supply.hotel_mode = HotelMode::Off;
    for s in [&mut cool, &mut off] {
        s.driver_direction = 1.0;
        s.driver_throttle = 1.0;
        s.step_realtime(40.0, |_| {});
    }
    assert_eq!(cool.state.power_supply.hotel_w, 176000.0);
    assert_eq!(off.state.power_supply.hotel_w, 16000.0);
    assert_eq!(cool.state.power_supply.auxiliary_w, 10000.0);
    assert!(cool.state.power_supply.energy_j > off.state.power_supply.energy_j);
    assert!(cool.state.cumulative_energy_j > off.state.cumulative_energy_j);
    assert!(cool.velocity_mps() < off.velocity_mps());
    let mut resumed = session("traction_operation/scenario_hotel.toml");
    resumed.restore_snapshot(cool.snapshot()).unwrap();
    cool.step_realtime(1.0, |_| {});
    resumed.step_realtime(1.0, |_| {});
    assert!((cool.state.power_supply.energy_j - resumed.state.power_supply.energy_j).abs() < 1e-6);
}

#[test]
fn explicit_diesel_thermal_protection_is_visible_and_optional() {
    let mut protected = session("traction_operation/scenario_thermal.toml");
    let mut diagnostic = session("traction_operation/scenario_thermal.toml");
    diagnostic.physics.diesel.cars[0].params.thermal.protection = false;
    for s in [&mut protected, &mut diagnostic] {
        s.driver_direction = 1.0;
        s.driver_throttle = 1.0;
        s.step_realtime(45.0, |_| {});
    }
    assert!(protected.state.diesel.cars[0].temperature_c > 70.0);
    assert!(
        protected
            .traction_diagnosis()
            .contains("Protección térmica")
    );
    assert!(protected.velocity_mps() < diagnostic.velocity_mps());
    assert_eq!(
        openrailsrs_sim::diesel_operation::available_fraction(
            &diagnostic.state.diesel,
            &diagnostic.physics.diesel,
            0
        ),
        1.0
    );
    let mut resumed = session("traction_operation/scenario_thermal.toml");
    resumed.restore_snapshot(protected.snapshot()).unwrap();
    protected.step_realtime(1.0, |_| {});
    resumed.step_realtime(1.0, |_| {});
    assert_eq!(
        protected.state.diesel.cars[0].temperature_c,
        resumed.state.diesel.cars[0].temperature_c
    );
}

#[test]
fn yard_mission_requires_cut_clear_points_real_pickup_and_reformed_departure() {
    let mut s = session("yard_training/scenario.toml");
    assert_eq!(s.gameplay.shunting_completed_s.len(), 0);
    assert!(s.uncouple_after(0).is_err());
    for i in [1, 2] {
        s.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    s.uncouple_after(0).unwrap();
    assert_eq!(s.gameplay.shunting_completed_s.len(), 1);
    assert!(s.recouple().is_ok());
    assert_eq!(s.gameplay.shunting_completed_s.len(), 1);
    s.uncouple_after(0).unwrap();
    drive_to(&mut s, 240.0);
    assert_eq!(s.gameplay.shunting_completed_s.len(), 2);
    let original_path = s.state.path_edges.clone();
    s.dispatch_switch("junction").unwrap();
    assert_eq!(s.gameplay.shunting_completed_s.len(), 3);
    assert_ne!(s.state.path_edges, original_path);
    assert_eq!(s.formation.parked_head_chainage_m, Some(160.0));
    let mut resumed = session("yard_training/scenario.toml");
    resumed.restore_snapshot(s.snapshot()).unwrap();
    drive_to(&mut resumed, 160.0);
    resumed.recouple().unwrap();
    assert_eq!(resumed.gameplay.shunting_completed_s.len(), 3);
    resumed.operate_car(1, CarOperation::Hose).unwrap();
    resumed.operate_car(0, CarOperation::RearCock).unwrap();
    resumed.operate_car(1, CarOperation::FrontCock).unwrap();
    for i in [1, 2] {
        resumed.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    assert_eq!(resumed.gameplay.shunting_completed_s.len(), 4);
    drive_to(&mut resumed, 420.0);
    assert_eq!(resumed.gameplay.shunting_completed_s.len(), 5);
    resumed.driver_throttle = 0.0;
    resumed.set_direction(1.0).unwrap();
    for _ in 0..16000 {
        resumed.step_autodrive(0.05, 0.3, |_| {});
        if resumed.arrived {
            break;
        }
    }
    assert!(resumed.arrived);
    assert!(
        resumed.gameplay.failure.is_none(),
        "{:?}",
        resumed.gameplay.failure
    );
    assert_eq!(resumed.gameplay.stop_results.len(), 1);
    assert!(
        resumed.exterior.door == DoorState::Open
            || resumed.gameplay.phase == openrailsrs_sim::service::ServicePhase::Completed
    );
}

#[test]
fn four_diesel_cooling_modes_match_200_unmodified_or161_checkpoints() {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../oracles/openrails-traction-operation.json"
    ))
    .unwrap();
    for cooling in 0..4 {
        let mut s = session("traction_operation/scenario_thermal.toml");
        s.physics.diesel.cars[0].params.thermal = openrailsrs_formats::DieselThermalParams {
            maximum_c: 100.0,
            time_constant_s: 20.0,
            cooling,
            protection: false,
        };
        s.state.diesel.cars[0].rpm = 900.0;
        s.state.velocity_mps = 20.0;
        let mut tick = 0;
        for point in reference["thermal"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["cooling"].as_u64() == Some(cooling.into()))
        {
            while tick < point["tick"].as_u64().unwrap() {
                let input = &reference["thermal_inputs"][(cooling as usize * 2000) + tick as usize];
                assert_eq!(input[0].as_u64(), Some(cooling.into()));
                assert_eq!(input[1].as_u64(), Some(tick + 1));
                let params = &s.physics.diesel.cars[0].params.thermal;
                s.state.diesel.cars[0].rpm = input[2].as_f64().unwrap();
                openrailsrs_sim::diesel_operation::advance_thermal_car(
                    &mut s.state.diesel.cars[0],
                    params,
                    300.0,
                    900.0,
                    input[3].as_f64().unwrap(),
                    0.05,
                );
                tick += 1;
            }
            let actual = &s.state.diesel.cars[0];
            assert!(
                (actual.temperature_c - point["temperature_c"].as_f64().unwrap()).abs() < 0.01,
                "cooling {cooling}, tick {tick}: {} vs {}",
                actual.temperature_c,
                point["temperature_c"]
            );
            assert_eq!(actual.cooling_on, point["cooling_on"].as_bool().unwrap());
        }
    }
}

#[test]
fn steam_strokes_rods_and_exhaust_share_wheel_phase_in_forward_and_reverse() {
    let mut s = session("traction_operation/scenario_steam.toml");
    s.exterior.door = DoorState::Closed;
    s.driver_direction = 1.0;
    s.driver_throttle = 0.5;
    s.driver_brake = 0.0;
    s.step_realtime(5.0, |_| {});
    let motion = s.steam_motion(0, 0.05).unwrap();
    let strokes = f64::from(s.physics.steam_params.as_ref().unwrap().cylinder_count) * 2.0;
    assert!(motion.working && motion.load > 0.0 && s.velocity_mps() > 0.0);
    assert!((motion.stroke_phase - motion.wheel_revolutions * strokes).abs() < 1e-10);
    s.driver_throttle = 0.0;
    s.driver_brake = 1.0;
    s.step_realtime(10.0, |_| {});
    let stopped_phase = s.steam_motion(0, 0.05).unwrap().stroke_phase;
    s.set_direction(0.0).unwrap();
    s.driver_brake = 0.0;
    s.driver_throttle = 0.5;
    s.step_realtime(5.0, |_| {});
    let reverse = s.steam_motion(0, 0.05).unwrap();
    assert!(
        reverse.stroke_phase < stopped_phase,
        "reverse phase {}, stopped {}, velocity {}, head {}, time {}, arrived {}",
        reverse.stroke_phase,
        stopped_phase,
        s.velocity_mps(),
        s.head_chainage_m(),
        s.time_s(),
        s.arrived
    );
    assert!(reverse.frequency_hz > 0.0);
    s.driver_throttle = 0.0;
    s.step_realtime(1.0, |_| {});
    assert!(!s.steam_motion(0, 0.05).unwrap().working);
}
