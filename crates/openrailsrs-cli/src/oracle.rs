//! Fixed-reference physics acceptance suite; outputs never overwrite scenarios.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_validate::{
    OrColumnMap, ValidationConfig, compare_traces, compare_traces_by_phases,
    normalize_trace_brake_to_fraction, parse_openrailsrs_run_csv, parse_or_dump_csv,
    phase_report_passes,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Suite {
    reference_version: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    scenario: PathBuf,
    driver: PathBuf,
    baseline: PathBuf,
    thresholds: ValidationConfig,
    #[serde(default)]
    phase_bounds: Vec<f64>,
    #[serde(default)]
    phase_max_velocity_rms: Option<f64>,
}

#[derive(Serialize)]
struct CaseReport {
    id: String,
    pass: bool,
    error: Option<String>,
    coverage: Option<openrailsrs_validate::oracle::TraceCoverage>,
    comparison: Option<openrailsrs_validate::ComparisonReport>,
    phases: Vec<openrailsrs_validate::PhaseReport>,
    reference_duplicate_times: usize,
}

fn run_case(root: &Path, case: &Case, out_dir: &Path) -> Result<CaseReport> {
    anyhow::ensure!(
        !case.id.is_empty()
            && case
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "invalid oracle case id"
    );
    let scenario_path = root.join(&case.scenario);
    let scenario_dir = scenario_path.parent().context("scenario parent")?;
    let mut scenario = openrailsrs_scenarios::load_scenario(&scenario_path)?;
    openrailsrs_scenarios::apply_scenario_runtime_overlay_dir(&mut scenario, scenario_dir)?;
    let output = out_dir.join(&case.id);
    std::fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    scenario.output.csv = output.join("run.csv").display().to_string();
    scenario.output.metadata = output.join("run.json").display().to_string();
    let mut driver = ScriptedDriver::from_csv(root.join(&case.driver))?;
    run_scenario_headless_with_driver(scenario_dir, &scenario, &mut driver)?;
    let mut reference = parse_or_dump_csv(&root.join(&case.baseline), &OrColumnMap::default())?;
    let reference_duplicate_times =
        openrailsrs_validate::oracle::canonicalize_reference_time(&mut reference)?;
    normalize_trace_brake_to_fraction(&mut reference, None);
    let candidate = parse_openrailsrs_run_csv(&output.join("run.csv"))?;
    let coverage = openrailsrs_validate::oracle::check_oracle_inputs(
        &reference,
        &candidate,
        &case.thresholds,
        1.0,
    )?;
    if !case.phase_bounds.is_empty() {
        anyhow::ensure!(
            case.phase_bounds.len() >= 2
                && case.phase_bounds.iter().all(|t| t.is_finite())
                && case.phase_bounds.windows(2).all(|w| w[0] < w[1])
                && (case.phase_bounds[0] - coverage.reference_start_s).abs() <= 1.0
                && (case.phase_bounds.last().unwrap() - coverage.reference_end_s).abs() <= 1.0,
            "oracle phase boundaries must cover the full reference in increasing order"
        );
        anyhow::ensure!(
            case.phase_max_velocity_rms
                .is_some_and(|limit| limit.is_finite() && limit >= 0.0),
            "a phased oracle needs a finite velocity RMS budget"
        );
    }
    let comparison = compare_traces(&reference, &candidate, &case.thresholds, 0.1)?;
    let phases = if case.phase_bounds.is_empty() {
        Vec::new()
    } else {
        compare_traces_by_phases(&reference, &candidate, &case.phase_bounds, 0.1)?
    };
    let phase_budget = ValidationConfig {
        max_velocity_rms: case.phase_max_velocity_rms,
        max_throttle_rms: None,
        max_throttle_max: None,
        max_brake_rms: None,
        max_brake_max: None,
        ..case.thresholds.clone()
    };
    let pass = comparison.pass
        && phases
            .iter()
            .all(|phase| phase_report_passes(phase, &phase_budget));
    Ok(CaseReport {
        id: case.id.clone(),
        pass,
        error: None,
        coverage: Some(coverage),
        comparison: Some(comparison),
        phases,
        reference_duplicate_times,
    })
}

pub fn run(manifest: &Path, out_dir: &Path) -> Result<bool> {
    let suite: Suite = toml::from_str(&std::fs::read_to_string(manifest)?)?;
    anyhow::ensure!(!suite.cases.is_empty(), "oracle suite is empty");
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .context("manifest must live in repository/oracles")?;
    std::fs::create_dir_all(out_dir)?;
    let mut results = Vec::new();
    for case in &suite.cases {
        let result = run_case(root, case, out_dir).unwrap_or_else(|error| CaseReport {
            id: case.id.clone(),
            pass: false,
            error: Some(format!("{error:#}")),
            coverage: None,
            comparison: None,
            phases: Vec::new(),
            reference_duplicate_times: 0,
        });
        if let Some(report) = &result.comparison {
            println!(
                "{} {}: velocity RMS {:.4} m/s, peak {:.4} m/s{}",
                if result.pass { "PASS" } else { "FAIL" },
                result.id,
                report.velocity.rms_diff,
                report.velocity.max_abs_diff,
                if case.thresholds.max_position_max.is_some()
                    || case.thresholds.max_position_rms.is_some()
                {
                    format!(", position peak {:.2} m", report.position.max_abs_diff)
                } else {
                    String::from(" (speed/controls oracle; distance unavailable)")
                }
            );
        } else {
            println!(
                "FAIL {}: {}",
                result.id,
                result.error.as_deref().unwrap_or("invalid reference")
            );
        }
        results.push(result);
    }
    let pass = results.iter().all(|case| case.pass);
    std::fs::write(
        out_dir.join("report.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "reference_version": suite.reference_version, "pass": pass, "cases": results,
        }))?,
    )?;
    Ok(pass)
}
