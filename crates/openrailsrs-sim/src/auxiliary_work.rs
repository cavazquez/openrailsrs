//! Metered compressor work and finite auxiliary storage. Steam auxiliaries
//! remove both water mass and heat from the boiler that feeds them.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuxiliaryState {
    pub battery_j: f64,
    pub initial_battery_j: f64,
    pub battery_used_j: f64,
    pub compressor_w: f64,
    pub compressor_work_j: f64,
    pub steam_used_kg: f64,
    pub injector_water_kg: f64,
    pub injector_steam_kg: f64,
}
impl AuxiliaryState {
    pub fn new(p: Option<&openrailsrs_scenarios::vehicle_systems::AuxiliaryDef>) -> Self {
        let energy = p.map_or(0., |p| p.battery_kwh * 3_600_000.);
        Self {
            battery_j: energy,
            initial_battery_j: energy,
            ..Default::default()
        }
    }
    pub fn valid(&self) -> bool {
        [
            self.battery_j,
            self.initial_battery_j,
            self.battery_used_j,
            self.compressor_w,
            self.compressor_work_j,
            self.steam_used_kg,
            self.injector_water_kg,
            self.injector_steam_kg,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && (self.battery_j + self.battery_used_j - self.initial_battery_j).abs() < 1e-5
    }
}
pub fn step_circuits(state: &mut crate::TrainSimState, dt: f64) {
    let mut supply = state.power_supply.clone();
    for c in &state.vehicle_systems {
        if c.definition.auxiliaries.is_some()
            && c.aux.battery_j > 0.
            && let Some(s) = supply
                .cars
                .iter_mut()
                .find(|s| s.vehicle == c.definition.vehicle)
        {
            s.supply.auxiliary = true;
        }
    }
    for circuit in &mut state.brake_system.circuits {
        if let Some(p) = state
            .vehicle_systems
            .iter()
            .find(|c| c.definition.vehicle == circuit.vehicle)
            .and_then(|c| c.definition.auxiliaries.as_ref())
        {
            circuit.reservoir_bar = (circuit.reservoir_bar - p.leak_bar_s * dt).max(0.);
        }
    }
    state.brake_system.step_circuits(&supply, dt);
    for c in &mut state.vehicle_systems {
        let Some(p) = &c.definition.auxiliaries else {
            continue;
        };
        let Some(circuit) = state
            .brake_system
            .circuits
            .iter_mut()
            .find(|a| a.vehicle == c.definition.vehicle)
        else {
            continue;
        };
        let low_power = state
            .power_supply
            .cars
            .iter()
            .any(|s| s.vehicle == c.definition.vehicle && s.supply.auxiliary);
        let pressure = circuit.reservoir_bar + 1.01325;
        let work = circuit.charged_bar_m3 * 100000. * pressure.ln() / p.compressor_efficiency
            + if circuit.compressor_on {
                p.compressor_idle_w * dt
            } else {
                0.
            };
        let steam_powered = p.steam_compressor_kg_j > 0.
            && (state
                .steam_boilers
                .iter()
                .find(|b| b.vehicle == c.definition.vehicle)
                .map(|b| &b.boiler)
                .or_else(|| {
                    (c.definition.vehicle == state.active_cab_vehicle.unwrap_or(0))
                        .then_some(state.boiler_state.as_ref())
                        .flatten()
                }))
            .is_some_and(|b| b.pressure_bar > 0.5 && b.water_kg > 0.);
        let delivered = if low_power || steam_powered {
            work
        } else {
            work.min(c.aux.battery_j)
        };
        if !low_power && !steam_powered {
            c.aux.battery_j -= delivered;
            c.aux.battery_used_j += delivered;
        }
        if delivered < work && work > 0. {
            let undelivered =
                circuit.charged_bar_m3 * (1. - delivered / work) / circuit.params.reservoir_m3;
            circuit.reservoir_bar = (circuit.reservoir_bar - undelivered).max(0.);
            circuit.compressor_on = false;
        }
        c.aux.compressor_w = if dt > 0. { delivered / dt } else { 0. };
        c.aux.compressor_work_j += delivered;
        if low_power && !steam_powered {
            state.power_supply.auxiliary_w += c.aux.compressor_w;
        }
    }
}
pub fn steam_extras(state: &mut crate::TrainSimState, dt: f64) {
    let active = state.active_cab_vehicle.unwrap_or(0);
    for c in &mut state.vehicle_systems {
        let Some(p) = &c.definition.auxiliaries else {
            continue;
        };
        let b = if state.steam_boilers.is_empty() {
            if c.definition.vehicle != active {
                continue;
            }
            state.boiler_state.as_mut()
        } else {
            state
                .steam_boilers
                .iter_mut()
                .find(|b| b.vehicle == c.definition.vehicle)
                .map(|b| &mut b.boiler)
        };
        let Some(b) = b else {
            continue;
        };
        c.aux.injector_water_kg += b.injection_kg_s * dt;
        c.aux.injector_steam_kg += b.injection_kg_s * dt * p.injector_steam_kg_per_water_kg;
        let requested =
            c.aux.compressor_w * dt * p.steam_compressor_kg_j + p.ejector_steam_kg_s * dt;
        let used = consume_steam(b, requested);
        c.aux.steam_used_kg += used;
    }
    if !state.steam_boilers.is_empty() {
        state.boiler_state = state
            .steam_boilers
            .iter()
            .find(|b| b.vehicle == active)
            .map(|b| b.boiler.clone());
    }
}
pub fn consume_steam(b: &mut crate::steam::BoilerState, requested: f64) -> f64 {
    let used = requested.max(0.).min(b.water_kg);
    b.water_kg -= used;
    let storage = b.water_kg.max(b.initial_water_kg * 0.15) * 12600.;
    b.pressure_bar = (b.pressure_bar - used * crate::steam::STEAM_ENTHALPY_J_KG / storage).max(0.);
    used
}
