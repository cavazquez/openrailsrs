use openrailsrs_sim::level_crossing::{CrossingDef, CrossingLane, CrossingState};
use openrailsrs_sim::{CarOperation, LiveDriveSession, LiveTraffic};
use std::path::PathBuf;
fn fixture() -> (PathBuf, openrailsrs_scenarios::ScenarioFile) {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/player_lab/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    scenario.simulation.multi_body = false;
    (path.parent().unwrap().to_path_buf(), scenario)
}
fn crossing(position: f64) -> CrossingState {
    CrossingState::new(CrossingDef {
        id: "fixture-crossing".into(),
        lanes: vec![CrossingLane {
            edge: "e1".into(),
            position_m: position,
        }],
        warning_s: 20.,
        minimum_m: 10.,
        animation_s: 4.,
    })
}
#[test]
fn crossing_observes_approach_full_tail_multiple_services_and_saved_opening() {
    let (root, mut scenario) = fixture();
    let mut entry:openrailsrs_scenarios::TrainEntryDef=toml::from_str("id='second'\nconsist='consists/lab.con'\nstart='yard_a'\ndestination='yard_b'\nstart_offset_m=180.0\nparked=true\noutput_csv='second.csv'").unwrap();
    entry.consist = scenario.train.consist.clone();
    scenario.extra_trains.push(entry);
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    player
        .state
        .crossings
        .insert("fixture-crossing".into(), crossing(150.));
    player.state.velocity_mps = 10.;
    traffic.update_crossings(&mut player, 0.);
    assert!(player.state.crossings["fixture-crossing"].road_closed);
    traffic.update_crossings(&mut player, 4.);
    assert_eq!(player.state.crossings["fixture-crossing"].fraction, 1.);
    player.state.velocity_mps = 0.;
    player.state.pos_on_edge_m = 350.;
    traffic.update_crossings(&mut player, 4.);
    assert!(
        player.state.crossings["fixture-crossing"].demand,
        "second train's tail still occupies crossing"
    );
    traffic.services[0].session.state.pos_on_edge_m = 350.;
    traffic.update_crossings(&mut player, 1.);
    assert!(!player.state.crossings["fixture-crossing"].demand);
    assert!(player.state.crossings["fixture-crossing"].road_closed);
    let saved = player.snapshot();
    let other = traffic.snapshot();
    let mut restored = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let mut trains = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    restored.restore_snapshot(saved).unwrap();
    trains.restore_snapshot(other).unwrap();
    assert_eq!(restored.state.crossings["fixture-crossing"].fraction, 0.75);
    trains.update_crossings(&mut restored, 0.);
    assert_eq!(
        restored.state.crossings["fixture-crossing"].fraction, 0.75,
        "pausing cannot open gate"
    );
    trains.update_crossings(&mut restored, 3.);
    assert!(!restored.state.crossings["fixture-crossing"].road_closed);
}
#[test]
fn parked_cut_protects_crossing_after_the_locomotive_leaves() {
    let (root, scenario) = fixture();
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    player
        .state
        .crossings
        .insert("fixture-crossing".into(), crossing(50.));
    player.operate_car(1, CarOperation::Handbrake).unwrap();
    player.uncouple_after(0).unwrap();
    player.state.pos_on_edge_m = 500.;
    traffic.update_crossings(&mut player, 0.);
    assert!(player.state.crossings["fixture-crossing"].road_closed);
    assert!(
        player
            .own_track_occupancy()
            .iter()
            .any(|o| o.start_m <= 50. && o.end_m >= 50.)
    );
}

#[test]
fn reversing_approach_uses_the_leading_rear_and_survives_restore() {
    let (root, scenario) = fixture();
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    player.state.pos_on_edge_m = 350.;
    player.state.velocity_mps = 10.;
    player.driver_direction = 0.;
    player
        .state
        .crossings
        .insert("fixture-crossing".into(), crossing(200.));
    assert!(
        player
            .own_track_occupancy()
            .iter()
            .all(|part| part.start_m > 210.)
    );
    traffic.update_crossings(&mut player, 0.);
    assert!(player.state.crossings["fixture-crossing"].demand);
    traffic.update_crossings(&mut player, 4.);
    assert_eq!(player.state.crossings["fixture-crossing"].fraction, 1.);

    let mut restored = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    restored.restore_snapshot(player.snapshot()).unwrap();
    traffic.update_crossings(&mut restored, 0.);
    assert!(restored.state.crossings["fixture-crossing"].road_closed);
    assert_eq!(restored.state.crossings["fixture-crossing"].fraction, 1.);

    // Choosing the forward direction away from the gate removes approach
    // demand; speed remains a magnitude throughout the simulation.
    restored.driver_direction = 1.;
    traffic.update_crossings(&mut restored, 4.);
    assert!(!restored.state.crossings["fixture-crossing"].road_closed);
}

#[test]
fn reversed_body_order_predicts_from_the_physically_leading_end() {
    let (root, scenario) = fixture();
    let mut player = LiveDriveSession::from_scenario(&root, &scenario).unwrap();
    let traffic = LiveTraffic::from_scenario(&root, &scenario).unwrap();
    player.state.path_reversed = true;
    player.state.pos_on_edge_m = 350.;
    player.state.velocity_mps = 5.;
    player.driver_direction = 1.;
    player
        .state
        .crossings
        .insert("fixture-crossing".into(), crossing(209.));
    // Nominal head is 141 m away; the physically leading car end is 101 m
    // away, inside a 5 m/s * 20 s + 10 m warning range.
    traffic.update_crossings(&mut player, 0.);
    assert!(player.state.crossings["fixture-crossing"].demand);
}
