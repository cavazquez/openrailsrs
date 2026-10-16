//! The native cycle is exercised through the playable train and transactional
//! snapshot API, with no reference rows supplying its dynamic state.
use openrailsrs_sim::{LiveDriveSession, steam::SteamCommand};
use std::path::PathBuf;

fn session() -> LiveDriveSession {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/physics_lab");
    let scenario =
        openrailsrs_scenarios::load_scenario(root.join("scenario_native_steam.toml")).unwrap();
    LiveDriveSession::from_scenario(&root, &scenario).unwrap()
}
fn drive(s: &mut LiveDriveSession, steps: usize) {
    s.driver_direction = 1.;
    s.driver_throttle = 0.65;
    s.driver_brake = 0.;
    for _ in 0..steps {
        s.step_realtime(0.05, |_| {});
    }
}
#[test]
fn native_cycle_and_vacuum_brake_resume_with_the_same_future() {
    let mut a = session();
    assert_eq!(a.driver_brake, 0.65_f32 as f64);
    a.steam_command(SteamCommand::Cutoff(0.65)).unwrap();
    a.steam_command(SteamCommand::Firing(0.7)).unwrap();
    drive(&mut a, 400);
    a.steam_command(SteamCommand::Injector1).unwrap();
    drive(&mut a, 20);
    assert!(a.velocity_mps() > 0.1);
    assert!(a.state.steam_boilers[0].valid());
    assert_eq!(
        openrailsrs_sim::consumable_mass::total_mass_kg(&a.physics, &a.state),
        a.physics.mass_kg,
        "A native tank without continuous freight loading keeps its authored mechanical mass"
    );
    let encoded = serde_json::to_vec(&a.snapshot()).unwrap();
    let mut b = session();
    b.restore_snapshot(serde_json::from_slice(&encoded).unwrap())
        .unwrap();
    assert_eq!(a.state.steam_boilers, b.state.steam_boilers);
    drive(&mut a, 200);
    drive(&mut b, 200);
    assert_eq!(a.state.steam_boilers, b.state.steam_boilers);
    assert_eq!(a.state.velocity_mps, b.state.velocity_mps);
    assert_eq!(a.state.odometer_m, b.state.odometer_m);
    assert_eq!(
        serde_json::to_value(&a.state.brake_system).unwrap(),
        serde_json::to_value(&b.state.brake_system).unwrap()
    );
}
#[test]
fn native_snapshot_rejects_missing_state_changed_hardware_and_inconsistent_mirrors() {
    let mut s = session();
    let good = s.snapshot();
    let mut missing = good.clone();
    missing.state.steam_boilers.clear();
    assert!(s.restore_snapshot(missing).is_err());
    let mut missing = good.clone();
    missing.state.steam_boilers[0].boiler.native_or161 = None;
    assert!(s.restore_snapshot(missing).is_err());
    let mut changed = good.clone();
    changed.state.steam_boilers[0]
        .boiler
        .native_or161
        .as_mut()
        .unwrap()
        .hardware
        .source
        .maximum_pressure_psi += 1.;
    assert!(s.restore_snapshot(changed).is_err());
    let mut changed = good.clone();
    changed.state.boiler_state.as_mut().unwrap().tender_water_kg -= 1.;
    assert!(s.restore_snapshot(changed).is_err());
    s.restore_snapshot(good).unwrap();
    let b = s.state.boiler_state.as_mut().unwrap();
    b.command(SteamCommand::AutomaticFireman);
    b.command(SteamCommand::CompoundStartingValve);
    assert!(!b.controls.automatic_fireman && !b.controls.compound_bypass);
}
#[test]
fn native_formation_requires_its_supported_vacuum_equipment_at_the_head() {
    let mut s = session();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/physics_lab");
    let original = openrailsrs_train::load_consist_with_asset_root(
        root.join("consists/native_steam.con"),
        &root,
    )
    .unwrap();
    s.state.brake_system = Default::default();
    assert!(openrailsrs_sim::steam_equipment::configure(&mut s.state, &original).is_err());
    let mut mixed = original.clone();
    mixed.vehicles.push(original.vehicles[0].clone());
    assert!(openrailsrs_sim::steam_equipment::configure(&mut s.state, &mixed).is_err());
}
