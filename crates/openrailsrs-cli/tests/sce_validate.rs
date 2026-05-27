//! End-to-end SCE Glasgow OR validation (skipped when `examples/sce/track.toml` is absent).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{OrColumnMap, compare_or_dump_with_run};

#[test]
fn sce_sim_validate_against_or_baseline() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sce = manifest.join("../../examples/sce");
    let track = sce.join("track.toml");
    if !track.exists() {
        return;
    }

    let scenario_path = sce.join("scenario.toml");
    let driver_path = sce.join("driver_or.csv");
    assert!(
        driver_path.exists(),
        "missing driver_or.csv — run: openrailsrs or-eval-driver --scenario scenario.toml ..."
    );

    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_from_scenario_file_with_driver(&scenario_path, &mut driver).expect("sce sim");

    let scenario = load_scenario(&scenario_path).expect("reload scenario");
    let validate = scenario
        .validate
        .expect("scenario.toml must define [validate] for this test");
    let baseline = sce.join(
        validate
            .baseline_or
            .as_ref()
            .expect("[validate].baseline_or"),
    );
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
        "SCE OR validation failed: velocity rms={:.3} max={:.3}, position rms={:.1} max={:.1}",
        report.velocity.rms_diff,
        report.velocity.max_abs_diff,
        report.position.rms_diff,
        report.position.max_abs_diff,
    );
}
