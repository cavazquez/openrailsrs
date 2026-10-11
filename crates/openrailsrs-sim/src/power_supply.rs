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
    pub passenger_loads: Vec<openrailsrs_formats::typed::power_supply::PassengerPowerParams>,
    pub loads: openrailsrs_scenarios::PowerLoadSection,
    pub active_fraction: f64,
    pub cars: Vec<PowerCar>,
}
impl PowerTrainConfig {
    pub fn has_loads(&self) -> bool {
        self.loads.total_w() > 0.0
            || self
                .passenger_loads
                .iter()
                .any(|p| p.continuous_w + p.heating_w + p.cooling_w > 0.0)
    }
    pub fn from_consist_with_loads(
        consist: &Consist,
        loads: &openrailsrs_scenarios::PowerLoadSection,
    ) -> Self {
        let mut config = Self::from_consist(consist);
        config.loads = loads.clone();
        config
    }
    pub fn from_consist(consist: &Consist) -> Self {
        Self {
            passenger_loads: consist
                .vehicles
                .iter()
                .map(|v| match v {
                    Vehicle::Loco(l) => l.passenger_power.clone(),
                    Vehicle::Wagon(w) => w.passenger_power.clone(),
                })
                .collect(),
            loads: Default::default(),
            active_fraction: 1.0,
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
    pub hotel_mode: HotelMode,
    pub auxiliary_w: f64,
    pub hotel_w: f64,
    pub energy_j: f64,
    pub last_energy_time_s: Option<f64>,
    pub initialized: bool,
    pub cars: Vec<PowerCarState>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotelMode {
    #[default]
    Auto,
    Off,
    Heating,
    Cooling,
}
impl HotelMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automática",
            Self::Off => "Apagada",
            Self::Heating => "Calefacción",
            Self::Cooling => "Aire acondicionado",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Off,
            Self::Off => Self::Heating,
            Self::Heating => Self::Cooling,
            Self::Cooling => Self::Auto,
        }
    }
}
impl PowerTrainState {
    pub fn vehicle_load_w(&self, vehicle: usize) -> f64 {
        let aux_count = self
            .cars
            .iter()
            .filter(|c| c.supply.auxiliary)
            .count()
            .max(1) as f64;
        let ets_count = self
            .cars
            .iter()
            .filter(|c| c.supply.train_supply)
            .count()
            .max(1) as f64;
        self.cars
            .iter()
            .find(|c| c.vehicle == vehicle)
            .map_or(0.0, |c| {
                (if c.supply.auxiliary {
                    self.auxiliary_w / aux_count
                } else {
                    0.0
                }) + if c.supply.train_supply {
                    self.hotel_w / ets_count
                } else {
                    0.0
                }
            })
    }
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
        [self.auxiliary_w, self.hotel_w, self.energy_j]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
            && self
                .last_energy_time_s
                .is_none_or(|t| t.is_finite() && t <= time_s && t >= 0.0)
            && self.cars.len() == config.cars.len()
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

/// Loads are declared per whole formation and shared only by live suppliers.
/// The same clock is used twice by the power-state update; integrate once.
pub fn update_loads(state: &mut TrainSimState, config: &PowerTrainConfig, ambient_c: f64) {
    let count = (config.passenger_loads.len() as f64 * config.active_fraction).round() as usize;
    let mut continuous = config.loads.continuous_w * config.active_fraction;
    let mut heating = config.loads.heating_w * config.active_fraction;
    let mut cooling = config.loads.cooling_w * config.active_fraction;
    for p in config.passenger_loads.iter().take(count) {
        continuous += p.continuous_w;
        heating += p.heating_w;
        cooling += p.cooling_w;
    }
    let supply = &mut state.power_supply;
    supply.auxiliary_w = if supply.cars.iter().any(|c| c.supply.auxiliary) {
        config.loads.auxiliary_w
    } else {
        0.0
    };
    let climate = match supply.hotel_mode {
        HotelMode::Auto if ambient_c < 18.0 => heating,
        HotelMode::Auto if ambient_c > 24.0 => cooling,
        HotelMode::Heating => heating,
        HotelMode::Cooling => cooling,
        _ => 0.0,
    };
    supply.hotel_w = if supply.cars.iter().any(|c| c.supply.train_supply) {
        continuous + climate
    } else {
        0.0
    };
    if let Some(previous) = supply.last_energy_time_s {
        supply.energy_j +=
            (supply.auxiliary_w + supply.hotel_w) * (state.time.0 - previous).max(0.0);
    }
    supply.last_energy_time_s = Some(state.time.0);
}
