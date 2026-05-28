//! OR-P1 integration: SCE cruise speed @ ~27 % throttle vs OR baseline (~14 mph).

use std::path::PathBuf;

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_validate::{OrColumnMap, parse_or_dump_csv, parse_openrailsrs_run_csv};

#[test]
fn sce_cruise_velocity_matches_or_within_half_mph() {
    let sce = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/sce");
    if !sce.join("track.toml").exists() {
        return;
    }

    let baseline = sce.join("../baselines/sce_glasgow/or_evaluation_speed.csv");
    if !baseline.exists() {
        return;
    }

    let or_trace = parse_or_dump_csv(&baseline, &OrColumnMap::default()).expect("parse OR");
    let or_at_80 = or_trace
        .samples
        .iter()
        .find(|s| (s.time_s - 80.0).abs() < 0.6)
        .map(|s| s.velocity_mps)
        .unwrap_or(0.0);
    assert!(
        or_at_80 > 4.0,
        "OR baseline should have cruise speed by t=80 s, got {or_at_80} m/s"
    );

    let mut scenario = load_scenario(sce.join("scenario.toml")).expect("scenario");
    assert!(
        !scenario.simulation.legacy_power_cap,
        "SCE scenario must use OR-P1 rail power cap"
    );
    scenario.simulation.duration = 85.0;

    let mut driver = ScriptedDriver::from_csv(sce.join("driver_or.csv")).expect("driver");
    run_scenario_headless_with_driver(&sce, &scenario, &mut driver).expect("sim");

    let run_csv = sce.join(&scenario.output.csv);
    let run = parse_openrailsrs_run_csv(&run_csv).expect("run csv");
    let sim_at_80 = run
        .samples
        .iter()
        .find(|s| (s.time_s - 80.0).abs() < 0.6)
        .map(|s| s.velocity_mps)
        .unwrap_or(0.0);

    const HALF_MPH_MPS: f64 = 0.5 / 2.237;
    let delta = (sim_at_80 - or_at_80).abs();
    assert!(
        delta <= HALF_MPH_MPS + 0.05,
        "cruise @ t=80 s: sim {sim_at_80:.3} m/s vs OR {or_at_80:.3} m/s (delta {delta:.3}, max {:.3})",
        HALF_MPH_MPS + 0.05
    );
}
