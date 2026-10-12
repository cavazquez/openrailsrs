//! Optional declared presentation data. It never changes railway forces.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VehiclePresentation {
    pub vehicle: usize,
    pub suspension: Option<SuspensionDef>,
    pub horn: Option<SoundEmitterDef>,
    pub passenger_lamps: Vec<PassengerLampDef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SuspensionDef {
    pub empty_mass_kg: f64,
    pub stiffness_npm: f64,
    pub travel_m: f64,
    pub damping_ratio: f64,
}
impl SuspensionDef {
    pub fn valid(&self) -> bool {
        [
            self.empty_mass_kg,
            self.stiffness_npm,
            self.travel_m,
            self.damping_ratio,
        ]
        .iter()
        .all(|v| v.is_finite())
            && (1000. ..=500000.).contains(&self.empty_mass_kg)
            && (100000. ..=100000000.).contains(&self.stiffness_npm)
            && (0.001..=0.15).contains(&self.travel_m)
            && (0.2..=2.).contains(&self.damping_ratio)
    }
    pub fn depression(&self, mass_kg: f64) -> f64 {
        ((mass_kg - self.empty_mass_kg).max(0.) * 9.81 / self.stiffness_npm).min(self.travel_m)
    }
    pub fn natural_frequency(&self, mass_kg: f64) -> f64 {
        (self.stiffness_npm / mass_kg.max(1000.)).sqrt()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SoundEmitterDef {
    pub position_m: [f32; 3],
    pub direction: [f32; 3],
}
impl SoundEmitterDef {
    pub fn valid(&self) -> bool {
        self.position_m
            .iter()
            .all(|p| p.is_finite() && p.abs() <= 100.)
            && self
                .direction
                .iter()
                .all(|p| p.is_finite() && p.abs() <= 1.)
            && self.direction.iter().map(|p| p * p).sum::<f32>() > 0.01
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PassengerLampDef {
    pub position_m: [f32; 3],
    pub direction: [f32; 3],
    pub lumens: f32,
    pub range_m: f32,
}
impl VehiclePresentation {
    pub fn valid(&self) -> bool {
        self.vehicle < 256
            && self.suspension.as_ref().is_none_or(SuspensionDef::valid)
            && self.horn.as_ref().is_none_or(SoundEmitterDef::valid)
            && self.passenger_lamps.len() <= 8
            && self.passenger_lamps.iter().all(|l| {
                l.position_m
                    .iter()
                    .all(|p| p.is_finite() && p.abs() <= 100.)
                    && l.direction.iter().all(|p| p.is_finite() && p.abs() <= 1.)
                    && l.direction.iter().map(|v| v * v).sum::<f32>() > 0.01
                    && l.lumens.is_finite()
                    && (1. ..=1000.).contains(&l.lumens)
                    && l.range_m.is_finite()
                    && (1. ..=8.).contains(&l.range_m)
            })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_loaded_spring_sits_lower_and_has_slower_response_without_exceeding_travel() {
        let spring = SuspensionDef {
            empty_mass_kg: 30000.,
            stiffness_npm: 1500000.,
            travel_m: 0.1,
            damping_ratio: 1.,
        };
        assert!(spring.valid());
        assert_eq!(spring.depression(30000.), 0.);
        assert!(spring.depression(45000.) > spring.depression(35000.));
        assert!(spring.natural_frequency(45000.) < spring.natural_frequency(30000.));
        assert_eq!(spring.depression(500000.), 0.1);
    }
}
