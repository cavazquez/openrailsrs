use openrailsrs_sim::LiveDriveSession;
fn session() -> LiveDriveSession {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/electric_supply");
    let s =
        openrailsrs_scenarios::load_scenario(dir.join("scenario_two_pantographs.toml")).unwrap();
    LiveDriveSession::from_scenario(&dir, &s).unwrap()
}
#[test]
fn two_collectors_keep_their_own_position_contact_and_command_across_a_neutral_section() {
    let mut s = session();
    assert_eq!(s.state.electric.cars[0].pantographs.len(), 2);
    // Vehicle centre at the first boundary: its collectors occupy different supplies.
    s.state.pos_on_edge_m = 160.;
    s.toggle_collector(0, 1).unwrap();
    for _ in 0..45 {
        openrailsrs_sim::electric::advance(&mut s.state, &s.path_data, &s.physics.electric, 0.05);
    }
    let p = &s.state.electric.cars[0].pantographs;
    assert_eq!(p[0].contact_voltage_v, 0.);
    assert_eq!(p[1].contact_voltage_v, 25000.);
    assert_eq!(p[0].fraction, 1.);
    assert_eq!(p[1].fraction, 1.);
    let p0 = p[0].clone();
    s.toggle_collector(0, 1).unwrap();
    openrailsrs_sim::electric::advance(&mut s.state, &s.path_data, &s.physics.electric, 0.05);
    assert_eq!(s.state.electric.cars[0].pantographs[0], p0);
    assert_eq!(s.state.electric.cars[0].contact_voltage_v, 0.);
    assert!(!s.state.electric.cars[0].main_power);
}
#[test]
fn midway_raise_and_filtered_voltage_restore_without_changing_the_other_collector() {
    let mut s = session();
    s.toggle_collector(0, 1).unwrap();
    s.step_realtime(0.7, |_| {});
    let fraction = s.state.electric.cars[0].pantographs[1].fraction;
    assert!(fraction > 0.2 && fraction < 0.8);
    let mut restored = session();
    restored.restore_snapshot(s.snapshot()).unwrap();
    for _ in 0..40 {
        s.step_realtime(0.05, |_| {});
        restored.step_realtime(0.05, |_| {});
    }
    assert_eq!(s.state.electric.cars, restored.state.electric.cars);
    assert!(s.toggle_collector(0, 4).is_err());
}
