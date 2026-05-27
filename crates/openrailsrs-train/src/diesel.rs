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
    /// RPM → throttle 0–1 (`ReverseThrottleRPMTab`, built from `ThrottleRPMTab` when absent).
    pub reverse_throttle_rpm_tab: Vec<(f64, f64)>,
}

/// Build OR `ReverseThrottleRPMTab` from `ThrottleRPMTab`.
pub fn build_reverse_throttle_rpm_tab(throttle_rpm_tab: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if throttle_rpm_tab.is_empty() {
        return Vec::new();
    }
    let mut pairs: Vec<(f64, f64)> = throttle_rpm_tab.iter().map(|(t, r)| (*r, *t)).collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    if pairs.first().map(|(r, _)| *r > 0.0).unwrap_or(true) {
        if let Some((_, idle_rpm)) = throttle_rpm_tab.first() {
            pairs.insert(0, (*idle_rpm, 0.0));
        }
    }
    pairs
}

fn interp_sorted_tab(tab: &[(f64, f64)], x: f64) -> f64 {
    if tab.is_empty() {
        return 0.0;
    }
    if x <= tab[0].0 {
        return tab[0].1;
    }
    if x >= tab.last().map(|(k, _)| *k).unwrap_or(f64::MAX) {
        return tab.last().map(|(_, v)| *v).unwrap_or(0.0);
    }
    for i in 1..tab.len() {
        let (x0, y0) = tab[i - 1];
        let (x1, y1) = tab[i];
        if x <= x1 {
            let span = x1 - x0;
            if span.abs() < 1e-9 {
                return y1;
            }
            let alpha = (x - x0) / span;
            return y0 + alpha * (y1 - y0);
        }
    }
    tab.last().map(|(_, v)| *v).unwrap_or(0.0)
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

    /// OR `ApparentThrottleSetting / 100` from `ReverseThrottleRPMTab[RealRPM]`.
    pub fn apparent_throttle_fraction(&self, rpm: f64) -> f64 {
        if self.reverse_throttle_rpm_tab.is_empty() {
            return 1.0;
        }
        interp_sorted_tab(&self.reverse_throttle_rpm_tab, rpm).clamp(0.0, 1.0)
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

/// OR traction ramp and continuous-force parameters from `.eng`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TractionDynamicsParams {
    pub max_force_n: f64,
    pub max_continuous_force_n: f64,
    pub force_ramp_up_nps: f64,
    pub force_ramp_down_nps: f64,
    pub force_ramp_down_to_zero_nps: f64,
    pub power_ramp_up_wps: f64,
    pub power_ramp_down_wps: f64,
    pub power_ramp_down_to_zero_wps: f64,
    pub continuous_force_time_factor_s: f64,
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
    /// OR `LocomotiveMaxRailOutputPowerW` (W); falls back to legacy `max_power_w`.
    pub max_rail_output_power_w: f64,
    /// OR `ORTSUnloadingSpeed` (m/s); 0 = disabled.
    pub unloading_speed_mps: f64,
    /// When true, skip apparent-throttle limiting on F(v) curves.
    pub tractive_force_power_limited: bool,
    /// MSTS `MaxContinuousForce` used for effort-scale calibration (N).
    pub max_continuous_force_n: f64,
    /// MSTS `MaxForce` peak rating for continuous-force derating (N).
    pub max_force_n: f64,
    /// OR `TractionForceRampUpNpS` (N/s); 0 = instant.
    pub traction_force_ramp_up_nps: f64,
    pub traction_force_ramp_down_nps: f64,
    /// Negative means “use ramp down” (OR default when unset).
    pub traction_force_ramp_down_to_zero_nps: f64,
    pub traction_power_ramp_up_wps: f64,
    pub traction_power_ramp_down_wps: f64,
    pub traction_power_ramp_down_to_zero_wps: f64,
    /// OR `ContinuousForceTimeFactor` (s); 0 = disabled.
    pub continuous_force_time_factor_s: f64,
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
            max_rail_output_power_w: 0.0,
            unloading_speed_mps: 0.0,
            tractive_force_power_limited: false,
            max_continuous_force_n: 0.0,
            max_force_n: 0.0,
            traction_force_ramp_up_nps: 0.0,
            traction_force_ramp_down_nps: 0.0,
            traction_force_ramp_down_to_zero_nps: -1.0,
            traction_power_ramp_up_wps: 0.0,
            traction_power_ramp_down_wps: 0.0,
            traction_power_ramp_down_to_zero_wps: -1.0,
            continuous_force_time_factor_s: 0.0,
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
            max_rail_output_power_w: 0.0,
            unloading_speed_mps: 0.0,
            tractive_force_power_limited: false,
            max_continuous_force_n: 0.0,
            max_force_n: 0.0,
            traction_force_ramp_up_nps: 0.0,
            traction_force_ramp_down_nps: 0.0,
            traction_force_ramp_down_to_zero_nps: -1.0,
            traction_power_ramp_up_wps: 0.0,
            traction_power_ramp_down_wps: 0.0,
            traction_power_ramp_down_to_zero_wps: -1.0,
            continuous_force_time_factor_s: 0.0,
        }
    }

    /// OR-P13: clone lead ORTS diesel for a trail motor with scaled power/continuous ratings.
    ///
    /// Used when the trail `.eng` only has legacy MSTS fields (`RunUpTimeToMaxForce`, P/v)
    /// but the consist lead locomotive has full `ORTSMaxTractiveForceCurves`.
    pub fn from_lead_orts_scaled(
        lead: &Self,
        max_power_w: f64,
        max_tractive_effort_n: f64,
        max_continuous_force_n: f64,
        run_up_time_s: Option<f64>,
    ) -> Option<Self> {
        let lead_engine = lead.engine.as_deref()?;
        if lead.notch_curves.is_empty() || max_power_w <= 0.0 {
            return None;
        }
        let lead_rail_w = if lead.max_rail_output_power_w > 0.0 {
            lead.max_rail_output_power_w
        } else {
            lead.max_power_w.unwrap_or(max_power_w)
        };
        let power_scale = max_power_w / lead_rail_w;
        let mut engine = lead_engine.clone();
        engine.power_tab = engine
            .power_tab
            .iter()
            .map(|(rpm, p)| (*rpm, p * power_scale))
            .collect();

        let continuous = if max_continuous_force_n > 0.0 {
            max_continuous_force_n
        } else {
            lead.max_continuous_force_n
        };

        let mut model = Self {
            notch_curves: lead.notch_curves.clone(),
            effort_scale: 1.0,
            engine: Some(Box::new(engine)),
            max_power_w: Some(max_power_w),
            legacy_run_up_time_s: run_up_time_s,
            adhesion_mass_kg: lead.adhesion_mass_kg,
            curtius_a: lead.curtius_a,
            curtius_b: lead.curtius_b,
            curtius_c: lead.curtius_c,
            motor_heating_time_s: lead.motor_heating_time_s,
            max_rail_output_power_w: max_power_w,
            unloading_speed_mps: lead.unloading_speed_mps,
            tractive_force_power_limited: lead.tractive_force_power_limited,
            max_continuous_force_n: continuous,
            max_force_n: if max_tractive_effort_n > 0.0 {
                max_tractive_effort_n
            } else {
                lead.max_force_n
            },
            traction_force_ramp_up_nps: lead.traction_force_ramp_up_nps,
            traction_force_ramp_down_nps: lead.traction_force_ramp_down_nps,
            traction_force_ramp_down_to_zero_nps: lead.traction_force_ramp_down_to_zero_nps,
            traction_power_ramp_up_wps: lead.traction_power_ramp_up_wps,
            traction_power_ramp_down_wps: lead.traction_power_ramp_down_wps,
            traction_power_ramp_down_to_zero_wps: lead.traction_power_ramp_down_to_zero_wps,
            continuous_force_time_factor_s: if continuous > 0.0 && max_tractive_effort_n > 0.0 {
                if lead.continuous_force_time_factor_s > 0.0 {
                    lead.continuous_force_time_factor_s
                } else {
                    1800.0
                }
            } else {
                0.0
            },
        };
        // Legacy trail `.eng` only exposes MSTS MaxForce; stall effort must match that,
        // not the lead ORTS 4× continuous calibration.
        if max_tractive_effort_n > 0.0 {
            let stall = model.force_at_raw(0.0, 1.0);
            if stall > 0.0 {
                model.effort_scale = (max_tractive_effort_n / stall).clamp(0.05, 4.0);
            }
        } else if continuous > 0.0 {
            model.calibrate_effort_scale(continuous);
        }
        Some(model)
    }

    /// Effective throttle for F(v) curves: `min(driver, apparent)` unless power-limited.
    pub fn effective_traction_throttle(&self, driver_throttle: f64, rpm: f64) -> f64 {
        let driver = driver_throttle.clamp(0.0, 1.0);
        if self.tractive_force_power_limited {
            return driver;
        }
        let apparent = self
            .engine
            .as_deref()
            .map(|e| e.apparent_throttle_fraction(rpm))
            .unwrap_or(1.0);
        driver.min(apparent)
    }

    /// OR rail P/v cap (W): `LocomotiveMaxRailOutputPowerW × t`, with optional unload decay.
    pub fn rail_power_cap_w(&self, effective_throttle: f64, v_mps: f64) -> f64 {
        let t = effective_throttle.clamp(0.0, 1.0);
        let base_w = if self.max_rail_output_power_w > 0.0 {
            self.max_rail_output_power_w
        } else {
            self.max_power_w.unwrap_or(0.0)
        };
        if base_w <= 0.0 || t <= 0.0 {
            return 0.0;
        }
        let mut power_w = base_w * t;
        let unload = self.unloading_speed_mps;
        if unload > 0.0 && v_mps > unload {
            let decay = 1.0 - (1.0 / unload) * (v_mps - unload);
            power_w *= decay.clamp(0.0, 1.0);
        }
        power_w
    }

    /// Legacy P/v cap from `DieselPowerTab` (SCE calibration hack).
    pub fn legacy_effective_power_w(&self, current_rpm: f64, throttle: f64) -> f64 {
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

    /// P/v cap power (W) for traction limiting.
    pub fn traction_power_cap_w(
        &self,
        current_rpm: f64,
        driver_throttle: f64,
        v_mps: f64,
        legacy_power_cap: bool,
    ) -> f64 {
        if legacy_power_cap {
            return self.legacy_effective_power_w(current_rpm, driver_throttle);
        }
        let t_eff = self.effective_traction_throttle(driver_throttle, current_rpm);
        self.rail_power_cap_w(t_eff, v_mps)
    }

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

    /// Wire OR traction ramp and continuous-force parameters from a parsed `.eng`.
    pub fn configure_traction_dynamics(&mut self, params: TractionDynamicsParams) {
        if params.max_force_n > 0.0 {
            self.max_force_n = params.max_force_n;
        }
        if params.max_continuous_force_n > 0.0 {
            self.max_continuous_force_n = params.max_continuous_force_n;
        }
        self.traction_force_ramp_up_nps = params.force_ramp_up_nps;
        self.traction_force_ramp_down_nps = params.force_ramp_down_nps;
        self.traction_force_ramp_down_to_zero_nps = if params.force_ramp_down_to_zero_nps != 0.0 {
            params.force_ramp_down_to_zero_nps
        } else {
            -1.0
        };
        self.traction_power_ramp_up_wps = params.power_ramp_up_wps;
        self.traction_power_ramp_down_wps = params.power_ramp_down_wps;
        self.traction_power_ramp_down_to_zero_wps = if params.power_ramp_down_to_zero_wps != 0.0 {
            params.power_ramp_down_to_zero_wps
        } else {
            -1.0
        };
        if self.max_force_n > 0.0 && self.max_continuous_force_n > 0.0 {
            self.continuous_force_time_factor_s = if params.continuous_force_time_factor_s > 0.0 {
                params.continuous_force_time_factor_s
            } else {
                1800.0
            };
        }
    }

    fn effective_ramp_down_to_zero_nps(&self) -> f64 {
        if self.traction_force_ramp_down_to_zero_nps >= 0.0 {
            self.traction_force_ramp_down_to_zero_nps
        } else {
            self.traction_force_ramp_down_nps
        }
    }

    fn effective_power_ramp_down_to_zero_wps(&self) -> f64 {
        if self.traction_power_ramp_down_to_zero_wps >= 0.0 {
            self.traction_power_ramp_down_to_zero_wps
        } else {
            self.traction_power_ramp_down_wps
        }
    }

    /// Instantaneous tractive demand before OR ramp / continuous limiting.
    pub fn target_traction_force_n(
        &self,
        v_mps: f64,
        driver_throttle: f64,
        rpm: f64,
        run_up_factor: f64,
        power_reduction: f64,
        legacy_power_cap: bool,
    ) -> f64 {
        let mut f_e = self.force_at_scaled(
            v_mps,
            driver_throttle,
            rpm,
            run_up_factor,
            power_reduction,
            legacy_power_cap,
        );
        let p_e = self.traction_power_cap_w(rpm, driver_throttle, v_mps, legacy_power_cap)
            * run_up_factor
            * (1.0 - power_reduction.clamp(0.0, 0.95));
        if v_mps > 0.5 && p_e > 0.0 {
            f_e = f_e.min(p_e / v_mps);
        }
        f_e
    }

    /// OR `MSTSLocomotive.UpdateForceWithRamp` — smooth force transitions (N/s and W/s).
    pub fn update_force_with_ramp(
        &self,
        mut force_n: f64,
        dt: f64,
        mut target_force_n: f64,
        max_force_n: f64,
        v_mps: f64,
        prev_v_mps: f64,
    ) -> f64 {
        if max_force_n.is_finite() {
            target_force_n = target_force_n.min(max_force_n);
            force_n = force_n.min(max_force_n);
        }
        let to_zero = target_force_n == 0.0;
        if v_mps > 0.0 {
            let power_w = force_n * prev_v_mps.max(0.0);
            let mut target_power_w = target_force_n * v_mps;
            if target_power_w > power_w && self.traction_power_ramp_up_wps > 0.0 {
                let max_change_w = self.traction_power_ramp_up_wps * dt;
                if power_w + max_change_w < target_power_w {
                    target_power_w = power_w + max_change_w;
                    target_force_n = target_force_n.min(target_power_w / v_mps);
                }
            }
            let ramp_down_wps = if to_zero {
                self.effective_power_ramp_down_to_zero_wps()
            } else {
                self.traction_power_ramp_down_wps
            };
            if target_power_w < power_w && ramp_down_wps > 0.0 {
                let max_change_w = ramp_down_wps * dt;
                if power_w - max_change_w > target_power_w {
                    target_power_w = power_w - max_change_w;
                    target_force_n = target_force_n.max(target_power_w / v_mps).min(force_n);
                }
            }
        }
        if target_force_n > force_n {
            if self.traction_force_ramp_up_nps > 0.0 {
                force_n = (force_n + self.traction_force_ramp_up_nps * dt).min(target_force_n);
            } else {
                force_n = target_force_n;
            }
        } else if target_force_n < force_n {
            let ramp_down_nps = if to_zero {
                self.effective_ramp_down_to_zero_nps()
            } else {
                self.traction_force_ramp_down_nps
            };
            if ramp_down_nps > 0.0 {
                force_n = (force_n - ramp_down_nps * dt).max(target_force_n);
            } else {
                force_n = target_force_n;
            }
        }
        force_n
    }

    /// OR continuous tractive-force derating from moving-average motor load.
    pub fn apply_continuous_force_limit(
        &self,
        force_n: f64,
        average_force_n: f64,
        power_reduction: f64,
    ) -> f64 {
        let max_force = self.max_force_n;
        let max_continuous = self.max_continuous_force_n;
        if max_force <= 0.0 || max_continuous <= 0.0 || power_reduction >= 1.0 {
            return force_n;
        }
        let coef = (max_force - max_continuous) / (max_force * max_continuous);
        force_n * (1.0 - coef * average_force_n * (1.0 - power_reduction))
    }

    /// OR `AverageForceN` low-pass filter (`ContinuousForceTimeFactor`).
    pub fn advance_average_force(
        &self,
        average_force_n: f64,
        traction_force_n: f64,
        dt: f64,
    ) -> f64 {
        let tau = self.continuous_force_time_factor_s;
        if tau <= 0.0 || self.max_force_n <= 0.0 || self.max_continuous_force_n <= 0.0 {
            return average_force_n;
        }
        let w = ((tau - dt) / tau).clamp(0.0, 1.0);
        w * average_force_n + (1.0 - w) * traction_force_n
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
            max_rail_output_power_w: max_power_w,
            unloading_speed_mps: 0.0,
            tractive_force_power_limited: false,
            max_continuous_force_n: max_tractive_effort_n,
            max_force_n: max_tractive_effort_n,
            traction_force_ramp_up_nps: 0.0,
            traction_force_ramp_down_nps: 0.0,
            traction_force_ramp_down_to_zero_nps: -1.0,
            traction_power_ramp_up_wps: 0.0,
            traction_power_ramp_down_wps: 0.0,
            traction_power_ramp_down_to_zero_wps: -1.0,
            continuous_force_time_factor_s: 0.0,
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

    /// Shaft power at `current_rpm` and `throttle` — delegates to legacy P/v cap.
    pub fn effective_power_w(&self, current_rpm: f64, throttle: f64) -> f64 {
        self.legacy_effective_power_w(current_rpm, throttle)
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
        let rpm = self
            .engine
            .as_deref()
            .map(|e| e.target_rpm(throttle))
            .unwrap_or(0.0);
        self.force_at_scaled(v_mps, throttle, rpm, 1.0, 0.0, true)
    }

    /// Like [`Self::force_at`] but scales legacy ramp-in and applies adhesion / heating.
    pub fn force_at_scaled(
        &self,
        v_mps: f64,
        driver_throttle: f64,
        rpm: f64,
        run_up_factor: f64,
        power_reduction: f64,
        legacy_power_cap: bool,
    ) -> f64 {
        let curve_throttle = if legacy_power_cap {
            driver_throttle.clamp(0.0, 1.0)
        } else {
            self.effective_traction_throttle(driver_throttle, rpm)
        };
        let raw = self.force_at_raw(v_mps, curve_throttle)
            * self.effort_scale
            * run_up_factor.clamp(0.0, 1.0);
        let reduced = raw * (1.0 - power_reduction.clamp(0.0, 0.95));
        reduced.min(self.adhesion_limit_n(v_mps))
    }

    /// Peak tractive effort at `(v, throttle)` before adhesion / heating caps.
    pub fn uncapped_force_at(&self, v_mps: f64, throttle: f64, run_up_factor: f64) -> f64 {
        let rpm = self
            .engine
            .as_deref()
            .map(|e| e.target_rpm(throttle))
            .unwrap_or(0.0);
        self.force_at_raw(v_mps, self.effective_traction_throttle(throttle, rpm))
            * self.effort_scale
            * run_up_factor.clamp(0.0, 1.0)
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
    fn apparent_throttle_limits_force_at_low_rpm() {
        let throttle_rpm = vec![(0.0, 325.0), (1.0, 750.0)];
        let engine = DieselEngineParams {
            power_tab: vec![(325.0, 100_000.0), (750.0, 1_000_000.0)],
            throttle_rpm_tab: throttle_rpm.clone(),
            idle_rpm: 325.0,
            max_rpm: 750.0,
            rpm_time_constant_s: 2.0,
            rate_of_change_up_rpm_pss: 0.0,
            rate_of_change_down_rpm_pss: 0.0,
            change_up_rpm_ps: 0.0,
            change_down_rpm_ps: 0.0,
            reverse_throttle_rpm_tab: build_reverse_throttle_rpm_tab(&throttle_rpm),
        };
        let mut m = sample_model();
        m.engine = Some(Box::new(engine));
        m.max_rail_output_power_w = 1_000_000.0;
        let apparent = m.engine.as_ref().unwrap().apparent_throttle_fraction(400.0);
        assert!(apparent < 0.5, "apparent={apparent}");
        let f_legacy = m.force_at_scaled(0.0, 1.0, 400.0, 1.0, 0.0, true);
        let f_or = m.force_at_scaled(0.0, 1.0, 400.0, 1.0, 0.0, false);
        assert!(f_or < f_legacy, "legacy={f_legacy} or={f_or}");
    }

    #[test]
    fn rail_power_cap_scales_with_effective_throttle() {
        let mut m = sample_model();
        m.max_rail_output_power_w = 2_000_000.0;
        let p_full = m.rail_power_cap_w(1.0, 10.0);
        let p_partial = m.rail_power_cap_w(0.27, 10.0);
        assert!((p_full - 2_000_000.0).abs() < 1.0);
        assert!((p_partial - 540_000.0).abs() < 1_000.0);
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
            reverse_throttle_rpm_tab: Vec::new(),
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
    fn update_force_with_ramp_limits_rise_rate() {
        let mut m = sample_model();
        m.traction_force_ramp_up_nps = 10_000.0;
        let f0 = m.update_force_with_ramp(0.0, 1.0, 50_000.0, f64::INFINITY, 0.0, 0.0);
        assert!((f0 - 10_000.0).abs() < 1.0, "f0={f0}");
        let f1 = m.update_force_with_ramp(f0, 1.0, 50_000.0, f64::INFINITY, 0.0, 0.0);
        assert!((f1 - 20_000.0).abs() < 1.0, "f1={f1}");
    }

    #[test]
    fn continuous_force_limit_derates_at_high_average() {
        let mut m = sample_model();
        m.max_force_n = 100_000.0;
        m.max_continuous_force_n = 50_000.0;
        m.continuous_force_time_factor_s = 1800.0;
        let limited = m.apply_continuous_force_limit(100_000.0, 50_000.0, 0.0);
        assert!((limited - 50_000.0).abs() < 1.0, "limited={limited}");
    }

    #[test]
    fn advance_average_force_low_pass() {
        let mut m = sample_model();
        m.max_force_n = 100_000.0;
        m.max_continuous_force_n = 50_000.0;
        m.continuous_force_time_factor_s = 100.0;
        let avg = m.advance_average_force(0.0, 100_000.0, 10.0);
        assert!(avg > 0.0 && avg < 100_000.0, "avg={avg}");
    }

    #[test]
    fn from_lead_orts_scaled_calibrates_stall_to_max_force() {
        let mut lead = DieselTractionModel::from_notch_curves(vec![
            (0.0, vec![(0.0, 0.0), (30.0, 0.0)]),
            (1.0, vec![(0.0, 200_000.0), (30.0, 80_000.0)]),
        ]);
        lead.max_rail_output_power_w = 745_513.0;
        lead.engine = Some(Box::new(DieselEngineParams {
            power_tab: vec![(325.0, 100_000.0), (750.0, 745_513.0)],
            throttle_rpm_tab: vec![(0.0, 325.0), (1.0, 750.0)],
            idle_rpm: 325.0,
            max_rpm: 750.0,
            rpm_time_constant_s: 2.0,
            rate_of_change_up_rpm_pss: 0.0,
            rate_of_change_down_rpm_pss: 0.0,
            change_up_rpm_ps: 0.0,
            change_down_rpm_ps: 0.0,
            reverse_throttle_rpm_tab: Vec::new(),
        }));
        let scaled = DieselTractionModel::from_lead_orts_scaled(
            &lead,
            1_000_000.0,
            150_650.0,
            130_000.0,
            Some(30.0),
        )
        .expect("scaled");
        let stall = scaled.force_at_raw(0.0, 1.0) * scaled.effort_scale;
        assert!((stall - 150_650.0).abs() < 1.0, "stall {stall}");
        assert_eq!(scaled.legacy_run_up_time_s, Some(30.0));
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
            reverse_throttle_rpm_tab: build_reverse_throttle_rpm_tab(&[
                (0.0, 650.0),
                (1.0, 1500.0),
            ]),
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

    #[test]
    fn build_reverse_throttle_tab_endpoints() {
        let tab = vec![(0.0, 325.0), (0.4, 450.0), (1.0, 750.0)];
        let rev = build_reverse_throttle_rpm_tab(&tab);
        assert!((rev.first().unwrap().0 - 325.0).abs() < 1e-6);
        let engine = DieselEngineParams {
            power_tab: vec![],
            throttle_rpm_tab: tab,
            idle_rpm: 325.0,
            max_rpm: 750.0,
            rpm_time_constant_s: 2.0,
            rate_of_change_up_rpm_pss: 0.0,
            rate_of_change_down_rpm_pss: 0.0,
            change_up_rpm_ps: 0.0,
            change_down_rpm_ps: 0.0,
            reverse_throttle_rpm_tab: rev,
        };
        assert!((engine.apparent_throttle_fraction(750.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn target_traction_force_limited_by_rail_power_at_speed() {
        let mut m = sample_model();
        m.max_rail_output_power_w = 800_000.0;
        m.engine = Some(Box::new(DieselEngineParams {
            power_tab: vec![(325.0, 100_000.0), (750.0, 800_000.0)],
            throttle_rpm_tab: vec![(0.0, 325.0), (1.0, 750.0)],
            idle_rpm: 325.0,
            max_rpm: 750.0,
            rpm_time_constant_s: 2.0,
            rate_of_change_up_rpm_pss: 0.0,
            rate_of_change_down_rpm_pss: 0.0,
            change_up_rpm_ps: 0.0,
            change_down_rpm_ps: 0.0,
            reverse_throttle_rpm_tab: build_reverse_throttle_rpm_tab(&[(0.0, 325.0), (1.0, 750.0)]),
        }));
        let v = 20.0;
        let target = m.target_traction_force_n(v, 1.0, 750.0, 1.0, 0.0, false);
        assert!(target <= 800_000.0 / v + 1.0, "target={target}");
    }
}
