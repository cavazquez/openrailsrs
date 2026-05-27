//! Per-second full-throttle audit (Exp B, 0–30 s): OR vs sim velocity and traction.

use std::path::{Path, PathBuf};

use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_train::load_consist_with_asset_root;
use openrailsrs_validate::{OrColumnMap, parse_or_dump_csv};

fn chiltern_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
}

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

fn sim_state_at(chiltern: &Path, t: f64) -> openrailsrs_sim::TrainSimState {
    let scenario = load_scenario(chiltern.join("scenario_throttle100.toml")).expect("scenario");
    let mut scenario = scenario;
    scenario.simulation.duration = t;
    let mut driver =
        ScriptedDriver::from_csv(chiltern.join("driver_throttle100.csv")).expect("driver");
    run_scenario_headless_with_driver(chiltern, &scenario, &mut driver)
        .expect("sim")
        .final_state
}

#[test]
fn chiltern_fullthrottle_audit_0_30s() {
    let chiltern = chiltern_dir();
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let or_path = chiltern.join("../baselines/chiltern_fullthrottle/or_evaluation_speed.csv");
    if !or_path.exists() {
        return;
    }
    let or_trace = parse_or_dump_csv(&or_path, &OrColumnMap::default()).expect("OR trace");

    let consist =
        load_consist_with_asset_root(chiltern.join("consists/birmingham_pullman.con"), &chiltern)
            .expect("consist");
    let models = consist.diesel_traction_models();
    let throttle = 1.0;
    let legacy = false;

    eprintln!("\n=== Chiltern full-throttle audit (Exp B, 0–30 s) ===");
    eprintln!(
        "{:>4} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "t", "v_or", "v_sim", "dv", "rpm0", "rpm1", "run1", "F0", "F1", "F_sum"
    );

    let mut sq_err = 0.0;
    let mut n = 0_usize;

    for t in (0..=30).step_by(5) {
        let t = t as f64;
        let v_or = or_velocity_at(&or_trace.samples, t).unwrap_or(0.0);
        let state = sim_state_at(&chiltern, t);
        let v_sim = state.velocity_mps;
        let dv = v_sim - v_or;
        sq_err += dv * dv;
        n += 1;

        let rpm0 = state.diesel_rpm.first().copied().unwrap_or(0.0);
        let rpm1 = state.diesel_rpm.get(1).copied().unwrap_or(0.0);
        let run1 = state.diesel_run_up.get(1).copied().unwrap_or(1.0);

        let mut f = [0.0_f64; 2];
        for (i, m) in models.iter().enumerate().take(2) {
            let rpm = state.diesel_rpm.get(i).copied().unwrap_or(0.0);
            let run_up = state.diesel_run_up.get(i).copied().unwrap_or(1.0);
            let run_factor = if m.legacy_run_up_time_s().is_some() {
                run_up
            } else {
                1.0
            };
            let heat = state.diesel_motor_heat.get(i).copied().unwrap_or(0.0);
            let pr = openrailsrs_train::DieselTractionModel::power_reduction_from_heat(heat);
            f[i] = m.target_traction_force_n(v_sim, throttle, rpm, run_factor, pr, legacy);
        }

        eprintln!(
            "{t:4.0} {v_or:8.3} {v_sim:8.3} {dv:+8.3} {rpm0:8.0} {rpm1:8.0} {run1:8.3} {} {} {}",
            f[0],
            f[1],
            f[0] + f[1]
        );
    }

    let rms = (sq_err / n as f64).sqrt();
    eprintln!("velocity RMS (5 s samples): {rms:.3} m/s");
}
