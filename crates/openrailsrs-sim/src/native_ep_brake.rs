//! Advanced cylinder travel and EP-only wire from pinned Open Rails 1.6.1:
//! AirSinglePipe.{AdvancedBrakeCylinderAir,CalculateBrakeCylinderPressure,Update}
//! and EPBrakeSystem.Update. Shoe force is distinct from wheel-rim brake force.
use openrailsrs_formats::NativeEpBrakeProfile;
use serde::{Deserialize, Serialize};

const ATMOSPHERE_PSI: f64 = 14.696;
pub(crate) const PSI_TO_BAR: f64 = 0.0689475729;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NativeEpState {
    pub profile: NativeEpBrakeProfile,
    pub pressure_psi: f64,
    auto_pressure_psi: f64,
    cylinder_air_psi_m3: f64,
    auto_air_psi_m3: f64,
    auxiliary_psi: f64,
    low_stage: bool,
    travel_table: Vec<(f64, f64)>,
}

impl NativeEpState {
    pub fn new(profile: NativeEpBrakeProfile) -> Self {
        let mut state = Self {
            profile,
            pressure_psi: 0.,
            auto_pressure_psi: 0.,
            cylinder_air_psi_m3: 0.,
            auto_air_psi_m3: 0.,
            auxiliary_psi: 70.,
            low_stage: false,
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
        self.area() * self.profile.stroke_m
    }
    fn pipe_volume(&self) -> f64 {
        self.cylinder_volume() * 0.2
    }
    fn travel_at_pressure(&self, pressure: f64) -> f64 {
        self.profile.stroke_m
            * (0.8 * (2. * pressure / self.profile.spring_psi - 1.).clamp(0., 1.)
                + 0.2
                    * ((pressure - self.profile.spring_psi) / (50. - self.profile.spring_psi))
                        .max(0.))
    }
    fn air_at_pressure(&self, pressure: f64) -> f64 {
        self.pipe_volume() * pressure
            + self.travel_at_pressure(pressure) * self.area() * (pressure + ATMOSPHERE_PSI)
    }
    fn pressure_from_air(&self, air: f64) -> f64 {
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
