//! Explicit rack infrastructure and vehicle compatibility. Coordinates refer
//! to the initial scenario path and stay fixed when the driver reverses it.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackDefinition {
    #[serde(default)]
    pub vehicles: Vec<RackVehicle>,
    #[serde(default)]
    pub sections: Vec<RackSection>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackVehicle {
    pub vehicle: usize,
    pub system: String,
    pub maximum_force_n: f64,
    pub maximum_power_w: f64,
    pub brake_force_n: f64,
    pub engagement_speed_mps: f64,
    pub maximum_speed_mps: f64,
    pub engagement_time_s: f64,
    #[serde(default)]
    pub initially_requested: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackSection {
    pub start_chainage_m: f64,
    pub end_chainage_m: f64,
    pub system: String,
    pub maximum_speed_mps: f64,
}
impl RackDefinition {
    pub fn valid(&self, count: usize, path_length_m: f64) -> bool {
        self.vehicles.len() <= count
            && self.sections.len() <= 4096
            && self.vehicles.iter().all(|v| v.vehicle < count && v.valid())
            && self
                .vehicles
                .iter()
                .map(|v| v.vehicle)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.vehicles.len()
            && self
                .sections
                .iter()
                .all(|s| s.valid() && s.end_chainage_m <= path_length_m)
            && self
                .sections
                .windows(2)
                .all(|p| p[0].end_chainage_m <= p[1].start_chainage_m)
    }
}
impl RackVehicle {
    pub fn valid(&self) -> bool {
        !self.system.trim().is_empty()
            && self.system.len() <= 64
            && (1. ..=10_000_000.).contains(&self.maximum_force_n)
            && (1. ..=100_000_000.).contains(&self.maximum_power_w)
            && (1. ..=10_000_000.).contains(&self.brake_force_n)
            && (0. ..=5.).contains(&self.engagement_speed_mps)
            && (self.engagement_speed_mps.max(0.1)..=30.).contains(&self.maximum_speed_mps)
            && (0. ..=60.).contains(&self.engagement_time_s)
    }
}
impl RackSection {
    pub fn valid(&self) -> bool {
        self.start_chainage_m.is_finite()
            && self.start_chainage_m >= 0.
            && self.end_chainage_m.is_finite()
            && self.end_chainage_m > self.start_chainage_m
            && !self.system.trim().is_empty()
            && self.system.len() <= 64
            && (0.1..=30.).contains(&self.maximum_speed_mps)
    }
}
