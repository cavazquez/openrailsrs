//! Resampled 0–30 s startup diagnostic: worst Δv samples and brake state.

use std::path::PathBuf;

use openrailsrs_validate::{
    OrColumnMap, parse_openrailsrs_run_csv, parse_or_dump_csv, resample_traces,
};

fn chiltern_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
}

#[test]
fn chiltern_startup_worst_samples_0_30s() {
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

    let mut worst: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (sa, sb) in a.iter().zip(b.iter()) {
        if sa.time_s > 30.0 {
            continue;
        }
        let dv = (sb.velocity_mps - sa.velocity_mps).abs();
        worst.push((dv, sa.time_s, sa.velocity_mps, sb.velocity_mps));
    }
    worst.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
    eprintln!("\n=== Worst Δv in 0–30 s (resampled 0.1 s) ===");
    for (dv, t, vo, vs) in worst.iter().take(8) {
        eprintln!("t={t:5.1} dv={dv:.3} v_or={vo:.3} v_sim={vs:.3}");
    }

    let mut sq = 0.0;
    let mut n = 0usize;
    for (sa, sb) in a.iter().zip(b.iter()) {
        if sa.time_s > 30.0 {
            continue;
        }
        let d = sb.velocity_mps - sa.velocity_mps;
        sq += d * d;
        n += 1;
    }
    eprintln!("phase RMS: {:.3} m/s (n={n})", (sq / n as f64).sqrt());
}
