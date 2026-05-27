//! Experimento E — throttle 50 % constante 30 s (calibración aislada).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{OrColumnMap, compare_or_dump_with_run};

#[test]
fn chiltern_throttle50_sanity() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let chiltern = manifest.join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_throttle50.toml");
    let driver_path = chiltern.join("driver_throttle50.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    let result = run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");
    let last_v = result.final_state.velocity_mps;
    assert!(
        last_v > 3.0 && last_v < 15.0,
        "expected sensible speed at 30 s with 50% throttle, got {last_v} m/s"
    );
}

#[test]
fn chiltern_throttle50_validate_against_or_baseline() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let chiltern = manifest.join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_throttle50.toml");
    let baseline = chiltern.join("../baselines/chiltern_throttle50/or_evaluation_speed.csv");
    if !baseline.exists() {
        return;
    }

    let driver_path = chiltern.join("driver_throttle50.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario
        .validate
        .expect("scenario_throttle50.toml must define [validate]");
    let run_csv = chiltern.join(&scenario.output.csv);

    let report = compare_or_dump_with_run(
        &baseline,
        &run_csv,
        &OrColumnMap::default(),
        &validate.thresholds,
        0.1,
    )
    .expect("compare-or");

    assert!(
        report.pass,
        "Chiltern throttle-50 OR validation failed: vel_rms={:?}",
        report.velocity.rms_diff
    );
}
