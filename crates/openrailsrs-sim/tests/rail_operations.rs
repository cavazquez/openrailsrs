use openrailsrs_scenarios::{ScenarioFile, TrainEntryDef, rail_operations::*};
use openrailsrs_sim::{
    LiveDriveSession, LiveTraffic,
    rail_operations::{IncidentChoice, coordinate},
};
fn example() -> (std::path::PathBuf, ScenarioFile) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
    (path.parent().unwrap().into(), scenario)
}
fn service(s: &ScenarioFile) -> TrainEntryDef {
    TrainEntryDef {
        bogie_dynamics: vec![],
        activity_uids: vec![],
        legs: vec![],
        presentation: vec![],
        cab_equipment: vec![],
        parked: false,
        pre_roll_s: 0.,
        relative_stop_times: false,
        freight: None,
        systems: vec![],
        id: "conector".into(),
        consist: s.train.consist.clone(),
        start: "mid".into(),
        destination: "b".into(),
        start_time_s: 300.,
        start_offset_m: 0.,
        stops: s
            .route
            .stops
            .iter()
            .filter(|s| s.node == "mid")
            .cloned()
            .collect(),
        waypoints: vec![],
        davis: None,
        switches: vec![],
        electric_pickups: vec![],
        output_csv: "traffic.csv".into(),
    }
}
#[test]
fn connection_timeout_and_incident_choice_survive_save_without_stopping_the_clock() {
    let (directory, mut scenario) = example();
    scenario.extra_trains = vec![service(&scenario)];
    scenario
        .simulation
        .rail_operations
        .connections
        .push(ConnectionDef {
            stop: 0,
            service: "conector".into(),
            service_stop: 0,
            max_wait_s: 12.,
        });
    scenario
        .simulation
        .rail_operations
        .incidents
        .push(IncidentDef {
            id: "demora".into(),
            service: "conector".into(),
            stop: 0,
            wait_s: 20.,
            penalty: 15.,
        });
    let mut player = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(&directory, &scenario).unwrap();
    coordinate(&mut player, &traffic);
    assert!(player.gameplay.departure_hold);
    assert_eq!(
        player.state.rail_operations.pending_incident.as_deref(),
        Some("demora")
    );
    player.decide_incident(IncidentChoice::Continue).unwrap();
    assert_eq!(player.gameplay.accrued_penalty, 15.);
    traffic.advance(&mut player, 6., None, |_| {});
    assert!(player.time_s() > 5.9);
    assert!(player.gameplay.departure_hold);
    let saved = player.snapshot();
    let others = traffic.snapshot();
    let mut restored = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
    let mut restored_traffic = LiveTraffic::from_scenario(&directory, &scenario).unwrap();
    restored.restore_snapshot(saved).unwrap();
    restored_traffic.restore_snapshot(others).unwrap();
    traffic.advance(&mut player, 7., None, |_| {});
    restored_traffic.advance(&mut restored, 7., None, |_| {});
    assert_eq!(
        player
            .state
            .rail_operations
            .connections
            .get(&0)
            .map(String::as_str),
        Some("Perdida")
    );
    assert!(!player.gameplay.departure_hold);
    assert_eq!(player.head_chainage_m(), restored.head_chainage_m());
    assert_eq!(
        player.state.rail_operations.connections,
        restored.state.rail_operations.connections
    );
}
#[test]
fn temporary_limit_remains_until_tail_clears_even_after_work_schedule_ends() {
    let (directory, mut s) = example();
    s.simulation.rail_operations.works.push(TrackWorkDef {
        id: "obra".into(),
        edge: "e1".into(),
        start_m: 30.,
        end_m: 100.,
        start_s: 0.,
        end_s: 5.,
        direction: 1,
        speed_kmh: 18.,
        closed: false,
    });
    let mut session = LiveDriveSession::from_scenario(&directory, &s).unwrap();
    let edge = session.state.path_edges[0].clone();
    session.state.rail_operations.definition.works[0].edge = edge;
    session.state.pos_on_edge_m = 50.;
    session.update_work_occupancy();
    assert_eq!(session.temporary_speed_limit(30.), 5.);
    session.state.time.0 = 8.;
    assert_eq!(session.temporary_speed_limit(30.), 5.);
    session.state.pos_on_edge_m = 100. + session.formation.length_m() + 1.;
    session.update_work_occupancy();
    assert_eq!(session.temporary_speed_limit(30.), 30.);
    session.state.time.0 = 0.;
    session.state.pos_on_edge_m = 50.;
    session.driver_direction = 0.;
    assert_eq!(session.temporary_speed_limit(30.), 30.);
}
