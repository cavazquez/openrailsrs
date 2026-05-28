//! Chiltern Birmingham with `multi_body = true` (sub-pasos acoplador, dt=1 s).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{OrColumnMap, compare_or_dump_phases, compare_or_dump_with_run};

#[test]
fn chiltern_multi_body_vs_or_baseline() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario_multi_body.toml");
    let driver_path = chiltern.join("driver_or.csv");
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("multi-body sim");

    let scenario = load_scenario(&scenario_path).expect("scenario");
    let validate = scenario.validate.expect("[validate]");
    let baseline = chiltern.join(validate.baseline_or.as_ref().expect("baseline_or"));
    let run_csv = chiltern.join(&scenario.output.csv);

    let report = compare_or_dump_with_run(
        &baseline,
        &run_csv,
        &OrColumnMap::default(),
        &validate.thresholds,
        0.1,
    )
    .expect("compare-or");

    // Global RMS ~0.39 m/s vs OR with dt=1 s + coupler sub-steps; startup ~0.54 m/s.
    const MULTI_BODY_MAX_VEL_RMS: f64 = 0.42;
    assert!(
        report.velocity.rms_diff <= MULTI_BODY_MAX_VEL_RMS,
        "multi-body velocity rms {:.3} m/s (max {MULTI_BODY_MAX_VEL_RMS})",
        report.velocity.rms_diff
    );

    let bounds = validate
        .phase_bounds
        .as_deref()
        .unwrap_or(&[0.0, 30.0, 61.0, 136.0]);
    let phases = compare_or_dump_phases(&baseline, &run_csv, &OrColumnMap::default(), bounds, 0.1)
        .expect("phases");
    let startup = phases.first().expect("phase 0");
    let startup_max = validate.phase_max_velocity_rms.unwrap_or(0.56);
    assert!(
        startup.velocity.rms_diff <= startup_max,
        "startup phase should track OR: rms {:.3} (max {startup_max})",
        startup.velocity.rms_diff
    );
}
