//! OR-P6: Chiltern 0–40 s — residual brake at activity start vs OR baseline.

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_validate::{OrColumnMap, compare_or_dump_phases, phase_report_passes};

#[test]
fn chiltern_phase_0_40s_startup_brake_or_p6() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let scenario_path = chiltern.join("scenario.toml");
    let driver_path = chiltern.join("driver_or.csv");
    let mut scenario = load_scenario(&scenario_path).expect("scenario");
    scenario.simulation.orts_inherit_partial_run_up = true;
    let mut driver = ScriptedDriver::from_csv(&driver_path).expect("load driver");
    run_scenario_headless_with_driver(&chiltern, &scenario, &mut driver).expect("sim");

    let baseline = chiltern.join(
        scenario
            .validate
            .as_ref()
            .and_then(|v| v.baseline_or.as_ref())
            .expect("baseline_or"),
    );
    let run_csv = chiltern.join(&scenario.output.csv);

    let phases = compare_or_dump_phases(
        &baseline,
        &run_csv,
        &OrColumnMap::default(),
        &[0.0, 40.0, 136.0],
        0.1,
    )
    .expect("compare-or phases");

    let startup = phases.first().expect("0–40 s phase");
    let config = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: Some(0.42),
        max_position_max: Some(9.0),
        ..Default::default()
    };
    assert!(
        phase_report_passes(startup, &config),
        "OR-P6 phase 0–40 s failed: vel_rms={:.3} max={:.3}, pos_rms={:.1} max={:.1}",
        startup.velocity.rms_diff,
        startup.velocity.max_abs_diff,
        startup.position.rms_diff,
        startup.position.max_abs_diff,
    );
}
