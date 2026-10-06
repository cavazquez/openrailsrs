use openrailsrs_sim::{LiveDriveSession, adhesion::RailWeather};
use std::path::PathBuf;

fn session(relative: &str) -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(relative);
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    let mut s = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
    s.driver_direction = 1.;
    s.driver_brake = 0.;
    s.driver_throttle = 1.;
    s
}
#[test]
fn dry_native_contact_preserves_the_pinned_diesel_dynamics_without_double_limiting() {
    let mut original = session("chiltern_local/scenario_oracle.toml");
    let mut weather = session("chiltern_local/scenario_oracle.toml");
    for _ in 0..20 {
        original.step_realtime(0.05, |_| {});
        weather.step_realtime(0.05, |_| {});
    }
    weather.set_rail_weather(RailWeather::Dry);
    for (axle, &index) in weather
        .state
        .native_dynamics
        .as_ref()
        .unwrap()
        .axles
        .iter()
        .zip(&weather.physics.diesel_vehicle_indices)
    {
        assert_eq!(
            weather.state.rail_adhesion.as_ref().unwrap().cars[index].wheel_speed_mps,
            axle.speed_mps
        );
    }
    for _ in 0..400 {
        original.step_realtime(0.05, |_| {});
        weather.step_realtime(0.05, |_| {});
        assert!((original.velocity_mps() - weather.velocity_mps()).abs() < 1e-10);
        assert!((original.state.odometer_m - weather.state.odometer_m).abs() < 1e-10);
    }
    assert!(
        weather.state.rail_adhesion.as_ref().unwrap().cars[7]
            .wheel_speed_mps
            .is_finite()
    );
}
#[test]
fn sandbox_phase_and_surface_survive_save_restore_and_corruption_is_atomic() {
    let mut source = session("chiltern_local/scenario_oracle.toml");
    source.set_rail_weather(RailWeather::Snow);
    source.toggle_sander().unwrap();
    source.step_realtime(2., |_| {});
    let serialized = serde_json::to_vec(&source.snapshot()).unwrap();
    let mut restored = session("chiltern_local/scenario_oracle.toml");
    restored
        .restore_snapshot(serde_json::from_slice(&serialized).unwrap())
        .unwrap();
    for _ in 0..100 {
        source.step_realtime(0.05, |_| {});
        restored.step_realtime(0.05, |_| {});
    }
    assert_eq!(
        serde_json::to_value(&source.state.rail_adhesion).unwrap(),
        serde_json::to_value(&restored.state.rail_adhesion).unwrap()
    );
    assert_eq!(source.state.odometer_m, restored.state.odometer_m);
    let before = serde_json::to_value(restored.snapshot()).unwrap();
    let mut bad = restored.snapshot();
    bad.state.rail_adhesion.as_mut().unwrap().cars[0].sand_m3 += 0.1;
    assert!(restored.restore_snapshot(bad).is_err());
    assert_eq!(serde_json::to_value(restored.snapshot()).unwrap(), before);
    let mut bad = restored.snapshot();
    bad.state.rail_adhesion.as_mut().unwrap().cars.clear();
    assert!(restored.restore_snapshot(bad).is_err());
    assert_eq!(serde_json::to_value(restored.snapshot()).unwrap(), before);
}
#[test]
fn native_wet_drive_and_sanding_reduce_rail_force_without_changing_driver_notch() {
    let mut dry = session("chiltern_local/scenario_oracle.toml");
    let mut wet = session("chiltern_local/scenario_oracle.toml");
    dry.set_rail_weather(RailWeather::Dry);
    wet.set_rail_weather(RailWeather::Snow);
    dry.step_realtime(20., |_| {});
    wet.step_realtime(20., |_| {});
    assert!(
        wet.velocity_mps() < dry.velocity_mps(),
        "wet {} dry {}",
        wet.velocity_mps(),
        dry.velocity_mps()
    );
    assert_eq!(wet.driver_throttle, dry.driver_throttle);
    let rail = wet.state.rail_adhesion.as_ref().unwrap();
    assert!(rail.cars.iter().any(|c| c.slipping));
    assert!(rail.cars.iter().any(|c| c.slip_distance_m > 1.));
    wet.toggle_sander().unwrap();
    wet.step_realtime(1., |_| {});
    assert!(wet.state.rail_adhesion.as_ref().unwrap().cars[0].using_sand);
    assert!(wet.state.rail_adhesion.as_ref().unwrap().cars[0].consumed_sand_m3 > 0.);
}
#[test]
fn steam_and_electric_fallbacks_have_physical_contact_and_keep_power_interlocks() {
    for path in [
        "traction_operation/scenario_steam.toml",
        "electric_supply/scenario.toml",
    ] {
        let mut s = session(path);
        if path.contains("steam") {
            // Native Hall stock also carries an incidental legacy force table.
            // The steam branch must take priority even when that model exists.
            s.physics.diesel_engines.push(
                session("traction_operation/scenario_two_diesel.toml")
                    .physics
                    .diesel_engines
                    .remove(0),
            );
        }
        s.set_rail_weather(RailWeather::Snow);
        s.step_realtime(15., |_| {});
        let r = s.state.rail_adhesion.as_ref().unwrap();
        assert!(s.velocity_mps() > 0.1, "{path} failed to move");
        assert!(r.cars[0].wheel_speed_mps > 0., "{path}");
        assert!(
            r.cars[0].requested_force_n > 0. && r.cars[0].rail_force_n > 0.,
            "{path}"
        );
        if path.contains("electric") {
            s.toggle_circuit_breaker().unwrap();
            s.step_realtime(1., |_| {});
            assert_eq!(
                s.state.rail_adhesion.as_ref().unwrap().cars[0].requested_force_n,
                0.
            );
        }
    }
}

#[test]
fn steam_racing_wheels_use_power_and_steam_within_the_authors_limits() {
    let mut rolling = session("traction_operation/scenario_steam.toml");
    let mut racing = session("traction_operation/scenario_steam.toml");
    for s in [&mut rolling, &mut racing] {
        s.set_rail_weather(RailWeather::Snow);
    }
    racing.state.rail_adhesion.as_mut().unwrap().cars[0].wheel_speed_mps = 80.;
    rolling.step_realtime(0.05, |_| {});
    racing.step_realtime(0.05, |_| {});
    assert!(
        racing.state.boiler_state.as_ref().unwrap().steam_usage_kg_s
            > rolling
                .state
                .boiler_state
                .as_ref()
                .unwrap()
                .steam_usage_kg_s
    );
    let rail = racing.state.rail_adhesion.as_ref().unwrap();
    assert!(rail.valid_for(racing.physics.rail_adhesion.as_ref().unwrap()));
    assert!(rail.cars[0].requested_force_n <= racing.physics.max_tractive_effort_n);
    assert!(rail.cars[0].requested_force_n * 80. <= racing.physics.max_power_w * (1. + 1e-10));
}

#[test]
fn detached_sandbox_is_retained_without_delivery_and_recoupling_does_not_refill_it() {
    use openrailsrs_sim::operations::CarOperation;
    let path = "traction_operation/scenario_two_diesel.toml";
    let mut s = session(path);
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    s.set_rail_weather(RailWeather::Snow);
    s.toggle_sander().unwrap();
    s.step_realtime(1., |_| {});
    let before = s.state.rail_adhesion.as_ref().unwrap().cars[1].sand_m3;
    assert!(s.state.rail_adhesion.as_ref().unwrap().cars[1].consumed_sand_m3 > 0.);
    s.operate_car(1, CarOperation::Handbrake).unwrap();
    s.uncouple_after(0).unwrap();
    s.step_realtime(1., |_| {});
    let parked = &s.state.rail_adhesion.as_ref().unwrap().cars[1];
    assert_eq!(parked.sand_m3, before);
    assert!(!parked.using_sand);
    let mut restored = session(path);
    restored.restore_snapshot(s.snapshot()).unwrap();
    restored.recouple().unwrap();
    assert_eq!(
        restored.state.rail_adhesion.as_ref().unwrap().cars[1].sand_m3,
        before
    );
    restored.step_realtime(1., |_| {});
    assert!(restored.state.rail_adhesion.as_ref().unwrap().cars[1].sand_m3 < before);
}
