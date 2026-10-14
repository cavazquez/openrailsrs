//! Resource readiness must agree with runtime acceptance of native equipment.
use openrailsrs_scenarios::{ScenarioFile, load_scenario};
use openrailsrs_sim::LiveDriveSession;
use std::path::{Path, PathBuf};

fn native_profile() -> (tempfile::TempDir, ScenarioFile) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("consists")).unwrap();
    // Representative fields from an original TGV profile, expressed in a small
    // test vehicle. OR 1.6.1 MSTSLocomotive uses SpeedDefaultMPH and does not cap
    // the terminal fade knot at 400 m/s; Pantograph.Parse accepts long delays.
    std::fs::write(
        root.path().join("native.eng"),
        r#"Engine (
          Name ( "Native equipment acceptance test" ) Type ( Electric )
          Mass ( 40000 ) Size ( 3 4 20 )
          MaxPower ( 1000000 ) MaxForce ( 100000 ) MaxVelocity ( 120 )
          MaxBrakeForce ( 120000 )
          DynamicBrakesMaximumForce ( 150kN )
          DynamicBrakesMinUsableSpeed ( 12 )
          DynamicBrakesFadingSpeed ( 20 )
          DynamicBrakesMaximumEffectiveSpeed ( 25 )
          DynamicBrakesMaximumSpeedForFadeOut ( 1800 )
          DynamicBrakesEffectAtMaximumFadeOut ( 0 )
          DynamicBrakesResistorCurrentLimit ( 1040 )
          DynamicBrakesDelayTimeBeforeEngaging ( 0.5 )
          ORTSPantographs (
            Pantograph ( Delay ( 0s ) )
            Pantograph ( Delay ( 999999s ) )
            Pantograph ( Delay ( 0s ) )
            Pantograph ( Delay ( 0s ) )
          )
        )"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("consists/native.con"),
        "Train ( Engine ( native.eng ) )",
    )
    .unwrap();
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/electric_supply");
    let mut scenario = load_scenario(directory.join("scenario_two_pantographs.toml")).unwrap();
    scenario.route.path = directory.join("route").to_string_lossy().into_owned();
    scenario.route.start_offset_m = Some(100.);
    scenario.route.stops.clear();
    scenario
        .route
        .electric_supply
        .as_mut()
        .unwrap()
        .sections
        .clear();
    scenario.train.consist = root
        .path()
        .join("consists/native.con")
        .to_string_lossy()
        .into_owned();
    (root, scenario)
}

fn load(root: &Path, scenario: &ScenarioFile) -> LiveDriveSession {
    LiveDriveSession::from_scenario(root, scenario).expect("native equipment must reach LiveDrive")
}

#[test]
fn native_terminal_fade_speed_and_disabled_pickup_survive_runtime_and_resume() {
    let (root, scenario) = native_profile();
    let mut session = load(root.path(), &scenario);
    let car = &session.state.vehicle_systems[0];
    assert!(car.valid());
    let brake = car.definition.electric_brake.as_ref().unwrap();
    assert!(brake.native_response);
    assert_eq!(brake.force_curve.len(), 7);
    assert_eq!(brake.force_curve[5], [1800. * 0.44704, 0.]);
    assert_eq!(brake.force_curve[6], [1800. * 0.44704 + 0.5, 0.]);
    assert_eq!(brake.maximum_current_a, Some(1040.));
    assert_eq!(brake.apply_delay_s, 0.5);
    assert_eq!(
        session.physics.electric.cars[0]
            .params
            .pantographs
            .iter()
            .map(|p| p.delay_s)
            .collect::<Vec<_>>(),
        vec![0., 999999., 0., 0.]
    );
    session.driver_direction = 1.;
    session.driver_brake = 0.;
    session.driver_throttle = 0.5;
    session.step_realtime(1., |_| {});
    assert!(session.state.vehicle_systems[0].valid());
    let mut resumed = load(root.path(), &scenario);
    resumed.restore_snapshot(session.snapshot()).unwrap();
    session.step_realtime(0.2, |_| {});
    resumed.step_realtime(0.2, |_| {});
    assert_eq!(session.state.vehicle_systems, resumed.state.vehicle_systems);
    assert_eq!(session.state.electric, resumed.state.electric);
}

#[test]
fn native_large_fade_knots_keep_finite_order_force_and_vehicle_reference_checks() {
    let (root, mut scenario) = native_profile();
    let mut session = load(root.path(), &scenario);
    let original = session.state.vehicle_systems[0].definition.clone();
    for invalid in [-1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut saved = session.snapshot();
        saved.state.vehicle_systems[0]
            .definition
            .electric_brake
            .as_mut()
            .unwrap()
            .force_curve[5][0] = invalid;
        assert!(session.restore_snapshot(saved).is_err());
        let mut saved = session.snapshot();
        saved.state.vehicle_systems[0]
            .definition
            .electric_brake
            .as_mut()
            .unwrap()
            .force_curve[5][1] = invalid;
        assert!(session.restore_snapshot(saved).is_err());
    }
    let mut reversed = original.clone();
    reversed
        .electric_brake
        .as_mut()
        .unwrap()
        .force_curve
        .swap(4, 5);
    assert!(!reversed.valid());
    let mut excessive = original.clone();
    excessive.electric_brake.as_mut().unwrap().force_curve[5][1] = 1e6 + 1.;
    assert!(!excessive.valid());
    let mut excessive = original.clone();
    excessive.electric_brake.as_mut().unwrap().force_curve[5][0] = f64::MAX;
    assert!(!excessive.valid());
    let mut custom = original.clone();
    custom.electric_brake.as_mut().unwrap().native_response = false;
    assert!(!custom.valid(), "bounded custom equipment remains bounded");
    let mut outside = original;
    outside.vehicle = 1;
    scenario.train.systems.push(outside);
    assert!(LiveDriveSession::from_scenario(root.path(), &scenario).is_err());
}
