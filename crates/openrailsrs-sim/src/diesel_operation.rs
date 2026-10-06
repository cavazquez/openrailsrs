//! Per-vehicle finite diesel tank and engine lifecycle. No rendering dependency.
use openrailsrs_train::{Consist, Vehicle, diesel::DieselEngineParams};
use serde::{Deserialize, Serialize};

pub const DIESEL_KG_PER_L: f64 = 0.8508; // OR 1.6.1 DieselWeightKgpL

#[derive(Clone, Debug)]
pub struct DieselCar {
    pub vehicle: usize,
    pub connected: bool,
    pub battery: bool,
    pub params: openrailsrs_formats::DieselOperatingParams,
    pub governor: DieselEngineParams,
}
#[derive(Clone, Debug, Default)]
pub struct DieselTrainConfig {
    pub cars: Vec<DieselCar>,
}
impl DieselTrainConfig {
    pub fn from_consist(consist: &Consist) -> Self {
        Self {
            cars: consist
                .vehicles
                .iter()
                .enumerate()
                .filter_map(|(vehicle, v)| {
                    let Vehicle::Loco(l) = v else { return None };
                    let params = l.diesel_operation.as_ref()?;
                    let governor = l
                        .diesel_traction
                        .as_ref()
                        .and_then(|m| m.engine.as_deref())
                        .cloned()
                        .unwrap_or_else(|| {
                            DieselEngineParams::from_msts_defaults(l.max_power_w, 300., 600., 40.)
                        });
                    Some(DieselCar {
                        vehicle,
                        connected: true,
                        battery: true,
                        params: (**params).clone(),
                        governor,
                    })
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnginePhase {
    Stopped,
    Starting,
    #[default]
    Running,
    Stopping,
}
impl EnginePhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Detenido",
            Self::Starting => "Arrancando",
            Self::Running => "En marcha",
            Self::Stopping => "Deteniéndose",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DieselCarState {
    pub vehicle: usize,
    pub command_running: bool,
    pub phase: EnginePhase,
    pub rpm: f64,
    pub demanded_rpm: f64,
    pub fuel_l: f64,
    pub consumed_l: f64,
    #[serde(default)]
    pub refilled_l: f64,
    pub flow_lps: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DieselTrainState {
    pub initialized: bool,
    pub cars: Vec<DieselCarState>,
}
impl DieselTrainState {
    pub fn initialize(&mut self, config: &DieselTrainConfig) {
        if self.initialized {
            return;
        }
        self.cars = config
            .cars
            .iter()
            .map(|c| {
                let running = c.params.capacity_l > 0.;
                DieselCarState {
                    vehicle: c.vehicle,
                    command_running: running,
                    phase: if running {
                        EnginePhase::Running
                    } else {
                        EnginePhase::Stopped
                    },
                    rpm: if running { c.governor.idle_rpm } else { 0. },
                    demanded_rpm: if running { c.governor.idle_rpm } else { 0. },
                    fuel_l: c.params.capacity_l,
                    consumed_l: 0.,
                    refilled_l: 0.,
                    flow_lps: 0.,
                }
            })
            .collect();
        self.initialized = true;
    }
    pub fn power_available(&self, vehicle: usize) -> bool {
        self.cars
            .iter()
            .find(|c| c.vehicle == vehicle)
            .is_none_or(|c| c.phase == EnginePhase::Running && c.command_running && c.fuel_l > 0.)
    }
    pub fn car(&self, vehicle: usize) -> Option<&DieselCarState> {
        self.cars.iter().find(|c| c.vehicle == vehicle)
    }
    pub fn valid_for(&self, config: &DieselTrainConfig) -> bool {
        if !self.initialized {
            return self.cars.is_empty() && config.cars.is_empty();
        }
        self.cars.len() == config.cars.len()
            && self.cars.iter().zip(&config.cars).all(|(s, c)| {
                s.vehicle == c.vehicle
                    && [
                        s.rpm,
                        s.demanded_rpm,
                        s.fuel_l,
                        s.consumed_l,
                        s.refilled_l,
                        s.flow_lps,
                    ]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.)
                    && s.fuel_l <= c.params.capacity_l
                    && s.rpm <= c.governor.max_rpm * 1.5
                    && s.demanded_rpm <= c.governor.max_rpm * 1.5
                    && (s.fuel_l + s.consumed_l - c.params.capacity_l - s.refilled_l).abs() < 1e-5
                    && (s.phase != EnginePhase::Stopped || s.rpm == 0.)
                    && (s.phase != EnginePhase::Running || s.fuel_l > 0.)
            })
    }
}

fn interpolate(tab: &[(f64, f64)], rpm: f64) -> f64 {
    if tab.len() < 2 {
        return tab.first().map_or(0., |p| p.1);
    }
    // OR's Interpolator extrapolates the end segments. Never allow a
    // questionable authored table to create fuel with negative consumption.
    let pair = tab
        .windows(2)
        .find(|p| rpm <= p[1].0)
        .unwrap_or(&tab[tab.len() - 2..]);
    let t = (rpm - pair[0].0) / (pair[1].0 - pair[0].0);
    (pair[0].1 + t * (pair[1].1 - pair[0].1)).max(0.)
}

/// Return grams burned this tick, including idle and starting consumption.
pub fn advance(
    state: &mut DieselTrainState,
    config: &DieselTrainConfig,
    throttle: f64,
    dt: f64,
    native: bool,
) -> f64 {
    state.initialize(config);
    if !dt.is_finite() || dt < 0. || !throttle.is_finite() {
        return 0.;
    }
    let mut consumed = 0.;
    for (s, car) in state.cars.iter_mut().zip(&config.cars) {
        if (!s.command_running || !car.battery || s.fuel_l <= 0.) && s.phase != EnginePhase::Stopped
        {
            s.phase = EnginePhase::Stopping;
            s.demanded_rpm = 0.;
        } else if s.command_running
            && car.battery
            && s.fuel_l > 0.
            && matches!(s.phase, EnginePhase::Stopped | EnginePhase::Stopping)
        {
            s.phase = EnginePhase::Starting;
            s.demanded_rpm = car.params.starting_rpm;
        }
        if s.phase == EnginePhase::Running {
            let throttle = if car.connected { throttle } else { 0. };
            s.demanded_rpm = car.governor.target_rpm(throttle);
            s.rpm = if native {
                car.governor.advance_native_rpm(s.rpm, throttle, dt)
            } else {
                car.governor.advance_rpm(s.rpm, throttle, dt)
            };
        } else if s.phase != EnginePhase::Stopped {
            s.rpm = car
                .governor
                .advance_native_target_rpm(s.rpm, s.demanded_rpm, dt);
        }
        if s.phase == EnginePhase::Starting {
            if s.rpm > 0.9 * car.params.starting_rpm && s.rpm <= car.params.starting_rpm {
                s.demanded_rpm = 1.1 * car.params.confirmation_rpm;
            }
            if s.rpm > car.params.confirmation_rpm {
                s.phase = EnginePhase::Running;
            }
        } else if s.rpm == 0. {
            s.phase = EnginePhase::Stopped;
        }
        let burning = s.phase == EnginePhase::Running
            || (matches!(s.phase, EnginePhase::Starting | EnginePhase::Stopping)
                && s.rpm >= car.params.starting_rpm);
        let flow = if burning {
            // OR Initialize also generates an RPM table from the legacy
            // DieselUsedPerHourAtIdle/MaxPower endpoints.
            interpolate(&car.params.consumption_lph, s.rpm) / 3600.
        } else {
            0.
        };
        let used = (flow * dt.max(0.)).min(s.fuel_l);
        s.fuel_l -= used;
        s.consumed_l += used;
        s.flow_lps = if dt > 0. { used / dt } else { flow };
        consumed += used * DIESEL_KG_PER_L * 1000.;
        if s.fuel_l <= 0. {
            s.command_running = false;
            s.phase = if s.rpm == 0. {
                EnginePhase::Stopped
            } else {
                EnginePhase::Stopping
            };
            s.demanded_rpm = 0.;
        }
    }
    consumed
}
