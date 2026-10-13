use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FreightService {
    /// Additional payload, never a replacement of the authored empty mass.
    pub loads: Vec<FreightLoad>,
    pub maximum_speed_mps: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FreightLoad {
    pub vehicle: usize,
    pub cargo_kg: f64,
}
impl FreightService {
    pub fn valid(&self) -> bool {
        self.loads.len() <= 256
            && self
                .maximum_speed_mps
                .is_none_or(|v| v.is_finite() && (1. ..=80.).contains(&v))
            && self.loads.iter().enumerate().all(|(i, p)| {
                p.vehicle < 256
                    && p.cargo_kg.is_finite()
                    && (0. ..=200000.).contains(&p.cargo_kg)
                    && self.loads[..i].iter().all(|o| o.vehicle != p.vehicle)
            })
    }
}
