//! End-to-end Chiltern OR validation (skipped when `examples/chiltern/track.toml` is absent).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_from_scenario_file_with_driver};
use openrailsrs_validate::{OrColumnMap, compare_or_dump_with_run};

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
        "Chiltern OR validation failed (see compare-or report)"
    );
}
