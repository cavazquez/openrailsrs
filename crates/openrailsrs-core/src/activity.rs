//! Author-provided activity conditions. Physical actors are identified by ACT/CON UID.
use serde::{Deserialize, Serialize};

/// Alternative continuous freight animations of one wagon share this load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreightProfile {
    pub empty_mass_kg: f64,
    pub capacity_kg: f64,
    pub initial_kg: f64,
    pub pickup_types: Vec<u32>,
    pub unloading_delay_s: f64,
}
impl FreightProfile {
    pub fn valid(&self) -> bool {
        [
            self.empty_mass_kg,
            self.capacity_kg,
            self.initial_kg,
            self.unloading_delay_s,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && self.empty_mass_kg > 0.
            && self.capacity_kg > 0.
            && self.initial_kg <= self.capacity_kg
            && !self.pickup_types.is_empty()
            && self.pickup_types.iter().all(|p| (1..=14).contains(p))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActivityDefinition {
    pub vehicle_uids: Vec<u32>,
    pub events: Vec<ActivityEvent>,
    pub sidings: Vec<ActivitySiding>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivitySiding {
    pub id: u32,
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityEvent {
    pub id: i32,
    pub name: String,
    pub activation: i32,
    pub reversible: bool,
    pub condition: ActivityCondition,
    pub outcomes: ActivityOutcomes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivityCondition {
    Time {
        seconds: f64,
    },
    Location {
        tile_x: i32,
        tile_z: i32,
        x: f64,
        z: f64,
        radius_m: f64,
        on_stop: bool,
        #[serde(default)]
        edge: Option<String>,
        #[serde(default)]
        position_m: Option<f64>,
    },
    AllStops,
    ReachSpeed {
        speed_mps: f64,
    },
    PickUpWagons {
        uids: Vec<u32>,
    },
    AssembleTrain {
        uids: Vec<u32>,
        siding: Option<u32>,
    },
    DropOffWagons {
        uids: Vec<u32>,
        siding: Option<u32>,
    },
    Unsupported {
        token: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActivityOutcomes {
    pub message: Option<String>,
    pub success: bool,
    pub failure: Option<String>,
    pub activate: Vec<i32>,
    pub restore: Vec<i32>,
    pub increment: Vec<i32>,
    pub decrement: Vec<i32>,
    pub unsupported: Vec<String>,
}

impl ActivityDefinition {
    pub fn valid(&self) -> bool {
        self.vehicle_uids.len() <= 256
            && self
                .vehicle_uids
                .iter()
                .enumerate()
                .all(|(i, id)| !self.vehicle_uids[..i].contains(id))
            && self.events.len() <= 4096
            && self.events.iter().enumerate().all(|(i, e)| {
                e.name.len() <= 8192
                    && self.events[..i].iter().all(|p| p.id != e.id)
                    && match &e.condition {
                        ActivityCondition::Time { seconds } => {
                            seconds.is_finite() && *seconds >= 0.
                        }
                        ActivityCondition::Location { x, z, radius_m, .. } => {
                            [*x, *z, *radius_m].iter().all(|v| v.is_finite()) && *radius_m > 0.
                        }
                        ActivityCondition::ReachSpeed { speed_mps } => {
                            speed_mps.is_finite() && *speed_mps >= 0.
                        }
                        ActivityCondition::PickUpWagons { uids }
                        | ActivityCondition::AssembleTrain { uids, .. }
                        | ActivityCondition::DropOffWagons { uids, .. } => {
                            !uids.is_empty()
                                && uids.len() <= 256
                                && uids
                                    .iter()
                                    .enumerate()
                                    .all(|(i, id)| !uids[..i].contains(id))
                        }
                        _ => true,
                    }
            })
            && self.sidings.iter().all(|s| {
                !s.edge.is_empty()
                    && s.start_m.is_finite()
                    && s.end_m.is_finite()
                    && s.start_m >= 0.
                    && s.end_m >= s.start_m
            })
    }
}
