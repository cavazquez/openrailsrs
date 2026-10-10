//! Per-locomotive native Rust power supply shared by player, AI and CLI.
use crate::{diesel_operation::EnginePhase, electric::BreakerState, state::TrainSimState};
use openrailsrs_core::power_supply::{
    PowerSource, PowerSupplyInput, PowerSupplyParams, PowerSupplyState,
};
use openrailsrs_train::{Consist, Vehicle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct PowerCar {
    pub vehicle: usize,
    pub source: PowerSource,
    pub params: PowerSupplyParams,
    pub enabled: bool,
    pub battery_on: bool,
    pub master_key_on: bool,
    pub train_supply_switch_on: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PowerTrainConfig {
    pub cars: Vec<PowerCar>,
}
impl PowerTrainConfig {
    pub fn from_consist(consist: &Consist) -> Self {
        Self {
            cars: consist
                .vehicles
                .iter()
                .enumerate()
                .filter_map(|(vehicle, v)| {
                    let Vehicle::Loco(l) = v else { return None };
                    Some(PowerCar {
                        vehicle,
                        params: (*l.power_supply).clone(),
                        source: if l.electric.is_some() {
                            PowerSource::Electric
                        } else if l.steam.is_some() {
                            PowerSource::Steam
                        } else {
                            PowerSource::Diesel
                        },
                        enabled: true,
                        battery_on: true,
                        master_key_on: true,
                        train_supply_switch_on: !l.power_supply.manual_train_supply,
                    })
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PowerCarState {
    pub vehicle: usize,
    pub supply: PowerSupplyState,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerTrainState {
    pub initialized: bool,
    pub cars: Vec<PowerCarState>,
}
impl PowerTrainState {
    pub fn power_available(&self, vehicle: usize) -> bool {
        self.cars
            .iter()
            .find(|s| s.vehicle == vehicle)
            .is_none_or(|s| s.supply.main)
    }
    pub fn valid_for(&self, config: &PowerTrainConfig, time_s: f64) -> bool {
        if !self.initialized {
            return self.cars.is_empty();
        }
        self.cars.len() == config.cars.len()
            && self
                .cars
                .iter()
                .zip(&config.cars)
                .all(|(s, c)| s.vehicle == c.vehicle && s.supply.valid(time_s))
    }
}

pub fn advance(state: &mut TrainSimState, config: &PowerTrainConfig) {
    if !state.power_supply.initialized {
        state.power_supply.cars = config
            .cars
            .iter()
            .map(|c| {
                let mut supply = PowerSupplyState::default();
                supply.hot_start(&c.params, state.time.0);
                PowerCarState {
                    vehicle: c.vehicle,
                    supply,
                }
            })
            .collect();
        state.power_supply.initialized = true;
    }
    for (car, current) in config.cars.iter().zip(&mut state.power_supply.cars) {
        let (source_available, contact_closed) = match car.source {
            PowerSource::Electric => state
                .electric
                .cars
                .iter()
                .find(|s| s.vehicle == car.vehicle)
                .map_or((false, false), |s| {
                    (
                        s.contact_voltage_v > 0.
                            && matches!(
                                s.loss,
                                crate::electric::PowerLoss::None
                                    | crate::electric::PowerLoss::Starting
                                    | crate::electric::PowerLoss::Breaker
                            ),
                        s.breaker == BreakerState::Closed,
                    )
                }),
            PowerSource::Diesel => (
                state.diesel.car(car.vehicle).is_none_or(|s| {
                    s.phase == EnginePhase::Running && s.command_running && s.fuel_l > 0.
                }),
                car.enabled,
            ),
            PowerSource::Steam => (true, true),
        };
        current.supply.update(
            PowerSupplyInput {
                source: car.source,
                source_available,
                contact_closed: contact_closed && car.enabled,
                battery_on: car.battery_on,
                master_key_on: car.master_key_on,
                train_supply_switch_on: !car.params.manual_train_supply
                    || car.train_supply_switch_on,
            },
            &car.params,
            state.time.0,
        );
    }
}
