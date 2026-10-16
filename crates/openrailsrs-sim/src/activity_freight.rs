//! Finite continuous wagon payload. It changes inertia without rewriting WAG mass.
use openrailsrs_core::activity::FreightProfile;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreightCar {
    pub vehicle: usize,
    pub base_mass_kg: f64,
    pub profile: FreightProfile,
    pub load_kg: f64,
    pub received_kg: f64,
    pub unloaded_kg: f64,
}
impl FreightCar {
    pub fn new(vehicle: usize, base_mass_kg: f64, profile: FreightProfile) -> Self {
        Self {
            vehicle,
            base_mass_kg,
            load_kg: profile.initial_kg,
            profile,
            received_kg: 0.,
            unloaded_kg: 0.,
        }
    }
    pub(crate) fn valid(&self, expected: &Self) -> bool {
        self.vehicle == expected.vehicle
            && self.base_mass_kg == expected.base_mass_kg
            && self.profile == expected.profile
            && self.profile.valid()
            && [self.load_kg, self.received_kg, self.unloaded_kg]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
            && self.load_kg <= self.profile.capacity_kg
            && (self.load_kg - self.profile.initial_kg - self.received_kg + self.unloaded_kg).abs()
                < 0.001
    }
    pub fn mass_delta_kg(&self) -> f64 {
        self.profile.empty_mass_kg + self.load_kg - self.base_mass_kg
    }
}
impl crate::LiveDriveSession {
    pub fn cargo_load_kg(&self, vehicle: usize) -> Option<(f64, f64)> {
        self.state
            .freight
            .iter()
            .find(|c| c.vehicle == vehicle)
            .map(|c| (c.load_kg, c.profile.capacity_kg))
    }
}
