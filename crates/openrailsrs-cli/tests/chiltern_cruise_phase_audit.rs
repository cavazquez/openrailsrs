//! Per-second cruise audit (61–136 s): OR vs sim velocity and traction balance.

use std::path::{Path, PathBuf};

use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_train::load_consist_with_asset_root;
use openrailsrs_validate::{
    OrColumnMap, compare_or_dump_phases, parse_openrailsrs_run_csv, parse_or_dump_csv,
    resample_traces,
};

fn chiltern_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
}

fn or_sample_at(
    or_samples: &[openrailsrs_validate::TraceSample],
    t: f64,
) -> openrailsrs_validate::TraceSample {
    or_samples
        .iter()
        .min_by(|a, b| {
            (a.time_s - t)
                .abs()
                .partial_cmp(&(b.time_s - t).abs())
                .unwrap()
        })
        .cloned()
        .unwrap_or(openrailsrs_validate::TraceSample {
            time_s: t,
            velocity_mps: 0.0,
            distance_m: 0.0,
            energy_kwh: None,
            throttle: None,
            brake: None,
        })
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
fn chiltern_cruise_phase_audit_61_136s() {
    let chiltern = chiltern_dir();
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let or_path = chiltern.join("../baselines/chiltern_birmingham/or_evaluation_speed.csv");
    let run_path = chiltern.join("run.csv");
    if !run_path.exists() {
        return;
    }

    let or_trace = parse_or_dump_csv(&or_path, &OrColumnMap::default()).expect("OR");
    let rs_trace = parse_openrailsrs_run_csv(&run_path).expect("run");

    let (a, b) = resample_traces(&or_trace, &rs_trace, 0.1).expect("resample");
    let mut worst = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for (sa, sb) in a.iter().zip(b.iter()) {
        if sa.time_s < 61.0 || sa.time_s > 136.0 {
            continue;
        }
        let dv = (sb.velocity_mps - sa.velocity_mps).abs();
        if dv > worst.0 {
            worst = (
                dv,
                sa.time_s,
                sa.velocity_mps,
                sb.velocity_mps,
                sb.distance_m - sa.distance_m,
            );
        }
    }

    let consist =
        load_consist_with_asset_root(chiltern.join("consists/birmingham_pullman.con"), &chiltern)
            .expect("consist");
    let models = consist.diesel_traction_models();
    let throttle = 0.8;

    eprintln!("\n=== Chiltern cruise audit (61–136 s) ===");
    eprintln!(
        "worst resampled: t={:.1} dv={:.3} v_or={:.3} v_sim={:.3} pos_err={:.1}m",
        worst.1, worst.0, worst.2, worst.3, worst.4
    );

    let phases = compare_or_dump_phases(
        &or_path,
        &run_path,
        &OrColumnMap::default(),
        &[0.0, 30.0, 61.0, 136.0],
        0.1,
    )
    .expect("phases");
    for p in &phases {
        eprintln!(
            "phase {} vel_rms={:.3} pos_rms={:.1}",
            p.label, p.velocity.rms_diff, p.position.rms_diff
        );
    }

    eprintln!(
        "\n{:>4} {:>8} {:>8} {:>8} {:>8} {:>8} {:>10}",
        "t", "v_or", "v_sim", "dv", "odom", "d_or", "F_net"
    );

    for t in (61..=136).step_by(15) {
        let t = t as f64;
        let or_s = or_sample_at(&or_trace.samples, t);
        let state = sim_state_at(&chiltern, t);
        let v_or = or_s.velocity_mps;
        let v_sim = state.velocity_mps;
        let dv = v_sim - v_or;

        let mut f_trac = 0.0;
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
            if v_sim > 0.5 && p > 0.0 && m.engine.is_some() {
                f = f.min(p / v_sim);
            }
            f_trac += f;
        }
        let f_res = consist.davis.a_n
            + consist.davis.b_n_per_mps * v_sim
            + consist.davis.c_n_per_mps2 * v_sim * v_sim;
        let f_net = f_trac - f_res;

        eprintln!(
            "{t:4.0} {v_or:8.3} {v_sim:8.3} {dv:+8.3} {:8.0} {:8.0} {f_net:10.0}",
            state.odometer_m, or_s.distance_m
        );
    }
}
