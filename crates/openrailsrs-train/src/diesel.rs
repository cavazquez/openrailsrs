//! Diesel/electric traction from ORTS per-notch F(v) curves.

use crate::model::TractiveCurve;

/// Diesel engine thermodynamic parameters (DieselPowerTab + ThrottleRPMTab).
///
/// Models the relationship between throttle position → engine RPM → shaft power,
/// with a first-order lag on RPM response (engine inertia / governor dynamics).
#[derive(Clone, Debug, PartialEq)]
pub struct DieselEngineParams {
    /// `DieselPowerTab`: sorted `(RPM, shaft_power_W)` pairs.
    pub power_tab: Vec<(f64, f64)>,
    /// `ThrottleRPMTab`: sorted `(throttle 0-1, target_RPM)` pairs.
    pub throttle_rpm_tab: Vec<(f64, f64)>,
    /// Idle RPM (engine at rest with throttle=0).
    pub idle_rpm: f64,
    /// Maximum RPM at full throttle.
    pub max_rpm: f64,
    /// First-order time constant for RPM response (seconds); default ~2s.
    pub rpm_time_constant_s: f64,
}

impl DieselEngineParams {
    /// Target RPM for a given throttle position (0-1).
    pub fn target_rpm(&self, throttle: f64) -> f64 {
        if self.throttle_rpm_tab.is_empty() {
            return self.idle_rpm + throttle * (self.max_rpm - self.idle_rpm);
        }
        let t = throttle.clamp(0.0, 1.0);
        let tab = &self.throttle_rpm_tab;
        if t <= tab.first().map(|(x, _)| *x).unwrap_or(0.0) {
            return tab.first().map(|(_, r)| *r).unwrap_or(self.idle_rpm);
        }
        if t >= tab.last().map(|(x, _)| *x).unwrap_or(1.0) {
            return tab.last().map(|(_, r)| *r).unwrap_or(self.max_rpm);
        }
        for i in 1..tab.len() {
            let (t0, r0) = tab[i - 1];
            let (t1, r1) = tab[i];
            if t <= t1 {
                let alpha = (t - t0) / (t1 - t0);
                return r0 + alpha * (r1 - r0);
            }
        }
        self.max_rpm
    }

    /// Shaft power (W) at a given RPM, interpolated from `DieselPowerTab`.
    pub fn power_at_rpm(&self, rpm: f64) -> f64 {
        if self.power_tab.is_empty() {
            return 0.0;
        }
        let tab = &self.power_tab;
        if rpm <= tab.first().map(|(r, _)| *r).unwrap_or(0.0) {
            return tab.first().map(|(_, p)| *p).unwrap_or(0.0);
        }
        if rpm >= tab.last().map(|(r, _)| *r).unwrap_or(f64::MAX) {
            return tab.last().map(|(_, p)| *p).unwrap_or(0.0);
        }
        for i in 1..tab.len() {
            let (r0, p0) = tab[i - 1];
            let (r1, p1) = tab[i];
            if rpm <= r1 {
                let alpha = (rpm - r0) / (r1 - r0);
                return p0 + alpha * (p1 - p0);
            }
        }
        0.0
    }

    /// Advance engine RPM toward target with first-order lag (Euler step).
    pub fn advance_rpm(&self, current_rpm: f64, throttle: f64, dt: f64) -> f64 {
        let target = self.target_rpm(throttle);
        let tau = self.rpm_time_constant_s.max(0.01);
        // Exponential approach: rpm += (target - rpm) * (1 - exp(-dt/tau))
        let factor = 1.0 - (-dt / tau).exp();
        current_rpm + (target - current_rpm) * factor
    }
}

/// ORTS `MaxTractiveForceCurves` / `ORTSMaxTractiveForceCurves` by throttle notch.
#[derive(Clone, Debug, PartialEq)]
pub struct DieselTractionModel {
    /// Sorted `(notch 0..1, F(v))` pairs; linear interpolation between notches.
    pub notch_curves: Vec<(f64, TractiveCurve)>,
    /// Scales curve forces (peak ORTS stall vs continuous `MaxForce`).
    pub effort_scale: f64,
    /// Optional diesel engine thermodynamic model (DieselPowerTab / ThrottleRPMTab).
    pub engine: Option<Box<DieselEngineParams>>,
}

impl Default for DieselTractionModel {
    fn default() -> Self {
        Self {
            notch_curves: Vec::new(),
            effort_scale: 1.0,
            engine: None,
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
            engine: None,
        }
    }

    /// Idle RPM; returns 0 if no engine params are configured.
    pub fn idle_rpm(&self) -> f64 {
        self.engine.as_deref().map(|e| e.idle_rpm).unwrap_or(0.0)
    }

    /// Power (W) available from the diesel engine at `current_rpm`.
    ///
    /// Returns `f64::MAX` when no engine model is configured (uncapped).
    pub fn engine_power_w(&self, current_rpm: f64) -> f64 {
        match &self.engine {
            Some(e) => e.power_at_rpm(current_rpm),
            None => f64::MAX,
        }
    }

    /// Advance the engine RPM one step; no-op when no engine params.
    pub fn advance_rpm(&self, current_rpm: f64, throttle: f64, dt: f64) -> f64 {
        match &self.engine {
            Some(e) => e.advance_rpm(current_rpm, throttle, dt),
            None => current_rpm,
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
