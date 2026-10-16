//! Live, shared-world acceptance for operational yards. Prescribed collision
//! initial conditions are identified separately from the driven exercises.
use openrailsrs_sim::operational_yards::{YardCommand, validate_shared_yard_snapshots};
use openrailsrs_sim::{LiveDriveSession, LiveTraffic};
use std::path::{Path, PathBuf};
fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/operational_yards")
}
fn definition(name: &str) -> openrailsrs_scenarios::ScenarioFile {
    openrailsrs_scenarios::load_scenario(directory().join(name)).unwrap()
}
fn sessions(s: &openrailsrs_scenarios::ScenarioFile) -> (LiveDriveSession, LiveTraffic) {
    let mut player = LiveDriveSession::from_scenario(&directory(), s).unwrap();
    player.driver_throttle = 0.;
    player.driver_brake = 1.;
    player.driver_direction = 0.5;
    (player, LiveTraffic::from_scenario(&directory(), s).unwrap())
}
fn resume(
    s: &openrailsrs_scenarios::ScenarioFile,
    player: &mut LiveDriveSession,
    traffic: &mut LiveTraffic,
) {
    let saved = serde_json::from_slice(&serde_json::to_vec(&player.snapshot()).unwrap()).unwrap();
    let other = serde_json::from_slice(&serde_json::to_vec(&traffic.snapshot()).unwrap()).unwrap();
    let (mut resumed, mut others) = sessions(s);
    others
        .reconstruct_service_history(
            &mut resumed,
            &traffic.control_history,
            &player.service_id,
            &traffic.couplings,
        )
        .unwrap();
    others
        .restore_shared_snapshots(&mut resumed, saved, other)
        .unwrap();
    *player = resumed;
    *traffic = others;
}
fn normal(s: &LiveDriveSession, p: (String, f64)) -> (String, f64) {
    let e = s.graph.edge(&p.0).unwrap();
    (
        p.0.trim_end_matches("_r").into(),
        if p.0.ends_with("_r") {
            e.length_m - p.1
        } else {
            p.1
        },
    )
}
fn physical(s: &LiveDriveSession) -> Vec<(String, (String, f64))> {
    s.vehicle_identities()
        .into_iter()
        .enumerate()
        .map(|(i, id)| {
            let offset = s.formation.center_offset_m(i);
            let p = s.presentation_position_for_car(offset, offset, 0.).unwrap();
            (id, normal(s, p))
        })
        .collect()
}
fn command(t: &mut LiveTraffic, p: &mut LiveDriveSession, c: YardCommand) {
    t.command_yard(p, c).unwrap();
}

#[test]
fn every_authored_yard_scenario_loads_real_stock_and_a_shared_world() {
    for name in [
        "scenario.toml",
        "scenario_turntable.toml",
        "scenario_transfer.toml",
        "scenario_meeting.toml",
        "scenario_pool.toml",
        "scenario_gravity.toml",
        "scenario_recovery.toml",
    ] {
        let s = definition(name);
        let (p, t) = sessions(&s);
        p.validate_snapshot(&p.snapshot()).unwrap();
        t.validate_snapshot(&t.snapshot()).unwrap();
        let player = p.snapshot();
        let others = t.snapshot();
        validate_shared_yard_snapshots(
            std::iter::once(&player).chain(others.iter().map(|s| &s.session)),
        )
        .unwrap();
    }
}
fn moving_device_acceptance(name: &str, id: &str) {
    let s = definition(name);
    let (mut p, mut t) = sessions(&s);
    let ids = p.vehicle_identities();
    let odometer = p.state.odometer_m;
    let fuel = p.state.diesel.cars.iter().map(|c| c.fuel_l).sum::<f64>();
    let initial = p.yard_device_poses()[0].clone();
    command(
        &mut t,
        &mut p,
        YardCommand::MoveDevice {
            device: id.into(),
            port: 1,
        },
    );
    assert!(p.yard_driving_blocked());
    let pause = serde_json::to_value(p.snapshot()).unwrap();
    t.advance(&mut p, 0., None, |_| {});
    assert_eq!(serde_json::to_value(p.snapshot()).unwrap(), pause);
    for _ in 0..100 {
        t.advance(&mut p, 0.05, None, |_| {});
    }
    let middle = p.yard_device_poses()[0].clone();
    assert!(middle.moving);
    assert!(p.yard_vehicle_pose(0).is_some());
    assert!(p.yard_driving_blocked());
    assert_eq!(p.vehicle_identities(), ids);
    assert_eq!(p.state.odometer_m, odometer);
    assert!(p.state.diesel.cars.iter().map(|c| c.fuel_l).sum::<f64>() <= fuel);
    let before = p.yard_vehicle_pose(0).unwrap().center_world;
    resume(&s, &mut p, &mut t);
    assert_eq!(p.yard_vehicle_pose(0).unwrap().center_world, before);
    for _ in 0..600 {
        t.advance(&mut p, 0.05, None, |_| {});
        if !p.yard_driving_blocked() {
            break;
        }
    }
    let final_pose = &p.yard_device_poses()[0];
    assert!(!final_pose.moving);
    assert_eq!(final_pose.aligned_port, 1);
    assert_ne!(
        initial.heading_rad + initial.center_world[2],
        final_pose.heading_rad + final_pose.center_world[2]
    );
    assert_eq!(p.state.path_edges[0], "deck1");
    assert_eq!(p.vehicle_identities(), ids);
    assert_eq!(p.state.odometer_m, odometer);
    assert!(p.yard_vehicle_pose(0).is_none());
    resume(&s, &mut p, &mut t);
    assert_eq!(p.state.path_edges[0], "deck1");
    assert!(!p.yard_driving_blocked());
}
#[test]
fn turntable_transports_physical_stock_and_resumes_mid_rotation() {
    moving_device_acceptance("scenario_turntable.toml", "turntable");
}
#[test]
fn transfer_table_transports_physical_stock_and_resumes_mid_translation() {
    moving_device_acceptance("scenario_transfer.toml", "transfer");
}
#[test]
fn moving_devices_reject_motion_partial_train_overload_and_corrupt_save_without_mutation() {
    let s = definition("scenario_turntable.toml");
    let (mut p, mut t) = sessions(&s);
    assert!(t.request_moving_device(&mut p, "turntable", 9).is_err());
    p.state.velocity_mps = 1.;
    let world = serde_json::to_value(&p.state.yard_operations).unwrap();
    assert!(t.request_moving_device(&mut p, "turntable", 1).is_err());
    assert_eq!(
        serde_json::to_value(&p.state.yard_operations).unwrap(),
        world
    );
    p.state.velocity_mps = 0.;
    let mut overload = s.clone();
    overload.simulation.yard_operations.moving_devices[0].max_mass_kg = 1000.;
    let (mut q, mut others) = sessions(&overload);
    assert!(
        others
            .request_moving_device(&mut q, "turntable", 1)
            .is_err()
    );
    let mut partial = s.clone();
    partial.route.start_offset_m = Some(10.);
    let (mut q, mut others) = sessions(&partial);
    assert!(
        others
            .request_moving_device(&mut q, "turntable", 1)
            .is_err()
    );
    command(
        &mut t,
        &mut p,
        YardCommand::MoveDevice {
            device: "turntable".into(),
            port: 1,
        },
    );
    let original = p.snapshot();
    let mut bad = original.clone();
    bad.state
        .yard_operations
        .devices
        .get_mut("turntable")
        .unwrap()
        .position = 9.;
    assert!(p.validate_snapshot(&bad).is_err());
    for variant in 0..3 {
        let mut bad = original.clone();
        let deck = bad
            .state
            .yard_operations
            .devices
            .get_mut("turntable")
            .unwrap();
        match variant {
            0 => deck.carrier.as_mut().unwrap().head_on_deck_m = 1.,
            1 => deck.carrier.as_mut().unwrap().source_port = 1,
            _ => deck.target_port = None,
        }
        assert!(
            p.validate_snapshot(&bad).is_err(),
            "invalid transported-stock invariant {variant}"
        );
    }
    let mut bad = original;
    bad.state
        .yard_operations
        .devices
        .get_mut("turntable")
        .unwrap()
        .carrier
        .as_mut()
        .unwrap()
        .vehicle_ids[0] = "another/car/0".into();
    assert!(p.validate_snapshot(&bad).is_err());
}
#[test]
fn pool_allocates_only_one_real_member_and_restores_its_claim() {
    let s = definition("scenario_pool.toml");
    let (mut p, mut t) = sessions(&s);
    let initial = t.services[0].session.vehicle_identities();
    assert_eq!(
        t.reserve_pool_material(&mut p, "depot", "first-duty")
            .unwrap(),
        "pool-loco"
    );
    assert!(
        t.reserve_pool_material(&mut p, "depot", "second-duty")
            .is_err()
    );
    resume(&s, &mut p, &mut t);
    assert_eq!(
        t.reserve_pool_material(&mut p, "depot", "first-duty")
            .unwrap(),
        "pool-loco"
    );
    assert_eq!(t.services[0].session.vehicle_identities(), initial);
    t.dispatch_pool_material(&mut p, "pool-loco").unwrap();
    assert!(t.dispatch_pool_material(&mut p, "pool-loco").is_err());
    let dispatched = serde_json::to_value(t.snapshot()).unwrap();
    assert!(t.return_pool_material(&mut p, "pool-loco").is_err());
    assert_eq!(serde_json::to_value(t.snapshot()).unwrap(), dispatched);
    assert_eq!(t.services.len(), 1);
    let mut bad = p.snapshot();
    bad.state
        .yard_operations
        .pool_members
        .get_mut("pool-loco")
        .unwrap()
        .reserved_job = Some("unpublished-job".into());
    assert!(p.validate_snapshot(&bad).is_err());
}
#[test]
fn pool_drives_out_and_back_returns_and_reuses_the_same_consumed_stock() {
    let s = definition("scenario_pool.toml");
    let (mut p, mut t) = sessions(&s);
    let original = t.services[0].session.vehicle_identities();
    let fuel = t.services[0]
        .session
        .state
        .diesel
        .cars
        .iter()
        .map(|c| c.fuel_l)
        .sum::<f64>();
    let mut previous_odometer = 0.;
    for duty in ["first-duty", "second-duty"] {
        t.reserve_pool_material(&mut p, "depot", duty).unwrap();
        t.dispatch_pool_material(&mut p, "pool-loco").unwrap();
        let mut saved = false;
        for _ in 0..40000 {
            t.advance(&mut p, 0.05, None, |_| {});
            if !saved && t.services[0].session.state.odometer_m > previous_odometer + 20. {
                resume(&s, &mut p, &mut t);
                saved = true;
            }
            if t.services[0].session.arrived {
                break;
            }
        }
        assert!(
            t.services[0].session.arrived,
            "{}",
            t.services[0]
                .session
                .operational_status()
                .unwrap_or_default()
        );
        assert!(saved);
        let odometer = t.services[0].session.state.odometer_m;
        assert!(odometer > previous_odometer + 400.);
        assert_eq!(t.services[0].session.vehicle_identities(), original);
        let pose = physical(&t.services[0].session);
        assert!(t.services[0].session.velocity_mps().abs() <= 0.005);
        t.return_pool_material(&mut p, "pool-loco").unwrap();
        assert_eq!(physical(&t.services[0].session), pose);
        assert_eq!(t.services[0].session.state.odometer_m, odometer);
        assert!(t.services[0].parked);
        assert!(
            t.services[0]
                .session
                .formation
                .cars
                .iter()
                .all(|c| c.handbrake)
        );
        assert!(t.reserve_pool_material(&mut p, "depot", duty).is_err());
        resume(&s, &mut p, &mut t);
        previous_odometer = odometer;
    }
    assert!(
        t.services[0]
            .session
            .state
            .diesel
            .cars
            .iter()
            .map(|c| c.fuel_l)
            .sum::<f64>()
            < fuel
    );
    assert_eq!(p.state.yard_operations.pool_members["pool-loco"].returns, 2);
}
#[test]
fn pool_and_actor_definition_errors_are_explicit() {
    let mut s = definition("scenario_pool.toml");
    let duplicate = s.simulation.yard_operations.pools[0].clone();
    let mut other = duplicate.clone();
    other.id = "another-pool".into();
    s.simulation.yard_operations.pools.push(other);
    assert!(s.validate().is_err());
    let mut s = definition("scenario_pool.toml");
    s.simulation.yard_operations.pools[0].members[0] = "absent".into();
    assert!(LiveTraffic::from_scenario(&directory(), &s).is_err());
    let mut s = definition("scenario_pool.toml");
    s.simulation.yard_operations.pools[0].jobs[0].not_before_s = 50.;
    let (mut p, mut t) = sessions(&s);
    t.reserve_pool_material(&mut p, "depot", "first-duty")
        .unwrap();
    assert!(
        t.dispatch_pool_material(&mut p, "pool-loco")
            .unwrap_err()
            .contains("autorizado")
    );
}
#[test]
fn alternative_classification_requires_explicit_exclusive_stock_destinations() {
    let mut s = definition("scenario_gravity.toml");
    s.simulation
        .rail_operations
        .yard_delivery_alternatives
        .clear();
    assert!(
        s.validate().is_err(),
        "unqualified double assignment must remain invalid"
    );
    let mut s = definition("scenario_gravity.toml");
    s.simulation.rail_operations.yard_delivery_alternatives[0].push("classification-a".into());
    assert!(
        s.validate().is_err(),
        "one destination cannot occur twice in a group"
    );
    let mut s = definition("scenario_gravity.toml");
    s.simulation.rail_operations.yard_deliveries[1].vehicles = vec![0];
    assert!(
        s.validate().is_err(),
        "alternatives must name exactly the same physical stock"
    );
}
#[test]
fn mobile_portion_drives_to_contact_transfers_real_identities_and_restores() {
    let s = definition("scenario_meeting.toml");
    let (mut p, mut t) = sessions(&s);
    let incoming = t
        .services
        .iter()
        .find(|a| a.id == "incoming")
        .unwrap()
        .session
        .vehicle_identities();
    let mut prior = 150.;
    let mut saved = false;
    for _ in 0..24000 {
        t.advance(&mut p, 0.05, None, |_| {});
        if let Some(actor) = t.services.iter().find(|a| a.id == "incoming") {
            let head = actor.session.head_chainage_m();
            assert!(head >= prior - 0.00001);
            assert!(head - prior < 0.2, "portion jumped by {} m", head - prior);
            prior = head;
            if head > 350. && !saved {
                resume(&s, &mut p, &mut t);
                saved = true;
            }
        } else {
            break;
        }
    }
    assert!(saved);
    assert_eq!(t.services.len(), 1);
    assert!(p.state.yard_operations.rendezvous["mobile-portion"].complete);
    assert_eq!(t.services[0].id, "receiver");
    assert!(
        incoming
            .iter()
            .all(|id| t.services[0].session.vehicle_identities().contains(id))
    );
    assert_eq!(t.couplings.len(), 1);
    resume(&s, &mut p, &mut t);
    assert_eq!(t.services.len(), 1);
    assert_eq!(t.couplings.len(), 1);
    assert!(p.state.yard_operations.rendezvous["mobile-portion"].complete);
}
#[test]
fn mobile_portion_timeout_and_incompatible_material_do_not_create_replacements() {
    let mut s = definition("scenario_meeting.toml");
    s.simulation.yard_operations.rendezvous[0].max_wait_s = 1.;
    let (mut p, mut t) = sessions(&s);
    for _ in 0..60 {
        t.advance(&mut p, 0.05, None, |_| {});
    }
    assert!(p.state.yard_operations.rendezvous["mobile-portion"].failed);
    assert_eq!(t.services.len(), 2);
    assert!(t.couplings.is_empty());
    let s = definition("scenario_meeting.toml");
    let (mut p, mut t) = sessions(&s);
    let i = t.services.iter().position(|a| a.id == "incoming").unwrap();
    t.services[i].parked = true;
    t.services[i].departed = true;
    let before = t.services[i].session.vehicle_identities();
    for _ in 0..40 {
        t.advance(&mut p, 0.05, None, |_| {});
    }
    assert!(!p.state.yard_operations.rendezvous["mobile-portion"].complete);
    assert!(
        p.state.yard_operations.rendezvous["mobile-portion"]
            .notice
            .contains("contacto")
    );
    assert_eq!(t.services[i].session.vehicle_identities(), before);
    assert!(t.couplings.is_empty());
}
#[test]
fn ai_runs_around_its_immobile_coaches_and_collects_from_the_other_end() {
    let s = definition("scenario.toml");
    let (mut p, mut t) = sessions(&s);
    let initial = physical(&t.services[0].session);
    t.start_ai_runaround(&mut p, "crew-loop").unwrap();
    let mut detached = false;
    let mut saw_loop = false;
    let mut saved = false;
    let mut rejoined = false;
    for _ in 0..48000 {
        t.advance(&mut p, 0.05, None, |_| {});
        let ai = &t.services[0].session;
        if !ai.detached_sections().is_empty() {
            detached = true;
            let current = physical(ai);
            for (id, position) in initial.iter().skip(1) {
                assert_eq!(
                    current.iter().find(|(other, _)| other == id).unwrap().1,
                    *position,
                    "parked coach moved during loop"
                );
            }
            saw_loop |=
                ai.state.path_edges.iter().any(|e| e == "bypass2") && ai.head_chainage_m() > 100.;
            if saw_loop && !saved {
                resume(&s, &mut p, &mut t);
                saved = true;
            }
        } else if detached {
            rejoined = true;
            assert_eq!(ai.formation.coupled_count, 3);
            assert_eq!(ai.vehicle_identities().last(), Some(&initial[0].0));
        }
        if p.state.yard_operations.run_arounds["crew-loop"].completed {
            break;
        }
    }
    assert!(
        detached && saw_loop && saved && rejoined,
        "{}; head={}; leg={}; path={:?}; count={}; sections={}; velocity={}; status={:?}",
        p.state.yard_operations.notice,
        t.services[0].session.head_chainage_m(),
        t.services[0].session.operational_leg_index(),
        t.services[0].session.state.path_edges,
        t.services[0].session.formation.coupled_count,
        t.services[0].session.detached_sections().len(),
        t.services[0].session.velocity_mps(),
        t.services[0].session.operational_status()
    );
    assert!(p.state.yard_operations.run_arounds["crew-loop"].completed);
    assert!(t.services[0].session.arrived);
    resume(&s, &mut p, &mut t);
    assert!(p.state.yard_operations.run_arounds["crew-loop"].completed);
}
#[test]
fn runaround_rejects_missing_cab_and_discontinuous_plan() {
    let mut s = definition("scenario.toml");
    s.simulation.yard_operations.run_arounds[0].keep = 3;
    assert!(LiveTraffic::from_scenario(&directory(), &s).is_err());
    let mut s = definition("scenario.toml");
    s.simulation.yard_operations.run_arounds[0].legs[1]
        .edges
        .swap(1, 2);
    assert!(LiveTraffic::from_scenario(&directory(), &s).is_err());
}
#[test]
fn gravity_classifies_both_declared_roads_using_grade_and_actual_retarder_work() {
    for route in 0..2 {
        let s = definition("scenario_gravity.toml");
        let (mut p, mut t) = sessions(&s);
        let positions = physical(&t.services[0].session);
        t.route_gravity_train(&mut p, "release-wagons", route)
            .unwrap();
        assert_eq!(physical(&t.services[0].session), positions);
        t.release_gravity_train(&mut p, "release-wagons").unwrap();
        assert!(
            t.route_gravity_train(&mut p, "release-wagons", 1 - route)
                .is_err()
        );
        let mut peak = 0.0_f64;
        let mut saved = false;
        let mut secured = false;
        for _ in 0..12000 {
            t.advance(&mut p, 0.05, None, |_| {});
            let actor = &t.services[0].session;
            peak = peak.max(actor.velocity_mps());
            if !saved && actor.head_chainage_m() > 350. {
                resume(&s, &mut p, &mut t);
                saved = true;
            }
            if t.services[0].session.head_chainage_m() > 380.
                && t.services[0].session.velocity_mps().abs() < 0.02
                && t.secure_gravity_train(&mut p, "release-wagons").is_ok()
            {
                secured = true;
                break;
            }
        }
        assert!(saved && secured, "{}", p.state.yard_operations.notice);
        assert!(peak > 3.);
        let road = if route == 0 { "road_a" } else { "road_b" };
        assert!(
            t.services[0]
                .session
                .own_track_occupancy()
                .iter()
                .all(|p| p.edge == road)
        );
        let retarder = if route == 0 {
            "road-a-retarder"
        } else {
            "road-b-retarder"
        };
        assert!(p.state.yard_operations.retarders[retarder].absorbed_j["gravity-wagons"] > 1000.);
        assert!(t.yard_inventory(&p).iter().any(|d| d.accepted
            && d.id
                == if route == 0 {
                    "classification-a"
                } else {
                    "classification-b"
                }));
        resume(&s, &mut p, &mut t);
        assert!(t.services[0].parked);
    }
}
#[test]
fn optional_runaway_requires_protection_and_real_stopping_before_recovery() {
    let s = definition("scenario_recovery.toml");
    let (mut p, mut t) = sessions(&s);
    assert!(t.secure_recovery(&mut p, "secure-on-slope").is_err());
    t.protect_recovery(&mut p, "secure-on-slope").unwrap();
    t.set_yard_retarder(&mut p, "crest-retarder", 1.).unwrap();
    let initial = physical(&t.services[0].session);
    t.release_gravity_train(&mut p, "release-wagons").unwrap();
    assert_eq!(physical(&t.services[0].session), initial);
    assert!(t.release_gravity_train(&mut p, "release-wagons").is_err());
    let mut recovered = false;
    let mut saved = false;
    for _ in 0..10000 {
        t.advance(&mut p, 0.05, None, |_| {});
        if !saved && t.services[0].session.velocity_mps() > 2. {
            resume(&s, &mut p, &mut t);
            saved = true;
        }
        if t.services[0].session.head_chainage_m() > 230.
            && t.services[0].session.velocity_mps() < 0.02
            && t.secure_recovery(&mut p, "secure-on-slope").is_ok()
        {
            recovered = true;
            break;
        }
    }
    assert!(saved && recovered, "{}", p.state.yard_operations.notice);
    assert!(
        t.services[0]
            .session
            .formation
            .cars
            .iter()
            .all(|c| c.handbrake)
    );
    assert!(
        p.state.yard_operations.recovery["secure-on-slope"]
            .recovered_at_s
            .is_some()
    );
    resume(&s, &mut p, &mut t);
    assert!(t.services[0].parked);
    assert!(t.release_gravity_train(&mut p, "release-wagons").is_err());
}
#[test]
fn recovery_rejects_late_protection_bad_retarder_and_moving_cuts() {
    let s = definition("scenario_recovery.toml");
    let (mut p, mut t) = sessions(&s);
    assert!(
        t.set_yard_retarder(&mut p, "crest-retarder", f64::NAN)
            .is_err()
    );
    assert!(t.set_yard_retarder(&mut p, "crest-retarder", 1.1).is_err());
    p.state.time.0 = 61.;
    t.protect_recovery(&mut p, "secure-on-slope").unwrap();
    assert!(
        p.state.yard_operations.recovery["secure-on-slope"]
            .failed
            .is_some()
    );
    assert!(t.secure_recovery(&mut p, "secure-on-slope").is_err());
    let actor = &mut t.services[0].session;
    actor.state.velocity_mps = 1.;
    assert!(actor.uncouple_after(0).is_err());
    assert_eq!(actor.formation.coupled_count, 2);
}
#[test]
fn collision_prescribed_speed_fixture_keeps_occupancy_records_impact_and_applies_emergency_brakes()
{
    let mut s = definition("scenario_gravity.toml");
    let mut obstacle:openrailsrs_scenarios::TrainEntryDef=toml::from_str("id='obstacle'\nconsist='consists/wagon.con'\nstart='crest'\ndestination='road_a_end'\nstart_offset_m=100.0\nparked=true\noutput_csv='obstacle.csv'").unwrap();
    obstacle.waypoints = vec!["crest".into(), "points".into(), "road_a_end".into()];
    s.extra_trains.push(obstacle);
    let (mut p, mut t) = sessions(&s);
    t.release_gravity_train(&mut p, "release-wagons").unwrap();
    let actor = &mut t.services[0].session;
    actor.state.velocity_mps = 2.;
    for body in &mut actor.state.vehicles {
        body.velocity_mps = 2.;
    }
    let mut collided = false;
    for _ in 0..120 {
        t.advance(&mut p, 0.05, None, |_| {});
        if !p.state.yard_operations.contacts.is_empty() {
            collided = true;
            break;
        }
    }
    assert!(collided);
    assert_eq!(t.services.len(), 2);
    let contact = &p.state.yard_operations.contacts[0];
    assert!(contact.impulse_ns > 0. && contact.absorbed_j > 0.);
    assert!(contact.unsafe_impact);
    assert!(
        t.services
            .iter()
            .all(|s| s.session.state.brake_system.emergency)
    );
    assert!(
        t.services
            .iter()
            .all(|s| !s.session.own_track_occupancy().is_empty())
    );
    resume(&s, &mut p, &mut t);
    assert!(!p.state.yard_operations.contacts.is_empty());
    // Continue through the braking reaction, not merely the first impact
    // notification. The contact solver may not repair overlap by translating
    // either formation; its smaller time quantum must bound penetration.
    let mut stopped = false;
    for tick in 0..240 {
        t.advance(&mut p, 0.05, None, |_| {});
        let left = t.services[0].session.own_track_occupancy();
        let right = t.services[1].session.own_track_occupancy();
        for a in &left {
            for b in right.iter().filter(|b| b.edge == a.edge) {
                let overlap = (a.end_m.min(b.end_m) - a.start_m.max(b.start_m)).max(0.);
                assert!(
                    overlap <= 0.005,
                    "contact penetration {overlap} m at tick {tick}"
                );
            }
        }
        if tick == 40 {
            resume(&s, &mut p, &mut t);
        }
        if tick > 40
            && t.services
                .iter()
                .all(|s| s.session.velocity_mps().abs() < 0.005)
        {
            stopped = true;
            break;
        }
    }
    assert!(
        stopped,
        "physical emergency braking must stop both interacting formations"
    );
    assert!(
        p.state
            .yard_operations
            .contacts
            .iter()
            .all(|c| c.absorbed_j.is_finite() && c.impulse_ns.is_finite())
    );
}
#[test]
fn inconsistent_shared_device_or_pool_world_is_rejected_atomically() {
    let s = definition("scenario_pool.toml");
    let (p, t) = sessions(&s);
    let before = serde_json::to_value(p.snapshot()).unwrap();
    let player = p.snapshot();
    let mut others = t.snapshot();
    others[0]
        .session
        .state
        .yard_operations
        .pool_members
        .get_mut("pool-loco")
        .unwrap()
        .reserved_job = Some("first-duty".into());
    assert!(
        validate_shared_yard_snapshots(
            std::iter::once(&player).chain(others.iter().map(|s| &s.session))
        )
        .is_err()
    );
    assert_eq!(serde_json::to_value(p.snapshot()).unwrap(), before);
}

#[test]
fn declared_retarder_adds_force_per_vehicle_and_dissipates_only_real_work() {
    let s = definition("scenario_gravity.toml");
    let (_, mut t) = sessions(&s);
    let actor = &mut t.services[0].session;
    // Controlled local retarder fixture: both wheelsets' nominal centres are
    // inside the declared crest zone, with prescribed pre/post velocities.
    actor.state.pos_on_edge_m = 260.;
    actor.state.velocity_mps = 4.;
    for v in &mut actor.state.vehicles {
        v.velocity_mps = 4.;
    }
    actor
        .state
        .yard_operations
        .retarders
        .get_mut("crest-retarder")
        .unwrap()
        .fraction = 0.5;
    let mut forces = vec![100.; 2];
    openrailsrs_sim::yard_gravity::apply_retarder_brakes(
        &mut actor.state,
        &actor.path_data,
        &actor.physics.vehicle_lengths_m,
        &mut forces,
    );
    assert_eq!(forces, vec![60100.; 2]);
    actor.state.odometer_m += 0.4;
    for v in &mut actor.state.vehicles {
        v.position_m += 0.4;
    }
    openrailsrs_sim::yard_gravity::account_retarder_work(&mut actor.state, 0.1);
    assert_eq!(
        actor.state.yard_operations.retarders["crest-retarder"].absorbed_j["gravity-wagons"],
        48000.
    );
    actor.state.velocity_mps = 0.;
    for v in &mut actor.state.vehicles {
        v.velocity_mps = 0.;
    }
    let mut forces = vec![0.; 2];
    openrailsrs_sim::yard_gravity::apply_retarder_brakes(
        &mut actor.state,
        &actor.path_data,
        &actor.physics.vehicle_lengths_m,
        &mut forces,
    );
    openrailsrs_sim::yard_gravity::account_retarder_work(&mut actor.state, 1.);
    assert_eq!(
        actor.state.yard_operations.retarders["crest-retarder"].absorbed_j["gravity-wagons"],
        48000.
    );
}
#[test]
fn unmatched_wayside_equipment_preserves_ordinary_vehicle_physics() {
    let mut s = definition("scenario_gravity.toml");
    for r in &mut s.simulation.yard_operations.retarders {
        r.default_fraction = 0.;
    }
    let (_, mut with) = sessions(&s);
    let mut baseline = s.clone();
    baseline.simulation.yard_operations = Default::default();
    let (_, mut without) = sessions(&baseline);
    for actor in [
        &mut with.services[0].session,
        &mut without.services[0].session,
    ] {
        actor.driver_throttle = 0.;
        actor.driver_brake = 0.;
        actor.driver_direction = 1.;
        for i in 0..actor.formation.coupled_count {
            if actor.formation.cars[i].handbrake {
                actor
                    .operate_car(i, openrailsrs_sim::CarOperation::Handbrake)
                    .unwrap();
            }
        }
    }
    for _ in 0..300 {
        with.services[0].session.step_realtime(0.05, |_| {});
        without.services[0].session.step_realtime(0.05, |_| {});
    }
    assert_eq!(
        with.services[0].session.state.velocity_mps,
        without.services[0].session.state.velocity_mps
    );
    assert_eq!(
        with.services[0].session.state.odometer_m,
        without.services[0].session.state.odometer_m
    );
}

#[test]
fn human_network_drivers_are_neither_consumed_nor_cut_by_automatic_yard_crews() {
    let s = definition("scenario.toml");
    let (mut p, mut t) = sessions(&s);
    t.set_remote_drivers(std::collections::HashSet::from(["yard-crew".into()]))
        .unwrap();
    assert!(t.start_ai_runaround(&mut p, "crew-loop").is_err());
    // A driven train already has a native detach row at its initial marker.
    // Remote step_realtime must not execute that crew operation implicitly.
    t.services[0].parked = false;
    for _ in 0..20 {
        t.advance(&mut p, 0.05, None, |_| {});
    }
    assert_eq!(t.services[0].session.formation.coupled_count, 3);
    assert!(t.services[0].session.detached_sections().is_empty());
    let s = definition("scenario_meeting.toml");
    let (mut p, mut t) = sessions(&s);
    t.set_remote_drivers(std::collections::HashSet::from(["incoming".into()]))
        .unwrap();
    let incoming = t.services.iter_mut().find(|s| s.id == "incoming").unwrap();
    incoming.session.state.pos_on_edge_m = 430.;
    incoming.session.driver_throttle = 0.;
    incoming.session.driver_brake = 1.;
    incoming.departed = true;
    for _ in 0..40 {
        t.advance(&mut p, 0.05, None, |_| {});
    }
    assert_eq!(t.services.len(), 2);
    assert!(t.couplings.is_empty());
    assert!(!p.state.yard_operations.rendezvous["mobile-portion"].complete);
}
