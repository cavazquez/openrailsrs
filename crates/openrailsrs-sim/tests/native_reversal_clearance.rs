//! Prescribed geometry regression; the installed MidEast QA drives its loop.
use openrailsrs_scenarios::model::RouteLeg;
use openrailsrs_sim::{LiveDriveSession, ServicePhase};
fn setup() -> (tempfile::TempDir, openrailsrs_scenarios::ScenarioFile) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/portion_operations")
        .canonicalize()
        .unwrap();
    let mut scenario = openrailsrs_scenarios::load_scenario(base.join("scenario.toml")).unwrap();
    let out = tempfile::tempdir().unwrap();
    let track = "[route]\nid='reversal-clearance'\n[[nodes]]\nid='a'\nx_m=0.0\ny_m=0.0\n[[nodes]]\nid='points'\nx_m=100.0\ny_m=0.0\n[[nodes]]\nid='buffer'\nx_m=132.2331\ny_m=0.0\n[[nodes]]\nid='loop-end'\nx_m=0.0\ny_m=10.0\n[[edges]]\nid='approach'\nfrom='a'\nto='points'\nlength_m=100.0\nspeed_limit_kmh=20.0\n[[edges]]\nid='headshunt'\nfrom='points'\nto='buffer'\nlength_m=32.2331\nspeed_limit_kmh=15.0\n[[edges]]\nid='headshunt_r'\nfrom='buffer'\nto='points'\nlength_m=32.2331\nspeed_limit_kmh=15.0\n[[edges]]\nid='loop'\nfrom='points'\nto='loop-end'\nlength_m=200.0\nspeed_limit_kmh=15.0\n";
    std::fs::write(out.path().join("track.toml"), track).unwrap();
    scenario.route.path = out.path().to_string_lossy().into_owned();
    scenario.route.start = "a".into();
    scenario.route.destination = "buffer".into();
    scenario.route.start_offset_m = Some(105.);
    let leg = |start: &str, end: &str, edges: &[&str], offset, finish, reversed| RouteLeg {
        start: start.into(),
        destination: end.into(),
        edges: edges.iter().map(|s| s.to_string()).collect(),
        start_offset_m: offset,
        end_chainage_m: finish,
        reversed,
        wait_s: 0.,
        stops: vec![],
        switches: vec![],
        operations: vec![],
        service_id: None,
        not_before_s: None,
    };
    scenario.route.legs = vec![
        leg("a", "buffer", &["approach", "headshunt"], 105., 116., false),
        leg(
            "buffer",
            "loop-end",
            &["headshunt_r", "loop"],
            27.2331,
            220.,
            true,
        ),
    ];
    scenario.train.consist = base
        .join("consists/five.con")
        .to_string_lossy()
        .into_owned();
    (out, scenario)
}
#[test]
fn an_uncleared_reversal_stays_pending_and_save_does_not_relocate_stock() {
    let (out, scenario) = setup();
    let mut s = LiveDriveSession::from_scenario(out.path(), &scenario).unwrap();
    for car in &mut s.formation.cars[1..] {
        car.handbrake = true;
    }
    s.uncouple_after_named(0, "Mk1").unwrap();
    s.driver_direction = 0.;
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    // Only 8 m of the 20 m locomotive have entered the headshunt.
    s.state.pos_on_edge_m = 8.;
    s.step_realtime(0.05, |_| {});
    assert_eq!(s.operational_leg_index(), 0);
    assert_ne!(s.gameplay.phase, ServicePhase::Failed);
    assert!(s.operational_status().unwrap().contains("librar"));
    let parked = s.detached_sections()[0].clone();
    let saved = s.snapshot();
    let mut resumed = LiveDriveSession::from_scenario(out.path(), &scenario).unwrap();
    resumed.restore_snapshot(saved).unwrap();
    // Prescribed next position tests alignment; the MidEast acceptance uses
    // actual regulator/brake integration to traverse these extra metres.
    resumed.state.pos_on_edge_m = 25.;
    resumed.step_realtime(0.05, |_| {});
    assert_eq!(resumed.operational_leg_index(), 1);
    assert_ne!(resumed.gameplay.phase, ServicePhase::Failed);
    assert_eq!(resumed.detached_sections()[0].front, parked.front);
    assert_eq!(resumed.detached_sections()[0].rear, parked.rear);
    resumed.validate_snapshot(&resumed.snapshot()).unwrap();
}
