//! Per-second startup audit (0–30 s): OR vs sim velocity, RPM, and traction.

use std::path::{Path, PathBuf};

use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
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
    let mut scenario = load_scenario(chiltern.join("scenario.toml")).expect("scenario");
    apply_scenario_runtime_overlay_dir(&mut scenario, chiltern).expect("overlay");
    scenario.simulation.duration = t;
    let mut driver = ScriptedDriver::from_csv(chiltern.join("driver_or.csv")).expect("driver");
    run_scenario_headless_with_driver(chiltern, &scenario, &mut driver)
        .expect("sim")
        .final_state
}

#[test]
fn chiltern_startup_audit_0_30s() {
    let chiltern = chiltern_dir();
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let or_path = chiltern.join("../baselines/chiltern_birmingham/or_evaluation_speed.csv");
    let or_trace = parse_or_dump_csv(&or_path, &OrColumnMap::default()).expect("OR trace");

    let consist =
        load_consist_with_asset_root(chiltern.join("consists/birmingham_pullman.con"), &chiltern)
            .expect("consist");
    let models = consist.diesel_traction_models();
    let throttle = 0.8;

    eprintln!("\n=== Chiltern startup audit (0–30 s) ===");
    let brake_scale = openrailsrs_validate::infer_brake_full_scale(&or_trace);
    let or_brake0 = or_trace
        .samples
        .iter()
        .find(|s| s.time_s.abs() < 0.5)
        .and_then(|s| s.brake);
    eprintln!("OR brake full scale (PSI): {brake_scale:.0}, sample@0s={or_brake0:?} PSI");
    eprintln!(
        "{:>4} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "t", "v_or", "v_sim", "dv", "odom", "rpm0", "rpm1", "run1", "F_trac"
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

        let mut f_sum = 0.0;
        for (i, m) in models.iter().enumerate() {
            let rpm = state.diesel_rpm.get(i).copied().unwrap_or(0.0);
            let run_up = state.diesel_run_up.get(i).copied().unwrap_or(1.0);
            let run_factor = if m.legacy_run_up_time_s().is_some() {
                run_up
            } else {
                1.0
            };
            let heat = state.diesel_motor_heat.get(i).copied().unwrap_or(0.0);
            let pr = openrailsrs_train::DieselTractionModel::power_reduction_from_heat(heat);
            let mut f = m.force_at_scaled(v_sim, throttle, run_factor, pr);
            let p = m.effective_power_w(rpm, throttle) * run_factor * (1.0 - pr.clamp(0.0, 0.95));
            if v_sim > 0.5 && p > 0.0 {
                f = f.min(p / v_sim);
            }
            f_sum += f;
        }

        eprintln!(
            "{t:4.0} {v_or:8.3} {v_sim:8.3} {dv:+8.3} {:8.0} {rpm0:8.0} {rpm1:8.0} {run1:8.3} {f_sum:8.0}",
            state.odometer_m
        );
    }

    let rms = (sq_err / n as f64).sqrt();
    eprintln!("velocity RMS (5 s samples): {rms:.3} m/s");

    // DMBSA target RPM at 80% throttle should reach ~1200 RPM within 30 s.
    let state30 = sim_state_at(&chiltern, 30.0);
    let target_rpm = models[0]
        .engine
        .as_ref()
        .map(|e| e.target_rpm(throttle))
        .unwrap_or(0.0);
    eprintln!(
        "t=30 DMBSA rpm={:.0} target={target_rpm:.0} DMBSH run_up={:.3}",
        state30.diesel_rpm.first().copied().unwrap_or(0.0),
        state30.diesel_run_up.get(1).copied().unwrap_or(0.0)
    );
}
