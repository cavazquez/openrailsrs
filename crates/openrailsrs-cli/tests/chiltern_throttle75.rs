//! Experimento C — throttle 75 % constante 60 s (equilibrio crucero, OR-P1).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{
    OrColumnMap, compare_or_dump_phases, compare_or_dump_with_run, phase_report_passes,
};

#[test]
fn chiltern_throttle75_sanity() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_throttle75.toml");
    let driver_path = chiltern.join("driver_throttle75.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    let result = run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");
    let last_v = result.final_state.velocity_mps;
    assert!(
        last_v > 12.0 && last_v < 28.0,
        "expected cruise-like speed at 60 s with 75% throttle, got {last_v} m/s"
    );
}

#[test]
fn chiltern_throttle75_validate_against_or_baseline() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_throttle75.toml");
    let baseline = chiltern.join("../baselines/chiltern_throttle75/or_evaluation_speed.csv");
    if !baseline.exists() {
        return;
    }

    let driver_path = chiltern.join("driver_throttle75.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario
        .validate
        .expect("scenario_throttle75.toml must define [validate]");
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
        "Chiltern throttle-75 OR validation failed: vel_rms={:.3} max={:.3}",
        report.velocity.rms_diff, report.velocity.max_abs_diff
    );

    let bounds = validate
        .phase_bounds
        .as_deref()
        .unwrap_or(&[0.0, 20.0, 60.0]);
    let phase_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: validate
            .phase_max_velocity_rms
            .or(validate.thresholds.max_velocity_rms),
        ..validate.thresholds.clone()
    };
    let phases = compare_or_dump_phases(&baseline, &run_csv, &OrColumnMap::default(), bounds, 0.1)
        .expect("compare-or phases");

    // Transitorio 0–20 s: umbral relajado; crucero 20–60 s: estricto (OR-P1).
    let cruise_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: Some(0.65),
        max_throttle_rms: None,
        max_brake_rms: None,
        ..Default::default()
    };
    for phase in phases.iter().take(phases.len().saturating_sub(1)) {
        assert!(
            phase_report_passes(phase, &cruise_thresholds),
            "Experimento C phase {} failed: vel_rms={:.3} max={:.3}",
            phase.label,
            phase.velocity.rms_diff,
            phase.velocity.max_abs_diff,
        );
    }
    let cruise = phases.last().expect("coast phase");
    assert!(
        phase_report_passes(cruise, &phase_thresholds),
        "Experimento C cruise phase {} failed: vel_rms={:.3} max={:.3}",
        cruise.label,
        cruise.velocity.rms_diff,
        cruise.velocity.max_abs_diff,
    );
}
