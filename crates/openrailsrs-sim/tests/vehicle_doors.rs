use openrailsrs_scenarios::vehicle_systems::{
    DoorSide, DoorsDef, PlatformSideDef, VehicleSystemsDef,
};
use openrailsrs_sim::LiveDriveSession;
fn session() -> LiveDriveSession {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/player_lab");
    let mut scenario = openrailsrs_scenarios::load_scenario(dir.join("scenario.toml")).unwrap();
    scenario.train.systems = (0..2)
        .map(|vehicle| VehicleSystemsDef {
            vehicle,
            doors: Some(DoorsDef {
                left: true,
                right: true,
                open_s: 2.,
                close_s: 3.,
                traction_interlock: true,
            }),
            ..Default::default()
        })
        .collect();
    LiveDriveSession::from_scenario(&dir, &scenario).unwrap()
}
#[test]
fn independent_vehicle_and_sides_have_real_interlocks_and_survive_midstroke_save() {
    let mut s = session();
    s.command_doors(Some(1), DoorSide::Left, true).unwrap();
    s.step_realtime(1., |_| {});
    assert_eq!(s.state.vehicle_systems[0].door_fraction, [0., 0.]);
    assert!(s.state.vehicle_systems[1].door_fraction[0] > 0.4);
    assert_eq!(s.state.vehicle_systems[1].door_fraction[1], 0.);
    assert!(!openrailsrs_sim::vehicle_systems::doors_allow_traction(
        &s.state, 2
    ));
    let saved = s.snapshot();
    let mut restored = session();
    restored.restore_snapshot(saved).unwrap();
    s.step_realtime(1.5, |_| {});
    restored.step_realtime(1.5, |_| {});
    assert_eq!(s.state.vehicle_systems, restored.state.vehicle_systems);
    s.command_doors(None, DoorSide::Both, false).unwrap();
    s.step_realtime(3.5, |_| {});
    assert!(openrailsrs_sim::vehicle_systems::doors_allow_traction(
        &s.state, 2
    ));
}
#[test]
fn platform_side_motion_and_config_changes_fail_without_opening_the_wrong_door() {
    let mut s = session();
    s.state.platform_sides = vec![PlatformSideDef {
        stop: 0,
        side: DoorSide::Left,
    }];
    s.gameplay
        .stop_targets
        .push(openrailsrs_sim::LiveStopTarget {
            node_id: "yard_a".into(),
            cum_dist_m: s.head_chainage_m(),
            arrive_s: 0.,
            depart_s: 60.,
            dwell_s: 5.,
            name: "Andén de prueba".into(),
            is_terminal: false,
            passengers_on: 0,
            passengers_off: 0,
        });
    let target = s.gameplay.stop_targets[0].cum_dist_m;
    // Align the synthetic stop with the current train, retaining its identity.
    s.gameplay.stop_targets[0].cum_dist_m = s.head_chainage_m();
    assert!(s.command_doors(None, DoorSide::Right, true).is_err());
    assert!(s.command_doors(None, DoorSide::Left, true).is_ok());
    s.state.velocity_mps = 1.;
    assert!(s.command_doors(None, DoorSide::Left, true).is_err());
    s.command_doors(None, DoorSide::Both, false).unwrap();
    s.state.velocity_mps = 0.;
    s.gameplay.stop_targets[0].cum_dist_m = target;
    let mut bad = s.snapshot();
    bad.state.vehicle_systems[0]
        .definition
        .doors
        .as_mut()
        .unwrap()
        .open_s = 5.;
    assert!(s.restore_snapshot(bad).is_err());
}
#[test]
fn legacy_coarse_doors_upgrade_to_declared_state_and_empty_equipment_does_not_change_physics() {
    let mut s = session();
    let mut old = s.snapshot();
    old.state.vehicle_systems.clear();
    old.exterior.set_door(openrailsrs_sim::DoorState::Open);
    s.restore_snapshot(old).unwrap();
    assert_eq!(
        s.equipped_door_state(),
        Some(openrailsrs_sim::DoorState::Open)
    );
    assert!(!openrailsrs_sim::vehicle_systems::doors_allow_traction(
        &s.state, 2
    ));
}
