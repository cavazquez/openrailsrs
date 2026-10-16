use openrailsrs_sim::LiveDriveSession;
fn session(code: f64) -> LiveDriveSession {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/activity_operations/scenario_pat.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&file).unwrap();
    scenario.route.legs[0].wait_s = code;
    LiveDriveSession::from_scenario(file.parent().unwrap(), &scenario).unwrap()
}
#[test]
fn native_pat_detach_executes_once_and_resumes_its_actual_remaining_wait() {
    let mut s = session(40102.);
    s.set_direction(1.).unwrap();
    s.step_realtime(1., |_| {});
    assert_eq!(s.formation.coupled_count, 1);
    assert_eq!(s.detached_sections().len(), 1);
    assert!(s.state.pat_operations_completed.contains(&0));
    assert!(!s.arrived);
    let parked = s.detached_sections()[0].front.clone();
    let saved = s.snapshot();
    let mut resumed = session(40102.);
    resumed.restore_snapshot(saved).unwrap();
    for live in [&mut s, &mut resumed] {
        live.step_realtime(2., |_| {});
    }
    assert_eq!(s.detached_sections().len(), 1);
    assert_eq!(s.detached_sections()[0].front, parked);
    assert_eq!(s.head_chainage_m(), resumed.head_chainage_m());
    assert_eq!(s.snapshot().operational_leg, 1);
    assert_eq!(
        s.snapshot().operational_leg,
        resumed.snapshot().operational_leg
    );
}
#[test]
fn unsupported_pat_codes_never_turn_into_time_or_a_completed_manoeuvre() {
    for code in [60003., 60001., 60002., 50320., 32400.] {
        let mut s = session(code);
        s.set_direction(1.).unwrap();
        s.step_realtime(3., |_| {});
        assert_eq!(s.formation.coupled_count, 4);
        assert!(s.state.pat_operations_completed.is_empty());
        assert_eq!(s.snapshot().operational_leg, 0);
        assert!(!s.arrived);
        assert!(s.operational_status().unwrap().contains("no admitida"));
        let mut resumed = session(code);
        resumed.restore_snapshot(s.snapshot()).unwrap();
        resumed.step_realtime(1., |_| {});
        assert_eq!(resumed.snapshot().operational_leg, 0);
    }
}
