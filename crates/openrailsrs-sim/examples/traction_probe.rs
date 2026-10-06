//! Check original formations without modifying their content or timetable.
//! Usage: traction_probe SCENARIO.toml
//! This isolates acceleration, coasting, braking and reserves from service rules;
//! it is an operational smoke test, not an Open Rails physics parity oracle.
use openrailsrs_sim::LiveDriveSession;

fn main() {
    let path = std::path::PathBuf::from(std::env::args_os().nth(1).expect("SCENARIO.toml"));
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).expect("scenario");
    scenario.route.stops.clear();
    scenario.route.assume_signals_clear = true;
    let mut session =
        LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).expect("formation");
    let initial_diesel = session.state.diesel.clone();
    let initial_boiler = session.state.boiler_state.clone();
    session.driver_direction = 1.;
    session.driver_brake = 0.;
    session.driver_throttle = 0.6;
    let mut maximum_speed: f64 = 0.;
    let mut powered_vehicles_seen = std::collections::BTreeSet::new();
    for _ in 0..1200 {
        session.step_realtime(0.05, |_| {});
        maximum_speed = maximum_speed.max(session.velocity_mps());
        powered_vehicles_seen.extend(
            session
                .state
                .electric
                .cars
                .iter()
                .filter(|s| s.main_power && s.contact_voltage_v > 0.)
                .map(|s| s.vehicle),
        );
    }
    let powered = session.state.clone();
    assert!(
        maximum_speed > 1.,
        "formation did not accelerate: {maximum_speed}"
    );
    session.driver_throttle = 0.;
    let before_coast = session.state.odometer_m;
    if !session.physics.electric.cars.is_empty() {
        assert!(
            session
                .physics
                .electric
                .cars
                .iter()
                .all(|c| powered_vehicles_seen.contains(&c.vehicle)),
            "an electric motor never received compatible power: {powered_vehicles_seen:?}"
        );
        session.toggle_circuit_breaker().unwrap();
    }
    for _ in 0..400 {
        session.step_realtime(0.05, |_| {});
    }
    let coast_distance = session.state.odometer_m - before_coast;
    assert!(coast_distance > 0., "formation did not coast");
    if !session.physics.electric.cars.is_empty() {
        assert!(
            powered
                .electric
                .cars
                .iter()
                .any(|s| s.main_power && s.contact_voltage_v > 0.)
        );
        assert!(session.state.electric.cars.iter().all(|s| !s.main_power));
    }
    session.driver_brake = 1.;
    let before_brake = session.state.odometer_m;
    for _ in 0..2400 {
        session.step_realtime(0.05, |_| {});
        if session.velocity_mps() < 0.1 {
            break;
        }
    }
    assert!(session.velocity_mps() < 0.1, "formation failed to stop");
    let saved = session.snapshot();
    let mut restored = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    restored
        .restore_snapshot(saved)
        .expect("valid saved reserves and brakes");
    assert_eq!(restored.state.diesel, session.state.diesel);
    let diesel_used: f64 = session.state.diesel.cars.iter().map(|c| c.consumed_l).sum();
    if !initial_diesel.cars.is_empty() {
        assert!(diesel_used > 0.);
    }
    if let (Some(initial), Some(final_boiler)) = (&initial_boiler, &session.state.boiler_state) {
        assert!(
            final_boiler.coal_kg < initial.coal_kg
                || final_boiler.fire_mass_kg < initial.fire_mass_kg
        );
        assert!(final_boiler.steam_usage_kg_s.is_finite());
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "scenario": scenario.scenario.name, "consist": scenario.train.consist,
        "scope": "60s acceleration, 20s coast, service brake and save/restore; signals and timetable isolated",
        "vehicles": session.formation.coupled_count,
        "maximum_speed_kmh": maximum_speed * 3.6, "coast_distance_m": coast_distance,
        "braking_distance_m": session.state.odometer_m - before_brake,
        "final_speed_kmh": session.velocity_mps() * 3.6,
        "diesel_used_l": diesel_used, "diesel": session.state.diesel,
        "boiler": session.state.boiler_state, "electric_powered": powered.electric,
        "electric_powered_vehicles_seen": powered_vehicles_seen,
        "electric_disconnected": session.state.electric, "energy_j": session.state.cumulative_energy_j,
        "save_restore_passed": true,
    })).unwrap());
}
