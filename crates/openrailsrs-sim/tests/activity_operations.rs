use openrailsrs_core::activity::{
    ActivityCondition, ActivityDefinition, ActivityEvent, ActivityOutcomes,
};
use openrailsrs_sim::{LiveDriveSession, refill::RefillStation};
fn session() -> LiveDriveSession {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/activity_operations/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&file).unwrap();
    LiveDriveSession::from_scenario(file.parent().unwrap(), &scenario).unwrap()
}
fn station(id: &str) -> RefillStation {
    RefillStation {
        id: id.into(),
        pickup_type: 2,
        capacity_kg: 10000.,
        feed_kg_s: 100.,
        opening_time_s: 1.,
        speed_range_mps: [0., 0.],
    }
}
#[test]
fn actual_payload_has_finite_mass_delay_cancel_and_resume() {
    let mut s = session();
    let mass = s.effective_mass_kg();
    s.begin_refill(station("coal"), 1, 0., 6.).unwrap();
    s.step_realtime(11., |_| {});
    assert!((s.cargo_load_kg(1).unwrap().0 - 1000.).abs() < 0.001);
    let fuel_burned_kg = s
        .state
        .diesel
        .cars
        .iter()
        .map(|c| c.consumed_l * openrailsrs_sim::diesel_operation::DIESEL_KG_PER_L)
        .sum::<f64>();
    assert!((s.effective_mass_kg() - mass - 1000. + fuel_burned_kg).abs() < 0.001);
    assert_eq!(s.cargo_load_kg(2).unwrap().0, 0.);
    let saved = s.snapshot();
    let json = serde_json::to_vec(&saved).unwrap();
    let mut restored = session();
    restored
        .restore_snapshot(serde_json::from_slice(&json).unwrap())
        .unwrap();
    for live in [&mut s, &mut restored] {
        live.step_realtime(30., |_| {});
        live.cancel_refill();
        live.step_realtime(2., |_| {});
    }
    assert_eq!(s.state.freight, restored.state.freight);
    assert_eq!(s.cargo_load_kg(1).unwrap().0, 2400.);
    s.begin_unload(station("unloader"), 1, 0., 6.).unwrap();
    s.step_realtime(6., |_| {});
    assert_eq!(s.cargo_load_kg(1).unwrap().0, 2400.);
    s.step_realtime(10., |_| {});
    assert!((s.cargo_load_kg(1).unwrap().0 - 1400.).abs() < 0.001);
    let before = s.cargo_load_kg(1);
    s.cancel_refill();
    s.step_realtime(3., |_| {});
    assert_eq!(s.cargo_load_kg(1), before);
    let mut forged = saved;
    forged.state.freight[0].profile.capacity_kg = 99999.;
    assert!(session().restore_snapshot(forged).is_err());
}
#[test]
fn native_events_preserve_activation_uids_and_do_not_succeed_at_pat_end() {
    let mut s = session();
    s.step_realtime(0.05, |_| {});
    assert_eq!(
        s.state.activity.as_ref().unwrap().progress[&1].times_triggered,
        1
    );
    assert!(!s.state.activity.as_ref().unwrap().successful);
    let mut broken = s.snapshot();
    broken.formation.cars[2].activity_uid = Some(555);
    assert!(session().restore_snapshot(broken).is_err());
    // AllStops with no station tasks must remain false, including at the end.
    let definition = ActivityDefinition {
        vehicle_uids: vec![0, 1, 2, 3],
        events: vec![ActivityEvent {
            id: 7,
            name: "Empty AllStops".into(),
            activation: 1,
            reversible: false,
            condition: ActivityCondition::AllStops,
            outcomes: ActivityOutcomes {
                success: true,
                ..Default::default()
            },
        }],
        ..Default::default()
    };
    s.state.activity = Some(openrailsrs_sim::activity::ActivityRuntime::new(definition));
    s.state.pos_on_edge_m = 499.9;
    s.state.edge_index = 1;
    s.state.velocity_mps = 5.;
    s.step_realtime(1., |_| {});
    assert!(!s.arrived);
    assert!(!s.state.activity.as_ref().unwrap().successful);
    assert_eq!(
        s.state.activity.as_ref().unwrap().progress[&7].times_triggered,
        0
    );
}

#[test]
fn declared_native_success_is_terminal_and_survives_resume() {
    let mut live = session();
    live.step_realtime(301., |_| {});
    assert!(live.state.activity.as_ref().unwrap().successful);
    assert!(live.arrived);
    assert_eq!(
        live.gameplay.phase,
        openrailsrs_sim::ServicePhase::Completed
    );
    let mut resumed = session();
    resumed.restore_snapshot(live.snapshot()).unwrap();
    resumed.step_realtime(1., |_| {});
    assert!(resumed.state.activity.as_ref().unwrap().successful);
    assert!(resumed.arrived);
    assert_eq!(
        resumed.gameplay.phase,
        openrailsrs_sim::ServicePhase::Completed
    );
}

fn loads_by_uid(s: &LiveDriveSession) -> std::collections::BTreeMap<u32, f64> {
    s.state
        .freight
        .iter()
        .map(|c| (s.formation.cars[c.vehicle].activity_uid.unwrap(), c.load_kg))
        .collect()
}

#[test]
fn loaded_wagon_uid_and_quantity_survive_section_permutation_and_resume() {
    use openrailsrs_sim::{CarOperation, sections::CouplingEnd};
    let mut live = session();
    live.begin_refill(station("coal"), 1, 0., 6.).unwrap();
    live.step_realtime(11., |_| {});
    live.cancel_refill();
    live.step_realtime(2., |_| {});
    let quantities = loads_by_uid(&live);
    assert!((quantities[&1] - 1000.).abs() < 0.001);
    for index in 1..4 {
        live.operate_car(index, CarOperation::Handbrake).unwrap();
    }
    live.uncouple_after_named(2, "B").unwrap();
    live.uncouple_after_named(0, "C").unwrap();
    // Prescribed contact after a run-around; journey geometry is covered by
    // sections_and_services. This case checks the cargo/ACT identity mapping.
    let contact = live
        .detached_sections()
        .into_iter()
        .find(|s| s.id == "B")
        .unwrap()
        .rear
        .unwrap();
    assert_eq!(contact.0, "e1");
    live.state.pos_on_edge_m = contact.1;
    live.recouple_section("B", CouplingEnd::Front).unwrap();
    let saved = live.snapshot();
    let mut resumed = session();
    resumed
        .restore_snapshot(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap())
        .unwrap();
    resumed.recouple_section("C", CouplingEnd::Front).unwrap();
    assert_eq!(
        resumed
            .formation
            .cars
            .iter()
            .map(|c| c.activity_uid.unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3, 0]
    );
    assert_eq!(loads_by_uid(&resumed), quantities);
    assert_eq!(resumed.cargo_load_kg(0).unwrap().0, quantities[&1]);
    resumed.validate_snapshot(&resumed.snapshot()).unwrap();
}

#[test]
fn returned_act_actor_keeps_loaded_uid_and_its_own_objectives() {
    use openrailsrs_sim::LiveTraffic;
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/activity_operations/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&file).unwrap();
    scenario.route.start_offset_m = Some(200.);
    let head = 200. - session().formation.length_m();
    let mut entry: openrailsrs_scenarios::TrainEntryDef = toml::from_str("id = 'ACT-Cargo'\nconsist = 'consists/cargo.con'\nstart = 'a'\ndestination = 'b'\noutput_csv = 'actor.csv'\nparked = true\nactivity_uids = [2147483648,2147483649,2147483650,2147483651]\n").unwrap();
    entry.start_offset_m = head;
    scenario.extra_trains.push(entry);
    let directory = file.parent().unwrap();
    let mut player = LiveDriveSession::from_scenario(directory, &scenario).unwrap();
    let mut traffic = LiveTraffic::from_scenario(directory, &scenario).unwrap();
    let donor = &mut traffic.services[0].session;
    donor
        .begin_refill(station("actor-coal"), 1, 0., 6.)
        .unwrap();
    donor.step_realtime(6., |_| {});
    donor.cancel_refill();
    donor.step_realtime(2., |_| {});
    let quantities = loads_by_uid(donor);
    assert!((quantities[&2147483649] - 500.).abs() < 0.001);
    traffic.couple_service(&mut player, 0).unwrap();
    assert_eq!(loads_by_uid(&player)[&2147483649], quantities[&2147483649]);
    traffic.split_service(&mut player, "ACT-Cargo").unwrap();
    let returned = &traffic.services[0].session;
    assert_eq!(loads_by_uid(returned), quantities);
    assert!(
        returned
            .state
            .activity
            .as_ref()
            .unwrap()
            .definition
            .events
            .is_empty()
    );
    assert!(
        !player
            .state
            .activity
            .as_ref()
            .unwrap()
            .definition
            .events
            .is_empty()
    );
    let saved_player = player.snapshot();
    let saved_traffic = traffic.snapshot();
    let receipts = traffic.couplings.clone();
    let mut fresh_player = LiveDriveSession::from_scenario(directory, &scenario).unwrap();
    let mut fresh_traffic = LiveTraffic::from_scenario(directory, &scenario).unwrap();
    fresh_traffic
        .reconstruct_couplings(&mut fresh_player, &receipts)
        .unwrap();
    fresh_traffic
        .restore_shared_snapshots(&mut fresh_player, saved_player, saved_traffic)
        .unwrap();
    assert_eq!(loads_by_uid(&fresh_traffic.services[0].session), quantities);
}
