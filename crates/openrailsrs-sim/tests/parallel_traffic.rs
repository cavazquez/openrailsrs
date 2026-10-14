use openrailsrs_sim::{LiveDriveSession, LiveTraffic};
use std::path::PathBuf;
fn setup() -> (tempfile::TempDir, openrailsrs_scenarios::ScenarioFile) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    let mut graph = "[route]\nid=\"parallel-test\"\n".to_string();
    for lane in 0..9 {
        graph += &format!(
            "\n[[nodes]]\nid=\"a{lane}\"\nx_m=0.0\ny_m={y}.0\n[[nodes]]\nid=\"b{lane}\"\nx_m=10000.0\ny_m={y}.0\n[[edges]]\nid=\"lane{lane}\"\nfrom=\"a{lane}\"\nto=\"b{lane}\"\nlength_m=10000.0\nspeed_limit_kmh=90.0\ngrade_percent=0.0\n",
            y = lane * 100
        );
    }
    std::fs::write(temp.path().join("track.toml"), graph).unwrap();
    let mut scenario =
        openrailsrs_scenarios::load_scenario(repo.join("examples/player_lab/scenario.toml"))
            .unwrap();
    scenario.route.path = temp.path().to_string_lossy().into_owned();
    scenario.route.start = "a0".into();
    scenario.route.destination = "b0".into();
    scenario.route.stops.clear();
    scenario.route.waypoints.clear();
    scenario.train.consist = repo
        .join("examples/player_lab/consists/lab.con")
        .to_string_lossy()
        .into_owned();
    for lane in 1..9 {
        let entry=toml::from_str(&format!("id=\"service-{lane}\"\nconsist={}\nstart=\"a{lane}\"\ndestination=\"b{lane}\"\nstart_offset_m=80.0\noutput_csv=\"service-{lane}.csv\"",serde_json::to_string(&scenario.train.consist).unwrap())).unwrap();
        scenario.extra_trains.push(entry);
    }
    (temp, scenario)
}
#[test]
fn one_and_eight_workers_produce_identical_moving_trains_and_restore() {
    let (root, scenario) = setup();
    let mut player_one = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut player_many = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut one = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    let mut many = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    one.set_worker_count(1).unwrap();
    many.set_worker_count(8).unwrap();
    for turn in 0..20 {
        one.advance(&mut player_one, 1., Some(0.5), |_| {});
        many.advance(&mut player_many, 1., Some(0.5), |_| {});
        assert_eq!(
            serde_json::to_value(player_one.snapshot()).unwrap(),
            serde_json::to_value(player_many.snapshot()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(one.snapshot()).unwrap(),
            serde_json::to_value(many.snapshot()).unwrap()
        );
        if turn == 10 {
            many.restore_snapshot(one.snapshot()).unwrap();
        }
    }
    assert!(
        one.services
            .iter()
            .all(|s| s.departed && s.session.state.odometer_m > 1.)
    );
    assert_eq!(many.worker_count(), 8);
    assert!(many.set_worker_count(0).is_err());
    assert!(many.set_worker_count(9).is_err());
}
#[test]
fn driver_transfers_to_stopped_service_preserves_resources_and_restore_reconstructs_roles() {
    let (root, scenario) = setup();
    let mut player = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    traffic.services[0].departed = true;
    let old_player_fuel = player.state.diesel.cars[0].fuel_l;
    traffic.services[0].session.state.diesel.cars[0].fuel_l -= 3.;
    traffic.services[0].session.state.diesel.cars[0].consumed_l += 3.;
    let target_fuel = traffic.services[0].session.state.diesel.cars[0].fuel_l;
    player.driver_direction = 0.5;
    traffic.take_control(&mut player, 0).unwrap();
    assert_eq!(player.service_id, "service-1");
    assert_eq!(traffic.services[0].id, "Jugador");
    assert_eq!(player.state.diesel.cars[0].fuel_l, target_fuel);
    assert_eq!(
        traffic.services[0].session.state.diesel.cars[0].fuel_l,
        old_player_fuel
    );
    let save_player = player.snapshot();
    let save_traffic = traffic.snapshot();
    let mut reload = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut other = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    other.restore_control(&mut reload, 0).unwrap();
    reload.restore_snapshot(save_player).unwrap();
    other.restore_snapshot(save_traffic).unwrap();
    assert_eq!(reload.state.diesel.cars[0].fuel_l, target_fuel);
    assert_eq!(
        other.services[0].session.state.diesel.cars[0].fuel_l,
        old_player_fuel
    );
}
#[test]
fn preexisting_native_ai_is_simulated_before_the_activity_and_restores_without_resetting_reserves()
{
    let (root, mut scenario) = setup();
    scenario.extra_trains[0].pre_roll_s = 20.;
    scenario.extra_trains[0].relative_stop_times = true;
    let traffic = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    let service = &traffic.services[0];
    assert!(service.departed);
    assert!(service.session.head_chainage_m() > 100.);
    assert!(service.session.state.diesel.cars[0].consumed_l > 0.);
    assert!(service.session.time_s() < 0.01);
    assert!(service.session.velocity_mps() > 0.5);
    let saved = traffic.snapshot();
    let mut restored = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    restored.restore_snapshot(saved).unwrap();
    assert_eq!(
        restored.services[0].session.state.diesel.cars[0].fuel_l,
        service.session.state.diesel.cars[0].fuel_l
    );
    assert_eq!(
        restored.services[0].session.head_chainage_m(),
        service.session.head_chainage_m()
    );
}
#[test]
fn parked_services_have_real_handbrakes_and_remain_stationary_until_the_driver_takes_over() {
    let (root, mut scenario) = setup();
    scenario.extra_trains[0].parked = true;
    let mut player = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    let fuel = traffic.services[0].session.state.diesel.cars[0].fuel_l;
    let head = traffic.services[0].session.head_chainage_m();
    for _ in 0..100 {
        traffic.advance(&mut player, 0.05, None, |_| {});
    }
    assert_eq!(traffic.services[0].session.head_chainage_m(), head);
    assert_eq!(
        traffic.services[0].session.state.diesel.cars[0].fuel_l,
        fuel
    );
    assert!(
        traffic.services[0]
            .session
            .formation
            .cars
            .iter()
            .all(|c| c.handbrake)
    );
    player.driver_direction = 0.5;
    traffic.take_control(&mut player, 0).unwrap();
    assert!(player.time_s() > 4.9);
    assert!(player.formation.cars.iter().all(|c| c.handbrake));
}
#[test]
fn repeated_driver_swaps_restore_the_whole_role_permutation() {
    let (root, scenario) = setup();
    let mut player = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    player.driver_direction = 0.5;
    for service in &mut traffic.services {
        service.departed = true;
        service.session.driver_direction = 0.5;
    }
    for id in ["service-1", "service-2", "Jugador"] {
        let index = traffic.services.iter().position(|s| s.id == id).unwrap();
        traffic.take_control(&mut player, index).unwrap();
    }
    let saved = player.snapshot();
    let other = traffic.snapshot();
    let mut fresh = LiveDriveSession::from_scenario(root.path(), &scenario).unwrap();
    let mut restored = LiveTraffic::from_scenario(root.path(), &scenario).unwrap();
    for id in &traffic.control_history {
        let index = restored.services.iter().position(|s| &s.id == id).unwrap();
        restored.restore_control(&mut fresh, index).unwrap();
    }
    fresh.restore_snapshot(saved).unwrap();
    restored.restore_snapshot(other).unwrap();
    assert_eq!(fresh.service_id, player.service_id);
    assert_eq!(
        restored
            .services
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>(),
        traffic
            .services
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
    );
}
