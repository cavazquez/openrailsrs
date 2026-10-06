//! Finite tender, fire bed and boiler, with automatic or manual fireman.
//! This conservation model does not reproduce OR's complete steam tables.
use openrailsrs_train::SteamParams;
use serde::{Deserialize, Serialize};

pub const MAX_CUTOFF: f64 = 0.75;
pub const ETA_INDICATOR: f64 = 0.85;
pub const STEAM_ENTHALPY_J_KG: f64 = 2_500_000.;
pub const SAFETY_VALVE_FACTOR: f64 = 1.05;
/// Approximate thermal storage: liquid water cp × saturation dT/dP.
const WATER_HEAT_STORAGE_J_KG_BAR: f64 = 12_600.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SteamControls {
    pub automatic_fireman: bool,
    pub cutoff: f64,
    pub firing: f64,
    pub damper: f64,
    pub injector1: bool,
    pub injector2: bool,
    pub blower: bool,
    pub cylinder_cocks: bool,
}
impl Default for SteamControls {
    fn default() -> Self {
        Self {
            automatic_fireman: true,
            cutoff: MAX_CUTOFF,
            firing: 0.5,
            damper: 0.5,
            injector1: false,
            injector2: false,
            blower: false,
            cylinder_cocks: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SteamCommand {
    AutomaticFireman,
    Cutoff(f64),
    Firing(f64),
    Damper(f64),
    Injector1,
    Injector2,
    Blower,
    CylinderCocks,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoilerState {
    pub pressure_bar: f64,
    /// Water in the boiler, not the tender.
    pub water_kg: f64,
    pub coal_kg: f64,
    pub initial_water_kg: f64,
    #[serde(default)]
    pub tender_water_kg: f64,
    #[serde(default)]
    pub fire_mass_kg: f64,
    #[serde(default)]
    pub fire_capacity_kg: f64,
    #[serde(default)]
    pub controls: SteamControls,
    #[serde(default)]
    pub low_water_failure: bool,
    #[serde(default)]
    pub safety_valve: bool,
    #[serde(default)]
    pub steam_usage_kg_s: f64,
    #[serde(default)]
    pub evaporation_kg_s: f64,
    #[serde(default)]
    pub injection_kg_s: f64,
    #[serde(default)]
    pub coal_burn_kg_s: f64,
    #[serde(default)]
    pub tractive_force_n: f64,
}
impl BoilerState {
    pub fn from_params(params: &SteamParams) -> Self {
        let capacity = params.operation.boiler_water_capacity_kg;
        Self {
            pressure_bar: params.working_pressure_bar,
            water_kg: capacity * 0.9,
            coal_kg: params.initial_coal_kg,
            initial_water_kg: capacity,
            tender_water_kg: params.initial_water_kg,
            fire_mass_kg: params.operation.max_fire_mass_kg * 0.5,
            fire_capacity_kg: params.operation.max_fire_mass_kg,
            controls: Default::default(),
            low_water_failure: false,
            safety_valve: false,
            steam_usage_kg_s: 0.,
            evaporation_kg_s: 0.,
            injection_kg_s: 0.,
            coal_burn_kg_s: 0.,
            tractive_force_n: 0.,
        }
    }
    pub fn command(&mut self, command: SteamCommand) {
        use SteamCommand::*;
        if matches!(
            command,
            Firing(_) | Damper(_) | Injector1 | Injector2 | Blower
        ) {
            self.controls.automatic_fireman = false;
        }
        match command {
            AutomaticFireman => self.controls.automatic_fireman = !self.controls.automatic_fireman,
            Cutoff(delta) if delta.is_finite() => {
                self.controls.cutoff = (self.controls.cutoff + delta).clamp(0., MAX_CUTOFF)
            }
            Firing(delta) if delta.is_finite() => {
                self.controls.firing = (self.controls.firing + delta).clamp(0., 1.)
            }
            Damper(delta) if delta.is_finite() => {
                self.controls.damper = (self.controls.damper + delta).clamp(0., 1.)
            }
            Injector1 => self.controls.injector1 = !self.controls.injector1,
            Injector2 => self.controls.injector2 = !self.controls.injector2,
            Blower => self.controls.blower = !self.controls.blower,
            CylinderCocks => self.controls.cylinder_cocks = !self.controls.cylinder_cocks,
            _ => (),
        }
    }
    pub fn valid_for(&self, p: &SteamParams) -> bool {
        [
            self.pressure_bar,
            self.water_kg,
            self.coal_kg,
            self.initial_water_kg,
            self.tender_water_kg,
            self.fire_mass_kg,
            self.fire_capacity_kg,
            self.steam_usage_kg_s,
            self.evaporation_kg_s,
            self.injection_kg_s,
            self.coal_burn_kg_s,
            self.tractive_force_n,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && self.pressure_bar <= p.working_pressure_bar * SAFETY_VALVE_FACTOR + 1e-6
            && self.water_kg <= p.operation.boiler_water_capacity_kg
            && self.initial_water_kg == p.operation.boiler_water_capacity_kg
            && self.tender_water_kg <= p.initial_water_kg
            && self.coal_kg <= p.initial_coal_kg
            && self.fire_mass_kg <= p.operation.max_fire_mass_kg
            && self.fire_capacity_kg == p.operation.max_fire_mass_kg
            && self.controls.cutoff.is_finite()
            && (0. ..=MAX_CUTOFF).contains(&self.controls.cutoff)
            && [self.controls.firing, self.controls.damper]
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
    }
    pub fn status(&self) -> &'static str {
        if self.low_water_failure {
            "Caldera dañada: nivel de agua insuficiente"
        } else if self.fire_mass_kg <= 1e-6 {
            "Fuego apagado"
        } else if self.tender_water_kg <= 1e-6 {
            "Ténder sin agua"
        } else if self.coal_kg <= 1e-6 {
            "Ténder sin carbón"
        } else if self.safety_valve {
            "Válvula de seguridad abierta"
        } else {
            "Caldera en servicio"
        }
    }
}

pub fn steam_step(
    b: &mut BoilerState,
    p: &SteamParams,
    regulator: f64,
    velocity_mps: f64,
    dt: f64,
) -> f64 {
    if !dt.is_finite() || dt <= 0. {
        return b.tractive_force_n;
    }
    let regulator = regulator.clamp(0., 1.);
    let capacity = p.operation.boiler_water_capacity_kg;
    b.low_water_failure |= b.water_kg <= capacity * 0.15;
    let force = if b.low_water_failure || b.water_kg <= capacity * 0.15 {
        0.
    } else {
        stall_force_n(p) * regulator * b.controls.cutoff / MAX_CUTOFF * b.pressure_bar
            / p.working_pressure_bar
            * if b.controls.cylinder_cocks { 0.8 } else { 1. }
    };
    let pressure_fraction = (b.pressure_bar / p.working_pressure_bar).clamp(0., 1.);
    let demand = force * velocity_mps.abs() / STEAM_ENTHALPY_J_KG
        + p.evaporation_rate_kg_per_s * 0.1 * regulator * pressure_fraction * b.controls.cutoff
            / MAX_CUTOFF;
    if b.controls.automatic_fireman {
        // Automatic assistance regulates controls, never creates consumables.
        // Hysteresis keeps the injector/SMS switch from chattering every tick.
        b.controls.injector1 = b.tender_water_kg > 0.
            && (b.water_kg < capacity * 0.75
                || b.controls.injector1 && b.water_kg < capacity * 0.9);
        b.controls.injector2 = b.tender_water_kg > 0.
            && (b.water_kg < capacity * 0.55
                || b.controls.injector2 && b.water_kg < capacity * 0.7);
        let desired_supply = demand + 0.1 + (p.working_pressure_bar - b.pressure_bar).max(0.) * 0.5;
        b.controls.damper = (desired_supply / p.evaporation_rate_kg_per_s.max(0.001)).clamp(0., 1.);
        b.controls.firing = ((p.operation.max_fire_mass_kg * 0.5 - b.fire_mass_kg).max(0.)
            / p.operation.max_firing_rate_kg_s.max(0.001)
            / 20.)
            .clamp(0., 1.);
    }
    let feed = (p.operation.max_firing_rate_kg_s * b.controls.firing * dt)
        .min(b.coal_kg)
        .min((p.operation.max_fire_mass_kg - b.fire_mass_kg).max(0.));
    b.coal_kg -= feed;
    b.fire_mass_kg += feed;
    let draft = (b.controls.damper + if b.controls.blower { 0.15 } else { 0. }).clamp(0., 1.);
    let fire_factor = (b.fire_mass_kg / (p.operation.max_fire_mass_kg * 0.5)).clamp(0., 1.);
    let burn = (p.coal_consumption_kg_per_s * draft * fire_factor * dt).min(b.fire_mass_kg);
    b.fire_mass_kg -= burn;
    b.coal_burn_kg_s = burn / dt;
    b.evaporation_kg_s = if b.low_water_failure {
        0.
    } else {
        p.evaporation_rate_kg_per_s * burn / (p.coal_consumption_kg_per_s.max(0.001) * dt)
    };
    let injector_count = usize::from(b.controls.injector1) + usize::from(b.controls.injector2);
    let injected = if b.pressure_bar > 2. && !b.low_water_failure {
        (injector_count as f64 * p.operation.injector_rate_kg_s * dt)
            .min(b.tender_water_kg)
            .min((capacity * 0.95 - b.water_kg).max(0.))
    } else {
        0.
    };
    b.tender_water_kg -= injected;
    b.water_kg += injected;
    b.injection_kg_s = injected / dt;
    let extras = injected / dt * 0.03
        + pressure_fraction
            * (if b.controls.blower { 0.05 } else { 0. }
                + if b.controls.cylinder_cocks && regulator > 0. {
                    0.15 * regulator
                } else {
                    0.
                }
                + 0.02);
    let used = ((demand + extras) * dt).min(b.water_kg);
    b.water_kg -= used;
    b.steam_usage_kg_s = used / dt;
    // Cold injected water consumes boiler heat as well as injector steam.
    let heat_j = (b.evaporation_kg_s * dt - used) * STEAM_ENTHALPY_J_KG - injected * 750_000.;
    let storage = b.water_kg.max(capacity * 0.15) * WATER_HEAT_STORAGE_J_KG_BAR;
    let pressure = (b.pressure_bar + heat_j / storage).max(0.);
    let maximum = p.working_pressure_bar * SAFETY_VALVE_FACTOR;
    b.safety_valve = pressure > maximum;
    if b.safety_valve {
        let vented = ((pressure - maximum) * storage / STEAM_ENTHALPY_J_KG).min(b.water_kg);
        b.water_kg -= vented;
        b.steam_usage_kg_s += vented / dt;
    }
    b.pressure_bar = pressure.min(maximum);
    if b.water_kg <= capacity * 0.15 {
        b.low_water_failure = true;
        b.pressure_bar = (b.pressure_bar - dt).max(0.);
    }
    b.tractive_force_n = if b.low_water_failure { 0. } else { force };
    b.tractive_force_n
}

pub fn stall_force_n(p: &SteamParams) -> f64 {
    p.cylinder_count as f64 * std::f64::consts::PI / 4.
        * p.cylinder_bore_m.powi(2)
        * p.piston_stroke_m
        * MAX_CUTOFF
        * p.working_pressure_bar
        * 1e5
        * ETA_INDICATOR
        / p.driving_wheel_radius_m
}
