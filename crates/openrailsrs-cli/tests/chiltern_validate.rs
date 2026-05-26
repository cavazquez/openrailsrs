//! End-to-end Chiltern OR validation (skipped when `examples/chiltern/track.toml` is absent).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{
    OrColumnMap, compare_or_dump_phases, compare_or_dump_with_run, phase_report_passes,
};

#[test]
fn chiltern_sim_validate_against_or_baseline() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let chiltern = manifest.join("../../examples/chiltern");
    let track = chiltern.join("track.toml");
    if !track.exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario.toml");
    let driver_path = chiltern.join("driver_or.csv");
    assert!(
        driver_path.exists(),
        "missing driver_or.csv — run or-eval-driver on the OR baseline first"
    );

    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("chiltern sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario
        .validate
        .expect("scenario.toml must define [validate] for this test");
    let baseline = chiltern.join(
        validate
            .baseline_or
            .as_ref()
            .expect("[validate].baseline_or"),
    );
    let run_csv = chiltern.join(&scenario.output.csv);
    let step = 0.1;

    let report = compare_or_dump_with_run(
        &baseline,
        &run_csv,
        &OrColumnMap::default(),
        &validate.thresholds,
        step,
    )
    .expect("compare-or");

    assert!(
        report.pass,
        "Chiltern OR validation failed (global): velocity rms={:.3} max={:.3}, position rms={:.1} max={:.1}",
        report.velocity.rms_diff,
        report.velocity.max_abs_diff,
        report.position.rms_diff,
        report.position.max_abs_diff,
    );

    let bounds = validate
        .phase_bounds
        .as_deref()
        .unwrap_or(&[0.0, 61.0, 136.0]);
    let phase_thresholds = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: validate
            .phase_max_velocity_rms
            .or(validate.thresholds.max_velocity_rms),
        ..validate.thresholds.clone()
    };
    let phases = compare_or_dump_phases(&baseline, &run_csv, &OrColumnMap::default(), bounds, step)
        .expect("compare-or phases");

    for phase in &phases {
        assert!(
            phase_report_passes(phase, &phase_thresholds),
            "Chiltern phase {} failed: velocity rms={:.3} max={:.3}, position rms={:.1} max={:.1}",
            phase.label,
            phase.velocity.rms_diff,
            phase.velocity.max_abs_diff,
            phase.position.rms_diff,
            phase.position.max_abs_diff,
        );
    }
}
