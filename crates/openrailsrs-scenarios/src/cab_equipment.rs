//! Optional author-declared cab equipment. No washer tank is invented for legacy stock.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CabEquipmentDef {
    pub vehicle: usize,
    pub left_window: bool,
    pub right_window: bool,
    pub cab_door: bool,
    pub washer_capacity_l: f64,
    pub washer_flow_lps: f64,
}
impl CabEquipmentDef {
    pub fn valid(&self) -> bool {
        self.vehicle < 256
            && self.washer_capacity_l.is_finite()
            && (0.0..=100.).contains(&self.washer_capacity_l)
            && self.washer_flow_lps.is_finite()
            && (0.0..=1.).contains(&self.washer_flow_lps)
            && (self.washer_capacity_l == 0. || self.washer_flow_lps > 0.)
    }
}
