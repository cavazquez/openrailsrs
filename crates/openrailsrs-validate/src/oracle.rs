//! Acceptance checks for reference traces. Partial overlap cannot certify parity.

use serde::Serialize;

use crate::{RunTrace, ValidateError, ValidationConfig};

#[derive(Debug, Serialize)]
pub struct TraceCoverage {
    pub reference_start_s: f64,
    pub reference_end_s: f64,
    pub candidate_start_s: f64,
    pub candidate_end_s: f64,
    pub covered_fraction: f64,
}

/// Evaluation logs can emit several frames with the same second-resolution
/// clock. Keep the final observation at that second, without sorting or hiding
/// backwards time. The raw, hashed reference file is never changed.
pub fn canonicalize_reference_time(trace: &mut RunTrace) -> Result<usize, ValidateError> {
    let mut canonical: Vec<crate::TraceSample> = Vec::with_capacity(trace.samples.len());
    let mut duplicates = 0;
    for sample in &trace.samples {
        if !sample.time_s.is_finite()
            || canonical
                .last()
                .is_some_and(|last| sample.time_s < last.time_s)
        {
            return Err(ValidateError::Msg(
                "reference clock is invalid or moves backwards".into(),
            ));
        }
        if canonical
            .last()
            .is_some_and(|last| sample.time_s == last.time_s)
        {
            *canonical.last_mut().unwrap() = sample.clone();
            duplicates += 1;
        } else {
            canonical.push(sample.clone());
        }
    }
    trace.samples = canonical;
    Ok(duplicates)
}

pub fn check_oracle_inputs(
    reference: &RunTrace,
    candidate: &RunTrace,
    thresholds: &ValidationConfig,
    endpoint_tolerance_s: f64,
) -> Result<TraceCoverage, ValidateError> {
    if !endpoint_tolerance_s.is_finite() || !(0.0..=1.0).contains(&endpoint_tolerance_s) {
        return Err(ValidateError::Msg(
            "oracle endpoint tolerance must be finite, in [0,1] seconds".into(),
        ));
    }
    if thresholds.max_velocity_rms.is_none() {
        return Err(ValidateError::Msg(
            "an oracle must declare max_velocity_rms".into(),
        ));
    }
    let limits = [
        thresholds.max_velocity_rms,
        thresholds.max_velocity_max,
        thresholds.max_position_rms,
        thresholds.max_position_max,
        thresholds.max_energy_rms,
        thresholds.max_energy_max,
        thresholds.max_throttle_rms,
        thresholds.max_throttle_max,
        thresholds.max_brake_rms,
        thresholds.max_brake_max,
    ];
    if limits
        .into_iter()
        .flatten()
        .any(|limit| !limit.is_finite() || limit < 0.0)
    {
        return Err(ValidateError::Msg(
            "oracle tolerances must be finite and non-negative".into(),
        ));
    }
    for trace in [reference, candidate] {
        if trace.samples.len() < 2 {
            return Err(ValidateError::Msg(
                "oracle trace needs at least two samples".into(),
            ));
        }
        for (index, sample) in trace.samples.iter().enumerate() {
            if !sample.time_s.is_finite()
                || !sample.velocity_mps.is_finite()
                || !sample.distance_m.is_finite()
                || [sample.energy_kwh, sample.throttle, sample.brake]
                    .into_iter()
                    .flatten()
                    .any(|v| !v.is_finite())
                || (index > 0 && sample.time_s <= trace.samples[index - 1].time_s)
            {
                return Err(ValidateError::Msg(format!(
                    "invalid/non-monotonic oracle sample {index} in {}",
                    trace.source
                )));
            }
            for (required, value, name) in [
                (
                    thresholds.max_energy_rms.is_some() || thresholds.max_energy_max.is_some(),
                    sample.energy_kwh,
                    "energy",
                ),
                (
                    thresholds.max_throttle_rms.is_some() || thresholds.max_throttle_max.is_some(),
                    sample.throttle,
                    "throttle",
                ),
                (
                    thresholds.max_brake_rms.is_some() || thresholds.max_brake_max.is_some(),
                    sample.brake,
                    "brake",
                ),
            ] {
                if required && value.is_none() {
                    return Err(ValidateError::Msg(format!(
                        "oracle requires {name} at every sample in {}",
                        trace.source
                    )));
                }
            }
        }
    }
    let r0 = reference.samples[0].time_s;
    let r1 = reference.samples.last().unwrap().time_s;
    let c0 = candidate.samples[0].time_s;
    let c1 = candidate.samples.last().unwrap().time_s;
    let covered = (r1.min(c1) - r0.max(c0)).max(0.0) / (r1 - r0);
    if c0 > r0 + endpoint_tolerance_s || c1 < r1 - endpoint_tolerance_s || covered < 0.98 {
        return Err(ValidateError::Msg(format!(
            "oracle coverage incomplete: reference [{r0:.3},{r1:.3}], candidate [{c0:.3},{c1:.3}] ({:.1}%)",
            covered * 100.0
        )));
    }
    Ok(TraceCoverage {
        reference_start_s: r0,
        reference_end_s: r1,
        candidate_start_s: c0,
        candidate_end_s: c1,
        covered_fraction: covered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TraceSample;

    fn trace(end: usize) -> RunTrace {
        RunTrace {
            source: "fixture".into(),
            samples: (0..=end)
                .map(|time| TraceSample {
                    time_s: time as f64,
                    velocity_mps: 10.0,
                    distance_m: time as f64 * 10.0,
                    energy_kwh: None,
                    throttle: None,
                    brake: None,
                })
                .collect(),
        }
    }

    fn budget() -> ValidationConfig {
        ValidationConfig {
            max_velocity_rms: Some(0.55),
            ..Default::default()
        }
    }

    #[test]
    fn a_matching_short_excerpt_cannot_certify_a_long_run() {
        assert!(check_oracle_inputs(&trace(120), &trace(10), &budget(), 1.0).is_err());
        assert!(check_oracle_inputs(&trace(120), &trace(120), &budget(), 1.0).is_ok());
    }

    #[test]
    fn missing_configured_columns_and_nan_are_rejected() {
        let mut limits = budget();
        limits.max_brake_rms = Some(0.1);
        assert!(check_oracle_inputs(&trace(10), &trace(10), &limits, 1.0).is_err());
        let mut bad = trace(10);
        bad.samples[5].velocity_mps = f64::NAN;
        assert!(check_oracle_inputs(&trace(10), &bad, &budget(), 1.0).is_err());
        limits = budget();
        limits.max_velocity_rms = Some(f64::INFINITY);
        assert!(check_oracle_inputs(&trace(10), &trace(10), &limits, 1.0).is_err());
    }

    #[test]
    fn non_monotonic_timestamps_are_rejected() {
        let mut bad = trace(10);
        bad.samples[5].time_s = 3.0;
        assert!(check_oracle_inputs(&trace(10), &bad, &budget(), 1.0).is_err());
    }

    #[test]
    fn reference_clock_keeps_last_duplicate_without_hiding_backwards_time() {
        let mut reference = trace(10);
        let mut last = reference.samples[0].clone();
        last.throttle = Some(0.8);
        reference.samples.insert(1, last);
        assert_eq!(canonicalize_reference_time(&mut reference).unwrap(), 1);
        assert_eq!(reference.samples[0].throttle, Some(0.8));
        reference.samples[5].time_s = 2.0;
        assert!(canonicalize_reference_time(&mut reference).is_err());
    }
}
