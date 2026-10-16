//! Optional declared presentation data. It never changes railway forces.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VehiclePresentation {
    pub vehicle: usize,
    /// Shape clearance only. Mechanical angle of attack and lateral travel are
    /// configured under train.bogie_dynamics and never use this clamp.
    pub bogie_yaw_limit_rad: Option<f32>,
    pub suspension: Option<SuspensionDef>,
    pub horn: Option<SoundEmitterDef>,
    pub passenger_lamps: Vec<PassengerLampDef>,
    /// Axis-aligned permitted eye volumes in the authored MSTS car frame.
    pub walk_boxes_m: Vec<[[f32; 3]; 2]>,
    /// Original mesh name and endpoints in the imported car's local Bevy frame
    /// (after MSTS Z conversion, before the car's world transform).
    pub connections: Vec<FlexibleConnectionDef>,
    pub jacobs: Option<JacobsDef>,
    pub mirrors: Vec<MirrorDef>,
    pub boarding_doors: Vec<PassengerDoorDef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PassengerDoorDef {
    pub position_m: [f32; 3],
    pub side: crate::vehicle_systems::DoorSide,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MirrorDef {
    pub position_m: [f32; 3],
    pub direction: [f32; 3],
    pub fov_degrees: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    Gangway,
    AirHose,
    ElectricCable,
    Coupler,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FlexibleConnectionDef {
    pub matrix: String,
    pub neighbor: usize,
    pub own_m: [f32; 3],
    pub rest_far_m: [f32; 3],
    pub other_m: [f32; 3],
    pub kind: ConnectionKind,
    pub maximum_span_m: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct JacobsDef {
    pub neighbor: usize,
    /// Native longitudinal coordinate, positive toward the car's rear (MSTS Z),
    /// matching the original bogie matrices and TrainCarSupports.
    pub own_support_m: f32,
    pub other_support_m: f32,
    pub bogie_matrix: String,
}
fn coordinate(p: &[f32; 3]) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() <= 100.)
}
impl FlexibleConnectionDef {
    pub fn valid(&self) -> bool {
        self.matrix.len() <= 128
            && !self.matrix.is_empty()
            && self.neighbor < 256
            && coordinate(&self.own_m)
            && coordinate(&self.rest_far_m)
            && coordinate(&self.other_m)
            && self.maximum_span_m.is_finite()
            && (0.02..=4.).contains(&self.maximum_span_m)
    }
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
        self.bogie_yaw_limit_rad
            .is_none_or(|v| v.is_finite() && (0.001..=1.2).contains(&v))
            && self.walk_boxes_m.len() <= 32
            && self.boarding_doors.len() <= 8
            && self
                .boarding_doors
                .iter()
                .all(|d| coordinate(&d.position_m) && d.position_m[0].abs() >= 1.)
            && self.mirrors.len() <= 2
            && self.mirrors.iter().all(|m| {
                coordinate(&m.position_m)
                    && coordinate(&m.direction)
                    && m.direction.iter().map(|p| p * p).sum::<f32>() > 0.01
                    && m.fov_degrees.is_finite()
                    && (25. ..=90.).contains(&m.fov_degrees)
            })
            && self.connections.len() <= 16
            && self.connections.iter().all(FlexibleConnectionDef::valid)
            && self.jacobs.as_ref().is_none_or(|j| {
                j.neighbor < 256
                    && j.neighbor != self.vehicle
                    && j.own_support_m.is_finite()
                    && j.other_support_m.is_finite()
                    && j.own_support_m.abs() <= 50.
                    && j.other_support_m.abs() <= 50.
                    && !j.bogie_matrix.is_empty()
                    && j.bogie_matrix.len() <= 128
            })
            && self.walk_boxes_m.iter().all(|b| {
                b.iter().flatten().all(|v| v.is_finite() && v.abs() <= 100.)
                    && (0..3).all(|i| b[0][i] < b[1][i])
            })
            && self.vehicle < 256
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
