use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ValidateError;
use crate::trace::parse_openrailsrs_run_csv;

// ── Stats per column ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize)]
pub struct SeriesStats {
    pub max_abs_diff: f64,
    pub mean_abs_diff: f64,
    pub rms_diff: f64,
    pub samples: u64,
}

// ── Tolerance thresholds ──────────────────────────────────────────────────────

/// Configurable per-column tolerance thresholds.
///
/// Any `None` field means "no limit" (that metric always passes).
///
/// ```toml
/// [validate]
/// max_velocity_rms   = 0.5   # m/s
/// max_position_max   = 10.0  # m
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ValidationConfig {
    /// Maximum allowed RMS error for `velocity_mps` (m/s).
    #[serde(default)]
    pub max_velocity_rms: Option<f64>,
    /// Maximum allowed peak absolute error for `velocity_mps` (m/s).
    #[serde(default)]
    pub max_velocity_max: Option<f64>,
    /// Maximum allowed RMS error for `odometer_m` (m).
    #[serde(default)]
    pub max_position_rms: Option<f64>,
    /// Maximum allowed peak absolute error for `odometer_m` (m).
    #[serde(default)]
    pub max_position_max: Option<f64>,
    /// Maximum allowed RMS error for `cumulative_energy_kwh` (kWh).
    #[serde(default)]
    pub max_energy_rms: Option<f64>,
    /// Maximum allowed peak absolute error for `cumulative_energy_kwh` (kWh).
    #[serde(default)]
    pub max_energy_max: Option<f64>,
}

impl Default for ValidationConfig {
    /// No thresholds set — every comparison passes by default.
    fn default() -> Self {
        Self {
            max_velocity_rms: None,
            max_velocity_max: None,
            max_position_rms: None,
            max_position_max: None,
            max_energy_rms: None,
            max_energy_max: None,
        }
    }
}

impl ValidationConfig {
    /// Returns a config that fails if any difference exceeds strict thresholds
    /// suitable for deterministic self-comparison (same seed, same scenario).
    pub fn strict() -> Self {
        Self {
            max_velocity_rms: Some(1e-6),
            max_velocity_max: Some(1e-6),
            max_position_rms: Some(1e-6),
            max_position_max: Some(1e-6),
            max_energy_rms: Some(1e-6),
            max_energy_max: Some(1e-6),
        }
    }
}

// ── Report ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ComparisonReport {
    pub file_a: String,
    pub file_b: String,
    pub time_alignment: String,
    /// Statistics for `velocity_mps`.
    pub velocity: SeriesStats,
    /// Statistics for `odometer_m`.
    pub position: SeriesStats,
    /// Statistics for `cumulative_energy_kwh`.
    pub energy: SeriesStats,
    /// `true` when every configured tolerance is satisfied (or no tolerances set).
    pub pass: bool,
    pub velocity_pass: bool,
    pub position_pass: bool,
    pub energy_pass: bool,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Compare two run CSVs using default (permissive) tolerances — always passes.
pub fn compare_csv_files(a: &Path, b: &Path) -> Result<ComparisonReport, ValidateError> {
    compare_csv_files_with_config(a, b, &ValidationConfig::default())
}

/// Compare two run CSVs, checking per-column tolerances from `config`.
pub fn compare_csv_files_with_config(
    a: &Path,
    b: &Path,
    config: &ValidationConfig,
) -> Result<ComparisonReport, ValidateError> {
    let rows_a = parse_openrailsrs_run_csv(a)?;
    let rows_b = parse_openrailsrs_run_csv(b)?;
    if rows_a.samples.len() != rows_b.samples.len() {
        return Err(ValidateError::Msg(format!(
            "row count mismatch: {} vs {}",
            rows_a.samples.len(),
            rows_b.samples.len()
        )));
    }
    let eps = 1e-4;
    let mut vel = SeriesStats::default();
    let mut pos = SeriesStats::default();
    let mut ene = SeriesStats::default();
    for (ra, rb) in rows_a.samples.iter().zip(rows_b.samples.iter()) {
        if (ra.time_s - rb.time_s).abs() > eps {
            return Err(ValidateError::Msg(format!(
                "time mismatch at row: {} vs {}",
                ra.time_s, rb.time_s
            )));
        }
        accumulate(&mut vel, ra.velocity_mps - rb.velocity_mps);
        accumulate(&mut pos, ra.distance_m - rb.distance_m);
        let ea = ra.energy_kwh.unwrap_or(0.0);
        let eb = rb.energy_kwh.unwrap_or(0.0);
        accumulate(&mut ene, ea - eb);
    }
    finalize_stats(&mut vel);
    finalize_stats(&mut pos);
    finalize_stats(&mut ene);

    let vel_pass = column_passes(&vel, config.max_velocity_rms, config.max_velocity_max);
    let pos_pass = column_passes(&pos, config.max_position_rms, config.max_position_max);
    let ene_pass = column_passes(&ene, config.max_energy_rms, config.max_energy_max);

    Ok(ComparisonReport {
        file_a: a.display().to_string(),
        file_b: b.display().to_string(),
        time_alignment: "by_row_same_time_s".into(),
        velocity: vel,
        position: pos,
        energy: ene,
        pass: vel_pass && pos_pass && ene_pass,
        velocity_pass: vel_pass,
        position_pass: pos_pass,
        energy_pass: ene_pass,
    })
}

/// Compare two normalized traces after resampling onto a common grid.
pub fn compare_traces(
    a: &crate::trace::RunTrace,
    b: &crate::trace::RunTrace,
    config: &ValidationConfig,
    step_s: f64,
) -> Result<ComparisonReport, ValidateError> {
    let (ra, rb) = crate::trace::resample_traces(a, b, step_s)?;
    let mut vel = SeriesStats::default();
    let mut pos = SeriesStats::default();
    let mut ene = SeriesStats::default();

    let both_have_energy = a.samples.iter().any(|s| s.energy_kwh.is_some())
        && b.samples.iter().any(|s| s.energy_kwh.is_some());

    for (sa, sb) in ra.iter().zip(rb.iter()) {
        accumulate(&mut vel, sa.velocity_mps - sb.velocity_mps);
        accumulate(&mut pos, sa.distance_m - sb.distance_m);
        if both_have_energy {
            let ea = sa.energy_kwh.unwrap_or(0.0);
            let eb = sb.energy_kwh.unwrap_or(0.0);
            accumulate(&mut ene, ea - eb);
        }
    }
    finalize_stats(&mut vel);
    finalize_stats(&mut pos);
    if both_have_energy {
        finalize_stats(&mut ene);
    }

    let vel_pass = column_passes(&vel, config.max_velocity_rms, config.max_velocity_max);
    let pos_pass = column_passes(&pos, config.max_position_rms, config.max_position_max);
    let ene_pass = if both_have_energy {
        column_passes(&ene, config.max_energy_rms, config.max_energy_max)
    } else {
        true
    };

    Ok(ComparisonReport {
        file_a: a.source.clone(),
        file_b: b.source.clone(),
        time_alignment: format!("resampled_linear_step_{step_s}s"),
        velocity: vel,
        position: pos,
        energy: ene,
        pass: vel_pass && pos_pass && ene_pass,
        velocity_pass: vel_pass,
        position_pass: pos_pass,
        energy_pass: ene_pass,
    })
}

/// Compare an Open Rails dump against an openrailsrs run CSV.
pub fn compare_or_dump_with_run(
    or_dump: &Path,
    run_csv: &Path,
    map: &crate::trace::OrColumnMap,
    config: &ValidationConfig,
    step_s: f64,
) -> Result<ComparisonReport, ValidateError> {
    let or_trace = crate::trace::parse_or_dump_csv(or_dump, map)?;
    let rs_trace = parse_openrailsrs_run_csv(run_csv)?;
    compare_traces(&or_trace, &rs_trace, config, step_s)
}

// ── Shared stats helpers (used by trace comparison) ───────────────────────────

pub(crate) fn accumulate(s: &mut SeriesStats, diff: f64) {
    let ad = diff.abs();
    s.max_abs_diff = s.max_abs_diff.max(ad);
    s.mean_abs_diff += ad;
    s.rms_diff += diff * diff;
    s.samples += 1;
}

pub(crate) fn finalize_stats(s: &mut SeriesStats) {
    if s.samples == 0 {
        return;
    }
    s.mean_abs_diff /= s.samples as f64;
    s.rms_diff = (s.rms_diff / s.samples as f64).sqrt();
}

pub(crate) fn column_passes(s: &SeriesStats, max_rms: Option<f64>, max_abs: Option<f64>) -> bool {
    if let Some(limit) = max_rms {
        if s.rms_diff > limit {
            return false;
        }
    }
    if let Some(limit) = max_abs {
        if s.max_abs_diff > limit {
            return false;
        }
    }
    true
}
