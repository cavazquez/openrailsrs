//! Declared mechanical bogie overrides. Visual yaw/pivots remain presentation.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BogieDynamicsDef {
    pub vehicle: usize,
    /// Formation offsets, front then rear, in metres about the car centre.
    #[serde(default)]
    pub support_offsets_m: Option<[f64; 2]>,
    #[serde(default)]
    pub lateral_suspension: Option<LateralSuspensionDef>,
    /// Explicit mechanical shared support with an adjacent body. This is
    /// independent of presentation.jacobs and its mesh/matrix binding.
    #[serde(default)]
    pub shared_with: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LateralSuspensionDef {
    pub stiffness_npm: f64,
    pub damping_nspm: f64,
    pub travel_m: f64,
    pub maximum_attack_rad: f64,
}
impl BogieDynamicsDef {
    pub fn valid(&self) -> bool {
        self.vehicle < 256
            && self.support_offsets_m.is_none_or(|p| {
                p.iter().all(|x| x.is_finite() && x.abs() <= 50.) && p[0] > 0. && p[1] < 0.
            })
            && self
                .shared_with
                .is_none_or(|v| v < 256 && self.vehicle.abs_diff(v) == 1)
            && self.lateral_suspension.as_ref().is_none_or(|p| {
                (10_000. ..=100_000_000.).contains(&p.stiffness_npm)
                    && (0. ..=10_000_000.).contains(&p.damping_nspm)
                    && (0.001..=0.15).contains(&p.travel_m)
                    && (0.001..=0.3).contains(&p.maximum_attack_rad)
            })
    }
}
