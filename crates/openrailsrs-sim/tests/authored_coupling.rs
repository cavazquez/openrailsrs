//! A new connection belongs to its actual ends, not a neighboring draft gear.
use openrailsrs_sim::{CarOperation, LiveDriveSession, LiveTraffic, TrafficService};

#[test]
fn unauthored_contact_does_not_inherit_an_adjacent_authored_joint() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation")
        .canonicalize()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let consists = temp.path().join("consists");
    std::fs::create_dir(&consists).unwrap();
    let engine =
        serde_json::to_string(&base.join("vehicles/coupler_diesel.eng").to_string_lossy()).unwrap();
    let wagon =
        serde_json::to_string(&base.join("vehicles/freight.wag").to_string_lossy()).unwrap();
    std::fs::write(
        consists.join("player.con"),
        format!("(Train (Engine {engine}) (Wagon {wagon}))\n"),
    )
    .unwrap();
    std::fs::write(
        consists.join("donor.con"),
        format!("(Train (Wagon {wagon}) (Engine {engine}))\n"),
    )
    .unwrap();
    let mut scenario =
        openrailsrs_scenarios::load_scenario(base.join("scenario_couplers.toml")).unwrap();
    scenario.route.path = base.join("route").to_string_lossy().into_owned();
    scenario.train.consist = "consists/donor.con".into();
    scenario.route.start_offset_m = Some(65.); // 20 m locomotive + 15 m wagon.
    let mut donor = LiveDriveSession::from_scenario(temp.path(), &scenario).unwrap();
    donor.select_cab(1).unwrap();
    donor.set_direction(1.).unwrap();
    donor.driver_throttle = 0.4;
    donor.driver_brake = 0.;
    donor.step_realtime(8., |_| {});
    donor.driver_throttle = 0.;
    donor.driver_brake = 1.;
    donor.step_realtime(14., |_| {});
    assert!(donor.velocity_mps() < 0.1);
    assert!(donor.state.couplers[0].native_profile.is_some());
    assert!(
        donor.state.couplers[0].memory.stored_energy_j
            + donor.state.couplers[0].memory.absorbed_energy_j
            > 1.
    );
    let donor_memory = serde_json::to_value(&donor.state.couplers[0].memory).unwrap();
    let relative_position = donor.state.vehicles[0].position_m - donor.state.vehicles[1].position_m;
    assert!(
        relative_position.abs() > 1e-6,
        "the donor must exercise a nonzero internal deflection"
    );
    scenario.train.consist = "consists/player.con".into();
    scenario.route.start_offset_m = Some(donor.head_chainage_m() + 35.);
    let mut player = LiveDriveSession::from_scenario(temp.path(), &scenario).unwrap();
    assert!(player.state.couplers[0].native_profile.is_some());
    for i in 0..2 {
        donor.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    let mut traffic = LiveTraffic::default();
    traffic.services.push(TrafficService {
        parked: true,
        id: "unpowered_front".into(),
        departure_s: 0.,
        session: donor,
        departed: false,
    });
    traffic.couple_service(&mut player, 0).unwrap();
    assert_eq!(player.state.couplers.len(), 3);
    assert!(player.state.couplers[0].native_profile.is_some());
    let new_joint = &player.state.couplers[1];
    assert!(
        new_joint.native_profile.is_none(),
        "both actual contact wagons have no Coupling; no neighboring native hardware may be inherited"
    );
    assert_eq!(
        new_joint.free_play_m,
        openrailsrs_sim::coupler::CouplerState::passenger().free_play_m
    );
    assert!(player.state.couplers[2].native_profile.is_some());
    assert_eq!(
        serde_json::to_value(&player.state.couplers[2].memory).unwrap(),
        donor_memory,
        "collecting a train cannot reset its internal gear history"
    );
    assert!(
        (player.state.vehicles[2].position_m
            - player.state.vehicles[3].position_m
            - relative_position)
            .abs()
            < 1e-9
    );
    let saved = player.snapshot();
    player.restore_snapshot(saved).unwrap();
}
