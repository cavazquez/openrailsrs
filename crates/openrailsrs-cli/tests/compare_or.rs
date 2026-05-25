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
