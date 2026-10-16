//! Mechanical rack engagement and braking; a declared Rust extension. OR
//! 1.6.1 has no equivalent rack contact solver. No native oracle is claimed.
pub use openrailsrs_scenarios::rack::{RackSection, RackVehicle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RackTrainConfig {
    pub vehicles: Vec<RackVehicle>,
    pub sections: Vec<RackSection>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RackPhase {
    #[default]
    Disengaged,
    Engaging,
    Engaged,
    Fault,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RackCarState {
    pub vehicle: usize,
    pub engage_requested: bool,
    pub phase: RackPhase,
    pub engagement_remaining_s: f64,
    pub section: Option<usize>,
    pub transmitted_force_n: f64,
    pub brake_force_n: f64,
    pub overspeed: bool,
    pub incompatible: bool,
    pub brake_work_j: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RackTrainState {
    pub cars: Vec<RackCarState>,
}
impl RackTrainConfig {
    pub fn from_scenario(
        def: Option<&openrailsrs_scenarios::rack::RackDefinition>,
        count: usize,
        path_length_m: f64,
    ) -> Result<Self, String> {
        let Some(def) = def else {
            return Ok(Self::default());
        };
        if !def.valid(count, path_length_m) {
            return Err("Cremallera: vehículos o tramos inválidos/solapados".into());
        }
        let mut vehicles = def.vehicles.clone();
        vehicles.sort_by_key(|v| v.vehicle);
        Ok(Self {
            vehicles,
            sections: def.sections.clone(),
        })
    }
    /// `old_indices[new]` selects/reorders stock; the route keeps its identity.
    pub fn select(&self, old_indices: &[usize]) -> Self {
        let vehicles = old_indices
            .iter()
            .enumerate()
            .filter_map(|(new, old)| {
                let mut p = self.vehicles.iter().find(|p| p.vehicle == *old)?.clone();
                p.vehicle = new;
                Some(p)
            })
            .collect();
        Self {
            vehicles,
            sections: self.sections.clone(),
        }
    }
    pub fn append(&mut self, other: &Self, offset: usize) -> Result<(), String> {
        if !self.sections.is_empty()
            && !other.sections.is_empty()
            && self.sections != other.sections
        {
            return Err("Las locomotoras no comparten el mismo mapa de cremallera".into());
        }
        if self.sections.is_empty() {
            self.sections = other.sections.clone();
        }
        for p in &other.vehicles {
            let mut p = p.clone();
            p.vehicle += offset;
            self.vehicles.push(p);
        }
        self.vehicles.sort_by_key(|p| p.vehicle);
        Ok(())
    }
}
impl RackTrainState {
    pub fn new(config: &RackTrainConfig) -> Self {
        Self {
            cars: config
                .vehicles
                .iter()
                .map(|p| RackCarState {
                    vehicle: p.vehicle,
                    engage_requested: p.initially_requested,
                    phase: RackPhase::Disengaged,
                    engagement_remaining_s: 0.,
                    section: None,
                    transmitted_force_n: 0.,
                    brake_force_n: 0.,
                    overspeed: false,
                    incompatible: false,
                    brake_work_j: 0.,
                })
                .collect(),
        }
    }
    pub fn valid_for(&self, config: &RackTrainConfig) -> bool {
        self.cars.len() == config.vehicles.len()
            && self.cars.iter().zip(&config.vehicles).all(|(s, p)| {
                s.vehicle == p.vehicle
                    && [
                        s.engagement_remaining_s,
                        s.transmitted_force_n,
                        s.brake_force_n,
                        s.brake_work_j,
                    ]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.)
                    && s.engagement_remaining_s <= p.engagement_time_s
                    && s.section.is_none_or(|i| i < config.sections.len())
                    && s.transmitted_force_n <= p.maximum_force_n + 1e-6
                    && s.brake_force_n <= p.brake_force_n + 1e-6
                    && (s.phase != RackPhase::Engaged || s.section.is_some() && s.engage_requested)
            })
    }
    pub fn request(&mut self, vehicle: usize, engaged: bool) -> Result<(), String> {
        let s = self
            .cars
            .iter_mut()
            .find(|c| c.vehicle == vehicle)
            .ok_or("Vehículo sin piñón de cremallera")?;
        s.engage_requested = engaged;
        if !engaged {
            s.phase = RackPhase::Disengaged;
            s.engagement_remaining_s = 0.;
            s.transmitted_force_n = 0.;
            s.brake_force_n = 0.;
        }
        Ok(())
    }
    pub fn select(&self, old_indices: &[usize]) -> Self {
        Self {
            cars: old_indices
                .iter()
                .enumerate()
                .filter_map(|(new, old)| {
                    let mut s = self.cars.iter().find(|s| s.vehicle == *old)?.clone();
                    s.vehicle = new;
                    Some(s)
                })
                .collect(),
        }
    }
    pub fn append(&mut self, other: &Self, offset: usize) {
        for s in &other.cars {
            let mut s = s.clone();
            s.vehicle += offset;
            self.cars.push(s);
        }
        self.cars.sort_by_key(|s| s.vehicle);
    }
}

/// Advance actual engagement at each vehicle centre. Returns the replacement
/// of wheel/rail traction with positive rack contact; never adds two motors.
pub fn apply_traction(
    state: &mut crate::TrainSimState,
    train: &crate::physics::TrainPhysics,
    path: &crate::path_data::PathData,
    dt: f64,
) -> f64 {
    let head = path.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    let length = train.vehicle_lengths_m.iter().sum::<f64>();
    let path_length = path.total_length_m();
    let mut front = 0.;
    let mut correction = 0.;
    for (vehicle, &car_length) in train.vehicle_lengths_m.iter().enumerate() {
        let offset = -front - car_length / 2.;
        front += car_length;
        let center = head
            + if state.path_reversed {
                -length - offset
            } else {
                offset
            };
        let canonical = if state.path_reversed {
            path_length - center
        } else {
            center
        };
        let Some(p) = train.rack.vehicles.iter().find(|p| p.vehicle == vehicle) else {
            continue;
        };
        let Some(s) = state.rack.cars.iter_mut().find(|s| s.vehicle == vehicle) else {
            continue;
        };
        let speed = state
            .vehicles
            .get(vehicle)
            .map_or(state.velocity_mps, |s| s.velocity_mps)
            .abs();
        let section = train
            .rack
            .sections
            .iter()
            .enumerate()
            .find(|(_, r)| canonical >= r.start_chainage_m && canonical < r.end_chainage_m);
        s.section = section.map(|(i, _)| i);
        s.incompatible = section.is_some_and(|(_, r)| r.system != p.system);
        s.overspeed =
            section.is_some_and(|(_, r)| speed > p.maximum_speed_mps.min(r.maximum_speed_mps));
        s.transmitted_force_n = 0.;
        s.brake_force_n = 0.;
        if !s.engage_requested || section.is_none() {
            s.phase = RackPhase::Disengaged;
            s.engagement_remaining_s = 0.;
            continue;
        }
        if s.incompatible || (s.phase != RackPhase::Engaged && speed > p.engagement_speed_mps) {
            s.phase = RackPhase::Fault;
        }
        if s.phase == RackPhase::Fault {
            continue;
        } // Release before a reset.
        if s.phase == RackPhase::Disengaged {
            s.phase = RackPhase::Engaging;
            s.engagement_remaining_s = p.engagement_time_s;
        }
        if s.phase == RackPhase::Engaging {
            s.engagement_remaining_s = (s.engagement_remaining_s - dt).max(0.);
            if s.engagement_remaining_s == 0. {
                s.phase = RackPhase::Engaged;
            }
        }
        if s.phase == RackPhase::Engaged {
            let available = state
                .motor_force_n
                .get(vehicle)
                .copied()
                .unwrap_or(0.)
                .max(0.);
            let force = if s.overspeed {
                0.
            } else {
                available
                    .min(p.maximum_force_n)
                    .min(p.maximum_power_w / speed.max(0.01))
            };
            let wheel = state.rail_force_n.get(vehicle).copied().unwrap_or(0.);
            s.transmitted_force_n = force;
            if let Some(slot) = state.rail_force_n.get_mut(vehicle) {
                *slot = force;
            }
            correction += force - wheel;
            // Positive tooth braking remains available on a steep/wet rail.
            s.brake_force_n = p.brake_force_n
                * if s.overspeed || state.brake_system.emergency {
                    1.
                } else {
                    state.brake.clamp(0., 1.)
                };
            s.brake_work_j += s.brake_force_n * speed * dt;
        }
    }
    correction
}
pub fn brake_forces(state: &crate::TrainSimState, count: usize) -> Vec<f64> {
    let mut forces = vec![0.; count];
    for s in &state.rack.cars {
        if let Some(f) = forces.get_mut(s.vehicle) {
            *f += s.brake_force_n;
        }
    }
    forces
}
