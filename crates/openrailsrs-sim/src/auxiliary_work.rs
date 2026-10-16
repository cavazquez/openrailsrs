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
    state
        .brake_system
        .step_circuits_with_diesel(&supply, &state.diesel, dt);
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
            .is_some_and(|b| {
                // The bounded native cycle accounts for its own fitted
                // auxiliaries. A separate generic compressor is not a free
                // steam-powered extension of that cycle.
                b.native_or161.is_none() && b.pressure_bar > 0.5 && b.water_kg > 0.
            });
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
        if b.native_or161.is_some() {
            // Native UpdateAuxiliaries already debits its fitted ejectors,
            // blower, generator and other native consumers. Do not withdraw
            // again through the common display mirror or claim that these
            // additional generic loads have been validated for this cycle.
            continue;
        }
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
    if b.native_or161.is_some() {
        return 0.;
    }
    if b.thermodynamics.is_some() {
        return crate::steam_thermodynamics::consume_auxiliary(b, requested);
    }
    let used = requested.max(0.).min(b.water_kg);
    b.water_kg -= used;
    let storage = b.water_kg.max(b.initial_water_kg * 0.15) * 12600.;
    b.pressure_bar = (b.pressure_bar - used * crate::steam::STEAM_ENTHALPY_J_KG / storage).max(0.);
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cycle_does_not_debit_generic_auxiliaries_or_display_mirrors() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/physics_lab");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario_native_steam.toml"))
                .unwrap();
        let mut s = crate::LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        let definition = openrailsrs_scenarios::vehicle_systems::VehicleSystemsDef {
            vehicle: 0,
            auxiliaries: Some(openrailsrs_scenarios::vehicle_systems::AuxiliaryDef {
                native_injector_mm: None,
                leak_bar_s: 0.,
                compressor_efficiency: 0.8,
                compressor_idle_w: 100.,
                battery_kwh: 0.,
                steam_compressor_kg_j: 0.01,
                ejector_steam_kg_s: 1.,
                injector_steam_kg_per_water_kg: 0.1,
                injector_max_pressure_bar: 16.,
                feedwater_c: 20.,
            }),
            ..Default::default()
        };
        let mut generic = crate::vehicle_systems::VehicleSystemState::new(definition);
        generic.aux.compressor_w = 1000.;
        s.state.vehicle_systems.push(generic);
        let original = s.state.boiler_state.clone();
        let boilers = s.state.steam_boilers.clone();
        steam_extras(&mut s.state, 1.);
        assert_eq!(s.state.boiler_state, original);
        assert_eq!(s.state.steam_boilers, boilers);
        assert_eq!(
            s.state.vehicle_systems.last().unwrap().aux.steam_used_kg,
            0.
        );
        assert_eq!(
            s.state
                .vehicle_systems
                .last()
                .unwrap()
                .aux
                .injector_water_kg,
            0.
        );
        assert_eq!(
            s.state
                .vehicle_systems
                .last()
                .unwrap()
                .aux
                .injector_steam_kg,
            0.
        );
        let b = s.state.boiler_state.as_mut().unwrap();
        assert_eq!(consume_steam(b, 100.), 0.);
        assert_eq!(s.state.boiler_state, original);
    }
}
