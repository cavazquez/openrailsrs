//! OR-P3: Chiltern startup must not overshoot OR velocity by >2 m/s (0–30 s).

use std::path::PathBuf;

use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_validate::{OrColumnMap, parse_or_dump_csv};

fn or_velocity_at(or_samples: &[openrailsrs_validate::TraceSample], t: f64) -> Option<f64> {
    or_samples
        .iter()
        .min_by(|a, b| {
            (a.time_s - t)
                .abs()
                .partial_cmp(&(b.time_s - t).abs())
                .unwrap()
        })
        .map(|s| s.velocity_mps)
}

#[test]
fn chiltern_startup_no_velocity_overshoot_vs_or() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let or_path = chiltern.join("../baselines/chiltern_birmingham/or_evaluation_speed.csv");
    let or_trace = parse_or_dump_csv(&or_path, &OrColumnMap::default()).expect("OR trace");

    let mut max_overshoot = 0.0_f64;
    for t in 0..=30 {
        let t = t as f64;
        let v_or = or_velocity_at(&or_trace.samples, t).unwrap_or(0.0);
        let mut scenario = load_scenario(chiltern.join("scenario.toml")).expect("scenario");
        apply_scenario_runtime_overlay_dir(&mut scenario, &chiltern).expect("overlay");
        scenario.simulation.duration = t;
        let mut driver = ScriptedDriver::from_csv(chiltern.join("driver_or.csv")).expect("driver");
        let state = run_scenario_headless_with_driver(&chiltern, &scenario, &mut driver)
            .expect("sim")
            .final_state;
        let overshoot = state.velocity_mps - v_or;
        if overshoot > max_overshoot {
            max_overshoot = overshoot;
        }
    }

    assert!(
        max_overshoot <= 2.0,
        "OR-P3 startup overshoot {max_overshoot:.3} m/s exceeds 2.0 m/s"
    );
}
