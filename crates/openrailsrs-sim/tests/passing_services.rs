use openrailsrs_sim::{LiveDriveSession, LiveTraffic};
use std::path::PathBuf;
fn setup() -> (tempfile::TempDir, openrailsrs_scenarios::ScenarioFile) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut scenario =
        openrailsrs_scenarios::load_scenario(root.join("examples/yard_training/scenario.toml"))
            .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut graph = "[route]\nid='passing'\n".to_string();
    for (id, x, y) in [
        ("a", 0., 0.),
        ("p", 0., 100.),
        ("junction", 100., 0.),
        ("exit", 400., 0.),
        ("destination", 600., 0.),
        ("express_end", 1000., 0.),
    ] {
        graph += &format!("\n[[nodes]]\nid='{id}'\nx_m={x:.1}\ny_m={y:.1}\n");
    }
    for (id, from, to, length) in [
        ("approach", "a", "junction", 100.),
        ("passing_approach", "p", "junction", 100.),
        ("shared", "junction", "exit", 300.),
        ("siding", "junction", "exit", 350.),
        ("exit_track", "exit", "destination", 200.),
        ("express_exit", "destination", "express_end", 400.),
    ] {
        graph += &format!(
            "\n[[edges]]\nid='{id}'\nfrom='{from}'\nto='{to}'\nlength_m={length:.1}\nspeed_limit_kmh=25.0\ngrade_percent=0.0\n"
        );
        graph += &format!(
            "\n[[edges]]\nid='{id}_r'\nfrom='{to}'\nto='{from}'\nlength_m={length:.1}\nspeed_limit_kmh=25.0\ngrade_percent=0.0\n"
        );
    }
    std::fs::write(temp.path().join("track.toml"), graph).unwrap();
    scenario.route.path = temp.path().to_string_lossy().into_owned();
    scenario.route.start_offset_m = Some(80.);
    scenario.route.stops.clear();
    scenario.gameplay.shunting.clear();
    scenario.route.waypoints = vec![
        "a".into(),
        "junction".into(),
        "exit".into(),
        "destination".into(),
    ];
    scenario.route.stops.push(openrailsrs_scenarios::StopDef {
        node: "junction".into(),
        name: Some("Parada local".into()),
        offset_m: -15.,
        arrive_s: 30.,
        depart_s: 60.,
        dwell_s: 30.,
        passengers_on: 10,
        passengers_off: 0,
    });
    scenario.train.consist = root
        .join("examples/yard_training/consists/yard.con")
        .to_string_lossy()
        .into_owned();
    let mut passing:openrailsrs_scenarios::TrainEntryDef=toml::from_str("id='express'\nconsist='x'\nstart='p'\ndestination='express_end'\nstart_offset_m=80.0\noutput_csv='express.csv'").unwrap();
    passing.consist = scenario.train.consist.clone();
    scenario.extra_trains.push(passing);
    scenario.simulation.rail_operations.passing_plans.push(toml::from_str("id='meet'\nwaiting_service='Jugador'\npassing_service='express'\nsiding_edge='siding'\nposition_m=220.0\nclear_edge='shared'\nalternatives=[['approach','siding','exit_track']]").unwrap());
    (temp, scenario)
}
#[test]
fn dispatcher_selects_a_siding_before_the_meet_and_releases_only_after_the_other_tail_clears() {
    acceptance(false);
    acceptance(true);
}
fn acceptance(opposed: bool) {
    let (root, mut s) = setup();
    if opposed {
        s.extra_trains[0].start = "destination".into();
        s.extra_trains[0].destination = "a".into();
        s.extra_trains[0].waypoints = vec![
            "destination".into(),
            "exit".into(),
            "junction".into(),
            "a".into(),
        ];
    }
    let mut player = LiveDriveSession::from_scenario(root.path(), &s).unwrap();
    let mut traffic = LiveTraffic::from_scenario(root.path(), &s).unwrap();
    let mut observed_hold = false;
    let mut observed_complete = false;
    let mut observed_saved = false;
    for _ in 0..16000 {
        traffic.advance(&mut player, 0.05, Some(0.5), |_| {});
        if player
            .state
            .rail_operations
            .passing_started
            .contains("meet")
        {
            observed_hold = true;
            assert!(player.state.path_edges.contains(&"siding".into()));
            if !player
                .state
                .rail_operations
                .passing_complete
                .contains("meet")
            {
                assert!(player.head_chainage_m() < 322.);
            }
        }
        if player.state.rail_operations.passing_seen.contains("meet") && !observed_saved {
            let saved = player.snapshot();
            let others = traffic.snapshot();
            let mut restored = LiveDriveSession::from_scenario(root.path(), &s).unwrap();
            let mut restored_traffic = LiveTraffic::from_scenario(root.path(), &s).unwrap();
            restored.restore_snapshot(saved).unwrap();
            restored_traffic.restore_snapshot(others).unwrap();
            player = restored;
            traffic = restored_traffic;
            observed_saved = true;
        }
        if player
            .state
            .rail_operations
            .passing_complete
            .contains("meet")
        {
            assert!(
                traffic.services[0]
                    .session
                    .own_track_occupancy()
                    .iter()
                    .all(|p| p.edge != "shared")
            );
            observed_complete = true;
        }
        if player.arrived && traffic.services[0].session.arrived {
            break;
        }
    }
    assert_eq!(player.gameplay.stop_results.len(), 1);
    assert_eq!(player.gameplay.stop_results[0].name, "Parada local");
    assert!(player.gameplay.missed_stops.is_empty());
    assert!(observed_hold && observed_saved && observed_complete);
    assert!(
        player.arrived && traffic.services[0].session.arrived,
        "opposed={opposed}, player=({:?},{:.1},{:.1},{:?}) other=({:?},{:.1},{:.1},{:?})",
        player.current_edge_id(),
        player.pos_on_edge_m(),
        player.velocity_mps(),
        player.gameplay.phase,
        traffic.services[0].session.current_edge_id(),
        traffic.services[0].session.pos_on_edge_m(),
        traffic.services[0].session.velocity_mps(),
        traffic.services[0].session.gameplay.phase
    );
}
#[test]
fn too_short_siding_is_rejected_with_a_diagnostic_and_no_position_change() {
    let (root, mut s) = setup();
    s.simulation.rail_operations.passing_plans[0].position_m = 30.;
    let mut player = LiveDriveSession::from_scenario(root.path(), &s).unwrap();
    let mut traffic = LiveTraffic::from_scenario(root.path(), &s).unwrap();
    traffic.services[0].departed = true;
    let head = player.head_chainage_m();
    traffic.synchronize_occupancy(&mut player);
    assert_eq!(player.head_chainage_m(), head);
    assert!(
        !player
            .state
            .rail_operations
            .passing_started
            .contains("meet")
    );
    assert!(player.state.rail_operations.notice.contains("corto"));
}

#[test]
fn shipped_overtake_and_meet_examples_complete_both_real_services_without_missing_the_local_stop() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["scenario_overtake.toml", "scenario_meet.toml"] {
        let path = root.join("examples/traffic_passing").join(name);
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        let mut player =
            LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        let mut traffic = LiveTraffic::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        for _ in 0..24000 {
            traffic.advance(&mut player, 0.05, Some(0.5), |_| {});
            if player.arrived && traffic.services[0].session.arrived {
                break;
            }
        }
        assert!(
            player.arrived && traffic.services[0].session.arrived,
            "{name}: {:?} / {:?}",
            player.gameplay.phase,
            traffic.services[0].session.gameplay.phase
        );
        assert!(
            player
                .state
                .rail_operations
                .passing_complete
                .contains("paso-programado")
        );
        assert!(player.gameplay.missed_stops.is_empty());
        assert_eq!(player.gameplay.stop_results[0].name, "Parada del local");
    }
}
