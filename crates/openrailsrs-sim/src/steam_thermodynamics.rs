//! Saturated vessel and cylinder energy model. The five property tables and
//! their Single interpolation come from GPL Open Rails 1.6.1 SteamTable.cs,
//! commit d16e670da333d26d2edfc97d5631a19dadf49ce5. The finite-volume vessel,
//! cylinder/receiver cycle and conservation ledger below are Rust extensions;
//! using the same properties does not claim full MSTSSteamLocomotive parity.
use crate::steam::BoilerState;
use openrailsrs_train::SteamParams;
use serde::{Deserialize, Serialize};
const PSI_PER_BAR: f64 = 14.503773773;
const BTU_LB_TO_J_KG: f64 = 2326.0003249;
const LB_FT3_TO_KG_M3: f64 = 16.018463374;
const ATM_PA: f64 = 101325.;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SteamProperties {
    pub temperature_c: f64,
    pub water_enthalpy_j_kg: f64,
    pub steam_enthalpy_j_kg: f64,
    pub water_density_kg_m3: f64,
    pub steam_density_kg_m3: f64,
}
fn interpolate(table: &[f32; 31], psi: f32) -> f64 {
    let index = ((psi / 10.).floor() as usize).min(29);
    let low = index as f32 * 10.;
    let a = (low + 10. - psi) / 10.;
    let b = (psi - low) / 10.;
    (a * table[index] + b * table[index + 1]) as f64
}
/// Input/output conversions are outside the original interpolation. Gauge
/// pressure 0..300 psi is the supported vessel domain (no table extrapolation).
pub fn properties(pressure_bar: f64) -> SteamProperties {
    let psi = (pressure_bar * PSI_PER_BAR).clamp(0., 300.) as f32;
    SteamProperties {
        temperature_c: (interpolate(&TEMPERATURE_F, psi) - 32.) * 5. / 9.,
        water_enthalpy_j_kg: interpolate(&WATER_HEAT, psi) * BTU_LB_TO_J_KG,
        steam_enthalpy_j_kg: interpolate(&STEAM_HEAT, psi) * BTU_LB_TO_J_KG,
        water_density_kg_m3: interpolate(&WATER_DENSITY, psi) * LB_FT3_TO_KG_M3,
        steam_density_kg_m3: interpolate(&STEAM_DENSITY, psi) * LB_FT3_TO_KG_M3,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThermodynamicState {
    #[serde(default)]
    pub thermal_power_w: f64,
    /// Applied boiler heat divided by saturated steam enthalpy. This is
    /// capacity, distinct from the observed net liquid-to-vapour mass change.
    #[serde(default)]
    pub thermal_capacity_kg_s: f64,
    pub volume_m3: f64,
    pub total_mass_kg: f64,
    pub steam_mass_kg: f64,
    pub internal_energy_j: f64,
    pub initial_mass_kg: f64,
    pub initial_energy_j: f64,
    pub fed_water_kg: f64,
    pub outlet_steam_kg: f64,
    pub heat_input_j: f64,
    pub feed_energy_j: f64,
    pub outlet_energy_j: f64,
    pub heat_loss_j: f64,
    pub safety_steam_kg: f64,
    pub receiver_mass_kg: f64,
    pub receiver_energy_j: f64,
    pub receiver_pressure_bar: f64,
    pub hp_force_n: f64,
    pub lp_force_n: f64,
    pub receiver_in_kg: f64,
    pub receiver_out_kg: f64,
    pub receiver_in_energy_j: f64,
    pub receiver_out_energy_j: f64,
    pub hp_work_j: f64,
    pub lp_work_j: f64,
}
fn mixture(mass: f64, volume: f64, pressure: f64) -> (f64, f64, f64) {
    let t = properties(pressure);
    let liquid = ((mass - volume * t.steam_density_kg_m3)
        / (1. - t.steam_density_kg_m3 / t.water_density_kg_m3))
        .clamp(0., mass);
    let steam = mass - liquid;
    let abs_pa = pressure * 100000. + ATM_PA;
    let energy = liquid * (t.water_enthalpy_j_kg - abs_pa / t.water_density_kg_m3)
        + steam * (t.steam_enthalpy_j_kg - abs_pa / t.steam_density_kg_m3);
    (liquid, steam, energy)
}
impl ThermodynamicState {
    pub fn new(p: &SteamParams, liquid: f64) -> Self {
        let model = p.operation.thermodynamics.as_ref().expect("enabled vessel");
        let initial_pressure = model.initial_pressure_bar(p.working_pressure_bar);
        let t = properties(initial_pressure);
        let steam = (model.boiler_volume_m3 - liquid / t.water_density_kg_m3).max(0.)
            * t.steam_density_kg_m3;
        let mass = liquid + steam;
        let energy = mixture(mass, model.boiler_volume_m3, initial_pressure).2;
        Self {
            thermal_power_w: 0.,
            thermal_capacity_kg_s: 0.,
            volume_m3: model.boiler_volume_m3,
            total_mass_kg: mass,
            steam_mass_kg: steam,
            internal_energy_j: energy,
            initial_mass_kg: mass,
            initial_energy_j: energy,
            fed_water_kg: 0.,
            outlet_steam_kg: 0.,
            heat_input_j: 0.,
            feed_energy_j: 0.,
            outlet_energy_j: 0.,
            heat_loss_j: 0.,
            safety_steam_kg: 0.,
            receiver_mass_kg: 0.,
            receiver_energy_j: 0.,
            receiver_pressure_bar: 0.,
            hp_force_n: 0.,
            lp_force_n: 0.,
            receiver_in_kg: 0.,
            receiver_out_kg: 0.,
            receiver_in_energy_j: 0.,
            receiver_out_energy_j: 0.,
            hp_work_j: 0.,
            lp_work_j: 0.,
        }
    }
    pub fn mass_residual_kg(&self) -> f64 {
        self.total_mass_kg - self.initial_mass_kg - self.fed_water_kg + self.outlet_steam_kg
    }
    pub fn energy_residual_j(&self) -> f64 {
        self.internal_energy_j - self.initial_energy_j - self.heat_input_j - self.feed_energy_j
            + self.outlet_energy_j
            + self.heat_loss_j
    }
    pub fn valid_for(&self, p: &SteamParams) -> bool {
        let Some(model) = &p.operation.thermodynamics else {
            return false;
        };
        let initial_liquid = model.boiler_volume_m3
            * properties(model.initial_pressure_bar(p.working_pressure_bar)).water_density_kg_m3
            * 0.9;
        let initial = Self::new(p, initial_liquid);
        [
            self.thermal_power_w,
            self.thermal_capacity_kg_s,
            self.volume_m3,
            self.total_mass_kg,
            self.steam_mass_kg,
            self.internal_energy_j,
            self.initial_mass_kg,
            self.initial_energy_j,
            self.fed_water_kg,
            self.outlet_steam_kg,
            self.heat_input_j,
            self.feed_energy_j,
            self.outlet_energy_j,
            self.heat_loss_j,
            self.safety_steam_kg,
            self.receiver_mass_kg,
            self.receiver_energy_j,
            self.receiver_pressure_bar,
            self.hp_force_n,
            self.lp_force_n,
            self.receiver_in_kg,
            self.receiver_out_kg,
            self.receiver_in_energy_j,
            self.receiver_out_energy_j,
            self.hp_work_j,
            self.lp_work_j,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && self.volume_m3 == model.boiler_volume_m3
            && (self.initial_mass_kg - initial.initial_mass_kg).abs() < 1e-6
            && (self.initial_energy_j - initial.initial_energy_j).abs() < 0.01
            && self.steam_mass_kg <= self.total_mass_kg
            && self.mass_residual_kg().abs() < 1e-6
            && self.energy_residual_j().abs() < 0.1
            && self.safety_steam_kg <= self.outlet_steam_kg
            && (self.receiver_mass_kg - self.receiver_in_kg + self.receiver_out_kg).abs() < 1e-6
            && (self.receiver_energy_j - self.receiver_in_energy_j
                + self.receiver_out_energy_j
                + self.hp_work_j)
                .abs()
                < 0.1
            && (p.operation.compound.is_some()
                || (self.receiver_mass_kg == 0. && self.receiver_energy_j == 0.))
    }
    pub fn state_consistent(&self, pressure_bar: f64, water_kg: f64) -> bool {
        let (liquid, steam, energy) = mixture(self.total_mass_kg, self.volume_m3, pressure_bar);
        (liquid - water_kg).abs() < 1e-6
            && (steam - self.steam_mass_kg).abs() < 1e-6
            && if pressure_bar < 1e-8 {
                self.internal_energy_j <= energy + self.total_mass_kg * 50.
            } else {
                (self.internal_energy_j - energy).abs() < self.total_mass_kg * 50.
            }
    }
    fn solve(&mut self, maximum_bar: f64) -> (f64, f64) {
        let mut low = 0.;
        let mut high = maximum_bar;
        for _ in 0..52 {
            let middle = (low + high) / 2.;
            if mixture(self.total_mass_kg, self.volume_m3, middle).2 < self.internal_energy_j {
                low = middle;
            } else {
                high = middle;
            }
        }
        let pressure = (low + high) / 2.;
        let (liquid, steam, _) = mixture(self.total_mass_kg, self.volume_m3, pressure);
        self.steam_mass_kg = steam;
        (pressure, liquid)
    }
    pub fn withdraw(&mut self, requested: f64, pressure: f64) -> f64 {
        let enthalpy = properties(pressure).steam_enthalpy_j_kg;
        let used = requested
            .max(0.)
            .min(self.total_mass_kg)
            .min(self.internal_energy_j / enthalpy);
        self.total_mass_kg -= used;
        self.internal_energy_j -= used * enthalpy;
        self.outlet_steam_kg += used;
        self.outlet_energy_j += used * enthalpy;
        used
    }
}

/// Removes an auxiliary's measured steam from the same finite vessel ledger.
pub fn consume_auxiliary(b: &mut BoilerState, requested: f64) -> f64 {
    let Some(t) = &mut b.thermodynamics else {
        return 0.;
    };
    let used = t.withdraw(requested, b.pressure_bar);
    let (pressure, liquid) = t.solve(b.pressure_bar.max(0.));
    b.pressure_bar = pressure;
    b.water_kg = liquid;
    used
}

#[allow(clippy::too_many_arguments)]
pub fn advance(
    b: &mut BoilerState,
    p: &SteamParams,
    throttle: f64,
    speed: f64,
    dt: f64,
    limit: f64,
    aux: Option<&openrailsrs_scenarios::vehicle_systems::AuxiliaryDef>,
) -> f64 {
    let Some(model) = p.operation.thermodynamics.as_ref() else {
        return 0.;
    };
    if !dt.is_finite() || dt <= 0. || !throttle.is_finite() {
        return b.tractive_force_n;
    }
    let throttle = throttle.clamp(0., 1.);
    let capacity = p.operation.boiler_water_capacity_kg;
    let properties_now = properties(b.pressure_bar);
    let design_liquid =
        model.boiler_volume_m3 * properties(p.working_pressure_bar).water_density_kg_m3;
    let level = b.water_kg / design_liquid.max(1.);
    b.low_water_failure |= level < 0.15;
    if b.controls.automatic_fireman {
        b.controls.injector1 =
            b.tender_water_kg > 0. && (level < 0.75 || b.controls.injector1 && level < 0.88);
        b.controls.injector2 =
            b.tender_water_kg > 0. && (level < 0.5 || b.controls.injector2 && level < 0.65);
        b.controls.damper =
            (0.25 + throttle * 0.75 + (p.working_pressure_bar - b.pressure_bar) * 0.1)
                .clamp(0., 1.);
        b.controls.firing = ((p.operation.max_fire_mass_kg * 0.5 - b.fire_mass_kg) / 20.
            + p.coal_consumption_kg_per_s * b.controls.damper)
            / p.operation.max_firing_rate_kg_s.max(1e-9);
        b.controls.firing = b.controls.firing.clamp(0., 1.);
    }
    let feed = (p.operation.max_firing_rate_kg_s * b.controls.firing * dt)
        .min(b.coal_kg)
        .min((p.operation.max_fire_mass_kg - b.fire_mass_kg).max(0.));
    b.coal_kg -= feed;
    b.fire_mass_kg += feed;
    let draft = (b.controls.damper + if b.controls.blower { 0.25 } else { 0. }).clamp(0., 1.);
    let burn = (p.coal_consumption_kg_per_s * draft * dt).min(b.fire_mass_kg);
    b.fire_mass_kg -= burn;
    b.coal_burn_kg_s = burn / dt;
    let mut heat = burn * model.coal_heat_j_kg * model.boiler_efficiency;
    // An authored maximum production caps transferred heat, not generated mass.
    heat = heat.min(
        p.evaporation_rate_kg_per_s
            * dt
            * (properties_now.steam_enthalpy_j_kg - properties_now.water_enthalpy_j_kg),
    );
    let injector_count = usize::from(b.controls.injector1) + usize::from(b.controls.injector2);
    let injector_flow = aux.and_then(|p| p.native_injector_mm).map_or(
        injector_count as f64 * p.operation.injector_rate_kg_s,
        |sizes| {
            [b.controls.injector1, b.controls.injector2]
                .into_iter()
                .zip(sizes)
                .filter(|(on, _)| *on)
                .map(|(_, size)| crate::steam_injectors::flow_kg_s(size, b.pressure_bar))
                .sum()
        },
    );
    let injected = if b.pressure_bar > 2.
        && aux.is_none_or(|p| b.pressure_bar <= p.injector_max_pressure_bar)
        && !b.low_water_failure
    {
        (injector_flow * dt)
            .min(b.tender_water_kg)
            .min((design_liquid * 0.95 - b.water_kg).max(0.))
            .min((capacity - b.water_kg).max(0.))
    } else {
        0.
    };
    b.tender_water_kg -= injected;
    b.injection_kg_s = injected / dt;
    let old_liquid = b.water_kg;
    let cutoff = b.controls.cutoff / crate::steam::MAX_CUTOFF;
    let mut force = if b.low_water_failure {
        0.
    } else {
        crate::steam::stall_force_n(p) * throttle * cutoff * b.pressure_bar / p.working_pressure_bar
    };
    let t = b.thermodynamics.as_mut().expect("initialized vessel");
    t.thermal_power_w = heat / dt;
    t.thermal_capacity_kg_s = t.thermal_power_w / properties_now.steam_enthalpy_j_kg;
    let usable = (properties_now.steam_enthalpy_j_kg - properties(0.).water_enthalpy_j_kg)
        * model.cylinder_efficiency;
    let mut hp_admission_kg = 0.;
    let mut lp_direct_kg = 0.;
    t.hp_force_n = force;
    t.lp_force_n = 0.;
    if let Some(compound) = &p.operation.compound {
        let receiver = if b.controls.compound_bypass {
            0.
        } else {
            t.receiver_pressure_bar.min(b.pressure_bar)
        };
        let hp = compound.hp_cylinders as f64 / p.cylinder_count as f64
            * force
            * (1. - receiver / b.pressure_bar.max(0.01));
        let lp = compound.lp_cylinders as f64 / p.cylinder_count as f64
            * force
            * (compound.lp_bore_m / p.cylinder_bore_m).powi(2)
            * if b.controls.compound_bypass {
                1.
            } else {
                receiver / b.pressure_bar.max(0.01)
            };
        let revs = speed.abs() / (std::f64::consts::TAU * p.driving_wheel_radius_m);
        let hp_volume = compound.hp_cylinders as f64 * std::f64::consts::PI / 4.
            * p.cylinder_bore_m.powi(2)
            * p.piston_stroke_m
            * 2.
            * revs
            * throttle
            * cutoff;
        let supply = if b.controls.compound_bypass {
            1.
        } else {
            (1. - receiver / (b.pressure_bar * compound.receiver_pressure_ratio).max(0.01))
                .clamp(0., 1.)
                .sqrt()
        };
        hp_admission_kg = hp_volume * properties_now.steam_density_kg_m3 * dt * supply;
        if b.controls.compound_bypass {
            lp_direct_kg = hp_volume * compound.lp_cylinders as f64 / compound.hp_cylinders as f64
                * (compound.lp_bore_m / p.cylinder_bore_m).powi(2)
                * properties_now.steam_density_kg_m3
                * dt;
        }
        let lp_capacity = compound.lp_cylinders as f64 * std::f64::consts::PI / 4.
            * compound.lp_bore_m.powi(2)
            * p.piston_stroke_m
            * 2.
            * revs
            * cutoff
            * properties(t.receiver_pressure_bar).steam_density_kg_m3
            * dt;
        let receiver_available = if b.controls.compound_bypass {
            lp_direct_kg
        } else {
            (t.receiver_mass_kg + hp_admission_kg).min(lp_capacity)
        };
        t.hp_force_n = if speed.abs() > 0.01 {
            hp.min(hp_admission_kg * usable / dt / speed.abs())
        } else {
            hp
        };
        t.lp_force_n = if speed.abs() > 0.01 {
            lp.min(receiver_available * usable / dt / speed.abs())
        } else {
            lp
        };
        force = t.hp_force_n + t.lp_force_n;
    }
    if b.controls.cylinder_cocks {
        force *= 0.8;
    }
    force = force.min(limit).max(0.);
    let force_scale = force / (t.hp_force_n + t.lp_force_n).max(1e-9);
    t.hp_force_n *= force_scale;
    t.lp_force_n *= force_scale;
    let injector_steam = injected * aux.map_or(0.05, |a| a.injector_steam_kg_per_water_kg);
    let requested = if p.operation.compound.is_some() {
        hp_admission_kg + lp_direct_kg
    } else {
        force * speed.abs() * dt / usable.max(1.)
            + p.evaporation_rate_kg_per_s * 0.02 * throttle * cutoff * dt
    } + model.idle_steam_kg_s * dt
        + injector_steam;
    let delivered = t.withdraw(requested, b.pressure_bar);
    if delivered < requested && requested > 0. {
        force *= delivered / requested;
        t.hp_force_n *= delivered / requested;
        t.lp_force_n *= delivered / requested;
    }
    t.total_mass_kg += injected;
    t.fed_water_kg += injected;
    let feed_energy = injected * 4180. * aux.map_or(model.feedwater_c, |a| a.feedwater_c);
    t.feed_energy_j += feed_energy;
    t.heat_input_j += heat;
    let loss = (model.heat_loss_w * dt).min((t.internal_energy_j + heat + feed_energy).max(0.));
    t.heat_loss_j += loss;
    t.internal_energy_j += heat + feed_energy - loss;
    if let Some(compound) = &p.operation.compound {
        let hp_flow = if b.controls.compound_bypass {
            0.
        } else {
            hp_admission_kg
                * if requested > 0. {
                    delivered / requested
                } else {
                    0.
                }
        };
        let hp_work = if b.controls.compound_bypass {
            0.
        } else {
            (t.hp_force_n * speed.abs() * dt).min(hp_flow * usable)
        };
        t.receiver_mass_kg += hp_flow;
        t.receiver_in_kg += hp_flow;
        let input_energy = hp_flow * properties_now.steam_enthalpy_j_kg;
        t.receiver_energy_j += input_energy - hp_work;
        t.receiver_in_energy_j += input_energy;
        t.hp_work_j += hp_work;
        let density = properties(t.receiver_pressure_bar).steam_density_kg_m3;
        let swept = compound.lp_cylinders as f64 * std::f64::consts::PI / 4.
            * compound.lp_bore_m.powi(2)
            * p.piston_stroke_m
            * 2.
            * speed.abs()
            / (std::f64::consts::TAU * p.driving_wheel_radius_m)
            * cutoff;
        let exhausted = if b.controls.compound_bypass {
            0.
        } else {
            (swept * density * dt).min(t.receiver_mass_kg)
        };
        let fraction = if t.receiver_mass_kg > 0. {
            exhausted / t.receiver_mass_kg
        } else {
            0.
        };
        t.receiver_out_kg += exhausted;
        t.receiver_out_energy_j += t.receiver_energy_j * fraction;
        t.lp_work_j += t.lp_force_n * speed.abs() * dt;
        t.receiver_energy_j *= 1. - fraction;
        t.receiver_mass_kg -= exhausted;
        // Low-flow drain avoids a trapped receiver creating permanent torque.
        if throttle == 0. {
            let fraction = 1. - (-dt / 10.).exp();
            t.receiver_out_kg += t.receiver_mass_kg * fraction;
            t.receiver_out_energy_j += t.receiver_energy_j * fraction;
            t.receiver_mass_kg *= 1. - fraction;
            t.receiver_energy_j *= 1. - fraction;
        }
        let temperature = properties_now.temperature_c + 273.15;
        t.receiver_pressure_bar =
            ((t.receiver_mass_kg * 461.5 * temperature / compound.receiver_volume_m3 - ATM_PA)
                / 100000.)
                .max(0.);
    }
    let maximum = p.working_pressure_bar * crate::steam::SAFETY_VALVE_FACTOR;
    let excess = (t.internal_energy_j - mixture(t.total_mass_kg, t.volume_m3, maximum).2).max(0.);
    b.safety_valve = excess > 0.1;
    if b.safety_valve {
        // Vent mass with its enthalpy; solve the exact mixture equation rather
        // than clamping pressure and silently destroying stored energy.
        let mut lo = 0.;
        let mut hi = t.total_mass_kg.min(excess / 100000. + 1.);
        let hg = properties(maximum).steam_enthalpy_j_kg;
        for _ in 0..52 {
            let m = (lo + hi) / 2.;
            if t.internal_energy_j - m * hg > mixture(t.total_mass_kg - m, t.volume_m3, maximum).2 {
                lo = m;
            } else {
                hi = m;
            }
        }
        let vented = t.withdraw((lo + hi) / 2., maximum);
        t.safety_steam_kg += vented;
    }
    let (pressure, liquid) = t.solve(maximum);
    b.pressure_bar = pressure;
    b.water_kg = liquid;
    b.steam_usage_kg_s = delivered / dt;
    b.evaporation_kg_s = (old_liquid + injected - liquid).max(0.) / dt;
    b.low_water_failure |= b.water_kg < design_liquid * 0.15;
    b.tractive_force_n = if b.low_water_failure { 0. } else { force };
    b.tractive_force_n
}
const TEMPERATURE_F: [f32; 31] = [
    212.0, 239.4, 258.7, 274.0, 286.7, 297.7, 307.3, 316.0, 323.9, 331.2, 337.9, 344.2, 350.1,
    355.6, 360.9, 365.9, 370.6, 375.6, 379.6, 383.8, 387.8, 391.7, 395.5, 399.1, 402.6, 406.0,
    409.4, 412.6, 415.7, 418.8, 421.8,
];
const WATER_HEAT: [f32; 31] = [
    180.15, 207.83, 227.51, 243.08, 256.09, 267.34, 277.31, 286.30, 294.49, 302.05, 309.08, 315.66,
    321.85, 327.70, 333.26, 338.56, 343.62, 348.47, 353.13, 357.62, 361.95, 366.14, 370.19, 374.12,
    377.94, 381.65, 385.26, 388.78, 392.21, 395.56, 398.84,
];
const WATER_DENSITY: [f32; 31] = [
    59.83, 59.11, 58.57, 58.12, 57.73, 57.39, 57.07, 56.79, 56.52, 56.27, 56.03, 55.81, 55.59,
    55.39, 55.19, 55.00, 54.82, 54.65, 54.48, 54.31, 54.15, 53.99, 53.84, 53.69, 53.54, 53.40,
    53.26, 53.12, 52.99, 52.86, 52.73,
];
const STEAM_HEAT: [f32; 31] = [
    1150.28, 1160.31, 1167.01, 1172.03, 1176.00, 1179.27, 1182.04, 1184.42, 1186.50, 1188.33,
    1189.95, 1191.41, 1192.72, 1193.91, 1194.99, 1195.97, 1196.86, 1197.68, 1198.43, 1199.12,
    1199.75, 1200.33, 1200.86, 1201.35, 1201.79, 1202.20, 1202.58, 1202.92, 1203.23, 1203.51,
    1203.77,
];
const STEAM_DENSITY: [f32; 31] = [
    0.0373, 0.0606, 0.0834, 0.1057, 0.1277, 0.1496, 0.1713, 0.1928, 0.2143, 0.2356, 0.2569, 0.2782,
    0.2994, 0.3205, 0.3416, 0.3627, 0.3838, 0.4048, 0.4259, 0.4469, 0.4680, 0.4890, 0.5101, 0.5312,
    0.5522, 0.5733, 0.5944, 0.6155, 0.6367, 0.6578, 0.6790,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn compound() -> SteamParams {
        openrailsrs_train::parse_steam_engine_toml(include_str!(
            "../../../examples/physics_lab/vehicles/compound.eng"
        ))
        .unwrap()
        .steam
        .unwrap()
    }

    #[test]
    fn sub_newton_hp_force_cannot_create_lp_work() {
        let p = compound();
        let mut b = BoilerState::from_params(&p);
        b.controls.automatic_fireman = false;
        b.controls.compound_bypass = false;
        b.controls.firing = 0.;
        b.controls.damper = 0.;
        let speed = 2_f64;
        let dt = 0.05;
        let force = advance(&mut b, &p, 0.5, speed, dt, 0.5, None);
        let t = b.thermodynamics.as_ref().unwrap();
        assert!((force - 0.5).abs() < 1e-12);
        assert!((t.hp_force_n - 0.5).abs() < 1e-12);
        assert_eq!(t.lp_force_n, 0.);
        assert_eq!(t.lp_work_j, 0., "zero LP force cannot perform work");
        assert!((t.hp_work_j - t.hp_force_n * speed * dt).abs() < 1e-12);
        assert!(t.receiver_in_kg > 0.);
        assert!(
            (t.receiver_energy_j - t.receiver_in_energy_j + t.receiver_out_energy_j + t.hp_work_j)
                .abs()
                < 1e-8
        );
        assert!(t.valid_for(&p));
    }

    #[test]
    fn bypass_keeps_hp_exhaust_out_of_the_receiver_work_ledger() {
        let p = compound();
        let mut b = BoilerState::from_params(&p);
        b.controls.automatic_fireman = false;
        b.controls.compound_bypass = true;
        let speed = 2_f64;
        let dt = 0.05;
        advance(&mut b, &p, 0.5, speed, dt, 0.5, None);
        let t = b.thermodynamics.as_ref().unwrap();
        assert!(t.hp_force_n > 0. && t.lp_force_n > 0.);
        assert_eq!(t.receiver_in_kg, 0.);
        assert_eq!(t.receiver_in_energy_j, 0.);
        assert_eq!(t.receiver_mass_kg, 0.);
        assert_eq!(t.receiver_energy_j, 0.);
        assert_eq!(t.hp_work_j, 0.);
        assert!((t.lp_work_j - t.lp_force_n * speed * dt).abs() < 1e-12);
        assert!(t.valid_for(&p));
    }
}
