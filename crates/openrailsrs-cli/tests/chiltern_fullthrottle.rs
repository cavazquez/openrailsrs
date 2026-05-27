//! Experimento B — throttle 100 % constante 120 s (curvas de tracción / OR-P3).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{
    OrColumnMap, compare_or_dump_phases, compare_or_dump_with_run, phase_report_passes,
};

#[test]
fn chiltern_fullthrottle_sanity() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_throttle100.toml");
    let driver_path = chiltern.join("driver_throttle100.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    let result = run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");
    let last_v = result.final_state.velocity_mps;
    assert!(
        last_v > 15.0 && last_v < 45.0,
        "expected high speed at 120 s with 100% throttle, got {last_v} m/s"
    );
}

#[test]
fn chiltern_fullthrottle_validate_against_or_baseline() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_throttle100.toml");
    let baseline = chiltern.join("../baselines/chiltern_fullthrottle/or_evaluation_speed.csv");
    if !baseline.exists() {
        return;
    }

    let driver_path = chiltern.join("driver_throttle100.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario
        .validate
        .expect("scenario_throttle100.toml must define [validate]");
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
        "Chiltern full-throttle OR validation failed: vel_rms={:.3} max={:.3}",
        report.velocity.rms_diff, report.velocity.max_abs_diff
    );

    let bounds = validate
        .phase_bounds
        .as_deref()
        .unwrap_or(&[0.0, 30.0, 120.0]);
    let phase_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: validate
            .phase_max_velocity_rms
            .or(validate.thresholds.max_velocity_rms),
        ..validate.thresholds.clone()
    };
    let phases = compare_or_dump_phases(&baseline, &run_csv, &OrColumnMap::default(), bounds, 0.1)
        .expect("compare-or phases");

    for phase in &phases {
        assert!(
            phase_report_passes(phase, &phase_thresholds),
            "Experimento B phase {} failed: vel_rms={:.3} max={:.3}, brake_rms={:.3} max={:.3}",
            phase.label,
            phase.velocity.rms_diff,
            phase.velocity.max_abs_diff,
            phase.brake.as_ref().map(|s| s.rms_diff).unwrap_or(0.0),
            phase.brake.as_ref().map(|s| s.max_abs_diff).unwrap_or(0.0),
        );
    }
}
