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
    pub bus_connected: Vec<bool>,
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
            bus_connected: vec![true; consist.vehicles.len()],
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
    pub bus_source: Vec<Option<usize>>,
    pub source_hotel_w: std::collections::BTreeMap<usize, f64>,
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
    pub fn vehicle_supplied(&self, vehicle: usize) -> bool {
        if self.bus_source.is_empty() {
            self.cars.iter().any(|c| c.supply.train_supply)
        } else {
            self.bus_source
                .get(vehicle)
                .copied()
                .flatten()
                .is_some_and(|id| {
                    self.cars
                        .iter()
                        .any(|c| c.vehicle == id && c.supply.train_supply)
                })
        }
    }
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
                    if self.source_hotel_w.is_empty() {
                        self.hotel_w / ets_count
                    } else {
                        self.source_hotel_w.get(&vehicle).copied().unwrap_or(0.)
                    }
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
            && self.bus_source.len() <= config.passenger_loads.len()
            && self
                .bus_source
                .iter()
                .flatten()
                .all(|id| config.cars.iter().any(|c| c.vehicle == *id))
            && self.source_hotel_w.iter().all(|(id, w)| {
                w.is_finite() && *w >= 0. && config.cars.iter().any(|c| c.vehicle == *id)
            })
            && (self.source_hotel_w.is_empty()
                || (self.source_hotel_w.values().sum::<f64>() - self.hotel_w).abs() < 1e-5)
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
            PowerSource::Diesel
                if state
                    .vehicle_systems
                    .iter()
                    .any(|c| c.definition.vehicle == car.vehicle && !c.motors.is_empty()) =>
            {
                (
                    state
                        .vehicle_systems
                        .iter()
                        .filter(|c| c.definition.vehicle == car.vehicle)
                        .flat_map(|c| &c.motors)
                        .any(|m| m.running && m.fuel_l > 0. && m.fault.is_none()),
                    car.enabled,
                )
            }
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
    update_bus(state, config, count);
    let powered_count = state
        .power_supply
        .bus_source
        .iter()
        .filter(|s| s.is_some())
        .count();
    let fraction = powered_count as f64 / count.max(1) as f64;
    continuous *= fraction;
    heating *= fraction;
    cooling *= fraction;
    for (i, p) in config.passenger_loads.iter().take(count).enumerate() {
        if !state.power_supply.vehicle_supplied(i) {
            continue;
        }
        continuous += p.continuous_w;
        heating += p.heating_w;
        cooling += p.cooling_w;
    }
    let supply = &mut state.power_supply;
    supply.auxiliary_w = if supply.cars.iter().any(|c| c.supply.auxiliary) {
        config.loads.auxiliary_w
            + state
                .vehicle_systems
                .iter()
                .filter(|c| {
                    !c.definition
                        .auxiliaries
                        .as_ref()
                        .is_some_and(|p| p.steam_compressor_kg_j > 0.)
                        && supply
                            .cars
                            .iter()
                            .any(|s| s.vehicle == c.definition.vehicle && s.supply.auxiliary)
                })
                .map(|c| c.aux.compressor_w)
                .sum::<f64>()
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
    supply.source_hotel_w.clear();
    if powered_count > 0 {
        // Aggregate demand is distributed among supplied cars; native per-car
        // loads only reach sources in the same connected electrical component.
        let aggregate = (config.loads.continuous_w
            + match supply.hotel_mode {
                HotelMode::Auto if ambient_c < 18. => config.loads.heating_w,
                HotelMode::Auto if ambient_c > 24. => config.loads.cooling_w,
                HotelMode::Heating => config.loads.heating_w,
                HotelMode::Cooling => config.loads.cooling_w,
                _ => 0.,
            })
            / config.passenger_loads.len().max(1) as f64;
        for (i, source) in supply.bus_source.iter().enumerate() {
            let Some(source) = source else {
                continue;
            };
            let p = &config.passenger_loads[i];
            let climate = match supply.hotel_mode {
                HotelMode::Auto if ambient_c < 18. => p.heating_w,
                HotelMode::Auto if ambient_c > 24. => p.cooling_w,
                HotelMode::Heating => p.heating_w,
                HotelMode::Cooling => p.cooling_w,
                _ => 0.,
            };
            *supply.source_hotel_w.entry(*source).or_default() +=
                aggregate + p.continuous_w + climate;
        }
    }
    if let Some(previous) = supply.last_energy_time_s {
        supply.energy_j +=
            (supply.auxiliary_w + supply.hotel_w) * (state.time.0 - previous).max(0.0);
    }
    supply.last_energy_time_s = Some(state.time.0);
}

pub(crate) fn update_bus(state: &mut TrainSimState, config: &PowerTrainConfig, count: usize) {
    let mut sources = vec![None; count];
    let connected = |i: usize| {
        config.bus_connected.get(i).copied().unwrap_or(true)
            && state
                .vehicle_systems
                .iter()
                .find(|c| c.definition.vehicle == i)
                .and_then(|c| c.definition.train_bus)
                .unwrap_or(true)
    };
    let mut start = 0;
    while start < count {
        let mut end = start + 1;
        while end < count && connected(end) {
            end += 1;
        }
        let source = state
            .power_supply
            .cars
            .iter()
            .filter(|c| c.vehicle >= start && c.vehicle < end && c.supply.train_supply)
            .min_by_key(|c| c.vehicle)
            .map(|c| c.vehicle);
        sources[start..end].fill(source);
        start = end;
    }
    state.power_supply.bus_source = sources;
}
