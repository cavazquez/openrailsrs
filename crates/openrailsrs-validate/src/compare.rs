use std::path::Path;

use csv::ReaderBuilder;
use serde::{Deserialize, Serialize};

use crate::ValidateError;

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
    let rows_a = load_numeric_rows(a)?;
    let rows_b = load_numeric_rows(b)?;
    if rows_a.len() != rows_b.len() {
        return Err(ValidateError::Msg(format!(
            "row count mismatch: {} vs {}",
            rows_a.len(),
            rows_b.len()
        )));
    }
    let eps = 1e-4;
    let mut vel = SeriesStats::default();
    let mut pos = SeriesStats::default();
    let mut ene = SeriesStats::default();
    for (ra, rb) in rows_a.iter().zip(rows_b.iter()) {
        if (ra.time - rb.time).abs() > eps {
            return Err(ValidateError::Msg(format!(
                "time mismatch at row: {} vs {}",
                ra.time, rb.time
            )));
        }
        accumulate(&mut vel, ra.velocity - rb.velocity);
        accumulate(&mut pos, ra.odometer - rb.odometer);
        accumulate(&mut ene, ra.energy - rb.energy);
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

// ── Internal helpers ──────────────────────────────────────────────────────────

struct Row {
    time: f64,
    velocity: f64,
    odometer: f64,
    energy: f64,
}

fn load_numeric_rows(path: &Path) -> Result<Vec<Row>, ValidateError> {
    let mut rdr = ReaderBuilder::new().has_headers(true).from_path(path)?;
    let headers = rdr.headers()?.clone();
    let idx = |name: &str| {
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .ok_or_else(|| ValidateError::Msg(format!("missing column {name}")))
    };
    let i_t = idx("time_s")?;
    let i_v = idx("velocity_mps")?;
    let i_o = idx("odometer_m")?;
    let i_e = idx("cumulative_energy_kwh")?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        out.push(Row {
            time: rec.get(i_t).unwrap_or("0").parse().unwrap_or(0.0),
            velocity: rec.get(i_v).unwrap_or("0").parse().unwrap_or(0.0),
            odometer: rec.get(i_o).unwrap_or("0").parse().unwrap_or(0.0),
            energy: rec.get(i_e).unwrap_or("0").parse().unwrap_or(0.0),
        });
    }
    Ok(out)
}

fn accumulate(s: &mut SeriesStats, diff: f64) {
    let ad = diff.abs();
    s.max_abs_diff = s.max_abs_diff.max(ad);
    s.mean_abs_diff += ad;
    s.rms_diff += diff * diff;
    s.samples += 1;
}

fn finalize_stats(s: &mut SeriesStats) {
    if s.samples == 0 {
        return;
    }
    s.mean_abs_diff /= s.samples as f64;
    s.rms_diff = (s.rms_diff / s.samples as f64).sqrt();
}

/// Check whether a column's stats satisfy the given optional thresholds.
fn column_passes(s: &SeriesStats, max_rms: Option<f64>, max_abs: Option<f64>) -> bool {
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
