//! Smoke test for `openrailsrs compare-or`.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_openrailsrs"))
}

fn validate_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("openrailsrs-validate")
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn compare_or_runs_on_synthetic_fixtures() {
    let out = Command::new(bin())
        .args([
            "compare-or",
            validate_fixture("or_dump_minimal.csv").to_str().unwrap(),
            validate_fixture("ors_run_aligned.csv").to_str().unwrap(),
            "--max-velocity-rms",
            "1e-6",
            "--max-position-max",
            "1e-6",
        ])
        .output()
        .expect("run compare-or");
    assert!(
        out.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Compare OR"));
    assert!(stdout.contains("overall: PASS"));
}

#[test]
fn compare_or_parses_chiltern_evaluation_baseline() {
    let baseline = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/baselines/chiltern_birmingham/or_evaluation_speed.csv");
    if !baseline.is_file() {
        return;
    }
    let out = Command::new(bin())
        .args([
            "compare-or",
            baseline.to_str().unwrap(),
            validate_fixture("ors_run_aligned.csv").to_str().unwrap(),
        ])
        .output()
        .expect("run compare-or on chiltern eval");
    assert!(
        out.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Compare OR"));
}

#[test]
fn compare_or_parses_or_performance_dump_subset() {
    let out = Command::new(bin())
        .args([
            "compare-or",
            validate_fixture("or_perf_subset.csv").to_str().unwrap(),
            validate_fixture("ors_run_aligned.csv").to_str().unwrap(),
        ])
        .output()
        .expect("run compare-or on perf subset");
    assert!(
        out.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn or_eval_driver_writes_csv() {
    let out_csv = std::env::temp_dir().join("openrailsrs_or_eval_driver_test.csv");
    let output = Command::new(bin())
        .args([
            "or-eval-driver",
            validate_fixture("or_eval_speed_minimal.csv")
                .to_str()
                .unwrap(),
            "--out",
            out_csv.to_str().unwrap(),
        ])
        .output()
        .expect("run or-eval-driver");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&out_csv).expect("read driver csv");
    assert!(text.contains("time_s,throttle,brake"));
    let _ = std::fs::remove_file(out_csv);
}
