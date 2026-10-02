//! A whole authored service through the same fixed-step session used by Bevy.

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{LiveDriveSession, ServicePhase, exterior::DoorState};
use openrailsrs_track::{SignalAspect, TrackSignal};

fn session() -> LiveDriveSession {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern_local/scenario.toml");
    let scenario = load_scenario(&path).expect("the playable service is a checked-in fixture");
    LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
}

fn drive(frame_dt: f64) -> LiveDriveSession {
    let mut train = session();
    while !train.arrived && train.time_s() < 1800.0 {
        train.step_autodrive(frame_dt, 0.75, |_| {});
    }
    assert_eq!(
        train.gameplay.phase,
        ServicePhase::Completed,
        "{:?}",
        train.gameplay.failure
    );
    assert_eq!(train.gameplay.stop_results.len(), 3);
    for (actual, target) in train
        .gameplay
        .stop_results
        .iter()
        .zip(&train.gameplay.stop_targets)
    {
        assert_eq!(actual.name, target.name);
        assert!(actual.position_error_m.abs() <= 10.0);
        assert!(actual.arrival_speed_mps <= 0.1);
        assert!(actual.dwell_s + 1e-8 >= target.dwell_s);
        assert!(actual.actual_depart_s + 1e-8 >= target.depart_s);
    }
    assert_eq!(train.state.passengers, 0);
    assert_eq!(train.exterior.door, DoorState::Closed);
    assert!(train.state.odometer_m > 6800.0 && train.state.odometer_m < 6900.0);
    train
}

#[test]
fn all_stations_are_served_independently_of_render_frame_rate() {
    let slow = drive(1.0 / 30.0);
    let fast = drive(1.0 / 144.0);
    assert_eq!(slow.time_s(), fast.time_s());
    assert_eq!(slow.state.odometer_m, fast.state.odometer_m);
    for (a, b) in slow
        .gameplay
        .stop_results
        .iter()
        .zip(&fast.gameplay.stop_results)
    {
        assert_eq!(a.actual_arrive_s, b.actual_arrive_s);
        assert_eq!(a.position_error_m, b.position_error_m);
    }
}

#[test]
fn spawn_offset_is_included_in_stops_and_neutral_or_open_doors_cut_traction() {
    let mut train = session();
    assert!(train.start_chainage_m > 250.0);
    assert!(train.distance_to_next_stop_m().unwrap() < 1e-6);
    assert_eq!(train.route_progress(), 0.0);
    train.gameplay.stop_targets.clear();
    train.driver_throttle = 1.0;
    train.step_realtime(1.0, |_| {});
    assert_eq!(train.state.throttle, 0.0);
    assert_eq!(train.velocity_mps(), 0.0);
    train.driver_direction = 1.0;
    train.toggle_doors();
    train.step_realtime(1.0, |_| {});
    assert_eq!(train.exterior.door, DoorState::Open);
    assert_eq!(train.state.throttle, 0.0);
    assert_eq!(train.velocity_mps(), 0.0);
    train.state.velocity_mps = 2.0;
    train.toggle_doors();
    assert_eq!(train.exterior.door, DoorState::Open);
}

#[test]
fn passing_a_station_or_a_red_signal_fails_without_awarding_an_arrival() {
    let mut skipped = session();
    skipped.state.pos_on_edge_m += 20.0;
    skipped.step_realtime(0.05, |_| {});
    assert_eq!(skipped.gameplay.phase, ServicePhase::Failed);
    assert!(skipped.gameplay.stop_results.is_empty());

    let mut red = session();
    red.gameplay.stop_targets.clear();
    red.graph
        .insert_signal(TrackSignal {
            id: "oracle_red".into(),
            edge_id: red.current_edge_id().unwrap().into(),
            position_m: red.pos_on_edge_m() + 1.0,
            aspect: SignalAspect::Stop,
            clear_after_s: None,
            script: None,
        })
        .unwrap();
    red.signal_runtime
        .insert("oracle_red".into(), SignalAspect::Stop);
    red.state.velocity_mps = 4.0;
    for vehicle in &mut red.state.vehicles {
        vehicle.velocity_mps = 4.0;
    }
    red.step_realtime(0.5, |_| {});
    assert_eq!(red.gameplay.phase, ServicePhase::Failed);
    assert!(red.gameplay.stop_results.is_empty());
    assert!(red.gameplay.failure.unwrap().contains("Señal"));
}

#[test]
fn station_nodes_outside_the_chosen_path_are_rejected() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern_local/scenario.toml");
    let mut scenario = load_scenario(&path).unwrap();
    scenario.route.stops[1].node = "missing_station".into();
    assert!(LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).is_err());
}
