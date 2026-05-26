use std::io::Write;

use openrailsrs_validate::{ValidationConfig, compare_csv_files, compare_csv_files_with_config};

const HEADER: &str =
    "time_s,edge_id,pos_on_edge_m,velocity_mps,odometer_m,cumulative_energy_kwh,throttle,brake\n";

fn write_csv(path: &std::path::Path, rows: &[&str]) {
    let mut f = std::fs::File::create(path).unwrap();
    write!(f, "{HEADER}").unwrap();
    for row in rows {
        writeln!(f, "{row}").unwrap();
    }
}

// ── Existing tests (unchanged API) ───────────────────────────────────────────

#[test]
fn identical_runs_zero_diff() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.csv");
    write_csv(&p, &["0,e1,0,0,0,0,0,0", "0.1,e1,0,1,0.1,0,1,0"]);
    let rep = compare_csv_files(&p, &p).expect("compare");
    assert_eq!(rep.velocity.max_abs_diff, 0.0);
    assert_eq!(rep.position.max_abs_diff, 0.0);
    assert!(rep.pass, "identical files should always pass");
}

#[test]
fn different_runs_have_non_zero_diff() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let b = dir.path().join("b.csv");
    write_csv(&a, &["0,e1,0,10,0,1,1,0", "1,e1,0,12,11,2,1,0"]);
    write_csv(&b, &["0,e1,0,8,0,0.5,1,0", "1,e1,0,9,9,1,1,0"]);

    let rep = compare_csv_files(&a, &b).expect("compare");
    assert!(rep.velocity.max_abs_diff > 0.0);
    assert!(rep.position.max_abs_diff > 0.0);
    assert!(rep.energy.max_abs_diff > 0.0);
}

// ── New tests for ValidationConfig ───────────────────────────────────────────

#[test]
fn identical_files_pass_strict_config() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.csv");
    write_csv(&p, &["0,e1,0,20,0,0,1,0", "1,e1,0,20,20,1,1,0"]);

    let rep = compare_csv_files_with_config(&p, &p, &ValidationConfig::strict())
        .expect("compare identical");
    assert!(rep.pass, "identical files must pass strict config");
    assert!(rep.velocity_pass);
    assert!(rep.position_pass);
    assert!(rep.energy_pass);
}

#[test]
fn velocity_difference_fails_tight_rms_threshold() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let b = dir.path().join("b.csv");
    // velocity differs by 2 m/s
    write_csv(&a, &["0,e1,0,10,0,0,1,0", "1,e1,0,12,11,1,1,0"]);
    write_csv(&b, &["0,e1,0,8,0,0,1,0", "1,e1,0,10,9,1,1,0"]);

    let config = ValidationConfig {
        max_velocity_rms: Some(0.5), // tighter than actual ~2 m/s difference
        ..Default::default()
    };
    let rep = compare_csv_files_with_config(&a, &b, &config).expect("compare");
    assert!(!rep.velocity_pass, "velocity exceeds RMS threshold");
    assert!(!rep.pass, "overall should fail");
    // position and energy have no threshold → still pass
    assert!(rep.position_pass);
    assert!(rep.energy_pass);
}

#[test]
fn velocity_difference_passes_loose_rms_threshold() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let b = dir.path().join("b.csv");
    write_csv(&a, &["0,e1,0,10,0,0,1,0", "1,e1,0,12,11,1,1,0"]);
    write_csv(&b, &["0,e1,0,10,0,0,1,0", "1,e1,0,12,11,1,1,0"]);

    let config = ValidationConfig {
        max_velocity_rms: Some(1.0),
        ..Default::default()
    };
    let rep = compare_csv_files_with_config(&a, &b, &config).expect("compare");
    assert!(
        rep.velocity_pass,
        "zero difference should pass any threshold"
    );
    assert!(rep.pass);
}

#[test]
fn position_max_abs_threshold_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let b = dir.path().join("b.csv");
    // odometer differs by 50 m in the second row
    write_csv(&a, &["0,e1,0,0,100,0,0,0", "10,e1,0,0,200,0,0,0"]);
    write_csv(&b, &["0,e1,0,0,100,0,0,0", "10,e1,0,0,150,0,0,0"]);

    let config = ValidationConfig {
        max_position_max: Some(10.0), // 50 m > 10 m → fail
        ..Default::default()
    };
    let rep = compare_csv_files_with_config(&a, &b, &config).expect("compare");
    assert!(!rep.position_pass);
    assert!(!rep.pass);
}

#[test]
fn no_thresholds_always_passes() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let b = dir.path().join("b.csv");
    // wildly different runs
    write_csv(&a, &["0,e1,0,100,9999,999,1,0"]);
    write_csv(&b, &["0,e1,0,0,0,0,0,0"]);

    let rep = compare_csv_files_with_config(&a, &b, &ValidationConfig::default()).expect("compare");
    // No thresholds → pass regardless of magnitude.
    assert!(rep.pass, "no thresholds should always pass");
    // But stats are still populated.
    assert!(rep.velocity.max_abs_diff > 0.0);
}

#[test]
fn phase_breakdown_splits_resampled_window() {
    use openrailsrs_validate::{RunTrace, TraceSample, compare_traces_by_phases};

    let mk = |v: f64, d: f64| TraceSample {
        time_s: 0.0,
        velocity_mps: v,
        distance_m: d,
        energy_kwh: None,
        throttle: None,
        brake: None,
    };

    let mut a = RunTrace {
        source: "a".into(),
        samples: Vec::new(),
    };
    let mut b = RunTrace {
        source: "b".into(),
        samples: Vec::new(),
    };
    for t in 0..=40 {
        let tf = t as f64;
        a.samples.push(TraceSample {
            time_s: tf,
            ..mk(10.0 + tf, tf * 10.0)
        });
        b.samples.push(TraceSample {
            time_s: tf,
            ..mk(tf, tf * 9.0)
        });
    }

    let phases = compare_traces_by_phases(&a, &b, &[0.0, 20.0, 40.0], 1.0).expect("phases");
    assert_eq!(phases.len(), 2);
    assert_eq!(phases[0].label, "0–20 s");
    assert!(phases[0].velocity.samples > 0);
    assert!(phases[1].velocity.samples > 0);
    assert!(phases[0].velocity.rms_diff > 0.0);
    assert!(phases[1].velocity.rms_diff > 0.0);
}

#[test]
fn smoke_self_compare_passes_strict() {
    // Compare the smoke scenario CSV against itself → must be exactly zero.
    let csv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/run.csv");
    if !csv.exists() {
        // CSV not generated yet — skip gracefully.
        eprintln!("smoke run.csv not found, skipping self-compare test");
        return;
    }
    let rep = compare_csv_files_with_config(&csv, &csv, &ValidationConfig::strict())
        .expect("self-compare");
    assert!(
        rep.pass,
        "self-compare with strict tolerance must pass (got vel_rms={:.2e})",
        rep.velocity.rms_diff
    );
}
