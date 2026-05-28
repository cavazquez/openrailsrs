//! SCE Glasgow with `multi_body = true` (Class 47 + 6 Mk2, dt=1 s).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{OrColumnMap, compare_or_dump_phases, compare_or_dump_with_run};

#[test]
fn sce_multi_body_sanity() {
    let sce = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/sce");
    if !sce.join("track.toml").exists() {
        return;
    }

    let scenario_path = sce.join("scenario_multi_body.toml");
    let driver_path = sce.join("driver_or.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    let result = run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let rows = std::fs::read_to_string(sce.join("run_multi_body.csv")).expect("run csv");
    let mut v_at_60 = 0.0_f64;
    for line in rows.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 4 {
            continue;
        }
        let t: f64 = parts[0].parse().unwrap_or(0.0);
        let v: f64 = parts[3].parse().unwrap_or(0.0);
        if (t - 60.0).abs() < 0.5 {
            v_at_60 = v;
        }
    }

    assert!(
        v_at_60 > 3.0,
        "expected acceleration by t=60 s, got {v_at_60} m/s (final v={})",
        result.final_state.velocity_mps
    );
}

#[test]
fn sce_multi_body_validate_against_or_baseline() {
    let sce = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/sce");
    if !sce.join("track.toml").exists() {
        return;
    }

    let scenario_path = sce.join("scenario_multi_body.toml");
    let baseline = sce.join("../baselines/sce_glasgow/or_evaluation_speed.csv");
    if !baseline.exists() {
        return;
    }

    let driver_path = sce.join("driver_or.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario.validate.expect("[validate]");
    let run_csv = sce.join(&scenario.output.csv);

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
        "SCE multi-body OR validation failed: vel_rms={:.3} max={:.3}",
        report.velocity.rms_diff, report.velocity.max_abs_diff
    );

    // Masa puntual ~0.30 m/s RMS global; multi-cuerpo arranque más holgura.
    const MULTI_BODY_MAX_VEL_RMS: f64 = 1.0;
    assert!(
        report.velocity.rms_diff <= MULTI_BODY_MAX_VEL_RMS,
        "multi-body velocity rms {:.3} m/s exceeds ceiling {MULTI_BODY_MAX_VEL_RMS}",
        report.velocity.rms_diff
    );

    let bounds = validate
        .phase_bounds
        .as_deref()
        .unwrap_or(&[0.0, 30.0, 60.0, 100.0]);
    let phase_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: validate
            .phase_max_velocity_rms
            .or(validate.thresholds.max_velocity_rms),
        ..validate.thresholds.clone()
    };
    let phases = compare_or_dump_phases(&baseline, &run_csv, &OrColumnMap::default(), bounds, 0.1)
        .expect("compare-or phases");

    let startup = phases.first().expect("startup phase");
    assert!(
        openrailsrs_validate::phase_report_passes(startup, &phase_thresholds),
        "SCE multi-body startup phase {} failed: vel_rms={:.3}",
        startup.label,
        startup.velocity.rms_diff,
    );

    // Arranque push-pull: holgura acopladores; umbral interino más laxo que Chiltern.
    const STARTUP_RMS_CEILING: f64 = 0.85;
    assert!(
        startup.velocity.rms_diff <= STARTUP_RMS_CEILING,
        "startup RMS {:.3} m/s exceeds interim ceiling {STARTUP_RMS_CEILING}",
        startup.velocity.rms_diff
    );
}
