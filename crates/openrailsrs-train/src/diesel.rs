//! Diesel/electric traction from ORTS per-notch F(v) curves.

use crate::model::TractiveCurve;

/// OR default Curtius-Kniffler coefficients (A, B, C) when not specified in `.eng`.
pub const OR_DEFAULT_CURTIUS: (f64, f64, f64) = (6.18, 44.0, 0.161);

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
    /// First-order time constant for RPM response (seconds); fallback when OR params absent.
    pub rpm_time_constant_s: f64,
    /// OR `RateOfChangeUpRPMpSS` — RPM acceleration (RPM/s² scale factor).
    pub rate_of_change_up_rpm_pss: f64,
    /// OR `RateOfChangeDownRPMpSS`.
    pub rate_of_change_down_rpm_pss: f64,
    /// OR `ChangeUpRPMpS` — max RPM change per second when increasing.
    pub change_up_rpm_ps: f64,
    /// OR `ChangeDownRPMpS`.
    pub change_down_rpm_ps: f64,
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

    /// Advance engine RPM toward target using OR `DieselEngine.cs` dynamics when OR
    /// rate parameters are present; otherwise first-order exponential lag.
    pub fn advance_rpm(&self, current_rpm: f64, throttle: f64, dt: f64) -> f64 {
        let target = self.target_rpm(throttle);
        if self.rate_of_change_up_rpm_pss > 0.0 && self.change_up_rpm_ps > 0.0 {
            return self.advance_rpm_orts(current_rpm, target, throttle, dt);
        }
        let tau = self.rpm_time_constant_s.max(0.01);
        let factor = 1.0 - (-dt / tau).exp();
        current_rpm + (target - current_rpm) * factor
    }

    fn advance_rpm_orts(&self, current_rpm: f64, target: f64, throttle: f64, dt: f64) -> f64 {
        let delta = target - current_rpm;
        if delta.abs() < 1e-6 {
            return current_rpm;
        }
        let increasing = delta > 0.0;
        let acc = if increasing {
            self.rate_of_change_up_rpm_pss
        } else {
            self.rate_of_change_down_rpm_pss
                .max(self.rate_of_change_up_rpm_pss)
        };
        let max_change = if increasing {
            self.change_up_rpm_ps
        } else {
            self.change_down_rpm_ps.max(self.change_up_rpm_ps)
        };
        let throttle_acc = if increasing {
            throttle.clamp(0.01, 1.0)
        } else {
            1.0
        };
        let mut d_rpm = (2.0 * acc * throttle_acc * delta.abs()).sqrt();
        d_rpm = d_rpm.clamp(0.01 * max_change, max_change);
        if !increasing {
            d_rpm = -d_rpm;
        }
        let step = d_rpm * dt;
        if increasing {
            (current_rpm + step).min(target)
        } else {
            (current_rpm + step).max(target)
        }
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
    /// Rated max power (W) for legacy P/v models without `DieselEngineParams`.
    pub max_power_w: Option<f64>,
    /// Legacy MSTS ramp time to full tractive effort (`RunUpTimeToMaxForce`).
    pub legacy_run_up_time_s: Option<f64>,
    /// Mass used for Curtius-Kniffler adhesion (drive-wheel weight or loco mass).
    pub adhesion_mass_kg: f64,
    /// Curtius-Kniffler A/B/C; adhesion disabled when A <= 0.
    pub curtius_a: f64,
    pub curtius_b: f64,
    pub curtius_c: f64,
    /// Motor heating time constant for dynamic `PowerReduction` (seconds).
    pub motor_heating_time_s: f64,
}

impl Default for DieselTractionModel {
    fn default() -> Self {
        Self {
            notch_curves: Vec::new(),
            effort_scale: 1.0,
            engine: None,
            max_power_w: None,
            legacy_run_up_time_s: None,
            adhesion_mass_kg: 0.0,
            curtius_a: 0.0,
            curtius_b: 0.0,
            curtius_c: 0.0,
            motor_heating_time_s: 120.0,
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
            max_power_w: None,
            legacy_run_up_time_s: None,
            adhesion_mass_kg: 0.0,
            curtius_a: 0.0,
            curtius_b: 0.0,
            curtius_c: 0.0,
            motor_heating_time_s: 120.0,
        }
    }

    /// Configure adhesion and motor-heating parameters from locomotive `.eng` data.
    pub fn configure_traction_limits(
        &mut self,
        loco_mass_kg: f64,
        drive_wheel_mass_kg: f64,
        curtius: (f64, f64, f64),
        motor_heating_time_s: f64,
    ) {
        self.adhesion_mass_kg = if drive_wheel_mass_kg > 0.0 {
            drive_wheel_mass_kg
        } else {
            loco_mass_kg
        };
        self.curtius_a = curtius.0;
        self.curtius_b = curtius.1;
        self.curtius_c = curtius.2;
        if motor_heating_time_s > 0.0 {
            self.motor_heating_time_s = motor_heating_time_s;
        }
    }

    /// OR Curtius-Kniffler adhesion limit (N) at speed `v_mps`.
    pub fn adhesion_limit_n(&self, v_mps: f64) -> f64 {
        if self.curtius_a <= 0.0 || self.adhesion_mass_kg <= 0.0 {
            return f64::INFINITY;
        }
        let v_kmh = v_mps * 3.6;
        self.adhesion_mass_kg * 9.81 * (self.curtius_a / (self.curtius_b + v_kmh) + self.curtius_c)
    }

    /// Legacy MSTS diesel: single full-notch curve from max power and tractive effort.
    pub fn from_power_and_effort(
        max_power_w: f64,
        max_tractive_effort_n: f64,
        run_up_time_s: f64,
    ) -> Self {
        let curve = TractiveCurve::from_power_and_effort(max_power_w, max_tractive_effort_n);
        Self {
            notch_curves: vec![(0.0, TractiveCurve::default()), (1.0, curve)],
            effort_scale: 1.0,
            engine: None,
            max_power_w: Some(max_power_w),
            legacy_run_up_time_s: if run_up_time_s > 0.0 {
                Some(run_up_time_s)
            } else {
                None
            },
            adhesion_mass_kg: 0.0,
            curtius_a: 0.0,
            curtius_b: 0.0,
            curtius_c: 0.0,
            motor_heating_time_s: 120.0,
        }
    }

    /// OR dynamic `PowerReduction` fraction from motor heat state `[0, 1]`.
    pub fn power_reduction_from_heat(heat: f64) -> f64 {
        heat.clamp(0.0, 0.35)
    }

    /// Advance motor heat one step; returns updated heat in `[0, 1]`.
    pub fn advance_motor_heat(
        &self,
        heat: f64,
        v_mps: f64,
        throttle: f64,
        run_up_factor: f64,
        dt: f64,
    ) -> f64 {
        let tau = self.motor_heating_time_s.max(1.0);
        let target = if throttle <= 0.0 {
            0.0
        } else {
            let f_peak = self.uncapped_force_at(v_mps, throttle, run_up_factor);
            let f_adh = self.adhesion_limit_n(v_mps);
            let util = if f_adh.is_finite() && f_adh > 0.0 {
                (f_peak / f_adh).clamp(0.0, 1.0)
            } else {
                throttle.clamp(0.0, 1.0)
            };
            util * util
        };
        (heat + dt / tau * (target - heat)).clamp(0.0, 1.0)
    }

    pub fn legacy_run_up_time_s(&self) -> Option<f64> {
        self.legacy_run_up_time_s
    }

    /// Idle RPM; returns 0 if no engine params are configured.
    pub fn idle_rpm(&self) -> f64 {
        self.engine.as_deref().map(|e| e.idle_rpm).unwrap_or(0.0)
    }

    /// Power (W) available from the diesel engine at `current_rpm`.
    ///
    /// Returns `f64::MAX` when no engine model is configured (uncapped here; use
    /// [`Self::effective_power_w`] for legacy P/v models).
    pub fn engine_power_w(&self, current_rpm: f64) -> f64 {
        match &self.engine {
            Some(e) => e.power_at_rpm(current_rpm),
            None => f64::MAX,
        }
    }

    /// Shaft power at `current_rpm` and `throttle`, including legacy P/v fallback.
    ///
    /// With a [`DieselEngineParams`] model, OR scales shaft power between idle and
    /// `DieselPowerTab(RPM)` at partial throttle; near notch power the tab maximum applies.
    pub fn effective_power_w(&self, current_rpm: f64, throttle: f64) -> f64 {
        let t = throttle.clamp(0.0, 1.0);
        match &self.engine {
            Some(e) => {
                let at_rpm = e.power_at_rpm(current_rpm);
                if t >= 0.5 {
                    return at_rpm;
                }
                let at_idle = e.power_at_rpm(e.idle_rpm);
                at_idle + (at_rpm - at_idle).max(0.0) * t
            }
            None => self.max_power_w.unwrap_or(0.0) * t,
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
        self.force_at_scaled(v_mps, throttle, 1.0, 0.0)
    }

    /// Like [`Self::force_at`] but scales legacy ramp-in and applies adhesion / heating.
    pub fn force_at_scaled(
        &self,
        v_mps: f64,
        throttle: f64,
        run_up_factor: f64,
        power_reduction: f64,
    ) -> f64 {
        let raw =
            self.force_at_raw(v_mps, throttle) * self.effort_scale * run_up_factor.clamp(0.0, 1.0);
        let reduced = raw * (1.0 - power_reduction.clamp(0.0, 0.95));
        reduced.min(self.adhesion_limit_n(v_mps))
    }

    /// Peak tractive effort at `(v, throttle)` before adhesion / heating caps.
    pub fn uncapped_force_at(&self, v_mps: f64, throttle: f64, run_up_factor: f64) -> f64 {
        self.force_at_raw(v_mps, throttle) * self.effort_scale * run_up_factor.clamp(0.0, 1.0)
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

    #[test]
    fn effective_power_scales_with_throttle_when_engine_present() {
        let engine = DieselEngineParams {
            power_tab: vec![(325.0, 100_000.0), (1500.0, 500_000.0)],
            throttle_rpm_tab: vec![(0.0, 325.0), (1.0, 1500.0)],
            idle_rpm: 325.0,
            max_rpm: 1500.0,
            rpm_time_constant_s: 2.0,
            rate_of_change_up_rpm_pss: 0.0,
            rate_of_change_down_rpm_pss: 0.0,
            change_up_rpm_ps: 0.0,
            change_down_rpm_ps: 0.0,
        };
        let mut m = sample_model();
        m.engine = Some(Box::new(engine));
        let idle = m.effective_power_w(1500.0, 0.0);
        let full = m.effective_power_w(1500.0, 1.0);
        let partial = m.effective_power_w(1500.0, 0.27);
        assert!((idle - 100_000.0).abs() < 1.0);
        assert!((full - 500_000.0).abs() < 1.0);
        assert!(partial > idle && partial < full, "partial={partial}");
    }

    #[test]
    fn from_power_and_effort_stall_force() {
        let m = DieselTractionModel::from_power_and_effort(1_000_000.0, 150_650.0, 0.0);
        let stall = m.force_at(0.0, 1.0);
        assert!((stall - 150_650.0).abs() < 1.0, "stall {stall}");
        assert_eq!(m.max_power_w, Some(1_000_000.0));
    }

    #[test]
    fn or_rpm_sqrt_differs_from_exponential_lag() {
        let base = DieselEngineParams {
            power_tab: vec![(650.0, 100_000.0), (1500.0, 500_000.0)],
            throttle_rpm_tab: vec![(0.0, 650.0), (1.0, 1500.0)],
            idle_rpm: 650.0,
            max_rpm: 1500.0,
            rpm_time_constant_s: 2.0,
            rate_of_change_up_rpm_pss: 10.0,
            rate_of_change_down_rpm_pss: 10.0,
            change_up_rpm_ps: 50.0,
            change_down_rpm_ps: 40.0,
        };
        let lag = DieselEngineParams {
            rate_of_change_up_rpm_pss: 0.0,
            change_up_rpm_ps: 0.0,
            ..base.clone()
        };
        let rpm_or = base.advance_rpm(650.0, 1.0, 1.0);
        let rpm_lag = lag.advance_rpm(650.0, 1.0, 1.0);
        assert_ne!(rpm_or, rpm_lag);
        assert!(rpm_or > 650.0 && rpm_or <= 1500.0);
    }

    #[test]
    fn adhesion_caps_stall_force() {
        let mut m = sample_model();
        m.configure_traction_limits(68_000.0, 64_000.0, OR_DEFAULT_CURTIUS, 120.0);
        let stall = m.force_at(0.0, 1.0);
        let limit = m.adhesion_limit_n(0.0);
        assert!(stall <= limit + 1.0, "stall {stall} limit {limit}");
    }
}
