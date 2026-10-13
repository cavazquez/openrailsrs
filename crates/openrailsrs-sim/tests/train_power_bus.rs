use openrailsrs_sim::{CarOperation, LiveDriveSession};
fn session() -> LiveDriveSession {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation");
    let s = openrailsrs_scenarios::load_scenario(dir.join("scenario_hotel.toml")).unwrap();
    LiveDriveSession::from_scenario(&dir, &s).unwrap()
}
#[test]
fn passenger_loads_follow_their_connected_supplier_and_reconnect_after_save() {
    let mut s = session();
    s.step_realtime(2., |_| {});
    assert!(s.state.power_supply.vehicle_supplied(1));
    let before = s.state.power_supply.hotel_w;
    assert!(before > 0.);
    s.operate_car(1, CarOperation::TrainBus).unwrap();
    assert!(!s.state.power_supply.vehicle_supplied(1));
    s.step_realtime(0.5, |_| {});
    assert!(s.state.power_supply.hotel_w < before);
    let mut restored = session();
    restored.restore_snapshot(s.snapshot()).unwrap();
    assert!(!restored.state.power_supply.vehicle_supplied(1));
    s.operate_car(1, CarOperation::TrainBus).unwrap();
    restored.operate_car(1, CarOperation::TrainBus).unwrap();
    s.step_realtime(1., |_| {});
    restored.step_realtime(1., |_| {});
    assert_eq!(s.state.power_supply, restored.state.power_supply);
    assert!(s.state.power_supply.vehicle_supplied(1));
    assert_eq!(s.state.power_supply.hotel_w, before);
    let mut invalid = s.snapshot();
    invalid.state.power_supply.bus_source = vec![Some(999)];
    assert!(s.restore_snapshot(invalid).is_err());
}
