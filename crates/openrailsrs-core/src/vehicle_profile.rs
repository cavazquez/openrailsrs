//! Personal tuning is keyed to an exact effective author-source version.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VehicleProfile {
    pub source_sha256: String,
    pub mass_factor: f64,
    pub brake_factor: f64,
    pub resistance_factor: f64,
}
impl Default for VehicleProfile {
    fn default() -> Self {
        Self {
            source_sha256: String::new(),
            mass_factor: 1.,
            brake_factor: 1.,
            resistance_factor: 1.,
        }
    }
}
impl VehicleProfile {
    pub fn valid(&self) -> bool {
        self.source_sha256.len() == 64
            && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && [self.mass_factor, self.brake_factor, self.resistance_factor]
                .iter()
                .all(|v| v.is_finite() && (0.25..=4.).contains(v))
    }
}
