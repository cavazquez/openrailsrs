//! Advanced cylinder travel and EP-only wire from pinned Open Rails 1.6.1:
//! AirSinglePipe.{AdvancedBrakeCylinderAir,CalculateBrakeCylinderPressure,Update}
//! and EPBrakeSystem.Update. Shoe force is distinct from wheel-rim brake force.
use openrailsrs_formats::NativeEpBrakeProfile;
use serde::{Deserialize, Serialize};

const ATMOSPHERE_PSI: f64 = 14.696;
pub(crate) const PSI_TO_BAR: f64 = 0.0689475729;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NativeEpState {
    #[serde(default)]
    linear_volume_m3: Option<f64>,
    pub profile: NativeEpBrakeProfile,
    pub pressure_psi: f64,
    auto_pressure_psi: f64,
    cylinder_air_psi_m3: f64,
    auto_air_psi_m3: f64,
    auxiliary_psi: f64,
    low_stage: bool,
    #[serde(default = "default_pipe_ratio")]
    pipe_volume_ratio: f64,
    travel_table: Vec<(f64, f64)>,
}

fn default_pipe_ratio() -> f64 {
    0.2
}

impl NativeEpState {
    pub(crate) fn auxiliary_pressure_psi(&self) -> f64 {
        self.auxiliary_psi
    }
    pub fn new(profile: NativeEpBrakeProfile) -> Self {
        Self::with_pipe_volume(profile, 0.2)
    }
    fn with_pipe_volume(profile: NativeEpBrakeProfile, pipe_volume_ratio: f64) -> Self {
        let mut state = Self {
            linear_volume_m3: None,
            profile,
            pressure_psi: 0.,
            auto_pressure_psi: 0.,
            cylinder_air_psi_m3: 0.,
            auto_air_psi_m3: 0.,
            auxiliary_psi: 70.,
            low_stage: false,
            pipe_volume_ratio,
            travel_table: Vec::with_capacity(7),
        };
        let reference = state.profile.reference_psi;
        let nominal: f64 = if (reference - 50.).abs() < 1e-8 {
            45.
        } else {
            50.
        };
        let last_pressure = (1.6 - 0.8) * (50. - state.profile.spring_psi) / 0.2;
        for pressure in [
            0.,
            state.profile.spring_psi / 2.,
            state.profile.spring_psi,
            nominal.min(reference),
            nominal.max(reference),
            last_pressure,
        ] {
            state.travel_table.push((
                state.air_at_pressure(pressure),
                state.travel_at_pressure(pressure),
            ));
        }
        state.travel_table[5].1 = state.profile.stroke_m * 1.6;
        state
            .travel_table
            .push((state.travel_table[5].0 * 2., state.profile.stroke_m * 1.6));
        state
    }
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.profile.diameter_m.powi(2) / 4.
    }
    fn cylinder_volume(&self) -> f64 {
        self.linear_volume_m3
            .unwrap_or(self.area() * self.profile.stroke_m)
    }
    fn pipe_volume(&self) -> f64 {
        self.cylinder_volume() * self.pipe_volume_ratio
    }
    fn travel_at_pressure(&self, pressure: f64) -> f64 {
        self.profile.stroke_m
            * (0.8 * (2. * pressure / self.profile.spring_psi - 1.).clamp(0., 1.)
                + 0.2
                    * ((pressure - self.profile.spring_psi) / (50. - self.profile.spring_psi))
                        .max(0.))
    }
    fn air_at_pressure(&self, pressure: f64) -> f64 {
        if let Some(volume) = self.linear_volume_m3 {
            return volume * pressure;
        }
        self.pipe_volume() * pressure
            + self.travel_at_pressure(pressure) * self.area() * (pressure + ATMOSPHERE_PSI)
    }
    fn pressure_from_air(&self, air: f64) -> f64 {
        if let Some(volume) = self.linear_volume_m3 {
            return air / volume;
        }
        let index = self
            .travel_table
            .partition_point(|point| point.0 <= air)
            .saturating_sub(1)
            .min(self.travel_table.len() - 2);
        let (a, x) = self.travel_table[index];
        let (b, y) = self.travel_table[index + 1];
        let travel = x + (air - a) / (b - a) * (y - x);
        let displacement = travel * self.area();
        (air - displacement * ATMOSPHERE_PSI) / (self.pipe_volume() + displacement)
    }
    fn calculate(&self, air: f64, delta_psi: f64, target: f64) -> (f64, f64) {
        let mut air = air + delta_psi * (self.cylinder_volume() + self.pipe_volume());
        let pressure = self.pressure_from_air(air);
        if delta_psi > 0. && (pressure > self.profile.max_psi + 0.1 || pressure > target + 0.1) {
            air = self.air_at_pressure(self.profile.max_psi.min(target));
        } else if delta_psi < 0. && (pressure < 0. || pressure < target - 0.1) {
            air = self.air_at_pressure(target.max(0.));
        }
        (air, self.pressure_from_air(air))
    }
    pub fn vent_fraction(&mut self, fraction: f64) {
        self.cylinder_air_psi_m3 *= fraction;
        self.pressure_psi = self.pressure_from_air(self.cylinder_air_psi_m3);
        self.auto_air_psi_m3 *= fraction;
        self.auto_pressure_psi *= fraction;
    }
    pub fn precharge(&mut self, command: f64) {
        self.auto_pressure_psi = command.clamp(0., 1.) * self.profile.service_psi;
        // EP.Initialize increases AutoCylPressure after AirSinglePipe.Initialize;
        // it deliberately leaves AutoCylAir at zero (important on AuxRes release).
        self.auto_air_psi_m3 = 0.;
        self.cylinder_air_psi_m3 = self.air_at_pressure(self.auto_pressure_psi);
        self.pressure_psi = self.pressure_from_air(self.cylinder_air_psi_m3);
    }
    pub fn step(&mut self, command: f64, dt: f64, speed_mps: f64) {
        let profile = &self.profile;
        if profile.low_stage_psi.is_some() {
            if !self.low_stage && speed_mps.abs() < profile.stage_down_mps {
                self.low_stage = true
            } else if self.low_stage && speed_mps.abs() > profile.stage_up_mps {
                self.low_stage = false
            }
        }
        let mut target = command.clamp(0., 1.) * profile.service_psi;
        if self.low_stage {
            target = target.min(profile.low_stage_psi.unwrap_or(target));
        }
        if command <= 0. || self.auto_pressure_psi > target {
            let delta =
                dt * profile.release_psi_s * self.auto_pressure_psi / profile.reference_psi * 2.5;
            if profile.main_reservoir {
                self.auto_pressure_psi = (self.auto_pressure_psi - delta).max(0.)
            } else {
                (self.auto_air_psi_m3, self.auto_pressure_psi) =
                    self.calculate(self.auto_air_psi_m3, -delta, 0.);
            }
        }
        // Cylinder update precedes the next EP application, exactly as the native
        // base.Update → EP.Update order; do not collapse it to the handle value.
        (self.cylinder_air_psi_m3, self.pressure_psi) = self.calculate(
            self.cylinder_air_psi_m3,
            self.auto_pressure_psi - self.pressure_psi,
            self.auto_pressure_psi,
        );
        self.auxiliary_psi = (self.auxiliary_psi + dt * self.profile.charging_psi_s).min(70.);
        if self.auto_pressure_psi < target {
            let ratio =
                self.profile.auxiliary_volume_m3 / (self.cylinder_volume() + self.pipe_volume());
            let delta = (dt * self.profile.application_psi_s)
                .min(target - self.auto_pressure_psi)
                .min((self.auxiliary_psi - self.auto_pressure_psi) * ratio / (1. + ratio))
                .max(0.);
            self.auxiliary_psi -= delta / ratio;
            self.auto_pressure_psi += delta;
        }
    }
    pub fn shoe_force_n(&self, nominal: f64) -> f64 {
        nominal
            * ((self.pressure_psi - self.profile.spring_psi)
                / (self.profile.reference_psi - self.profile.spring_psi))
                .max(0.)
    }
    pub fn friction_coefficient(&self, shoe_force: f64, speed_mps: f64) -> Option<f64> {
        let (k1, k2, k3, k4, k5) = match self.profile.shoe_type.to_ascii_lowercase().as_str() {
            "cast_iron_p10" => (0.05, 62.5, 31.25, 100., 20.),
            "cast_iron_p6" => (0.024, 62.5, 12.5, 100., 20.),
            "high_friction_composite" => (0.055, 200., 50., 150., 75.),
            "disc_pads" => (0.385, -24.5, -27.2, 39.5, 33.),
            _ => return None,
        };
        let per_shoe_kn = (shoe_force / self.profile.shoe_count / 1000.).min(20.);
        let speed_kmh = speed_mps.abs() * 3.6;
        Some(k1 * (per_shoe_kn + k2) / (per_shoe_kn + k3) * (speed_kmh + k4) / (speed_kmh + k5))
    }
}

/// Normal UIC distributor applications with pneumatic pipe and relay valves.
/// Emergency, quick-service bulbs and wheel-slide equipment are not modeled here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NativeAirState {
    pub profile: openrailsrs_formats::NativeAirBrakeProfile,
    pub cylinder: NativeEpState,
    pub pipe_psi: f64,
    #[serde(default)]
    pub emergency: Option<AirEmergencyState>,
    #[serde(default)]
    pub independent_psi: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum AirValve {
    Release,
    Apply,
    Lap,
    Emergency,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AirEmergencyState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_previous_kind: Option<String>,
    #[serde(default)]
    pub controller_minimum_reduction: bool,
    pub valve: AirValve,
    pub reservoir_psi: f64,
    pub control_psi: f64,
    pub equalizing_psi: f64,
    pub previous_pipe_psi: f64,
    pub smoothed_pipe_change_psi_s: f64,
    pub time_s: f64,
    pub dump_started_s: Option<f64>,
    pub dump_open: bool,
}
impl NativeAirState {
    pub fn new(profile: openrailsrs_formats::NativeAirBrakeProfile) -> Self {
        let mut cylinder = if let Some(volume) = profile.fixed_cylinder_volume_m3 {
            NativeEpState {
                profile: profile.cylinder.clone(),
                linear_volume_m3: Some(volume),
                pressure_psi: 0.,
                auto_pressure_psi: 0.,
                cylinder_air_psi_m3: 0.,
                auto_air_psi_m3: 0.,
                auxiliary_psi: profile.controller.charged_psi,
                low_stage: false,
                pipe_volume_ratio: 0.,
                travel_table: vec![],
            }
        } else {
            NativeEpState::with_pipe_volume(profile.cylinder.clone(), profile.pipe_volume_ratio)
        };
        cylinder.auxiliary_psi = profile.controller.charged_psi;
        let emergency = profile.emergency.as_ref().map(|_| AirEmergencyState {
            controller_previous_kind: None,
            controller_minimum_reduction: false,
            valve: AirValve::Release,
            reservoir_psi: if profile.emergency.as_ref().unwrap().reservoir_present {
                profile.controller.charged_psi
            } else {
                // AirSinglePipe keeps its field initializer when no emergency
                // reservoir is fitted (Initialize only charges fitted tanks).
                // This diagnostic pressure never supplies absent hardware.
                64.
            },
            control_psi: profile.controller.charged_psi,
            previous_pipe_psi: profile.controller.charged_psi,
            equalizing_psi: profile.controller.charged_psi,
            smoothed_pipe_change_psi_s: 0.,
            time_s: 0.,
            dump_started_s: None,
            dump_open: false,
        });
        Self {
            pipe_psi: profile.controller.charged_psi,
            cylinder,
            profile,
            emergency,
            independent_psi: 0.,
        }
    }
    pub(crate) fn initialize_native(&mut self, equalizing_psi: f64, immediate_release: bool) {
        self.initialize_released(equalizing_psi);
        if immediate_release {
            return;
        }
        let count = self.profile.cylinder_count;
        let c = &mut self.cylinder;
        let total_air = (self.profile.controller.charged_psi - equalizing_psi).max(0.)
            * c.profile.auxiliary_volume_m3;
        c.auto_pressure_psi = c
            .pressure_from_air(total_air / count)
            .min(c.profile.max_psi);
        c.auto_air_psi_m3 = c.air_at_pressure(c.auto_pressure_psi);
        c.cylinder_air_psi_m3 = c.air_at_pressure(c.auto_pressure_psi * self.profile.relay_ratio);
        c.pressure_psi = c.pressure_from_air(c.cylinder_air_psi_m3);
        c.auxiliary_psi = (self.profile.controller.charged_psi
            - c.auto_air_psi_m3 * count / c.profile.auxiliary_volume_m3)
            .max(equalizing_psi);
        if let Some(e) = &mut self.emergency {
            if self.profile.emergency.as_ref().unwrap().reservoir_present {
                e.reservoir_psi = c.auxiliary_psi.max(self.profile.controller.charged_psi);
            }
            e.valve = if c.auto_pressure_psi < 1. {
                AirValve::Release
            } else {
                AirValve::Lap
            };
        }
    }
    /// MSTSBrakeController.UpdatePressure, for the supported authored notches.
    /// Calculations use native bar/Single boundaries; the valve kernel stores
    /// PSI. The handle is the actual controller value, not SetPercent demand.
    pub(crate) fn update_train_controller(&mut self, command: f64, main_psi: f64, dt: f64) {
        let p = &self.profile.controller;
        let Some(notched) = &p.notched else {
            return;
        };
        let Some(e) = &mut self.emergency else {
            return;
        };
        let (x, kind) = notched.setting(command);
        let bar = PSI_TO_BAR as f32;
        let maximum = p.charged_psi as f32 * bar;
        let minimum = notched.minimum_reduction_psi as f32 * bar;
        let full = p.full_service_drop_psi as f32 * bar;
        let main = main_psi as f32 * bar;
        let mut pressure = e.equalizing_psi as f32 * bar;
        let h = dt as f32;
        let apply = p.application_psi_s as f32 * bar;
        let release = p.release_psi_s as f32 * bar;
        if e.controller_previous_kind.is_none() && matches!(kind, "lap" | "minimalreduction") {
            e.controller_minimum_reduction = true;
        }
        let change = |value: &mut f32, target: f32, rate: f32, increase: bool| {
            let difference = if increase {
                target - *value
            } else {
                *value - target
            };
            if difference > 0. {
                let dp = (rate
                    * h
                    * if difference < 0.5 {
                        (difference / 0.5 + 0.1).min(1.)
                    } else {
                        1.
                    })
                .min(difference);
                if increase {
                    *value += dp;
                } else {
                    *value -= dp;
                }
            }
        };
        if matches!(kind, "hold" | "lap" | "minimalreduction") {
            if e.controller_minimum_reduction {
                change(&mut pressure, maximum - minimum, apply, false);
            }
        } else {
            e.controller_minimum_reduction = false;
        }
        match kind {
            "release" | "fullquickrelease" => {
                let rate = if kind == "fullquickrelease" {
                    notched.quick_release_psi_s as f32 * bar
                } else {
                    release
                };
                change(&mut pressure, maximum.min(main), rate, true);
                change(
                    &mut pressure,
                    maximum,
                    notched.overcharge_elimination_psi_s as f32 * bar,
                    false,
                );
            }
            "slowservice" => change(
                &mut pressure,
                maximum - full,
                notched.slow_application_psi_s as f32 * bar,
                false,
            ),
            "apply" => pressure -= x * apply * h,
            "fullservice" => {
                e.controller_minimum_reduction = true;
                change(&mut pressure, maximum - full, apply, false);
            }
            "continuousservice"
            | "graduatedselflaplimitedholding"
            | "graduatedselflaplimited"
            | "graduatedselflap"
            | "suppression" => {
                e.controller_minimum_reduction = true;
                let target = maximum - minimum * (1. - x) - full * x;
                change(&mut pressure, target, apply, false);
                // ForceControllerReleaseGraduated=false is the native default.
                // Only GSelfLap releases automatically at a lower demand.
                if matches!(kind, "graduatedselflap" | "graduatedselflaplimited") {
                    change(&mut pressure, target.min(main), release, true);
                }
            }
            "lap" if e.controller_previous_kind.as_deref() == Some("running") => {
                e.controller_minimum_reduction = true
            }
            "minimalreduction"
                if matches!(
                    e.controller_previous_kind.as_deref(),
                    Some("running" | "release" | "fullquickrelease")
                ) =>
            {
                e.controller_minimum_reduction = true
            }
            "emergency" => pressure -= p.emergency_psi_s as f32 * bar * h,
            "dummy" => {
                let fraction = notched
                    .notches
                    .iter()
                    .rev()
                    .find(|n| n.0 as f32 <= command as f32)
                    .is_some_and(|n| n.1);
                let target = maximum - full * if fraction { x } else { command as f32 };
                change(&mut pressure, target.min(main), release, true);
                change(&mut pressure, target, apply, false);
            }
            _ => (),
        }
        e.equalizing_psi = pressure.max(0.) as f64 / bar as f64;
        e.controller_previous_kind = Some(kind.to_string());
    }
    /// Source-equivalent immediate-release initialization. The train's EQ
    /// pressure is independent of the controller's maximum charge pressure;
    /// a partially charged pipe must not precharge every car to that maximum.
    pub(crate) fn initialize_released(&mut self, equalizing_psi: f64) {
        self.pipe_psi = equalizing_psi;
        self.independent_psi = 0.;
        self.cylinder.auto_pressure_psi = 0.;
        self.cylinder.auto_air_psi_m3 = 0.;
        self.cylinder.cylinder_air_psi_m3 = self.cylinder.air_at_pressure(0.);
        self.cylinder.pressure_psi = self
            .cylinder
            .pressure_from_air(self.cylinder.cylinder_air_psi_m3);
        self.cylinder.auxiliary_psi = self.profile.controller.charged_psi.max(equalizing_psi);
        if let Some(e) = &mut self.emergency {
            e.valve = AirValve::Release;
            if self.profile.emergency.as_ref().unwrap().reservoir_present {
                e.reservoir_psi = self
                    .cylinder
                    .auxiliary_psi
                    .max(self.profile.controller.charged_psi);
            }
            e.control_psi = self.profile.controller.charged_psi;
            e.equalizing_psi = equalizing_psi;
            e.previous_pipe_psi = equalizing_psi;
            e.smoothed_pipe_change_psi_s = 0.;
            e.time_s = 0.;
            e.dump_started_s = None;
            e.dump_open = false;
        }
    }
    fn target(&self) -> f64 {
        ((self.profile.controller.charged_psi - self.pipe_psi) * self.profile.triple_ratio)
            .max(0.)
            .min(self.profile.cylinder.service_psi / self.profile.relay_ratio)
    }
    pub fn precharge(&mut self, command: f64) {
        self.pipe_psi = self.profile.controller.charged_psi
            - command.clamp(0., 1.) * self.profile.controller.full_service_drop_psi;
        let target = (self.target() * self.profile.relay_ratio).min(self.profile.cylinder.max_psi);
        self.cylinder.auto_pressure_psi = self.target();
        self.cylinder.auto_air_psi_m3 = self
            .cylinder
            .air_at_pressure(self.cylinder.auto_pressure_psi);
        self.cylinder.cylinder_air_psi_m3 = self.cylinder.air_at_pressure(target);
        self.cylinder.pressure_psi = self
            .cylinder
            .pressure_from_air(self.cylinder.cylinder_air_psi_m3);
        if let Some(e) = &mut self.emergency {
            e.previous_pipe_psi = self.pipe_psi;
            e.equalizing_psi = self.pipe_psi;
            e.smoothed_pipe_change_psi_s = 0.;
            e.dump_started_s = None;
            e.dump_open = false;
            e.valve = if command > 0. {
                AirValve::Lap
            } else {
                AirValve::Release
            };
        }
    }
    pub fn step_with_emergency(&mut self, command: f64, emergency: bool, dt: f64) {
        if self.emergency.is_some() {
            self.advance_emergency(dt);
            return;
        }
        let control = &self.profile.controller;
        let demanded_pipe = if emergency {
            0.0
        } else {
            control.charged_psi - command.clamp(0., 1.) * control.full_service_drop_psi
        };
        let rate = if demanded_pipe < self.pipe_psi {
            control.application_psi_s
        } else {
            control.release_psi_s
        };
        self.pipe_psi += (demanded_pipe - self.pipe_psi).clamp(-rate * dt, rate * dt);
        let target = self.target();
        let cylinder = &mut self.cylinder;
        let old = cylinder.auto_pressure_psi;
        let mut delta = if old < target {
            (dt * cylinder.profile.application_psi_s).min(target - old)
        } else {
            -(dt * cylinder.profile.release_psi_s * old / cylinder.profile.reference_psi * 2.5)
                .min(old - target)
        };
        // Native distributor sensitivity is 0.1 bar. Once applying, the valve
        // approaches the demand; otherwise it holds the trapped cylinder air.
        if old < target && target - old < 1. {
            delta *= (0.1 + 0.9 * (target - old)).clamp(0.1, 1.);
        }
        if self.profile.relay_ratio != 1. {
            cylinder.auto_pressure_psi = (old + delta).max(0.);
        } else {
            (cylinder.auto_air_psi_m3, cylinder.auto_pressure_psi) =
                cylinder.calculate(cylinder.auto_air_psi_m3, delta, target);
        }
        let demanded =
            (cylinder.auto_pressure_psi * self.profile.relay_ratio).min(cylinder.profile.max_psi);
        (cylinder.cylinder_air_psi_m3, cylinder.pressure_psi) = cylinder.calculate(
            cylinder.cylinder_air_psi_m3,
            demanded - cylinder.pressure_psi,
            demanded,
        );
    }

    pub fn valid(&self) -> bool {
        self.pipe_psi.is_finite()
            && (0. ..=1000.).contains(&self.pipe_psi)
            && self.independent_psi.is_finite()
            && (0. ..=1000.).contains(&self.independent_psi)
            && [
                self.cylinder.pressure_psi,
                self.cylinder.auto_pressure_psi,
                self.cylinder.cylinder_air_psi_m3,
                self.cylinder.auto_air_psi_m3,
                self.cylinder.auxiliary_psi,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= -0.001)
            && match (&self.emergency, &self.profile.emergency) {
                (None, None) => true,
                (Some(s), Some(p)) => {
                    p.valid()
                        && [
                            s.reservoir_psi,
                            s.control_psi,
                            s.equalizing_psi,
                            s.previous_pipe_psi,
                            s.time_s,
                        ]
                        .iter()
                        .all(|v| v.is_finite() && *v >= 0.)
                        && s.smoothed_pipe_change_psi_s.is_finite()
                        && s.controller_previous_kind.as_ref().is_none_or(|kind| {
                            self.profile
                                .controller
                                .notched
                                .as_ref()
                                .is_some_and(|n| n.notches.iter().any(|v| &v.2 == kind))
                        })
                        && s.dump_started_s
                            .is_none_or(|v| v.is_finite() && v >= 0. && v <= s.time_s)
                        && (s.dump_open == s.dump_started_s.is_some())
                }
                _ => false,
            }
    }

    /// Supported branch of original AirSinglePipe.Update: advanced cylinder,
    /// auxiliary source, plain triple/distributor, optional emergency reservoir
    /// and quick action. Unsupported extra valves are rejected by the loader.
    fn advance_emergency(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let p = self.profile.emergency.as_ref().expect("emergency hardware");
        let s = self.emergency.as_mut().expect("emergency state");
        let cylinder = &mut self.cylinder;
        let ratio = self.profile.triple_ratio;
        let count = self.profile.cylinder_count;
        let aux_volume = cylinder.profile.auxiliary_volume_m3;
        let emergency_aux = p.reservoir_volume_m3 / aux_volume;
        let aux_pipe = aux_volume / p.pipe_volume_m3;
        let old_valve = s.valve;
        // Original calculates threshold before selecting this tick's valve.
        let mut threshold = if old_valve == AirValve::Emergency {
            cylinder.profile.max_psi
        } else if p.distributor {
            ((s.control_psi - self.pipe_psi) * ratio).max(0.)
        } else if old_valve == AirValve::Release {
            0.
        } else {
            cylinder.profile.max_psi
        };
        if old_valve != AirValve::Emergency {
            threshold = threshold.min(cylinder.profile.service_psi);
        }
        if p.distributor && threshold < p.initial_application_psi * ratio {
            threshold = 0.;
        }
        let change = ((self.pipe_psi - s.previous_pipe_psi) / dt.max(0.0001)) as f32;
        let smoothing_rate = (-60. * (1. - 1. / (60. * 0.25_f64)).ln()) as f32;
        let weight = (-smoothing_rate * dt as f32).exp();
        s.smoothed_pipe_change_psi_s = if weight < 0.5 {
            change
        } else {
            s.smoothed_pipe_change_psi_s as f32 * weight + change * (1. - weight)
        } as f64;
        let tripped = if p.legacy_actuation {
            self.pipe_psi <= 0.75 * s.reservoir_psi * ratio / (ratio + 1.)
        } else {
            -s.smoothed_pipe_change_psi_s > p.actuation_psi_s
        };
        if p.actuation_psi_s > 0. && tripped {
            s.valve = AirValve::Emergency;
        } else if p.distributor {
            let application = s.control_psi - self.pipe_psi;
            let target = application * ratio;
            if s.valve != AirValve::Emergency
                && target
                    > cylinder.auto_pressure_psi
                        + if old_valve == AirValve::Apply {
                            0.
                        } else {
                            p.sensitivity_psi * ratio
                        }
                && (old_valve != AirValve::Release || application > p.initial_application_psi)
            {
                s.valve = AirValve::Apply;
            } else if target
                < cylinder.auto_pressure_psi
                    - if old_valve == AirValve::Release {
                        0.
                    } else {
                        p.sensitivity_psi * ratio
                    }
                || application < p.initial_application_psi
            {
                s.valve = AirValve::Release;
            } else if s.valve != AirValve::Emergency || self.pipe_psi > cylinder.auxiliary_psi {
                s.valve = AirValve::Lap;
            }
        } else if s.valve != AirValve::Emergency
            && self.pipe_psi
                < cylinder.auxiliary_psi
                    - if old_valve == AirValve::Apply {
                        0.
                    } else {
                        p.sensitivity_psi
                    }
            && (old_valve != AirValve::Release
                || self.pipe_psi < cylinder.auxiliary_psi - p.initial_application_psi)
        {
            s.valve = AirValve::Apply;
        } else if self.pipe_psi
            > cylinder.auxiliary_psi
                + if old_valve == AirValve::Release {
                    0.
                } else {
                    p.sensitivity_psi * 1.5
                }
        {
            s.valve = AirValve::Release;
        } else if s.valve == AirValve::Apply {
            s.valve = AirValve::Lap;
        }
        if s.valve == AirValve::Emergency && old_valve != AirValve::Emergency {
            s.dump_started_s = Some(s.time_s);
        } else if s.valve != AirValve::Emergency {
            s.dump_started_s = None;
        }
        if matches!(s.valve, AirValve::Apply | AirValve::Emergency) {
            let rate = if s.valve == AirValve::Emergency
                && (!p.quick_action || self.pipe_psi < cylinder.auto_pressure_psi)
            {
                p.maximum_application_psi_s
            } else {
                cylinder.profile.application_psi_s
            };
            let mut dp = dt * rate;
            if cylinder.auxiliary_psi - cylinder.auto_pressure_psi < 20. {
                dp = dp.min(
                    (p.maximum_application_psi_s
                        * (cylinder.auxiliary_psi - cylinder.auto_pressure_psi)
                        / 20.)
                        .max(0.),
                );
            }
            if s.valve != AirValve::Emergency && self.pipe_psi > cylinder.auxiliary_psi - 1. {
                dp *= (0.1 + 0.9 * (cylinder.auxiliary_psi - self.pipe_psi)).clamp(0.1, 1.);
            } else if p.distributor && cylinder.auto_pressure_psi > threshold - 1. {
                dp *= (0.1 + 0.9 * (threshold - cylinder.auto_pressure_psi)).clamp(0.1, 1.);
            }
            dp = dp
                .min((cylinder.auxiliary_psi - cylinder.auto_pressure_psi) * ratio / (1. + ratio))
                .min(threshold - cylinder.auto_pressure_psi)
                .min((cylinder.auxiliary_psi - self.pipe_psi) * ratio)
                .max(0.);
            let before_air = cylinder.auto_air_psi_m3;
            (cylinder.auto_air_psi_m3, cylinder.auto_pressure_psi) =
                cylinder.calculate(cylinder.auto_air_psi_m3, dp, threshold);
            cylinder.auxiliary_psi -= (cylinder.auto_air_psi_m3 - before_air) * count / aux_volume;
            if s.valve == AirValve::Emergency {
                if p.reservoir_present {
                    if p.dump_duration_s != 0. && s.dump_started_s.is_none() && self.pipe_psi > 20.
                    {
                        // AirSinglePipe's accelerated emergency release: once
                        // the timed vent closes and BP exceeds its native
                        // 20 psi threshold, route BC -> auxiliary -> pipe.
                        let release = (dt * cylinder.profile.release_psi_s).min(
                            ((cylinder.auto_pressure_psi - cylinder.auxiliary_psi) * ratio
                                / (1. + ratio))
                                .max(0.),
                        );
                        (cylinder.auto_air_psi_m3, cylinder.auto_pressure_psi) = cylinder
                            .calculate(cylinder.auto_air_psi_m3, -release, cylinder.auxiliary_psi);
                        cylinder.auxiliary_psi += release / ratio;
                        let transfer = (dt * cylinder.profile.charging_psi_s).min(
                            ((cylinder.auxiliary_psi - self.pipe_psi) / (1. + aux_pipe)).max(0.),
                        );
                        cylinder.auxiliary_psi -= transfer;
                        self.pipe_psi += transfer * aux_pipe;
                    } else {
                        let transfer = (dt * p.maximum_application_psi_s)
                            .min((s.reservoir_psi - cylinder.auxiliary_psi) / (1. + emergency_aux))
                            .max(0.);
                        s.reservoir_psi -= transfer;
                        cylinder.auxiliary_psi += transfer * emergency_aux;
                    }
                }
                if let Some(start) = s.dump_started_s
                    && (p.dump_duration_s == 0. && self.pipe_psi < 1.
                        || p.dump_duration_s > 0. && s.time_s - start > p.dump_duration_s)
                {
                    s.dump_started_s = None;
                }
                if s.dump_started_s.is_some() {
                    self.pipe_psi = (self.pipe_psi - dt * p.dump_psi_s).max(0.);
                }
                if p.quick_action && self.pipe_psi > cylinder.auto_pressure_psi {
                    let volume_ratio = (cylinder.cylinder_volume() + cylinder.pipe_volume())
                        * count
                        / p.pipe_volume_m3;
                    let dp = (dt * p.maximum_application_psi_s)
                        .min(cylinder.profile.max_psi - cylinder.auto_pressure_psi)
                        .min((self.pipe_psi - cylinder.auto_pressure_psi) / (1. + volume_ratio))
                        .max(0.);
                    let before = cylinder.auto_air_psi_m3;
                    (cylinder.auto_air_psi_m3, cylinder.auto_pressure_psi) =
                        cylinder.calculate(cylinder.auto_air_psi_m3, dp, self.pipe_psi);
                    self.pipe_psi = (self.pipe_psi
                        - (cylinder.auto_air_psi_m3 - before) * count / p.pipe_volume_m3)
                        .max(0.);
                }
            }
        }
        if s.valve == AirValve::Release {
            if p.distributor && s.control_psi < self.pipe_psi {
                s.control_psi = self.pipe_psi;
            }
            if cylinder.auxiliary_psi > self.pipe_psi && p.insensitivity_psi_s > 0. {
                let transfer = (dt * p.insensitivity_psi_s)
                    .min((cylinder.auxiliary_psi - self.pipe_psi) / (1. + aux_pipe));
                cylinder.auxiliary_psi -= transfer;
                self.pipe_psi += transfer * aux_pipe;
            }
            if cylinder.auto_pressure_psi > threshold {
                let release = dt
                    * cylinder.profile.release_psi_s
                    * if cylinder.linear_volume_m3.is_some() {
                        1.
                    } else {
                        cylinder.auto_pressure_psi / cylinder.profile.reference_psi * 2.5
                    };
                (cylinder.auto_air_psi_m3, cylinder.auto_pressure_psi) =
                    cylinder.calculate(cylinder.auto_air_psi_m3, -release, threshold);
            }
        }
        if p.reservoir_present {
            if s.valve == AirValve::Release && s.reservoir_psi > self.pipe_psi {
                let dp = (dt * cylinder.profile.charging_psi_s)
                    .min(
                        (s.reservoir_psi - cylinder.auxiliary_psi) * emergency_aux
                            / (1. + emergency_aux),
                    )
                    .min(self.pipe_psi - cylinder.auxiliary_psi)
                    .max(0.);
                cylinder.auxiliary_psi += dp;
                s.reservoir_psi -= dp / emergency_aux;
            }
            if cylinder.auxiliary_psi > s.reservoir_psi
                && (p.distributor || s.valve == AirValve::Release)
            {
                let dp = (dt * p.reservoir_charging_psi_s)
                    .min((cylinder.auxiliary_psi - s.reservoir_psi) / (1. + emergency_aux));
                s.reservoir_psi += dp;
                cylinder.auxiliary_psi -= dp * emergency_aux;
            }
        }
        if cylinder.auxiliary_psi < self.pipe_psi && (p.distributor || s.valve == AirValve::Release)
        {
            let mut dp = dt * cylinder.profile.charging_psi_s;
            if cylinder.auxiliary_psi > self.pipe_psi - 1. {
                dp *= (0.1 + 0.9 * (self.pipe_psi - cylinder.auxiliary_psi)).clamp(0.1, 1.);
            }
            dp = dp.min((self.pipe_psi - cylinder.auxiliary_psi) / (1. + aux_pipe));
            cylinder.auxiliary_psi += dp;
            self.pipe_psi -= dp * aux_pipe;
        } else if !p.distributor && p.insensitivity_psi_s > 0. {
            let difference = self.pipe_psi - cylinder.auxiliary_psi;
            let dp = dt * p.insensitivity_psi_s / 6.
                * difference.signum()
                * (difference.abs() / 0.1).min(1.);
            cylinder.auxiliary_psi += dp;
            self.pipe_psi -= dp * aux_pipe;
        }
        let demand = cylinder.auto_pressure_psi.max(self.independent_psi);
        (cylinder.cylinder_air_psi_m3, cylinder.pressure_psi) = cylinder.calculate(
            cylinder.cylinder_air_psi_m3,
            demand - cylinder.pressure_psi,
            demand,
        );
        s.dump_open = s.dump_started_s.is_some();
        s.previous_pipe_psi = self.pipe_psi;
        s.time_s += dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(main_reservoir: bool) -> NativeEpBrakeProfile {
        NativeEpBrakeProfile {
            diameter_m: 0.3556,
            stroke_m: 0.2032,
            spring_psi: 5.,
            reference_psi: 50.,
            max_psi: if main_reservoir { 45. } else { 90. },
            service_psi: if main_reservoir { 45. } else { 90. },
            application_psi_s: 3.,
            release_psi_s: 10.,
            auxiliary_volume_m3: 0.06923534,
            charging_psi_s: 2.,
            shoe_count: 8.,
            shoe_type: "Cast_Iron_P10".into(),
            main_reservoir,
            low_stage_psi: if main_reservoir { None } else { Some(50.) },
            stage_up_mps: 17.8816,
            stage_down_mps: 13.4112,
        }
    }
    #[test]
    fn released_initialization_keeps_eq_distinct_from_charged_reservoirs() {
        let ast = openrailsrs_formats::parse_vehicle_text(include_str!(
            "../../../examples/physics_lab/vehicles/hydraulic.eng"
        ))
        .unwrap();
        let profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast)
            .native_air
            .unwrap();
        let mut state = NativeAirState::new(profile);
        state.initialize_released(0.);
        assert_eq!(state.pipe_psi, 0.);
        assert_eq!(state.cylinder.auto_pressure_psi, 0.);
        assert_eq!(state.cylinder.auxiliary_psi, 90.);
        let emergency = state.emergency.as_ref().unwrap();
        assert_eq!(emergency.reservoir_psi, 90.);
        assert_eq!(emergency.control_psi, 90.);
        assert_eq!(emergency.previous_pipe_psi, 0.);
        assert_eq!(emergency.smoothed_pipe_change_psi_s, 0.);
        assert!(state.valid());
    }
    #[test]
    fn emergency_release_returns_auxiliary_air_only_after_the_timed_vent_closes() {
        let ast = openrailsrs_formats::parse_vehicle_text(include_str!(
            "../../../examples/physics_lab/vehicles/hydraulic.eng"
        ))
        .unwrap();
        let profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast)
            .native_air
            .unwrap();
        let mut released = NativeAirState::new(profile);
        released.pipe_psi = 20.4;
        released.cylinder.auto_pressure_psi = 70.;
        released.cylinder.auto_air_psi_m3 = released.cylinder.air_at_pressure(70.);
        released.cylinder.auxiliary_psi = 78.;
        let e = released.emergency.as_mut().unwrap();
        e.valve = AirValve::Emergency;
        e.previous_pipe_psi = 20.4;
        e.time_s = 6.;
        e.dump_started_s = None;
        let reservoir = e.reservoir_psi;
        let mut dumping = released.clone();
        dumping.emergency.as_mut().unwrap().dump_started_s = Some(5.9);
        dumping.emergency.as_mut().unwrap().dump_open = true;
        released.advance_emergency(0.05);
        dumping.advance_emergency(0.05);
        assert!(released.pipe_psi > 20.4);
        assert_eq!(
            released.emergency.as_ref().unwrap().reservoir_psi,
            reservoir
        );
        assert!(released.cylinder.auxiliary_psi < 78.);
        assert!(dumping.pipe_psi < 20.4);
        assert!(dumping.emergency.as_ref().unwrap().reservoir_psi < reservoir);
        assert!(released.valid() && dumping.valid());
    }
    #[test]
    fn absent_emergency_tank_keeps_native_diagnostic_without_supplying_air() {
        let ast = openrailsrs_formats::parse_vehicle_text(include_str!(
            "../../../examples/physics_lab/vehicles/hydraulic.eng"
        ))
        .unwrap();
        let mut profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast)
            .native_air
            .unwrap();
        profile.emergency.as_mut().unwrap().reservoir_present = false;
        let mut state = NativeAirState::new(profile);
        state.initialize_released(0.);
        assert_eq!(state.emergency.as_ref().unwrap().reservoir_psi, 64.);
        for _ in 0..100 {
            state.pipe_psi = 90.;
            state.advance_emergency(0.05);
        }
        assert_eq!(state.emergency.as_ref().unwrap().reservoir_psi, 64.);
        assert_eq!(state.cylinder.pressure_psi, 0.);
        assert!(state.valid());
    }
    #[test]
    fn native_cylinder_travel_and_shoe_force_have_distinct_units() {
        let mut cylinder = NativeEpState::new(profile(true));
        cylinder.precharge(0.25);
        assert!((cylinder.pressure_psi - 11.3333988).abs() < 0.0001);
        let mut coach = NativeEpState::new(profile(false));
        coach.precharge(0.25);
        assert!((coach.pressure_psi - 22.71336).abs() < 0.0001);
        cylinder.precharge(1.);
        let shoes = cylinder.shoe_force_n(367174.);
        let rim = shoes * cylinder.friction_coefficient(shoes, 0.).unwrap();
        assert!((rim - 131347.1).abs() < 1.);
    }
    #[test]
    fn ep_aux_release_differs_from_main_reservoir_and_stages_have_hysteresis() {
        let mut main = NativeEpState::new(profile(true));
        let mut aux = NativeEpState::new(profile(false));
        main.precharge(1.);
        aux.precharge(1.);
        for _ in 0..20 {
            main.step(0., 0.05, 0.);
            aux.step(0., 0.05, 0.);
        }
        // Main-reservoir release reduces the control pressure exponentially:
        // 45 * (1 - dt * rate / reference * 2.5)^20. Cylinder travel remains
        // a separate state, so it is not an arbitrary fixed pressure target.
        assert!((main.auto_pressure_psi - 45. * 0.975f64.powi(20)).abs() < 1e-10);
        assert!((20.0..35.0).contains(&main.pressure_psi));
        assert!(aux.pressure_psi < 0.001);
        aux.step(1., 0.05, 0.);
        assert!(aux.low_stage);
        aux.step(1., 0.05, 18.);
        assert!(!aux.low_stage);
        aux.step(1., 0.05, 15.);
        assert!(!aux.low_stage);
        aux.step(1., 0.05, 13.);
        assert!(aux.low_stage);
    }
}
