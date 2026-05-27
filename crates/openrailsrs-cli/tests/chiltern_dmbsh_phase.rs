//! OR-P13: Chiltern phase 40–65 s (second motor / acceleration window).

use std::path::PathBuf;

use openrailsrs_validate::{OrColumnMap, compare_or_dump_phases, phase_report_passes};

#[test]
fn chiltern_phase_40_65s_within_or_threshold() {
    let chiltern = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    if !chiltern.join("track.toml").exists() {
        return;
    }
    let baseline = chiltern.join("../baselines/chiltern_birmingham/or_evaluation_speed.csv");
    let run_csv = chiltern.join("run.csv");
    if !run_csv.exists() {
        return;
    }

    let phases = compare_or_dump_phases(
        &baseline,
        &run_csv,
        &OrColumnMap::default(),
        &[0.0, 40.0, 65.0, 136.0],
        0.1,
    )
    .expect("compare phases");

    let phase = phases
        .iter()
        .find(|p| p.label.contains("40") && p.label.contains("65"))
        .expect("40-65 s phase");
    let config = openrailsrs_validate::ValidationConfig {
        max_velocity_rms: Some(0.42),
        ..Default::default()
    };
    assert!(
        phase_report_passes(phase, &config),
        "OR-P13 phase 40-65 s failed: vel_rms={:.3} max={:.3}",
        phase.velocity.rms_diff,
        phase.velocity.max_abs_diff
    );
}
