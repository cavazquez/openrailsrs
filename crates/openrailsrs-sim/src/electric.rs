//! Per-vehicle electrical supply. No dependence on Bevy, scenery or GPU assets.
use crate::{SimError, path_data::PathData, state::TrainSimState};
use openrailsrs_core::electrification::{
    ElectricPickup, ElectricSupply, ElectricVehicleParams, RouteElectricSupply,
};
use openrailsrs_train::{Consist, Vehicle};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct ElectricCar {
    pub flipped: bool,
    pub vehicle: usize,
    pub offset_m: f64,
    pub enabled: bool,
    pub params: ElectricVehicleParams,
}

#[derive(Clone, Debug, Default)]
pub struct ElectricTrainConfig {
    pub length_m: f64,
    pub route: RouteElectricSupply,
    pub cars: Vec<ElectricCar>,
    pub fallback_cars: Vec<(usize, openrailsrs_train::TractiveCurve)>,
}

impl ElectricTrainConfig {
    pub fn load(
        route_dir: &Path,
        route: Option<&RouteElectricSupply>,
        pickups: &[openrailsrs_core::electrification::ElectricPickupOverride],
        consist: &Consist,
        graph: &openrailsrs_track::TrackGraph,
    ) -> Result<Self, SimError> {
        #[derive(Deserialize)]
        struct Metadata {
            route: openrailsrs_route::load::RouteMeta,
        }
        let text = std::fs::read_to_string(route_dir.join("track.toml"))
            .map_err(|e| SimError::Msg(format!("electric route metadata: {e}")))?;
        let metadata: Metadata = toml::from_str(&text)
            .map_err(|e| SimError::Msg(format!("electric route metadata: {e}")))?;
        let supply = if let Some(supply) = route.or(metadata.route.electric_supply.as_ref()) {
            supply.clone()
        } else if openrailsrs_formats::find_trk_path(route_dir).is_some() {
            let native = openrailsrs_formats::RouteFile::from_route_dir(route_dir)
                .map_err(|e| SimError::Msg(format!("electrification: {e}")))?;
            RouteElectricSupply {
                supply: ElectricSupply {
                    kind: if native.overhead_wire.electrified {
                        ElectricPickup::Overhead
                    } else {
                        ElectricPickup::None
                    },
                    voltage_v: if native.overhead_wire.electrified {
                        native.max_line_voltage_v
                    } else {
                        0.
                    },
                },
                sections: vec![],
            }
        } else {
            RouteElectricSupply::default()
        };
        let valid_supply = |s: &ElectricSupply| {
            s.voltage_v.is_finite()
                && s.voltage_v >= 0.
                && (s.kind != ElectricPickup::None || s.voltage_v == 0.)
        };
        if !valid_supply(&supply.supply) {
            return Err(SimError::Msg("Tensión de vía inválida".into()));
        }
        for (i, section) in supply.sections.iter().enumerate() {
            let valid_edge = graph.edge(&section.edge).is_some_and(|e| {
                section.start_m.is_finite()
                    && section.end_m.is_finite()
                    && section.start_m >= 0.
                    && section.start_m < section.end_m
                    && section.end_m <= e.length_m
            });
            if !valid_edge
                || !valid_supply(&section.supply)
                || supply.sections[..i].iter().any(|s| {
                    s.edge == section.edge && s.start_m < section.end_m && section.start_m < s.end_m
                })
            {
                return Err(SimError::Msg(format!(
                    "Sector eléctrico inválido o superpuesto: {}",
                    section.edge
                )));
            }
        }
        let mut result = Self {
            length_m: consist.vehicle_lengths_m().iter().sum(),
            route: supply,
            cars: vec![],
            fallback_cars: Self::fallback_curves(consist),
        };
        let mut offset = 0.;
        for (i, vehicle) in consist.vehicles.iter().enumerate() {
            let length = match vehicle {
                Vehicle::Loco(l) => l.length_m,
                Vehicle::Wagon(w) => w.length_m,
            };
            if let Vehicle::Loco(loco) = vehicle
                && let Some(params) = &loco.electric
            {
                result.cars.push(ElectricCar {
                    flipped: loco.flipped,
                    vehicle: i,
                    offset_m: offset + length * 0.5,
                    enabled: true,
                    params: (**params).clone(),
                });
            }
            offset += length;
        }
        for (i, item) in pickups.iter().enumerate() {
            if item.kind == ElectricPickup::None
                || pickups[..i].iter().any(|p| p.vehicle == item.vehicle)
            {
                return Err(SimError::Msg("Toma eléctrica inválida o repetida".into()));
            }
            let car = result
                .cars
                .iter_mut()
                .find(|c| c.vehicle == item.vehicle)
                .ok_or_else(|| {
                    SimError::Msg(format!("El vehículo {} no es eléctrico", item.vehicle))
                })?;
            car.params.pickup = item.kind;
        }
        Ok(result)
    }

    pub fn fallback_curves(consist: &Consist) -> Vec<(usize, openrailsrs_train::TractiveCurve)> {
        consist
            .vehicles
            .iter()
            .enumerate()
            .filter_map(|(i, v)| match v {
                Vehicle::Loco(l) if l.steam.is_none() => Some((
                    i,
                    l.tractive_curve.clone().unwrap_or_else(|| {
                        openrailsrs_train::TractiveCurve::from_power_and_effort(
                            l.max_power_w,
                            l.max_tractive_effort_n,
                        )
                    }),
                )),
                _ => None,
            })
            .collect()
    }

    pub fn supply_at(
        &self,
        state: &TrainSimState,
        path: &PathData,
        chainage: f64,
    ) -> ElectricSupply {
        let mut position = chainage.max(0.);
        for (edge, data) in state.path_edges.iter().zip(&path.edges) {
            if position < data.length_m {
                return self
                    .route
                    .sections
                    .iter()
                    .find(|s| s.edge == *edge && position >= s.start_m && position < s.end_m)
                    .map_or_else(|| self.route.supply.clone(), |s| s.supply.clone());
            }
            position -= data.length_m;
        }
        self.route.supply.clone()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakerState {
    #[default]
    Open,
    Closing,
    Closed,
}
impl BreakerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Abierto",
            Self::Closing => "Cerrando",
            Self::Closed => "Cerrado",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerLoss {
    #[default]
    None,
    NoSupply,
    IncompatiblePickup,
    Voltage,
    Pantograph,
    Breaker,
    Starting,
    Isolated,
}
impl PowerLoss {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Tracción disponible",
            Self::NoSupply => "Sin tensión de vía",
            Self::IncompatiblePickup => "Toma incompatible",
            Self::Voltage => "Tensión incompatible",
            Self::Pantograph => "Pantógrafo sin contacto",
            Self::Breaker => "Disyuntor abierto",
            Self::Starting => "Conectando alimentación",
            Self::Isolated => "Motor aislado",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElectricCarState {
    #[serde(default)]
    pub pantographs: Vec<PantographState>,
    #[serde(default)]
    pub pantograph_filter: crate::voltage_filter::VoltageFilter,
    #[serde(default)]
    pub voltage_filter: crate::voltage_filter::VoltageFilter,
    #[serde(default)]
    pub filtered_contact_voltage_v: f64,
    #[serde(default)]
    pub filter_voltage_v: f64,
    pub vehicle: usize,
    pub pantograph_fraction: f64,
    pub breaker: BreakerState,
    pub breaker_elapsed_s: f64,
    pub power_elapsed_s: f64,
    pub line_voltage_v: f64,
    pub contact_voltage_v: f64,
    pub main_power: bool,
    pub loss: PowerLoss,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PantographState {
    /// None follows the consist command; a local command controls only this collector.
    pub command_up: Option<bool>,
    pub fraction: f64,
    pub contact_voltage_v: f64,
    #[serde(default = "contact_enabled_default")]
    pub contact_enabled: bool,
}
fn contact_enabled_default() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElectricTrainState {
    pub initialized: bool,
    pub pantograph_command_up: bool,
    pub breaker_command_closed: bool,
    pub cars: Vec<ElectricCarState>,
}
impl Default for ElectricTrainState {
    fn default() -> Self {
        Self {
            initialized: false,
            pantograph_command_up: true,
            breaker_command_closed: true,
            cars: vec![],
        }
    }
}

impl ElectricTrainState {
    pub fn power_available(&self, vehicle: usize) -> bool {
        self.cars
            .iter()
            .find(|c| c.vehicle == vehicle)
            .is_none_or(|c| c.main_power)
    }

    /// Fail closed on malformed new saves; legacy saves without this field are
    /// initialized from the current content on their first simulation quantum.
    pub fn valid_for(&self, config: &ElectricTrainConfig) -> bool {
        if !self.initialized {
            return self.cars.is_empty();
        }
        self.cars.len() == config.cars.len()
            && self.cars.iter().zip(&config.cars).all(|(state, car)| {
                state.vehicle == car.vehicle
                    && (state.pantographs.is_empty()
                        || state.pantographs.len() == car.params.pantographs.len())
                    && state.pantographs.iter().all(|p| {
                        p.fraction.is_finite()
                            && (0. ..=1.).contains(&p.fraction)
                            && p.contact_voltage_v.is_finite()
                            && p.contact_voltage_v >= 0.
                    })
                    && state.pantograph_filter.valid()
                    && state.voltage_filter.valid()
                    && [state.filtered_contact_voltage_v, state.filter_voltage_v]
                        .iter()
                        .all(|v| v.is_finite() && *v >= 0.)
                    && state.pantograph_fraction.is_finite()
                    && (0. ..=1.).contains(&state.pantograph_fraction)
                    && [
                        state.breaker_elapsed_s,
                        state.power_elapsed_s,
                        state.line_voltage_v,
                        state.contact_voltage_v,
                    ]
                    .into_iter()
                    .all(|v| v.is_finite() && v >= 0.)
                    && state.breaker_elapsed_s <= car.params.breaker_delay_s
                    && state.power_elapsed_s <= car.params.power_on_delay_s
                    && (!state.main_power
                        || (state.breaker == BreakerState::Closed
                            && state.contact_voltage_v >= car.params.minimum_voltage_v
                            && state.contact_voltage_v > 0.
                            && state.loss == PowerLoss::None))
            })
    }
}

/// Called by the shared physics step, including headless and AI services.
pub fn advance(state: &mut TrainSimState, path: &PathData, config: &ElectricTrainConfig, dt: f64) {
    let dt = if dt.is_finite() { dt.max(0.) } else { 0. };
    let hot_start = !state.electric.initialized;
    if hot_start {
        state.electric.cars = config
            .cars
            .iter()
            .map(|car| ElectricCarState {
                vehicle: car.vehicle,
                pantograph_fraction: if car.params.pickup == ElectricPickup::Overhead
                    && state.electric.pantograph_command_up
                {
                    1.
                } else {
                    0.
                },
                ..Default::default()
            })
            .collect();
        state.electric.initialized = true;
    }
    if config.cars.is_empty() {
        return;
    }
    let chainage = path.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    let supplies: Vec<_> = config
        .cars
        .iter()
        .map(|c| {
            config.supply_at(
                state,
                path,
                chainage
                    + if state.path_reversed {
                        -config.length_m + c.offset_m
                    } else {
                        -c.offset_m
                    },
            )
        })
        .collect();
    let collector_supplies = config
        .cars
        .iter()
        .map(|car| {
            let center = chainage
                + if state.path_reversed {
                    -config.length_m + car.offset_m
                } else {
                    -car.offset_m
                };
            car.params
                .pantographs
                .iter()
                .map(|p| {
                    config.supply_at(
                        state,
                        path,
                        center
                            + p.offset_m
                                * if car.flipped { -1. } else { 1. }
                                * if state.path_reversed { -1. } else { 1. },
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (((car, mut supply), current), collector_supplies) in config
        .cars
        .iter()
        .zip(supplies)
        .zip(&mut state.electric.cars)
        .zip(collector_supplies)
    {
        let p = &car.params;
        let previous_fraction = current.pantograph_fraction;
        let up = state.electric.pantograph_command_up;
        let mut contact_delay = if p.pickup == ElectricPickup::Overhead && up {
            (1. - current.pantograph_fraction) * p.pantograph_delay_s
        } else {
            0.
        };
        let delta = if p.pantograph_delay_s > 0. {
            dt / p.pantograph_delay_s
        } else {
            1.
        };
        current.pantograph_fraction = if p.pickup == ElectricPickup::Overhead {
            (current.pantograph_fraction + if up { delta } else { -delta }).clamp(0., 1.)
        } else {
            0.
        };
        current.line_voltage_v = supply.voltage_v;
        let mut pickup_ready =
            p.pickup != ElectricPickup::Overhead || (up && current.pantograph_fraction >= 1.);
        current.contact_voltage_v = if pickup_ready && supply.kind == p.pickup {
            supply.voltage_v
        } else {
            0.
        };
        if p.pickup == ElectricPickup::Overhead && !p.pantographs.is_empty() {
            if current.pantographs.is_empty() {
                current.pantographs = p
                    .pantographs
                    .iter()
                    .enumerate()
                    .map(|(i, _)| PantographState {
                        command_up: if i == 0 { None } else { Some(false) },
                        fraction: if i == 0 { previous_fraction } else { 0. },
                        contact_enabled: true,
                        ..Default::default()
                    })
                    .collect();
            }
            contact_delay = f64::INFINITY;
            pickup_ready = false;
            current.contact_voltage_v = 0.;
            for ((collector, params), line) in current
                .pantographs
                .iter_mut()
                .zip(&p.pantographs)
                .zip(&collector_supplies)
            {
                let command = collector.command_up.unwrap_or(up);
                if command
                    && collector.contact_enabled
                    && line.kind == p.pickup
                    && line.voltage_v > 0.
                {
                    contact_delay = contact_delay.min((1. - collector.fraction) * params.delay_s);
                }
                let delta = if params.delay_s > 0. {
                    dt / params.delay_s
                } else {
                    1.
                };
                collector.fraction =
                    (collector.fraction + if command { delta } else { -delta }).clamp(0., 1.);
                let ready = command && collector.fraction >= 1.;
                pickup_ready |= ready;
                collector.contact_voltage_v =
                    if ready && collector.contact_enabled && line.kind == p.pickup {
                        line.voltage_v
                    } else {
                        0.
                    };
                current.contact_voltage_v =
                    current.contact_voltage_v.max(collector.contact_voltage_v);
            }
            current.pantograph_fraction = current
                .pantographs
                .iter()
                .map(|p| p.fraction)
                .fold(0., f64::max);
            if let Some(line) = collector_supplies
                .iter()
                .filter(|s| s.kind == p.pickup && s.voltage_v > 0.)
                .max_by(|a, b| a.voltage_v.total_cmp(&b.voltage_v))
            {
                supply = line.clone();
            }
            current.line_voltage_v = supply.voltage_v;
            if !contact_delay.is_finite() {
                contact_delay = 0.;
            }
        }
        let loss = if !car.enabled {
            PowerLoss::Isolated
        } else if supply.kind == ElectricPickup::None || supply.voltage_v <= 0. {
            PowerLoss::NoSupply
        } else if supply.kind != p.pickup {
            PowerLoss::IncompatiblePickup
        } else if !pickup_ready {
            PowerLoss::Pantograph
        } else if current.contact_voltage_v <= 0. {
            PowerLoss::NoSupply
        } else if current.contact_voltage_v < p.minimum_voltage_v
            || p.maximum_voltage_v
                .is_some_and(|v| current.contact_voltage_v > v)
        {
            PowerLoss::Voltage
        } else if !state.electric.breaker_command_closed {
            PowerLoss::Breaker
        } else {
            PowerLoss::None
        };
        if loss != PowerLoss::None {
            current.breaker = BreakerState::Open;
            current.breaker_elapsed_s = 0.;
            current.power_elapsed_s = 0.;
            current.main_power = false;
            current.loss = loss;
            update_voltage_filters(current, dt);
            continue;
        }
        if hot_start {
            current.breaker = BreakerState::Closed;
            current.breaker_elapsed_s = p.breaker_delay_s;
            current.power_elapsed_s = p.power_on_delay_s;
        }
        let contact_dt = (dt - contact_delay).max(0.);
        let available_dt = if current.breaker == BreakerState::Closed {
            contact_dt
        } else {
            let remaining = (p.breaker_delay_s - current.breaker_elapsed_s).max(0.);
            current.breaker_elapsed_s =
                (current.breaker_elapsed_s + contact_dt).min(p.breaker_delay_s);
            if contact_dt >= remaining {
                current.breaker = BreakerState::Closed;
                contact_dt - remaining
            } else {
                current.breaker = BreakerState::Closing;
                0.
            }
        };
        if current.breaker == BreakerState::Closed {
            current.power_elapsed_s =
                (current.power_elapsed_s + available_dt).min(p.power_on_delay_s);
        }
        current.main_power = current.breaker == BreakerState::Closed
            && current.power_elapsed_s >= p.power_on_delay_s;
        current.loss = if current.main_power {
            PowerLoss::None
        } else {
            PowerLoss::Starting
        };
        update_voltage_filters(current, dt);
    }
}

fn update_voltage_filters(current: &mut ElectricCarState, dt: f64) {
    if dt <= 0. {
        return;
    }
    current.filtered_contact_voltage_v = current
        .pantograph_filter
        .filter(current.contact_voltage_v, dt);
    let connected = if current.breaker == BreakerState::Closed {
        current.filtered_contact_voltage_v
    } else {
        0.
    };
    current.filter_voltage_v = current.voltage_filter.filter(connected, dt);
}
