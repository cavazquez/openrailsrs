//! Diesel/electric traction from ORTS per-notch F(v) curves.

use crate::model::TractiveCurve;

/// ORTS `MaxTractiveForceCurves` / `ORTSMaxTractiveForceCurves` by throttle notch.
#[derive(Clone, Debug, PartialEq)]
pub struct DieselTractionModel {
    /// Sorted `(notch 0..1, F(v))` pairs; linear interpolation between notches.
    pub notch_curves: Vec<(f64, TractiveCurve)>,
    /// Scales curve forces (peak ORTS stall vs continuous `MaxForce`).
    pub effort_scale: f64,
}

impl Default for DieselTractionModel {
    fn default() -> Self {
        Self {
            notch_curves: Vec::new(),
            effort_scale: 1.0,
        }
    }
}

impl DieselTractionModel {
    pub fn from_notch_curves(curves: Vec<(f64, Vec<(f64, f64)>)>) -> Self {
        let mut notch_curves: Vec<(f64, TractiveCurve)> = curves
            .into_iter()
            .filter(|(_, pts)| !pts.is_empty())
            .map(|(notch, pts)| (notch, TractiveCurve { points: pts }))
            .collect();
        notch_curves.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self {
            notch_curves,
            effort_scale: 1.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.notch_curves.is_empty()
    }

    pub fn calibrate_effort_scale(&mut self, max_continuous_force_n: f64) {
        if max_continuous_force_n <= 0.0 || self.notch_curves.is_empty() {
            return;
        }
        let stall = self.force_at_raw(0.0, 1.0);
        if stall > 0.0 {
            // ORTS peak stall ≫ continuous MaxForce; use ~4× continuous as effective ceiling.
            self.effort_scale = ((max_continuous_force_n * 4.0) / stall).clamp(0.05, 1.0);
        }
    }

    /// Tractive effort (N) at speed `v_mps` with driver throttle in `[0, 1]`.
    pub fn force_at(&self, v_mps: f64, throttle: f64) -> f64 {
        self.force_at_raw(v_mps, throttle) * self.effort_scale
    }

    fn force_at_raw(&self, v_mps: f64, throttle: f64) -> f64 {
        if throttle <= 0.0 || self.notch_curves.is_empty() {
            return 0.0;
        }
        let throttle = throttle.clamp(0.0, 1.0);
        let hi = self
            .notch_curves
            .iter()
            .position(|(n, _)| *n >= throttle)
            .unwrap_or(self.notch_curves.len() - 1);
        let lo = hi.saturating_sub(1);
        let (t0, c0) = &self.notch_curves[lo];
        let (t1, c1) = &self.notch_curves[hi];
        let f0 = c0.interpolate(v_mps).unwrap_or(0.0);
        if lo == hi || (t1 - t0).abs() < 1e-9 {
            return f0;
        }
        let alpha = ((throttle - t0) / (t1 - t0)).clamp(0.0, 1.0);
        let f1 = c1.interpolate(v_mps).unwrap_or(0.0);
        f0 + alpha * (f1 - f0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_model() -> DieselTractionModel {
        DieselTractionModel::from_notch_curves(vec![
            (0.0, vec![(0.0, 0.0), (10.0, 0.0)]),
            (0.5, vec![(0.0, 50_000.0), (20.0, 30_000.0)]),
            (1.0, vec![(0.0, 100_000.0), (20.0, 60_000.0)]),
        ])
    }

    #[test]
    fn interpolates_between_notches() {
        let m = sample_model();
        let f_half = m.force_at(0.0, 0.75);
        assert!((75_000.0..=100_000.0).contains(&f_half), "got {f_half}");
    }

    #[test]
    fn zero_throttle_is_zero_force() {
        assert_eq!(sample_model().force_at(5.0, 0.0), 0.0);
    }

    #[test]
    fn effort_scale_reduces_stall_force() {
        let mut m = sample_model();
        m.calibrate_effort_scale(20_000.0);
        assert!(m.effort_scale < 1.0);
        assert!((m.force_at(0.0, 1.0) - 80_000.0).abs() < 1.0);
    }
}
