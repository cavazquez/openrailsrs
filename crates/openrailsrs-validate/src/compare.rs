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
    /// Maximum allowed RMS error for `throttle` (0–1).
    #[serde(default)]
    pub max_throttle_rms: Option<f64>,
    /// Maximum allowed peak absolute error for `throttle` (0–1).
    #[serde(default)]
    pub max_throttle_max: Option<f64>,
    /// Maximum allowed RMS error for `brake` (0–1).
    #[serde(default)]
    pub max_brake_rms: Option<f64>,
    /// Maximum allowed peak absolute error for `brake` (0–1).
    #[serde(default)]
    pub max_brake_max: Option<f64>,
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
            max_throttle_rms: None,
            max_throttle_max: None,
            max_brake_rms: None,
            max_brake_max: None,
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
            max_throttle_rms: Some(1e-6),
            max_throttle_max: Some(1e-6),
            max_brake_rms: Some(1e-6),
            max_brake_max: Some(1e-6),
        }
    }
}

// ── Report ────────────────────────────────────────────────────────────────────

/// Per-window statistics for phased OR vs sim diagnostics.
#[derive(Debug, Serialize)]
pub struct PhaseReport {
    pub label: String,
    pub t_start_s: f64,
    pub t_end_s: f64,
    pub velocity: SeriesStats,
    pub position: SeriesStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<SeriesStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brake: Option<SeriesStats>,
}

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
    /// Statistics for `throttle` when both traces carry the column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<SeriesStats>,
    /// Statistics for `brake` when both traces carry the column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brake: Option<SeriesStats>,
    /// `true` when every configured tolerance is satisfied (or no tolerances set).
    pub pass: bool,
    pub velocity_pass: bool,
    pub position_pass: bool,
    pub energy_pass: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle_pass: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brake_pass: Option<bool>,
}

/// Difference snapshot at one explicit checkpoint time.
#[derive(Debug, Clone, Serialize)]
pub struct CheckpointDiff {
    pub time_s: f64,
    pub or_velocity_mps: f64,
    pub sim_velocity_mps: f64,
    pub velocity_abs_diff: f64,
    pub or_distance_m: f64,
    pub sim_distance_m: f64,
    pub position_abs_diff: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub or_throttle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_throttle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle_abs_diff: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub or_brake: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_brake: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brake_abs_diff: Option<f64>,
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
        throttle: None,
        brake: None,
        pass: vel_pass && pos_pass && ene_pass,
        velocity_pass: vel_pass,
        position_pass: pos_pass,
        energy_pass: ene_pass,
        throttle_pass: None,
        brake_pass: None,
    })
}

struct TraceDiffContext {
    both_have_energy: bool,
    both_have_throttle: bool,
    both_have_brake: bool,
}

#[derive(Default)]
struct TraceDiffAccum {
    velocity: SeriesStats,
    position: SeriesStats,
    energy: SeriesStats,
    throttle: SeriesStats,
    brake: SeriesStats,
}

fn trace_diff_context(a: &crate::trace::RunTrace, b: &crate::trace::RunTrace) -> TraceDiffContext {
    TraceDiffContext {
        both_have_energy: a.samples.iter().any(|s| s.energy_kwh.is_some())
            && b.samples.iter().any(|s| s.energy_kwh.is_some()),
        both_have_throttle: trace_has_throttle(a) && trace_has_throttle(b),
        both_have_brake: trace_has_brake(a) && trace_has_brake(b),
    }
}

fn accumulate_resampled_pair(
    accum: &mut TraceDiffAccum,
    ctx: &TraceDiffContext,
    sa: &crate::trace::TraceSample,
    sb: &crate::trace::TraceSample,
) {
    accumulate(&mut accum.velocity, sa.velocity_mps - sb.velocity_mps);
    accumulate(&mut accum.position, sa.distance_m - sb.distance_m);
    if ctx.both_have_energy {
        let ea = sa.energy_kwh.unwrap_or(0.0);
        let eb = sb.energy_kwh.unwrap_or(0.0);
        accumulate(&mut accum.energy, ea - eb);
    }
    if ctx.both_have_throttle {
        let ta = sa.throttle.unwrap_or(0.0);
        let tb = sb.throttle.unwrap_or(0.0);
        accumulate(&mut accum.throttle, ta - tb);
    }
    if ctx.both_have_brake {
        let ba = sa.brake.unwrap_or(0.0);
        let bb = sb.brake.unwrap_or(0.0);
        accumulate(&mut accum.brake, ba - bb);
    }
}

fn finalize_trace_diff(
    mut accum: TraceDiffAccum,
    ctx: &TraceDiffContext,
) -> (
    SeriesStats,
    SeriesStats,
    SeriesStats,
    Option<SeriesStats>,
    Option<SeriesStats>,
) {
    finalize_stats(&mut accum.velocity);
    finalize_stats(&mut accum.position);
    if ctx.both_have_energy {
        finalize_stats(&mut accum.energy);
    }
    let throttle_stats = if ctx.both_have_throttle {
        finalize_stats(&mut accum.throttle);
        Some(accum.throttle)
    } else {
        None
    };
    let brake_stats = if ctx.both_have_brake {
        finalize_stats(&mut accum.brake);
        Some(accum.brake)
    } else {
        None
    };
    (
        accum.velocity,
        accum.position,
        accum.energy,
        throttle_stats,
        brake_stats,
    )
}

fn sample_in_phase(t: f64, t_start: f64, t_end: f64, last_phase: bool) -> bool {
    if t < t_start {
        return false;
    }
    if last_phase {
        t <= t_end + 1e-9
    } else {
        t < t_end
    }
}

/// Split a resampled OR vs sim comparison into time windows.
///
/// `boundaries` must be strictly increasing with at least two values, e.g. `[0.0, 20.0, 65.0]`
/// yields phases `[0, 20)` and `[20, 65]`.
pub fn compare_traces_by_phases(
    a: &crate::trace::RunTrace,
    b: &crate::trace::RunTrace,
    boundaries: &[f64],
    step_s: f64,
) -> Result<Vec<PhaseReport>, ValidateError> {
    if boundaries.len() < 2 {
        return Err(ValidateError::Msg(
            "phase_bounds needs at least two values (e.g. 0,20,65)".into(),
        ));
    }
    for w in boundaries.windows(2) {
        if w[1] <= w[0] {
            return Err(ValidateError::Msg(format!(
                "phase_bounds must be strictly increasing (got {} then {})",
                w[0], w[1]
            )));
        }
    }

    let (ra, rb) = crate::trace::resample_traces(a, b, step_s)?;
    let ctx = trace_diff_context(a, b);
    let phase_count = boundaries.len() - 1;
    let mut reports = Vec::with_capacity(phase_count);

    for i in 0..phase_count {
        let t_start = boundaries[i];
        let t_end = boundaries[i + 1];
        let last_phase = i + 1 == phase_count;
        let mut accum = TraceDiffAccum::default();

        for (sa, sb) in ra.iter().zip(rb.iter()) {
            if sample_in_phase(sa.time_s, t_start, t_end, last_phase) {
                accumulate_resampled_pair(&mut accum, &ctx, sa, sb);
            }
        }

        if accum.velocity.samples == 0 {
            return Err(ValidateError::Msg(format!(
                "phase {t_start}–{t_end} s has no resampled samples (step={step_s})"
            )));
        }

        let (velocity, position, _energy, throttle, brake) = finalize_trace_diff(accum, &ctx);
        reports.push(PhaseReport {
            label: format!("{t_start:.0}–{t_end:.0} s"),
            t_start_s: t_start,
            t_end_s: t_end,
            velocity,
            position,
            throttle,
            brake,
        });
    }

    Ok(reports)
}

/// Compare two normalized traces after resampling onto a common grid.
pub fn compare_traces(
    a: &crate::trace::RunTrace,
    b: &crate::trace::RunTrace,
    config: &ValidationConfig,
    step_s: f64,
) -> Result<ComparisonReport, ValidateError> {
    let (ra, rb) = crate::trace::resample_traces(a, b, step_s)?;
    let ctx = trace_diff_context(a, b);
    let mut accum = TraceDiffAccum::default();

    for (sa, sb) in ra.iter().zip(rb.iter()) {
        accumulate_resampled_pair(&mut accum, &ctx, sa, sb);
    }

    let (vel, pos, ene, throttle_stats, brake_stats) = finalize_trace_diff(accum, &ctx);

    let vel_pass = column_passes(&vel, config.max_velocity_rms, config.max_velocity_max);
    let pos_pass = column_passes(&pos, config.max_position_rms, config.max_position_max);
    let ene_pass = if ctx.both_have_energy {
        column_passes(&ene, config.max_energy_rms, config.max_energy_max)
    } else {
        true
    };
    let throttle_pass = throttle_stats
        .as_ref()
        .map(|s| column_passes(s, config.max_throttle_rms, config.max_throttle_max));
    let brake_pass = brake_stats
        .as_ref()
        .map(|s| column_passes(s, config.max_brake_rms, config.max_brake_max));

    let pass = vel_pass
        && pos_pass
        && ene_pass
        && throttle_pass.unwrap_or(true)
        && brake_pass.unwrap_or(true);

    Ok(ComparisonReport {
        file_a: a.source.clone(),
        file_b: b.source.clone(),
        time_alignment: format!("resampled_linear_step_{step_s}s"),
        velocity: vel,
        position: pos,
        energy: ene,
        throttle: throttle_stats,
        brake: brake_stats,
        pass,
        velocity_pass: vel_pass,
        position_pass: pos_pass,
        energy_pass: ene_pass,
        throttle_pass,
        brake_pass,
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
    let mut or_trace = crate::trace::parse_or_dump_csv(or_dump, map)?;
    crate::trace::normalize_trace_brake_to_fraction(&mut or_trace, None);
    let rs_trace = parse_openrailsrs_run_csv(run_csv)?;
    compare_traces(&or_trace, &rs_trace, config, step_s)
}

/// Phased OR vs sim diagnostic on resampled traces.
pub fn compare_or_dump_phases(
    or_dump: &Path,
    run_csv: &Path,
    map: &crate::trace::OrColumnMap,
    boundaries: &[f64],
    step_s: f64,
) -> Result<Vec<PhaseReport>, ValidateError> {
    let mut or_trace = crate::trace::parse_or_dump_csv(or_dump, map)?;
    crate::trace::normalize_trace_brake_to_fraction(&mut or_trace, None);
    let rs_trace = parse_openrailsrs_run_csv(run_csv)?;
    compare_traces_by_phases(&or_trace, &rs_trace, boundaries, step_s)
}

/// Compare OR vs sim at explicit checkpoint times (seconds).
pub fn compare_or_dump_checkpoints(
    or_dump: &Path,
    run_csv: &Path,
    map: &crate::trace::OrColumnMap,
    checkpoints_s: &[f64],
    step_s: f64,
) -> Result<Vec<CheckpointDiff>, ValidateError> {
    let mut or_trace = crate::trace::parse_or_dump_csv(or_dump, map)?;
    crate::trace::normalize_trace_brake_to_fraction(&mut or_trace, None);
    let rs_trace = parse_openrailsrs_run_csv(run_csv)?;
    compare_traces_at_checkpoints(&or_trace, &rs_trace, checkpoints_s, step_s)
}

/// Compare two traces at explicit checkpoint times (seconds).
pub fn compare_traces_at_checkpoints(
    a: &crate::trace::RunTrace,
    b: &crate::trace::RunTrace,
    checkpoints_s: &[f64],
    step_s: f64,
) -> Result<Vec<CheckpointDiff>, ValidateError> {
    if checkpoints_s.is_empty() {
        return Ok(Vec::new());
    }
    let mut checkpoints = checkpoints_s.to_vec();
    checkpoints.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    for w in checkpoints.windows(2) {
        if w[1] <= w[0] {
            return Err(ValidateError::Msg(
                "checkpoints must be strictly increasing".into(),
            ));
        }
    }

    let (ra, rb) = crate::trace::resample_traces(a, b, step_s)?;
    let t_min = ra.first().map(|s| s.time_s).unwrap_or(0.0);
    let t_max = ra.last().map(|s| s.time_s).unwrap_or(0.0);
    let mut out = Vec::with_capacity(checkpoints.len());
    for &t in &checkpoints {
        if t < t_min || t > t_max {
            return Err(ValidateError::Msg(format!(
                "checkpoint {t:.3}s out of range [{t_min:.3}, {t_max:.3}]"
            )));
        }
        let Some(sa) = sample_at_time(&ra, t) else {
            return Err(ValidateError::Msg(format!(
                "cannot sample OR trace at checkpoint {t:.3}s"
            )));
        };
        let Some(sb) = sample_at_time(&rb, t) else {
            return Err(ValidateError::Msg(format!(
                "cannot sample sim trace at checkpoint {t:.3}s"
            )));
        };
        let throttle_abs_diff = match (sa.throttle, sb.throttle) {
            (Some(x), Some(y)) => Some((x - y).abs()),
            _ => None,
        };
        let brake_abs_diff = match (sa.brake, sb.brake) {
            (Some(x), Some(y)) => Some((x - y).abs()),
            _ => None,
        };
        out.push(CheckpointDiff {
            time_s: t,
            or_velocity_mps: sa.velocity_mps,
            sim_velocity_mps: sb.velocity_mps,
            velocity_abs_diff: (sa.velocity_mps - sb.velocity_mps).abs(),
            or_distance_m: sa.distance_m,
            sim_distance_m: sb.distance_m,
            position_abs_diff: (sa.distance_m - sb.distance_m).abs(),
            or_throttle: sa.throttle,
            sim_throttle: sb.throttle,
            throttle_abs_diff,
            or_brake: sa.brake,
            sim_brake: sb.brake,
            brake_abs_diff,
        });
    }
    Ok(out)
}

fn sample_at_time(
    samples: &[crate::trace::TraceSample],
    t: f64,
) -> Option<crate::trace::TraceSample> {
    if samples.is_empty() {
        return None;
    }
    if t <= samples[0].time_s {
        return Some(samples[0].clone());
    }
    if t >= samples[samples.len() - 1].time_s {
        return Some(samples[samples.len() - 1].clone());
    }
    let idx = samples.binary_search_by(|s| {
        s.time_s
            .partial_cmp(&t)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    match idx {
        Ok(i) => Some(samples[i].clone()),
        Err(i) if i > 0 && i < samples.len() => {
            let a = &samples[i - 1];
            let b = &samples[i];
            let dt = (b.time_s - a.time_s).max(1e-9);
            let f = ((t - a.time_s) / dt).clamp(0.0, 1.0);
            Some(crate::trace::TraceSample {
                time_s: t,
                velocity_mps: lerp(a.velocity_mps, b.velocity_mps, f),
                distance_m: lerp(a.distance_m, b.distance_m, f),
                energy_kwh: lerp_opt(a.energy_kwh, b.energy_kwh, f),
                throttle: lerp_opt(a.throttle, b.throttle, f),
                brake: lerp_opt(a.brake, b.brake, f),
            })
        }
        _ => None,
    }
}

fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + f * (b - a)
}

fn lerp_opt(a: Option<f64>, b: Option<f64>, f: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(lerp(x, y, f)),
        _ => None,
    }
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

fn trace_has_throttle(t: &crate::trace::RunTrace) -> bool {
    t.samples.iter().any(|s| s.throttle.is_some())
}

fn trace_has_brake(t: &crate::trace::RunTrace) -> bool {
    t.samples.iter().any(|s| s.brake.is_some())
}

pub fn phase_report_passes(phase: &PhaseReport, config: &ValidationConfig) -> bool {
    column_passes(
        &phase.velocity,
        config.max_velocity_rms,
        config.max_velocity_max,
    ) && column_passes(
        &phase.position,
        config.max_position_rms,
        config.max_position_max,
    ) && phase
        .throttle
        .as_ref()
        .map(|s| column_passes(s, config.max_throttle_rms, config.max_throttle_max))
        .unwrap_or(true)
        && phase
            .brake
            .as_ref()
            .map(|s| column_passes(s, config.max_brake_rms, config.max_brake_max))
            .unwrap_or(true)
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
