use openrailsrs_sim::{CarOperation, LiveDriveSession, LiveTraffic};
use std::path::PathBuf;
fn source(name: &str) -> (PathBuf, openrailsrs_scenarios::ScenarioFile) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(name);
    (
        path.parent().unwrap().to_path_buf(),
        openrailsrs_scenarios::load_scenario(&path).unwrap(),
    )
}
#[test]
fn delivery_requires_exact_wagons_full_siding_and_secured_detachment_then_pickup_removes_acceptance()
 {
    let (root, scenario) = source("examples/yard_training/scenario_delivery.toml");
    let mut s = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    assert!(!s.yard_deliveries()[0].accepted);
    for i in [1, 2] {
        s.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    s.uncouple_after(0).unwrap();
    assert!(s.yard_deliveries()[0].accepted);
    let save = s.snapshot();
    let mut restored = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    restored.restore_snapshot(save).unwrap();
    assert!(restored.yard_deliveries()[0].accepted);
    restored.recouple().unwrap();
    assert!(!restored.yard_deliveries()[0].accepted);
}
#[test]
fn independent_contact_moves_vehicles_once_preserves_pressure_fuel_and_can_reconstruct_a_save() {
    let (root, mut scenario) = source("examples/yard_training/scenario.toml");
    scenario.gameplay.shunting.clear();
    let mut donor: openrailsrs_scenarios::TrainEntryDef = toml::from_str("id='parked'\nconsist='consists/yard.con'\nstart='a'\ndestination='destination'\nstart_offset_m=100.0\noutput_csv='parked.csv'").unwrap();
    donor.start_offset_m = scenario.route.start_offset_m.unwrap() - 60.;
    scenario.extra_trains.push(donor);
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    let mut expected_cylinders = player.state.brake_system.cylinders.clone();
    expected_cylinders.extend(
        traffic.services[0]
            .session
            .state
            .brake_system
            .cylinders
            .clone(),
    );
    let fuel = player
        .state
        .diesel
        .cars
        .iter()
        .chain(&traffic.services[0].session.state.diesel.cars)
        .map(|c| c.fuel_l)
        .sum::<f64>();
    let total_mass = player.effective_mass_kg() + traffic.services[0].session.effective_mass_kg();
    assert!(traffic.couple_service(&mut player, 0).is_err());
    for i in 0..3 {
        traffic.services[0]
            .session
            .operate_car(i, CarOperation::Handbrake)
            .unwrap();
    }
    player.driver_throttle = 0.2;
    assert!(traffic.couple_service(&mut player, 0).is_err());
    player.driver_throttle = 0.;
    traffic.couple_service(&mut player, 0).unwrap();
    assert_eq!(player.formation.cars.len(), 6);
    assert!(traffic.services.is_empty());
    assert!((player.effective_mass_kg() - total_mass).abs() < 1e-6);
    assert_eq!(
        player
            .state
            .diesel
            .cars
            .iter()
            .map(|c| c.fuel_l)
            .sum::<f64>(),
        fuel
    );
    for (actual, expected) in player
        .state
        .brake_system
        .cylinders
        .iter()
        .zip(&expected_cylinders)
    {
        assert_eq!(actual.pipe_pressure_bar(), expected.pipe_pressure_bar());
        assert_eq!(actual.current_force_n, expected.current_force_n);
    }
    let saved = player.snapshot();
    player.validate_snapshot(&saved).unwrap();
    let mut restored = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let mut restore_traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    restore_traffic
        .reconstruct_couplings(&mut restored, &traffic.couplings)
        .unwrap();
    restored.restore_snapshot(saved).unwrap();
    assert_eq!(restored.formation.cars.len(), 6);
    assert!(restore_traffic.services.is_empty());
    for i in 3..6 {
        restored.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    restored.operate_car(3, CarOperation::Hose).unwrap();
    restored.operate_car(3, CarOperation::FrontCock).unwrap();
    restored.operate_car(2, CarOperation::RearCock).unwrap();
    restored.driver_direction = 1.;
    restored.driver_brake = 0.;
    restored.driver_throttle = 0.5;
    let initial = restored.head_chainage_m();
    for _ in 0..400 {
        restored.step_realtime(0.05, |_| {});
    }
    assert!(restored.head_chainage_m() > initial + 1.);
    assert_eq!(
        restored
            .own_track_occupancy()
            .iter()
            .map(|o| o.owner.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1
    );
}
#[test]
fn depot_transfers_finite_oil_and_coolant_without_resetting_fuel_or_temperature() {
    let (root, scenario) = source("examples/traction_operation/scenario_depot.toml");
    let mut s = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    assert!(s.begin_depot_work("workshop").is_err());
    let depot = s.state.rail_operations.definition.depots[0].id.clone();
    for c in &mut s.formation.cars {
        c.handbrake = true;
    }
    s.driver_direction = 0.5;
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    for v in &mut s.state.vehicle_systems {
        for m in &mut v.motors {
            m.running = false;
            m.rpm = 0.;
            m.oil_l -= 25.;
            m.coolant_l -= 35.;
        }
    }
    let fuel = s
        .state
        .vehicle_systems
        .iter()
        .flat_map(|v| &v.motors)
        .map(|m| m.fuel_l)
        .sum::<f64>();
    s.begin_depot_work(&depot).unwrap();
    for _ in 0..300 {
        s.step_realtime(0.05, |_| {});
    }
    assert!(s.state.depot.active.is_some());
    let saved = s.snapshot();
    let mut restored = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    restored.restore_snapshot(saved).unwrap();
    for _ in 0..400 {
        restored.step_realtime(0.05, |_| {});
    }
    assert!(restored.state.depot.completed.contains(&depot));
    assert_eq!(restored.state.depot.used_oil_l[&depot], 50.);
    assert_eq!(restored.state.depot.used_coolant_l[&depot], 70.);
    assert_eq!(
        restored
            .state
            .vehicle_systems
            .iter()
            .flat_map(|v| &v.motors)
            .map(|m| m.fuel_l)
            .sum::<f64>(),
        fuel
    );
    assert!(
        restored
            .state
            .vehicle_systems
            .iter()
            .flat_map(|v| &v.motors)
            .all(|m| m.oil_c < 80. && m.coolant_c < 80.)
    );
}
#[test]
fn two_crews_deliver_distinct_wagon_lots_and_foreign_wagons_make_a_siding_invalid() {
    let (root, mut scenario) = source("examples/yard_training/scenario_delivery.toml");
    let mut entry:openrailsrs_scenarios::TrainEntryDef=toml::from_str("id='east'\nconsist='consists/yard.con'\nstart='c'\ndestination='destination'\nstart_offset_m=160.0\nparked=true\noutput_csv='east.csv'").unwrap();
    entry.consist = scenario.train.consist.clone();
    scenario.extra_trains.push(entry);
    let mut destination = scenario.simulation.rail_operations.yard_deliveries[0].clone();
    destination.id = "east-lot".into();
    destination.service = Some("east".into());
    destination.edge = "exit_c".into();
    scenario
        .simulation
        .rail_operations
        .yard_deliveries
        .push(destination);
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    for i in [1, 2] {
        player.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    player.uncouple_after(0).unwrap();
    traffic.services[0].session.uncouple_after(0).unwrap();
    assert!(traffic.yard_inventory(&player).iter().all(|d| d.accepted));
    let mut rebuilt = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    rebuilt.restore_snapshot(traffic.snapshot()).unwrap();
    assert!(rebuilt.yard_inventory(&player).iter().all(|d| d.accepted));
    rebuilt.services[0].session.recouple().unwrap();
    assert!(rebuilt.yard_inventory(&player)[0].accepted);
    assert!(!rebuilt.yard_inventory(&player)[1].accepted);
}

#[test]
fn coupling_an_unstarted_parked_donor_produces_an_immediately_valid_supply_snapshot() {
    let (root, scenario) = source("examples/traffic_passing/scenario_coupling.toml");
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    for _ in 0..20 {
        traffic.advance(&mut player, 0.05, None, |_| {});
    }
    assert!(player.state.power_supply.initialized);
    assert!(!traffic.services[0].session.state.power_supply.initialized);
    traffic.couple_service(&mut player, 0).unwrap();
    assert_eq!(player.state.power_supply.cars.len(), 2);
    let saved = player.snapshot();
    player.validate_snapshot(&saved).unwrap();
    player.restore_snapshot(saved).unwrap();
    assert_eq!(player.formation.coupled_count, 4);
}
