use openrailsrs_core::electrification::{
    ElectricPickup, ElectricPickupOverride, ElectricSection, ElectricSupply,
};
use openrailsrs_scenarios::{ScenarioFile, load_scenario};
use openrailsrs_sim::{
    LiveDriveSession, LiveTraffic,
    electric::{BreakerState, PowerLoss},
    physics::step,
};
use std::path::PathBuf;

fn fixture() -> (PathBuf, ScenarioFile) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/electric_supply/scenario.toml");
    let mut scenario = load_scenario(&path).unwrap();
    scenario.route.stops.clear();
    (path.parent().unwrap().to_path_buf(), scenario)
}
fn session() -> LiveDriveSession {
    let (dir, scenario) = fixture();
    let mut session = LiveDriveSession::from_scenario(&dir, &scenario).unwrap();
    session.driver_direction = 1.;
    session.driver_throttle = 1.;
    session
}
fn motor_force(session: &LiveDriveSession) -> f64 {
    session.state.diesel_traction_force_n.iter().sum()
}

#[test]
fn electric_supply_power_loss_coasts_and_recovers() {
    let mut s = session();
    s.step_realtime(5., |_| {});
    assert!(s.velocity_mps() > 1.);
    assert!(motor_force(&s) > 0.);
    s.toggle_pantograph().unwrap();
    assert!(!s.state.electric.cars[0].main_power);
    let before = s.velocity_mps();
    s.step_realtime(1., |_| {});
    assert_eq!(motor_force(&s), 0.);
    assert!(s.velocity_mps() > 0. && s.velocity_mps() < before);
    assert_eq!(s.state.electric.cars[0].loss, PowerLoss::Pantograph);
    s.step_realtime(2., |_| {});
    s.toggle_pantograph().unwrap();
    s.step_realtime(1.5, |_| {});
    assert_eq!(motor_force(&s), 0.);
    s.step_realtime(1., |_| {});
    assert_eq!(s.state.electric.cars[0].breaker, BreakerState::Closing);
    s.step_realtime(1.1, |_| {});
    assert!(s.state.electric.cars[0].main_power);
    assert!(motor_force(&s) > 0.);
}

#[test]
fn electric_supply_neutral_section_is_per_vehicle_and_reconnects() {
    let mut s = session();
    // Supply position, not the rendered sector or odometer, drives the cutoff.
    s.state.pos_on_edge_m = 160.;
    s.state.velocity_mps = 15.;
    s.step_realtime(0.1, |_| {});
    assert_eq!(motor_force(&s), 0.);
    assert_eq!(s.state.electric.cars[0].loss, PowerLoss::NoSupply);
    assert!(s.velocity_mps() > 14.);
    s.state.pos_on_edge_m = 240.;
    s.step_realtime(1., |_| {});
    assert!(!s.state.electric.cars[0].main_power);
    s.step_realtime(0.6, |_| {});
    assert!(motor_force(&s) > 0.);
}

#[test]
fn electric_supply_absent_incompatible_or_out_of_voltage_range_cannot_drive() {
    for kind in [
        ElectricPickup::None,
        ElectricPickup::ThirdRail,
        ElectricPickup::FourthRail,
    ] {
        let (dir, mut scenario) = fixture();
        let supply = scenario.route.electric_supply.as_mut().unwrap();
        supply.supply = ElectricSupply {
            kind,
            voltage_v: if kind == ElectricPickup::None {
                0.
            } else {
                750.
            },
        };
        let mut s = LiveDriveSession::from_scenario(&dir, &scenario).unwrap();
        s.driver_throttle = 1.;
        s.driver_direction = 1.;
        s.step_realtime(3., |_| {});
        assert_eq!(s.velocity_mps(), 0.);
        assert_eq!(motor_force(&s), 0.);
        assert!(!s.state.electric.cars[0].main_power);
    }
    let mut s = session();
    s.physics.electric.cars[0].params.maximum_voltage_v = Some(1000.);
    s.step_realtime(0.1, |_| {});
    assert_eq!(s.state.electric.cars[0].loss, PowerLoss::Voltage);
    assert_eq!(motor_force(&s), 0.);
}

#[test]
fn electric_supply_conductor_rails_need_no_pantograph_but_do_need_a_breaker() {
    for kind in [ElectricPickup::ThirdRail, ElectricPickup::FourthRail] {
        let (dir, mut scenario) = fixture();
        scenario.route.electric_supply.as_mut().unwrap().supply = ElectricSupply {
            kind,
            voltage_v: 750.,
        };
        scenario.train.electric_pickups = vec![ElectricPickupOverride { vehicle: 0, kind }];
        let mut s = LiveDriveSession::from_scenario(&dir, &scenario).unwrap();
        s.driver_throttle = 1.;
        s.driver_direction = 1.;
        assert!(!s.exterior.pantograph_command_up);
        assert!(s.toggle_pantograph().is_err());
        s.step_realtime(2., |_| {});
        assert!(motor_force(&s) > 0.);
        s.toggle_circuit_breaker().unwrap();
        s.step_realtime(0.1, |_| {});
        assert_eq!(motor_force(&s), 0.);
        assert_eq!(s.state.electric.cars[0].loss, PowerLoss::Breaker);
        s.toggle_circuit_breaker().unwrap();
        s.step_realtime(1., |_| {});
        assert!(!s.state.electric.cars[0].main_power);
        s.step_realtime(0.6, |_| {});
        assert!(motor_force(&s) > 0.);
    }
}

#[test]
fn electric_supply_save_preserves_reconnection_and_rejects_invalid_state() {
    let mut s = session();
    s.toggle_circuit_breaker().unwrap();
    s.toggle_circuit_breaker().unwrap();
    s.step_realtime(0.4, |_| {});
    let saved = s.snapshot();
    let mut restored = session();
    restored
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    for _ in 0..25 {
        s.step_realtime(0.05, |_| {});
        restored.step_realtime(0.05, |_| {});
        assert_eq!(s.state.electric, restored.state.electric);
        assert_eq!(s.velocity_mps(), restored.velocity_mps());
    }
    let mut invalid = saved.clone();
    invalid.state.electric.cars[0].pantograph_fraction = 1.1;
    assert!(restored.restore_snapshot(invalid).is_err());
    let (dir, mut changed_scenario) = fixture();
    changed_scenario
        .route
        .electric_supply
        .as_mut()
        .unwrap()
        .supply
        .voltage_v = 1500.;
    let mut changed = LiveDriveSession::from_scenario(&dir, &changed_scenario).unwrap();
    assert!(changed.restore_snapshot(saved).is_err());
}

#[test]
fn electric_supply_headless_and_ai_use_the_same_cutoff() {
    let mut s = session();
    s.state.throttle = 1.;
    s.state.pos_on_edge_m = 160.;
    s.state.velocity_mps = 10.;
    step(&mut s.state, &s.path_data, &s.physics, 0.05);
    assert_eq!(motor_force(&s), 0.);
    let (dir, mut scenario) = fixture();
    let extra: openrailsrs_scenarios::TrainEntryDef = toml::from_str("id='electric-ai'\nconsist='consists/electric.con'\nstart='a'\ndestination='b'\nstart_offset_m=160\noutput_csv='ai.csv'\n").unwrap();
    scenario.extra_trains.push(extra);
    let mut traffic = LiveTraffic::from_scenario(&dir, &scenario).unwrap();
    let ai = &mut traffic.services[0].session;
    ai.state.velocity_mps = 10.;
    ai.driver_direction = 1.;
    ai.driver_throttle = 1.;
    ai.step_realtime(0.1, |_| {});
    assert_eq!(motor_force(ai), 0.);
    assert_eq!(ai.state.electric.cars[0].loss, PowerLoss::NoSupply);
}

#[test]
fn electric_supply_mixed_formation_keeps_diesel_and_other_pickups_powered() {
    let (directory, mut scenario) = fixture();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("consists")).unwrap();
    std::fs::create_dir_all(temp.path().join("vehicles")).unwrap();
    let electric = std::fs::read_to_string(directory.join("vehicles/electric.eng")).unwrap();
    std::fs::write(temp.path().join("vehicles/electric.eng"), &electric).unwrap();
    std::fs::write(
        temp.path().join("vehicles/diesel.eng"),
        electric.replace("Type Electric", "Type Diesel"),
    )
    .unwrap();
    std::fs::write(temp.path().join("consists/mixed.con"), "(Train (Engine \"vehicles/electric.eng\") (Engine \"vehicles/diesel.eng\") (Engine \"vehicles/electric.eng\"))").unwrap();
    scenario.route.path = directory.join("route").display().to_string();
    scenario.train.consist = "consists/mixed.con".into();
    let mut s = LiveDriveSession::from_scenario(temp.path(), &scenario).unwrap();
    s.driver_direction = 1.;
    s.driver_throttle = 1.;
    s.state.pos_on_edge_m = 160.;
    s.state.velocity_mps = 15.;
    s.step_realtime(0.05, |_| {});
    assert_eq!(s.state.diesel_traction_force_n[0], 0.);
    assert!(s.state.diesel_traction_force_n[1] > 0.);
    assert!(s.state.diesel_traction_force_n[2] > 0.);
    assert_eq!(s.state.electric.cars[0].loss, PowerLoss::NoSupply);
    assert!(s.state.electric.cars[1].main_power);
    s.state.velocity_mps = 0.;
    s.driver_throttle = 0.;
    s.operate_car(2, openrailsrs_sim::CarOperation::Battery)
        .unwrap();
    assert_eq!(s.state.electric.cars[1].loss, PowerLoss::Isolated);
}

#[test]
fn electric_supply_rejects_unknown_edges_and_overlapping_sections() {
    let (dir, mut scenario) = fixture();
    let supply = scenario.route.electric_supply.as_mut().unwrap();
    supply.sections.push(supply.sections[0].clone());
    assert!(LiveDriveSession::from_scenario(&dir, &scenario).is_err());
    scenario.route.electric_supply.as_mut().unwrap().sections = vec![ElectricSection {
        edge: "missing".into(),
        start_m: 0.,
        end_m: 10.,
        supply: ElectricSupply::default(),
    }];
    assert!(LiveDriveSession::from_scenario(&dir, &scenario).is_err());
}

#[test]
fn electric_supply_also_cuts_the_curve_only_fallback() {
    let mut s = session();
    s.physics.diesel_engines.clear();
    s.physics.diesel_vehicle_indices.clear();
    s.state.diesel_traction_force_n.clear();
    s.state.throttle = 1.;
    s.state.velocity_mps = 10.;
    s.toggle_pantograph().unwrap();
    let before = s.state.cumulative_energy_j;
    step(&mut s.state, &s.path_data, &s.physics, 0.1);
    assert_eq!(s.state.cumulative_energy_j, before);
    assert!(s.velocity_mps() < 10.);
}

#[test]
fn electric_supply_matches_pinned_native_pantograph_and_main_power_oracle() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../oracles/openrails-electric.json");
    let oracle: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut s = session();
    let params = &mut s.physics.electric.cars[0].params;
    params.pantograph_delay_s = oracle["pantograph_delay_s"].as_f64().unwrap();
    params.breaker_delay_s = 0.;
    params.power_on_delay_s = oracle["power_on_delay_s"].as_f64().unwrap();
    s.state.electric.cars[0].pantograph_fraction = 0.;
    for point in oracle["pantographs"].as_array().unwrap() {
        s.state.electric.pantograph_command_up = point["command_up"].as_bool().unwrap();
        openrailsrs_sim::electric::advance(&mut s.state, &s.path_data, &s.physics.electric, 0.5);
        assert!(
            (s.state.electric.cars[0].pantograph_fraction - point["fraction"].as_f64().unwrap())
                .abs()
                < 1e-6
        );
    }
    let mut previous = 0.;
    // Apply input transitions at each checkpoint; advance the interval under the
    // previous command, matching the native Timer's explicit simulation clock.
    for point in oracle["checkpoints"].as_array().unwrap() {
        let time = point["time_s"].as_f64().unwrap();
        openrailsrs_sim::electric::advance(
            &mut s.state,
            &s.path_data,
            &s.physics.electric,
            time - previous,
        );
        let state = point["pantograph"].as_str().unwrap();
        s.state.electric.pantograph_command_up = state != "Down";
        s.state.electric.cars[0].pantograph_fraction = match state {
            "Up" => 1.,
            "Raising" => 0.5,
            _ => 0.,
        };
        s.state.electric.breaker_command_closed = point["breaker"] == "Closed";
        openrailsrs_sim::electric::advance(&mut s.state, &s.path_data, &s.physics.electric, 0.);
        assert_eq!(
            s.state.electric.cars[0].main_power,
            point["main_power"].as_bool().unwrap(),
            "native checkpoint {time}"
        );
        previous = time;
    }
}
