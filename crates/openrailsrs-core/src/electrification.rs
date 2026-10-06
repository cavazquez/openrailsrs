//! Physical supply metadata, independent of the renderer and wire meshes.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElectricPickup {
    #[default]
    None,
    Overhead,
    ThirdRail,
    FourthRail,
}

impl ElectricPickup {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Sin electrificar",
            Self::Overhead => "Catenaria",
            Self::ThirdRail => "Tercer riel",
            Self::FourthRail => "Cuarto riel",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElectricSupply {
    #[serde(default)]
    pub kind: ElectricPickup,
    #[serde(default)]
    pub voltage_v: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElectricSection {
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    #[serde(flatten)]
    pub supply: ElectricSupply,
}

/// Sections override the route-wide supply over [start_m, end_m).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteElectricSupply {
    #[serde(flatten)]
    pub supply: ElectricSupply,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<ElectricSection>,
}

/// Immutable authored parameters. Legacy MSTS electric stock uses overhead by
/// default, as in OR 1.6.1; scenarios can explicitly specify conductor rails.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElectricVehicleParams {
    pub pickup: ElectricPickup,
    pub pantograph_delay_s: f64,
    pub breaker_delay_s: f64,
    pub power_on_delay_s: f64,
    pub minimum_voltage_v: f64,
    pub maximum_voltage_v: Option<f64>,
}

impl Default for ElectricVehicleParams {
    fn default() -> Self {
        Self {
            pickup: ElectricPickup::Overhead,
            pantograph_delay_s: 0.,
            breaker_delay_s: 0.,
            power_on_delay_s: 0.,
            minimum_voltage_v: 1.,
            maximum_voltage_v: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElectricPickupOverride {
    /// Zero-based index in the complete formation, including unpowered cars.
    pub vehicle: usize,
    pub kind: ElectricPickup,
}
