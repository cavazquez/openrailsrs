//! Default locomotive power logic ported from the pinned OR 1.6.1 scripts.
//! Physical source/contact states are inputs; no renderer or .NET is required.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    #[default]
    Diesel,
    Electric,
    Steam,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerSupplyParams {
    pub main_delay_s: f64,
    pub auxiliary_delay_s: f64,
    pub relay_delay_s: f64,
    pub train_supply_fitted: bool,
    pub manual_train_supply: bool,
    pub train_supply_min_rpm: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct PowerSupplyInput {
    pub source: PowerSource,
    pub source_available: bool,
    pub contact_closed: bool,
    pub battery_on: bool,
    pub master_key_on: bool,
    pub train_supply_switch_on: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerSupplyState {
    pub main: bool,
    pub auxiliary: bool,
    pub low_voltage: bool,
    pub cab: bool,
    pub train_supply: bool,
    pub dynamic_brake_available: bool,
    pub minimum_diesel_rpm: f64,
    pub main_started_s: Option<f64>,
    pub auxiliary_started_s: Option<f64>,
    pub relay_started_s: Option<f64>,
}

impl PowerSupplyState {
    /// OR timers start on the first eligible update. A contact interruption
    /// resets main power; diesel auxiliaries keep running with an open relay.
    pub fn update(&mut self, input: PowerSupplyInput, params: &PowerSupplyParams, time_s: f64) {
        self.low_voltage = input.battery_on;
        self.cab = input.battery_on && input.master_key_on;
        let steam = input.source == PowerSource::Steam;
        let source = steam || input.source_available;
        let contact_closed = if input.source == PowerSource::Diesel {
            timer(
                &mut self.relay_started_s,
                source && input.contact_closed,
                params.relay_delay_s,
                time_s,
            )
        } else {
            input.contact_closed
        };
        let main_ready = steam || (source && contact_closed);
        let auxiliary_ready =
            steam || (source && (input.source == PowerSource::Diesel || input.contact_closed));
        self.main = steam
            || timer(
                &mut self.main_started_s,
                main_ready,
                params.main_delay_s,
                time_s,
            );
        self.auxiliary = steam
            || timer(
                &mut self.auxiliary_started_s,
                auxiliary_ready,
                params.auxiliary_delay_s,
                time_s,
            );
        self.train_supply =
            params.train_supply_fitted && self.auxiliary && input.train_supply_switch_on;
        self.minimum_diesel_rpm = if input.source == PowerSource::Diesel && self.train_supply {
            params.train_supply_min_rpm
        } else {
            0.
        };
        self.dynamic_brake_available = input.source == PowerSource::Electric || self.main;
    }

    pub fn hot_start(&mut self, params: &PowerSupplyParams, time_s: f64) {
        self.main_started_s = Some(time_s - params.main_delay_s);
        self.auxiliary_started_s = Some(time_s - params.auxiliary_delay_s);
        self.relay_started_s = Some(time_s - params.relay_delay_s);
    }

    pub fn valid(&self, time_s: f64) -> bool {
        self.minimum_diesel_rpm.is_finite()
            && self.minimum_diesel_rpm >= 0.
            && [
                self.main_started_s,
                self.auxiliary_started_s,
                self.relay_started_s,
            ]
            .into_iter()
            .flatten()
            .all(|t| t.is_finite() && t <= time_s)
            && (!self.cab || self.low_voltage)
            && (!self.train_supply || self.auxiliary)
    }
}

fn timer(started: &mut Option<f64>, eligible: bool, delay_s: f64, time_s: f64) -> bool {
    if !eligible {
        *started = None;
        return false;
    }
    let start = *started.get_or_insert(time_s);
    time_s >= start + delay_s
}
