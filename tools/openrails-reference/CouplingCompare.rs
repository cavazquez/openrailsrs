//! QA client linked against already-built Rust libraries; never replaces their
//! physics. OR simple rigid coupling and Rust elastic coupling are compared at
//! the same prescribed static load balance, not at the same spring extension.
use openrailsrs_sim::{CarOperation, CouplerState, LiveDriveSession, LiveTraffic, VehicleState};
use serde_json::{Value, json};
use std::path::Path;

fn capture(s: &LiveDriveSession) -> Value {
    json!({
        "vehicles": s.formation.cars,
        "cylinders": s.state.brake_system.cylinders,
        "fuel": s.state.diesel.cars,
        "vehicle_systems": s.state.vehicle_systems,
        "effective_mass_kg": s.effective_mass_kg(),
        "speed_mps": s.velocity_mps(),
        "head_m": s.head_chainage_m(),
        "occupancy": s.own_track_occupancy(),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "CouplingCompare SCENARIO.toml OR.json OUTPUT.json");
    let input = Path::new(&args[1]);
    let root = input.parent().unwrap();
    let mut scenario = openrailsrs_scenarios::load_scenario(input).unwrap();
    let first = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    scenario.extra_trains[0].start_offset_m =
        scenario.route.start_offset_m.unwrap() - first.formation.length_m();
    let mut player = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(root, &scenario).unwrap();
    assert_eq!(player.formation.cars.len(), 4);
    assert_eq!(traffic.services[0].session.formation.cars.len(), 4);
    // A parked service starts secured. Deliberately release it through the real
    // car operation before exercising the rejection for an unsecured donor.
    for i in 0..4 {
        if traffic.services[0].session.formation.cars[i].handbrake {
            traffic.services[0].session.operate_car(i, CarOperation::Handbrake).unwrap();
        }
    }
    assert!(traffic.couple_service(&mut player, 0).is_err());
    for i in 0..4 {
        traffic.services[0].session.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    player.driver_throttle = 0.2;
    assert!(traffic.couple_service(&mut player, 0).is_err());
    player.driver_throttle = 0.;
    let before_player = capture(&player);
    let before_donor = capture(&traffic.services[0].session);
    let mass = player.effective_mass_kg() + traffic.services[0].session.effective_mass_kg();
    traffic.couple_service(&mut player, 0).unwrap();
    assert_eq!(player.formation.cars.len(), 8);
    assert!(traffic.services.is_empty());
    assert!((player.effective_mass_kg() - mass).abs() < 1e-6);
    let after = capture(&player);
    let saved = player.snapshot();
    player.validate_snapshot(&saved).unwrap();
    let mut restored = LiveDriveSession::from_scenario(root, &scenario).unwrap();
    let mut restored_traffic = LiveTraffic::from_scenario(root, &scenario).unwrap();
    restored_traffic.reconstruct_couplings(&mut restored, &traffic.couplings).unwrap();
    restored.restore_snapshot(saved).unwrap();
    assert_eq!(restored.formation.cars.len(), 8);
    assert!(restored_traffic.services.is_empty());
    let restored_before_drive = capture(&restored);
    for i in 4..8 {
        restored.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    restored.operate_car(4, CarOperation::Hose).unwrap();
    restored.operate_car(4, CarOperation::FrontCock).unwrap();
    restored.operate_car(3, CarOperation::RearCock).unwrap();
    restored.driver_direction = 1.;
    restored.driver_brake = 0.;
    restored.driver_throttle = 0.5;
    let initial = restored.head_chainage_m();
    for _ in 0..400 {
        restored.step_realtime(0.05, |_| {});
    }
    assert!(restored.head_chainage_m() > initial + 1.);
    let reference: Value = serde_json::from_slice(&std::fs::read(&args[2]).unwrap()).unwrap();
    let mut forces = vec![];
    for row in reference["coupling"]["equilibrium_forces"].as_array().unwrap() {
        let masses = [row["masses_kg"][0].as_f64().unwrap(), row["masses_kg"][1].as_f64().unwrap()];
        let external = [row["input_force_n"][0].as_f64().unwrap(), row["input_force_n"][1].as_f64().unwrap()];
        let equilibrium_accel = (external[0] + external[1]) / (masses[0] + masses[1]);
        // Prescribe the elastic pre-load from inputs alone. No OR force/result
        // is used to select it; both models must balance the same external load.
        let preload = external[0] - masses[0] * equilibrium_accel;
        let mut joint = CouplerState::passenger();
        joint.extension_m = if preload == 0. { 0. } else {
            preload.signum() * joint.free_play_m + preload / joint.stiffness_n_per_m
        };
        let extension = joint.extension_m;
        let force = joint.force_n(0.);
        let mut cars = [VehicleState { velocity_mps: 0., position_m: 0. },
            VehicleState { velocity_mps: 0., position_m: 0. }];
        openrailsrs_sim::coupler::multi_body_step_distributed(
            &mut cars, &mut [joint], &external, &[0., 0.], &[0., 0.], &masses, 0.05, 1.,
        );
        let accelerations = [cars[0].velocity_mps / 0.05, cars[1].velocity_mps / 0.05];
        let expected = -row["coupler_force_or_n"].as_f64().unwrap();
        assert!((force - expected).abs() <= 0.1, "{} force", row["id"]);
        for i in 0..2 {
            assert!((accelerations[i] - row["resulting_acceleration_mps2"][i].as_f64().unwrap()).abs() <= 1e-5);
        }
        forces.push(json!({"id": row["id"], "masses_kg": masses, "input_force_n": external,
            "elastic_extension_m": extension, "coupler_force_rust_n": force,
            "or_force_with_rust_sign_n": expected, "maximum_force_error_n": (force - expected).abs(),
            "acceleration_mps2": accelerations, "force_tolerance_n": 0.1,
            "acceleration_tolerance_mps2": 1e-5,
            "scope": "Prescribed static equilibrium; different rigid/elastic extension and transient laws"}));
    }
    std::fs::write(&args[3], serde_json::to_vec_pretty(&json!({
        "before_player": before_player, "before_donor": before_donor, "after": after,
        "restored_before_drive": restored_before_drive,
        "moving_after_restore": {"distance_m": restored.head_chainage_m() - initial,
            "time_s": 20., "vehicles": restored.formation.cars.len()},
        "removed_donor": traffic.services.is_empty(), "restored_without_duplicates": true,
        "rejected_unsecured_donor": true, "rejected_traction_during_contact": true,
        "force_equilibrium": forces,
        "scope": "Same original Pullman files on an owned synthetic directed track; state transfer, restore, driving and static load balance, not OR route/impact parity"
    })).unwrap()).unwrap();
}
