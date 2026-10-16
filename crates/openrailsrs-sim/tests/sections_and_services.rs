use openrailsrs_sim::sections::CouplingEnd;
use openrailsrs_sim::{CarOperation, LiveDriveSession, LiveTraffic};
use std::path::Path;

fn scene(name: &str) -> (LiveDriveSession, LiveTraffic) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/portion_operations");
    let scenario = openrailsrs_scenarios::load_scenario(dir.join(name)).unwrap();
    (
        LiveDriveSession::from_scenario(&dir, &scenario).unwrap(),
        LiveTraffic::from_scenario(&dir, &scenario).unwrap(),
    )
}
fn positions(s: &LiveDriveSession) -> Vec<Option<(String, f64)>> {
    (0..s.formation.cars.len())
        .map(|i| {
            let x = s.formation.center_offset_m(i);
            if i < s.formation.coupled_count {
                s.position_at_head_offset(x)
            } else {
                s.presentation_position_for_car(x, x, 0.)
            }
        })
        .collect()
}
fn secure(s: &mut LiveDriveSession) {
    for i in 1..s.formation.coupled_count {
        if !s.formation.cars[i].handbrake {
            s.operate_car(i, CarOperation::Handbrake).unwrap();
        }
    }
}

#[test]
fn two_named_sections_preserve_track_brakes_and_resume_collecting_recent_first() {
    let (mut s, _) = scene("scenario.toml");
    secure(&mut s);
    let before = positions(&s);
    s.state.brake_system.cylinders[3].current_force_n = 1234.;
    s.state.brake_system.cylinders[4].current_force_n = 2345.;
    s.state.couplers[3].extension_m = 0.0003;
    s.uncouple_after_named(2, "Rama-B").unwrap();
    s.uncouple_after_named(0, "Rama-C").unwrap();
    assert_eq!(s.detached_sections().len(), 2);
    assert_eq!(positions(&s), before);
    assert!(!s.section_track_occupancy("Rama-B").unwrap().is_empty());
    assert!(!s.section_track_occupancy("Rama-C").unwrap().is_empty());
    let saved = s.snapshot();
    s.validate_snapshot(&saved).unwrap();
    let (mut restored, _) = scene("scenario.toml");
    restored
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    restored
        .recouple_section("Rama-C", CouplingEnd::Rear)
        .unwrap();
    restored
        .recouple_section("Rama-B", CouplingEnd::Rear)
        .unwrap();
    assert_eq!(positions(&restored), before);
    assert_eq!(
        restored.state.brake_system.cylinders[3].current_force_n,
        1234.
    );
    assert_eq!(
        restored.state.brake_system.cylinders[4].current_force_n,
        2345.
    );
    assert_eq!(restored.state.couplers[3].extension_m, 0.0003);
    assert_eq!(restored.formation.coupled_count, 5);
    restored.validate_snapshot(&restored.snapshot()).unwrap();
}

#[test]
fn oldest_section_can_be_picked_at_front_then_restore_permuted_hardware() {
    let (mut s, _) = scene("scenario.toml");
    secure(&mut s);
    let before = positions(&s);
    s.uncouple_after_named(2, "B").unwrap();
    s.uncouple_after_named(0, "C").unwrap();
    // Prescribed contact pose of the locomotive after a run-around. This is a
    // geometry/state test, not evidence that a loop journey was driven here.
    s.state.pos_on_edge_m = 100.;
    let parked_before = positions(&s)[1..].to_vec();
    s.recouple_section("B", CouplingEnd::Front).unwrap();
    assert_eq!(s.formation.coupled_count, 3);
    assert_eq!(s.formation.vehicle_ids[0], "Jugador/car/3");
    let saved = s.snapshot();
    s.validate_snapshot(&saved).unwrap();
    let (mut resumed, _) = scene("scenario.toml");
    resumed.restore_snapshot(saved).unwrap();
    resumed.recouple_section("C", CouplingEnd::Front).unwrap();
    assert_eq!(
        resumed.formation.vehicle_ids,
        vec![
            "Jugador/car/1",
            "Jugador/car/2",
            "Jugador/car/3",
            "Jugador/car/4",
            "Jugador/car/0"
        ]
    );
    for (i, old) in parked_before.iter().enumerate() {
        assert_eq!(&positions(&resumed)[i], old);
    }
    assert_eq!(positions(&resumed)[..4], before[1..]);
    resumed.validate_snapshot(&resumed.snapshot()).unwrap();
}

#[test]
fn rejects_duplicate_section_invalid_track_and_distant_pickup_without_losing_state() {
    let (mut s, _) = scene("scenario.toml");
    secure(&mut s);
    s.uncouple_after_named(2, "B").unwrap();
    assert!(s.uncouple_after_named(0, "B").is_err());
    s.state.pos_on_edge_m += 10.;
    let before = serde_json::to_value(s.snapshot()).unwrap();
    assert!(s.recouple_section("B", CouplingEnd::Rear).is_err());
    assert_eq!(serde_json::to_value(s.snapshot()).unwrap(), before);
    let mut bad = s.snapshot();
    bad.formation.sections[0].path_edges = vec!["approach".into(), "exit_b".into()];
    assert!(s.validate_snapshot(&bad).unwrap_err().contains("salta"));
    let mut bad = s.snapshot();
    bad.formation.vehicle_ids[1] = bad.formation.vehicle_ids[0].clone();
    assert!(s.validate_snapshot(&bad).is_err());
}

#[test]
fn a_cut_without_track_under_the_whole_formation_rejects_without_mutation() {
    for reversed in [false, true] {
        let (mut s, _) = scene("scenario.toml");
        secure(&mut s);
        s.state.path_reversed = reversed;
        let length = s.formation.length_m();
        let path_length: f64 = s
            .state
            .path_edges
            .iter()
            .map(|id| s.graph.edge(id).unwrap().length_m)
            .sum();
        for head in [0., length - 0.01, path_length + 0.01] {
            s.state.edge_index = 0;
            s.state.pos_on_edge_m = head;
            s.driver_throttle = 0.4;
            let before = serde_json::to_value(s.snapshot()).unwrap();
            let error = s.uncouple_after_named(0, "Sin-vía").unwrap_err();
            assert!(error.contains("vía suficiente"), "{error}");
            assert_eq!(serde_json::to_value(s.snapshot()).unwrap(), before);
            assert!(s.detached_sections().is_empty());
        }
    }
}

#[test]
fn a_cut_exactly_inside_track_keeps_every_position_and_restores_in_both_directions() {
    for reversed in [false, true] {
        let (mut s, _) = scene("scenario.toml");
        secure(&mut s);
        s.state.path_reversed = reversed;
        s.state.pos_on_edge_m = s.formation.length_m();
        let before = positions(&s);
        s.uncouple_after_named(0, "Límite-real").unwrap();
        assert_eq!(positions(&s), before);
        s.validate_snapshot(&s.snapshot()).unwrap();
        let (mut restored, _) = scene("scenario.toml");
        restored.restore_snapshot(s.snapshot()).unwrap();
        assert_eq!(positions(&restored), before);
    }
}

#[test]
fn foreign_portion_returns_to_service_with_schedule_brakes_identity_and_save_replay() {
    let (mut s, mut traffic) = scene("scenario_services.toml");
    let before = traffic.services[0].session.snapshot();
    let dep = traffic.services[0].departure_s;
    traffic.couple_service(&mut s, 0).unwrap();
    assert!(traffic.services.is_empty());
    assert_eq!(s.formation.coupled_count, 6);
    // Cylinder pressure/force and finite reservoirs survive; the new independent
    // cab recalculates pipe isolation and propagation distance at its own ends.
    let foreign_pressure: Vec<_> = s.state.brake_system.cylinders[3..]
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).unwrap();
            v.as_object_mut().unwrap().remove("position_m");
            v.as_object_mut().unwrap().remove("air_isolated");
            v
        })
        .collect();
    traffic.split_service(&mut s, "Porción-Sur").unwrap();
    assert_eq!(s.formation.coupled_count, 3);
    assert_eq!(traffic.services.len(), 1);
    assert_eq!(traffic.services[0].departure_s, dep);
    let returned: Vec<_> = traffic.services[0]
        .session
        .state
        .brake_system
        .cylinders
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).unwrap();
            v.as_object_mut().unwrap().remove("position_m");
            v.as_object_mut().unwrap().remove("air_isolated");
            v
        })
        .collect();
    assert_eq!(returned, foreign_pressure);
    assert_eq!(
        traffic.services[0]
            .session
            .snapshot()
            .gameplay
            .stop_targets
            .len(),
        before.gameplay.stop_targets.len()
    );
    let player_saved = s.snapshot();
    let traffic_saved = traffic.snapshot();
    let receipts = traffic.couplings.clone();
    let (mut restored, mut replay) = scene("scenario_services.toml");
    replay
        .reconstruct_couplings(&mut restored, &receipts)
        .unwrap();
    replay
        .restore_shared_snapshots(&mut restored, player_saved, traffic_saved)
        .unwrap();
    assert_eq!(replay.services.len(), 1);
    assert_eq!(
        restored.formation.cars.len() + replay.services[0].session.formation.cars.len(),
        6
    );
    traffic.couple_service(&mut s, 0).unwrap();
    assert_eq!(s.formation.cars.len(), 6);
}

fn release(s: &mut LiveDriveSession) {
    for i in 0..s.formation.coupled_count {
        if s.formation.cars[i].handbrake {
            s.operate_car(i, CarOperation::Handbrake).unwrap();
        }
        if i > 0 && !s.formation.cars[i].hose_connected {
            s.operate_car(i, CarOperation::Hose).unwrap();
        }
        if !s.formation.cars[i].front_cock_open {
            s.operate_car(i, CarOperation::FrontCock).unwrap();
        }
        if !s.formation.cars[i].rear_cock_open {
            s.operate_car(i, CarOperation::RearCock).unwrap();
        }
    }
}
#[test]
fn reformed_train_drives_after_real_pressure_release_and_save_continues_identically() {
    let (mut s, _) = scene("scenario.toml");
    secure(&mut s);
    s.uncouple_after_named(2, "B").unwrap();
    s.uncouple_after_named(0, "C").unwrap();
    s.recouple_section("C", CouplingEnd::Rear).unwrap();
    s.recouple_section("B", CouplingEnd::Rear).unwrap();
    release(&mut s);
    s.driver_direction = 1.;
    s.driver_throttle = 0.4;
    s.driver_brake = 0.;
    let before = s.head_chainage_m();
    for _ in 0..400 {
        s.step_realtime(0.05, |_| {});
    }
    assert!(
        s.head_chainage_m() - before > 1.,
        "{}",
        s.head_chainage_m() - before
    );
    let (mut restored, _) = scene("scenario.toml");
    restored.restore_snapshot(s.snapshot()).unwrap();
    for _ in 0..200 {
        s.step_realtime(0.05, |_| {});
        restored.step_realtime(0.05, |_| {});
    }
    assert_eq!(s.head_chainage_m(), restored.head_chainage_m());
    assert_eq!(s.velocity_mps(), restored.velocity_mps());
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    for _ in 0..400 {
        s.step_realtime(0.05, |_| {});
    }
    assert!(s.velocity_mps() < 0.1);
}
#[test]
fn timetable_foreign_operations_use_physical_contact_and_preserve_progress_on_restore() {
    use openrailsrs_scenarios::model::{RouteLeg, TimetableAction, TimetableOperation};
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/portion_operations");
    let mut definition =
        openrailsrs_scenarios::load_scenario(dir.join("scenario_services.toml")).unwrap();
    definition.route.legs = vec![RouteLeg {
        operations: vec![
            TimetableOperation {
                chainage_m: 240.,
                section: "Porción-Sur".into(),
                manual: false,
                action: TimetableAction::JoinService {
                    service: "porción-sur".into(),
                    front: None,
                },
            },
            TimetableOperation {
                chainage_m: 240.,
                section: "Porción-Sur".into(),
                manual: true,
                action: TimetableAction::SplitService {
                    service: "Porción-Sur".into(),
                },
            },
        ],
        service_id: Some("Central".into()),
        not_before_s: None,
        start: "a".into(),
        destination: "destination".into(),
        edges: vec![
            "approach".into(),
            "stem".into(),
            "straight".into(),
            "exit_b".into(),
        ],
        start_offset_m: 240.,
        end_chainage_m: 1000.,
        reversed: false,
        wait_s: 0.,
        stops: vec![],
        switches: vec![],
    }];
    let mut s = LiveDriveSession::from_scenario(&dir, &definition).unwrap();
    let mut traffic = LiveTraffic::from_scenario(&dir, &definition).unwrap();
    traffic.advance(&mut s, 0.05, None, |_| {});
    assert_eq!(s.formation.coupled_count, 6);
    assert_eq!(s.snapshot().operational_operation, 1);
    let saved = s.snapshot();
    let traffic_saved = traffic.snapshot();
    let receipts = traffic.couplings.clone();
    let mut restored = LiveDriveSession::from_scenario(&dir, &definition).unwrap();
    let mut replay = LiveTraffic::from_scenario(&dir, &definition).unwrap();
    replay
        .reconstruct_couplings(&mut restored, &receipts)
        .unwrap();
    replay
        .restore_shared_snapshots(&mut restored, saved, traffic_saved)
        .unwrap();
    for i in 3..6 {
        restored.operate_car(i, CarOperation::Handbrake).unwrap();
    }
    replay.split_service(&mut restored, "Porción-Sur").unwrap();
    assert_eq!(restored.snapshot().operational_operation, 2);
    assert_eq!(replay.services[0].departure_s, 42.);
}

#[test]
fn moving_backwards_cannot_cut_join_or_return_a_portion() {
    let (mut s, mut t) = scene("scenario_services.toml");
    secure(&mut s);
    s.state.velocity_mps = -2.;
    assert!(s.uncouple_after_named(0, "moving").is_err());
    assert!(t.couple_service_at(&mut s, 0, CouplingEnd::Rear).is_err());
    s.state.velocity_mps = 0.;
    t.couple_service(&mut s, 0).unwrap();
    s.state.velocity_mps = -2.;
    assert!(t.split_service(&mut s, "Porción-Sur").is_err());
    s.state.velocity_mps = 0.;
    t.split_service(&mut s, "Porción-Sur").unwrap();
    s.uncouple_after_named(0, "own").unwrap();
    s.state.velocity_mps = -2.;
    assert!(s.recouple_section("own", CouplingEnd::Rear).is_err());
}

#[test]
fn parked_native_bearings_remain_with_their_vehicle_through_cut_pickup_and_restore() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/portion_operations");
    let mut definition = openrailsrs_scenarios::load_scenario(dir.join("scenario.toml")).unwrap();
    definition.simulation.native_physics = Some(Default::default());
    let mut s = LiveDriveSession::from_scenario(&dir, &definition).unwrap();
    secure(&mut s);
    s.step_realtime(0.05, |_| {});
    s.state.native_dynamics.as_mut().unwrap().bearing_c = vec![31., 32., 33., 34., 35.];
    s.uncouple_after_named(2, "B").unwrap();
    s.uncouple_after_named(0, "C").unwrap();
    s.state.native_dynamics.as_mut().unwrap().bearing_c[0] = 41.;
    let saved = s.snapshot();
    let mut resumed = LiveDriveSession::from_scenario(&dir, &definition).unwrap();
    resumed.restore_snapshot(saved).unwrap();
    resumed.recouple_section("C", CouplingEnd::Rear).unwrap();
    resumed.recouple_section("B", CouplingEnd::Rear).unwrap();
    assert_eq!(
        resumed.state.native_dynamics.as_ref().unwrap().bearing_c,
        vec![41., 32., 33., 34., 35.]
    );
    let mut invalid = resumed.snapshot();
    invalid
        .formation
        .native_bearing_c
        .insert("other/car/0".into(), 20.);
    assert!(resumed.validate_snapshot(&invalid).is_err());
}

#[test]
fn joining_transfers_both_authorities_and_split_partitions_the_same_physical_grant() {
    use openrailsrs_sim::native_signals::TrackOccupancy;
    let (mut s, mut traffic) = scene("scenario_services.toml");
    for session in [&mut s, &mut traffic.services[0].session] {
        for (id, position) in [("authority-start", 250.), ("authority-end", 260.)] {
            session
                .graph
                .insert_signal(openrailsrs_track::TrackSignal {
                    id: id.into(),
                    edge_id: "approach".into(),
                    position_m: position,
                    aspect: openrailsrs_track::SignalAspect::Clear,
                    clear_after_s: None,
                    script: None,
                })
                .unwrap();
        }
    }
    s.own_track_reservations = vec![TrackOccupancy {
        edge: "approach".into(),
        start_m: 180.,
        end_m: 260.,
        owner: s.service_id.clone(),
    }];
    traffic.services[0].session.own_track_reservations = vec![TrackOccupancy {
        edge: "approach".into(),
        start_m: 120.,
        end_m: 180.,
        owner: "Porción-Sur".into(),
    }];
    traffic.couple_service(&mut s, 0).unwrap();
    assert!(
        s.own_track_reservations
            .iter()
            .any(|g| g.start_m == 120. && g.end_m == 260. && g.owner == "Jugador")
    );
    traffic.split_service(&mut s, "Porción-Sur").unwrap();
    assert!(
        s.own_track_reservations
            .iter()
            .any(|g| g.start_m == 180. && g.end_m == 260. && g.owner == "Jugador")
    );
    assert!(
        traffic.services[0]
            .session
            .own_track_reservations
            .iter()
            .any(|g| g.start_m == 120. && g.end_m == 180. && g.owner == "Porción-Sur")
    );
    assert!(
        openrailsrs_sim::SessionSnapshot::validate_shared_dispatcher([
            &s.snapshot(),
            &traffic.services[0].session.snapshot()
        ])
        .is_ok()
    );
}

#[test]
fn foreign_front_pickup_and_return_preserve_both_train_positions_and_identity() {
    let (mut s, mut traffic) = scene("scenario_services.toml");
    // Prescribed locomotive contact pose, not a claim that this fixture drove
    // a loop. The installed MidEast replay drives the actual native loop.
    s.state.pos_on_edge_m = 120.;
    let own = positions(&s);
    let foreign = positions(&traffic.services[0].session);
    traffic
        .couple_service_at(&mut s, 0, CouplingEnd::Front)
        .unwrap();
    assert_eq!(&positions(&s)[..3], foreign.as_slice());
    assert_eq!(&positions(&s)[3..], own.as_slice());
    assert_eq!(s.cab_vehicle(), 3);
    traffic.split_service(&mut s, "Porción-Sur").unwrap();
    assert_eq!(positions(&s), own);
    assert_eq!(positions(&traffic.services[0].session), foreign);
    s.validate_snapshot(&s.snapshot()).unwrap();
    traffic.validate_snapshot(&traffic.snapshot()).unwrap();
}

#[test]
fn a_front_pickup_keeps_the_rear_cab_in_control_of_its_own_pipe_component() {
    let (mut s, _) = scene("scenario.toml");
    secure(&mut s);
    s.uncouple_after_named(2, "B").unwrap();
    s.uncouple_after_named(0, "C").unwrap();
    s.state.pos_on_edge_m = 100.; // prescribed contact geometry only
    s.recouple_section("B", CouplingEnd::Front).unwrap();
    assert_eq!(s.cab_vehicle(), 2);
    assert!(
        s.state.brake_system.cylinders[..2]
            .iter()
            .all(|c| c.air_isolated)
    );
    assert!(!s.state.brake_system.cylinders[2].air_isolated);
    s.operate_car(2, CarOperation::Hose).unwrap();
    s.operate_car(1, CarOperation::RearCock).unwrap();
    s.operate_car(2, CarOperation::FrontCock).unwrap();
    assert!(
        s.state
            .brake_system
            .cylinders
            .iter()
            .all(|c| !c.air_isolated)
    );
    s.operate_car(1, CarOperation::RearCock).unwrap();
    assert!(
        s.state.brake_system.cylinders[..2]
            .iter()
            .all(|c| c.air_isolated)
    );
    assert!(!s.state.brake_system.cylinders[2].air_isolated);
}

#[test]
fn a_future_service_cannot_be_collected_before_its_authorized_spawn() {
    let (mut s, mut traffic) = scene("scenario_services.toml");
    traffic.services[0].parked = false;
    traffic.services[0].departed = false;
    let before = serde_json::to_value(s.snapshot()).unwrap();
    assert!(
        traffic
            .couple_service(&mut s, 0)
            .unwrap_err()
            .contains("presente")
    );
    assert_eq!(serde_json::to_value(s.snapshot()).unwrap(), before);
    traffic.services[0].departure_s = 0.;
    traffic.couple_service(&mut s, 0).unwrap();
    assert!(traffic.couplings[0].departed);
}

#[test]
fn control_swaps_between_transfers_replay_in_chronological_receiver_order() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/portion_operations");
    let mut definition =
        openrailsrs_scenarios::load_scenario(dir.join("scenario_services.toml")).unwrap();
    let mut third = definition.extra_trains[0].clone();
    third.id = "Auxiliar".into();
    third.start_offset_m = 320.;
    definition.extra_trains.push(third);
    let make = || {
        (
            LiveDriveSession::from_scenario(&dir, &definition).unwrap(),
            LiveTraffic::from_scenario(&dir, &definition).unwrap(),
        )
    };
    let (mut player, mut traffic) = make();
    traffic.couple_service(&mut player, 0).unwrap();
    traffic.split_service(&mut player, "Porción-Sur").unwrap();
    player.driver_direction = 0.5;
    let foreign = traffic
        .services
        .iter()
        .position(|s| s.id == "Porción-Sur")
        .unwrap();
    traffic.take_control(&mut player, foreign).unwrap();
    assert_eq!(player.service_id, "Porción-Sur");
    let old_player = traffic
        .services
        .iter()
        .position(|s| s.id == "Jugador")
        .unwrap();
    for i in 0..3 {
        traffic.services[old_player]
            .session
            .operate_car(i, CarOperation::Handbrake)
            .unwrap();
    }
    traffic
        .couple_service_at(&mut player, old_player, CouplingEnd::Front)
        .unwrap();
    assert_eq!(
        traffic
            .couplings
            .iter()
            .map(|r| r.receiver.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["Jugador", "Jugador", "Porción-Sur"]
    );
    let saved = player.snapshot();
    let others = traffic.snapshot();
    let receipts = traffic.couplings.clone();
    let history = traffic.control_history.clone();
    let poses = positions(&player);
    let (mut resumed, mut replay) = make();
    replay
        .reconstruct_service_history(&mut resumed, &history, "Porción-Sur", &receipts)
        .unwrap();
    replay
        .restore_shared_snapshots(&mut resumed, saved, others)
        .unwrap();
    assert_eq!(resumed.service_id, "Porción-Sur");
    assert_eq!(positions(&resumed), poses);
    assert_eq!(resumed.vehicle_identities(), player.vehicle_identities());
    assert_eq!(
        resumed.formation.cars.len()
            + replay
                .services
                .iter()
                .map(|s| s.session.formation.cars.len())
                .sum::<usize>(),
        9
    );
    assert_eq!(replay.control_history, history);
}

fn delivery_definition(
    service: &str,
    vehicles: Vec<usize>,
    start_m: f64,
    end_m: f64,
) -> openrailsrs_scenarios::rail_operations::YardDeliveryDef {
    openrailsrs_scenarios::rail_operations::YardDeliveryDef {
        service: Some(service.into()),
        id: "identity-delivery".into(),
        destination: "Same physical siding".into(),
        edge: "approach".into(),
        start_m,
        end_m,
        vehicles,
    }
}
fn delivery_scene(
    name: &str,
    delivery: &openrailsrs_scenarios::rail_operations::YardDeliveryDef,
) -> (LiveDriveSession, LiveTraffic) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/portion_operations");
    let mut scenario = openrailsrs_scenarios::load_scenario(dir.join(name)).unwrap();
    scenario.simulation.rail_operations.yard_deliveries = vec![delivery.clone()];
    (
        LiveDriveSession::from_scenario(&dir, &scenario).unwrap(),
        LiveTraffic::from_scenario(&dir, &scenario).unwrap(),
    )
}
#[test]
fn front_foreign_pickup_inventory_uses_original_ids_through_json_restore_and_delivery() {
    let delivery = delivery_definition("Porción-Sur", vec![1, 2], 120., 160.);
    let (mut player, mut traffic) = delivery_scene("scenario_services.toml", &delivery);
    // Prescribed physical contact, as in the front-coupling geometry test.
    // The test verifies identity and delivery, not a driven run-around.
    player.state.pos_on_edge_m = 120.;
    let own = positions(&player);
    let foreign = positions(&traffic.services[0].session);
    let original_ids = traffic.services[0].session.vehicle_identities();
    traffic
        .couple_service_at(&mut player, 0, CouplingEnd::Front)
        .unwrap();
    assert_eq!(&positions(&player)[..3], foreign.as_slice());
    assert_eq!(&positions(&player)[3..], own.as_slice());
    assert_eq!(&player.vehicle_identities()[..3], original_ids.as_slice());
    let status = &traffic.yard_inventory(&player)[0];
    assert_eq!(status.actual_vehicles, vec![1, 2]);
    assert!(
        !status.accepted,
        "coaches are still attached to powered stock"
    );
    assert!(status.reason.contains("Separá"), "{}", status.reason);
    let saved_player =
        serde_json::from_slice(&serde_json::to_vec(&player.snapshot()).unwrap()).unwrap();
    let saved_traffic =
        serde_json::from_slice(&serde_json::to_vec(&traffic.snapshot()).unwrap()).unwrap();
    let receipts: Vec<openrailsrs_sim::independent_coupling::CouplingReceipt> =
        serde_json::from_slice(&serde_json::to_vec(&traffic.couplings).unwrap()).unwrap();
    let (mut restored, mut others) = delivery_scene("scenario_services.toml", &delivery);
    others
        .reconstruct_couplings(&mut restored, &receipts)
        .unwrap();
    others
        .restore_shared_snapshots(&mut restored, saved_player, saved_traffic)
        .unwrap();
    assert_eq!(positions(&restored), positions(&player));
    assert_eq!(restored.vehicle_identities(), player.vehicle_identities());
    assert!(
        others.yard_inventory(&restored)[0]
            .reason
            .contains("Separá")
    );
    others.split_service(&mut restored, "Porción-Sur").unwrap();
    others.services[0]
        .session
        .uncouple_after_named(0, "delivered-coaches")
        .unwrap();
    assert_eq!(positions(&others.services[0].session), foreign);
    assert_eq!(
        others.services[0].session.vehicle_identities(),
        original_ids
    );
    assert_eq!(
        restored.formation.cars.len() + others.services[0].session.formation.cars.len(),
        6
    );
    assert!(others.yard_inventory(&restored)[0].accepted);
}
#[test]
fn own_permuted_section_delivery_keeps_original_vehicle_numbers_after_restore() {
    let delivery = delivery_definition("Jugador", vec![3, 4], 100., 140.);
    let (mut player, traffic) = delivery_scene("scenario.toml", &delivery);
    secure(&mut player);
    player.uncouple_after_named(2, "B").unwrap();
    assert!(player.yard_deliveries()[0].accepted);
    assert!(traffic.yard_inventory(&player)[0].accepted);
    player.uncouple_after_named(0, "C").unwrap();
    // Prescribed front contact after a run-around; the parked stock never moves.
    player.state.pos_on_edge_m = 100.;
    player.recouple_section("B", CouplingEnd::Front).unwrap();
    assert_eq!(
        &player.vehicle_identities()[..2],
        &["Jugador/car/3", "Jugador/car/4"]
    );
    for status in [
        player.yard_deliveries().remove(0),
        traffic.yard_inventory(&player).remove(0),
    ] {
        assert_eq!(status.actual_vehicles, vec![0, 1]);
        assert!(!status.accepted);
        assert!(status.reason.contains("Separá"), "{}", status.reason);
    }
    let saved = serde_json::from_slice(&serde_json::to_vec(&player.snapshot()).unwrap()).unwrap();
    let (mut restored, others) = delivery_scene("scenario.toml", &delivery);
    restored.restore_snapshot(saved).unwrap();
    assert_eq!(positions(&restored), positions(&player));
    assert_eq!(restored.vehicle_identities(), player.vehicle_identities());
    assert!(restored.yard_deliveries()[0].reason.contains("Separá"));
    assert!(
        others.yard_inventory(&restored)[0]
            .reason
            .contains("Separá")
    );
}
