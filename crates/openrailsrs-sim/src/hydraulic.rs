//! Opt-in hydrodynamic transmission. All converter curves are authored inputs.
//! OR 1.6.1 names a Hydraulic transmission but supplies no equivalent converter
//! solver; this module is a Rust physical extension, never a native oracle.
use openrailsrs_formats::typed::HydraulicProfile;
use openrailsrs_train::diesel::DieselEngineParams;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HydraulicState {
    pub gear: usize,
    pub shift_remaining_s: f64,
    pub lock_up: bool,
    pub pump_rpm: f64,
    pub turbine_rpm: f64,
    pub speed_ratio: f64,
    pub torque_ratio: f64,
    pub pump_torque_nm: f64,
    pub turbine_torque_nm: f64,
    pub input_power_w: f64,
    pub output_power_w: f64,
    pub loss_power_w: f64,
    pub temperature_c: f64,
    pub heat_j: f64,
    pub cooled_j: f64,
    pub force_n: f64,
    pub overtemperature: bool,
}
impl Default for HydraulicState {
    fn default() -> Self {
        Self {
            gear: 0,
            shift_remaining_s: 0.,
            lock_up: false,
            pump_rpm: 0.,
            turbine_rpm: 0.,
            speed_ratio: 0.,
            torque_ratio: 0.,
            pump_torque_nm: 0.,
            turbine_torque_nm: 0.,
            input_power_w: 0.,
            output_power_w: 0.,
            loss_power_w: 0.,
            temperature_c: 20.,
            heat_j: 0.,
            cooled_j: 0.,
            force_n: 0.,
            overtemperature: false,
        }
    }
}
fn curve(p: &HydraulicProfile, ratio: f64) -> (f64, f64) {
    let j = p
        .converter
        .partition_point(|x| x.speed_ratio <= ratio)
        .saturating_sub(1)
        .min(p.converter.len() - 2);
    let a = &p.converter[j];
    let b = &p.converter[j + 1];
    let alpha = ((ratio - a.speed_ratio) / (b.speed_ratio - a.speed_ratio)).clamp(0., 1.);
    (
        a.torque_ratio + alpha * (b.torque_ratio - a.torque_ratio),
        a.capacity_nm_per_rad_s2 + alpha * (b.capacity_nm_per_rad_s2 - a.capacity_nm_per_rad_s2),
    )
}
impl HydraulicState {
    pub fn valid_for(&self, p: &HydraulicProfile) -> bool {
        self.gear < p.gear_ratios.len()
            && [
                self.shift_remaining_s,
                self.pump_rpm,
                self.turbine_rpm,
                self.speed_ratio,
                self.torque_ratio,
                self.pump_torque_nm,
                self.turbine_torque_nm,
                self.input_power_w,
                self.output_power_w,
                self.loss_power_w,
                self.heat_j,
                self.cooled_j,
                self.force_n,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            && (0. ..=500.).contains(&self.temperature_c)
            && self.shift_remaining_s <= p.shift_time_s
            && self.speed_ratio <= 1.
            && self.output_power_w <= self.input_power_w + 1e-6
            && (self.input_power_w - self.output_power_w - self.loss_power_w).abs() < 1e-5
            && (self.heat_j - self.cooled_j - (self.temperature_c - 20.) * p.heat_capacity_j_per_k)
                .abs()
                < 0.1
    }
    /// Integrates crankshaft inertia against measured converter pump load.
    /// Fuel is accounted by diesel_operation from this input power and RPM.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        p: &HydraulicProfile,
        engine: &DieselEngineParams,
        rpm: f64,
        throttle: f64,
        speed_mps: f64,
        running: bool,
        dt: f64,
    ) -> f64 {
        if !p.valid() || !dt.is_finite() || dt <= 0. {
            return rpm;
        }
        let throttle = throttle.clamp(0., 1.);
        let scale = std::f64::consts::TAU / 60.;
        let turbine = speed_mps.abs() / p.wheel_radius_m * p.gear_ratios[self.gear];
        let mut omega = (rpm * scale).max(0.);
        let ratio = (turbine / omega.max(0.01)).clamp(0., 1.);
        if self.shift_remaining_s <= 0. && !self.lock_up {
            if ratio > p.shift_up_ratio && self.gear + 1 < p.gear_ratios.len() {
                self.gear += 1;
                self.shift_remaining_s = p.shift_time_s;
            } else if ratio < p.shift_down_ratio && self.gear > 0 {
                self.gear -= 1;
                self.shift_remaining_s = p.shift_time_s;
            }
        }
        let turbine = speed_mps.abs() / p.wheel_radius_m * p.gear_ratios[self.gear];
        let ratio = (turbine / omega.max(0.01)).clamp(0., 1.);
        if self.lock_up && (ratio < p.lock_up_release_ratio || !running || throttle == 0.) {
            self.lock_up = false;
        } else if !self.lock_up
            && running
            && throttle > 0.
            && self.shift_remaining_s == 0.
            && p.lock_up_ratio.is_some_and(|r| ratio >= r)
        {
            self.lock_up = true;
        }
        let engaged = running && throttle > 0. && !self.overtemperature;
        let steps = (dt / 0.005).ceil().clamp(1., 2000.) as usize;
        let sub_dt = dt / steps as f64;
        let target = engine.target_rpm(throttle) * scale;
        let max_torque = engine.power_at_rpm(engine.max_rpm) / (engine.max_rpm * scale).max(1.);
        let mut input_energy = 0.;
        let mut output_energy = 0.;
        let mut force_impulse = 0.;
        for _ in 0..steps {
            let shifting = self.shift_remaining_s > 0.;
            self.shift_remaining_s = (self.shift_remaining_s - sub_dt).max(0.);
            let ratio = (turbine / omega.max(0.01)).clamp(0., 1.);
            let (torque_ratio, capacity) = curve(p, ratio);
            let governor = ((target - omega) * p.engine_inertia_kg_m2 / 0.25)
                .clamp(0., max_torque * throttle.max(0.08));
            let friction = max_torque * 0.02 * (omega / (engine.idle_rpm * scale).max(1.)).min(2.);
            let engine_torque = if running { governor + friction } else { 0. };
            let mut pump = if engaged && !shifting {
                capacity * omega * omega
            } else {
                0.
            };
            if self.lock_up && engaged {
                omega = turbine;
                pump = engine_torque.min(max_torque * throttle);
            } else {
                omega = (omega
                    + (engine_torque - friction - pump) / p.engine_inertia_kg_m2 * sub_dt)
                    .clamp(0., engine.max_rpm * 1.2 * scale);
            }
            let turbine_torque = pump * if self.lock_up { 1. } else { torque_ratio };
            // A converter cannot deliver more turbine power than its pump
            // input, including across inertia/shift integration boundaries.
            let input = pump * omega;
            let turbine_torque = if turbine > 0. {
                turbine_torque.min(input / turbine)
            } else {
                turbine_torque
            };
            let output = turbine_torque * turbine * p.final_efficiency;
            let force =
                turbine_torque * p.gear_ratios[self.gear] * p.final_efficiency / p.wheel_radius_m;
            input_energy += input * sub_dt;
            output_energy += output * sub_dt;
            force_impulse += force * sub_dt;
            self.pump_torque_nm = pump;
            self.turbine_torque_nm = turbine_torque;
            self.torque_ratio = if self.lock_up { 1. } else { torque_ratio };
            self.speed_ratio = (turbine / omega.max(0.01)).clamp(0., 1.);
        }
        self.input_power_w = input_energy / dt;
        self.output_power_w = output_energy / dt;
        self.loss_power_w = (input_energy - output_energy).max(0.) / dt;
        self.force_n = force_impulse / dt;
        self.heat_j += self.loss_power_w * dt;
        let cooling = ((self.temperature_c - 20.).max(0.) * p.cooling_w_per_k * dt)
            .min((self.heat_j - self.cooled_j).max(0.));
        self.cooled_j += cooling;
        self.temperature_c = 20. + (self.heat_j - self.cooled_j) / p.heat_capacity_j_per_k;
        if self.temperature_c >= p.max_temperature_c {
            self.overtemperature = true;
        } else if self.temperature_c < p.max_temperature_c - 10. {
            self.overtemperature = false;
        }
        self.pump_rpm = omega / scale;
        self.turbine_rpm = turbine / scale;
        self.pump_rpm
    }
}
