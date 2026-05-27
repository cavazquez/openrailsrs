//! Experimento A (multi-body) — frenada + costa libre vs baseline OR.

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{
    OrColumnMap, compare_or_dump_phases, compare_or_dump_with_run, phase_report_passes,
};

#[test]
fn chiltern_brake_coast_multi_body_sanity() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_brake_coast_multi_body.toml");
    let driver_path = chiltern.join("driver_brake_coast.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    let result = run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let rows =
        std::fs::read_to_string(chiltern.join("run_brake_coast_multi_body.csv")).expect("run csv");
    let mut v_at_release = 0.0_f64;
    let mut v_at_end = result.final_state.velocity_mps;
    for line in rows.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 4 {
            continue;
        }
        let t: f64 = parts[0].parse().unwrap_or(0.0);
        let v: f64 = parts[3].parse().unwrap_or(0.0);
        if (t - 115.0).abs() < 0.5 {
            v_at_release = v;
        }
        if (t - 180.0).abs() < 0.5 {
            v_at_end = v;
        }
    }

    assert!(
        v_at_release > 10.0 && v_at_release < 20.0,
        "expected meaningful speed at brake release (t=115 s), got {v_at_release} m/s"
    );
    assert!(
        v_at_end > 5.0,
        "expected coast-down still moving at t=180 s, got {v_at_end} m/s"
    );
}

#[test]
fn chiltern_brake_coast_multi_body_validate_against_or_baseline() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_brake_coast_multi_body.toml");
    let baseline = chiltern.join("../baselines/chiltern_brake_coast/or_evaluation_speed.csv");
    if !baseline.exists() {
        return;
    }

    let driver_path = chiltern.join("driver_brake_coast.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario.validate.expect("[validate]");
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
        "multi-body brake-coast OR validation failed: vel_rms={:.3} max={:.3}",
        report.velocity.rms_diff,
        report.velocity.max_abs_diff
    );

    let bounds = validate
        .phase_bounds
        .as_deref()
        .unwrap_or(&[0.0, 100.0, 115.0, 180.0]);
    let phase_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: validate
            .phase_max_velocity_rms
            .or(validate.thresholds.max_velocity_rms),
        ..validate.thresholds.clone()
    };
    let phases = compare_or_dump_phases(&baseline, &run_csv, &OrColumnMap::default(), bounds, 0.1)
        .expect("compare-or phases");

    let coast = phases.last().expect("coast phase");
    assert!(
        phase_report_passes(coast, &phase_thresholds),
        "Experimento A multi-body coast phase {} failed: vel_rms={:.3} max={:.3}",
        coast.label,
        coast.velocity.rms_diff,
        coast.velocity.max_abs_diff,
    );

    // Masa puntual ~0.07 m/s RMS en 115–180 s; multi con costa escalar ~0.13 m/s.
    const COAST_RMS_CEILING: f64 = 0.15;
    assert!(
        coast.velocity.rms_diff <= COAST_RMS_CEILING,
        "coast RMS {:.3} m/s exceeds interim ceiling {COAST_RMS_CEILING}",
        coast.velocity.rms_diff
    );

    let cruise_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: Some(0.65),
        max_throttle_rms: None,
        max_brake_rms: Some(0.55),
        max_position_rms: None,
        max_position_max: None,
        ..Default::default()
    };
    for phase in phases.iter().take(phases.len().saturating_sub(1)) {
        assert!(
            phase_report_passes(phase, &cruise_thresholds),
            "Experimento A multi-body phase {} failed: vel_rms={:.3}",
            phase.label,
            phase.velocity.rms_diff,
        );
    }
}
