//! Authored operational yards. Distances are real track metres and poses are
//! world coordinates, independent of the viewer's floating origin.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct YardOperationsDef {
    pub moving_devices: Vec<MovingDeviceDef>,
    pub pools: Vec<MaterialPoolDef>,
    pub rendezvous: Vec<RendezvousDef>,
    pub run_arounds: Vec<RunAroundDef>,
    pub gravity_releases: Vec<GravityReleaseDef>,
    pub retarders: Vec<RetarderDef>,
    pub recovery_exercises: Vec<RecoveryExerciseDef>,
}
fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn bounded(v: f64, min: f64, max: f64) -> bool {
    v.is_finite() && (min..=max).contains(&v)
}
fn unique<T>(items: &[T], id: impl Fn(&T) -> &str) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(i, v)| text(id(v)) && items[..i].iter().all(|p| id(p) != id(v)))
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovingDeviceKind {
    Turntable,
    TransferTable,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovingPortDef {
    pub id: String,
    /// Radians for a turntable, metres laterally for a transfer table.
    pub position: f64,
    /// A real deck vector followed by the declared departure itinerary.
    pub path: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovingDeviceDef {
    pub id: String,
    pub kind: MovingDeviceKind,
    pub center_x_m: f64,
    pub center_z_m: f64,
    #[serde(default)]
    pub elevation_m: f64,
    /// Fixed deck heading of a transfer table; turntable ports supply headings.
    #[serde(default)]
    pub heading_rad: f64,
    pub deck_length_m: f64,
    pub deck_width_m: f64,
    pub max_mass_kg: f64,
    /// Radians/s for rotation, m/s for translation. No instantaneous operation.
    pub speed: f64,
    pub initial_port: usize,
    pub ports: Vec<MovingPortDef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialPoolDef {
    pub id: String,
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    /// Existing physical service IDs, never recipes for another copy of stock.
    pub members: Vec<String>,
    pub jobs: Vec<PoolJobDef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoolJobDef {
    pub id: String,
    pub not_before_s: f64,
    pub deadline_s: f64,
    pub min_vehicles: usize,
    pub max_vehicles: usize,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RendezvousDef {
    pub id: String,
    pub receiver: String,
    pub donor: String,
    pub edge: String,
    pub position_m: f64,
    pub not_before_s: f64,
    pub max_wait_s: f64,
    #[serde(default)]
    pub front: bool,
    #[serde(default)]
    pub receiver_leg: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct YardLegDef {
    pub edges: Vec<String>,
    pub start_offset_m: f64,
    pub end_chainage_m: f64,
    pub reversed: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunAroundDef {
    pub id: String,
    pub service: String,
    pub keep: usize,
    pub section: String,
    /// Outbound headshunt, opposite-side loop, pickup and departure legs.
    pub legs: Vec<YardLegDef>,
    pub detach_chainage_m: f64,
    pub pickup_chainage_m: f64,
    #[serde(default)]
    pub automatic: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GravityRouteDef {
    pub id: String,
    pub edges: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GravityReleaseDef {
    pub id: String,
    pub service: String,
    pub release_s: f64,
    /// Explicitly insufficient initial braking for the optional exercise.
    pub initial_brake_fraction: f64,
    pub initial_handbrakes: Vec<usize>,
    #[serde(default)]
    pub routes: Vec<GravityRouteDef>,
    #[serde(default)]
    pub automatic: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetarderDef {
    pub id: String,
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    /// Force opposing wheel movement, never a prescribed speed.
    pub max_force_n: f64,
    pub default_fraction: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecoveryExerciseDef {
    pub id: String,
    pub service: String,
    pub protection_edge: String,
    pub stop_edge: String,
    pub stop_start_m: f64,
    pub stop_end_m: f64,
    pub protect_before_s: f64,
    pub max_safe_contact_mps: f64,
}
impl YardOperationsDef {
    pub fn is_empty(&self) -> bool {
        self.moving_devices.is_empty()
            && self.pools.is_empty()
            && self.rendezvous.is_empty()
            && self.run_arounds.is_empty()
            && self.gravity_releases.is_empty()
            && self.retarders.is_empty()
            && self.recovery_exercises.is_empty()
    }
    pub fn valid(&self) -> bool {
        self.moving_devices.len() <= 32
            && unique(&self.moving_devices, |x| &x.id)
            && self.moving_devices.iter().all(|d| {
                [d.center_x_m, d.center_z_m, d.elevation_m, d.heading_rad]
                    .iter()
                    .all(|x| x.is_finite())
                    && bounded(d.deck_length_m, 5., 500.)
                    && bounded(d.deck_width_m, 1., 20.)
                    && bounded(d.max_mass_kg, 1000., 5e6)
                    && bounded(d.speed, 0.001, 10.)
                    && (2..=64).contains(&d.ports.len())
                    && d.initial_port < d.ports.len()
                    && unique(&d.ports, |p| &p.id)
                    && d.ports.iter().all(|p| {
                        bounded(p.position, -10000., 10000.)
                            && !p.path.is_empty()
                            && p.path.len() <= 4096
                            && p.path.iter().all(|s| text(s))
                    })
            })
            && self.pools.len() <= 64
            && unique(&self.pools, |x| &x.id)
            && self
                .pools
                .iter()
                .flat_map(|p| &p.members)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.pools.iter().map(|p| p.members.len()).sum::<usize>()
            && self.pools.iter().all(|p| {
                text(&p.edge)
                    && bounded(p.start_m, 0., 1e9)
                    && bounded(p.end_m, p.start_m + 0.01, 1e9)
                    && (1..=256).contains(&p.members.len())
                    && p.members
                        .iter()
                        .enumerate()
                        .all(|(i, s)| text(s) && !p.members[..i].contains(s))
                    && p.jobs.len() <= 256
                    && unique(&p.jobs, |j| &j.id)
                    && p.jobs.iter().all(|j| {
                        bounded(j.not_before_s, 0., 604800.)
                            && bounded(j.deadline_s, j.not_before_s, 604800.)
                            && j.min_vehicles > 0
                            && j.max_vehicles >= j.min_vehicles
                            && j.max_vehicles <= 256
                    })
            })
            && self.rendezvous.len() <= 128
            && unique(&self.rendezvous, |x| &x.id)
            && self.rendezvous.iter().all(|r| {
                text(&r.receiver)
                    && text(&r.donor)
                    && r.receiver != r.donor
                    && text(&r.edge)
                    && bounded(r.position_m, 0., 1e9)
                    && bounded(r.not_before_s, 0., 604800.)
                    && bounded(r.max_wait_s, 1., 21600.)
                    && r.receiver_leg.is_none_or(|i| i < 256)
            })
            && self.run_arounds.len() <= 64
            && unique(&self.run_arounds, |x| &x.id)
            && self.run_arounds.iter().all(|r| {
                text(&r.service)
                    && text(&r.section)
                    && r.keep > 0
                    && r.keep <= 256
                    && (3..=16).contains(&r.legs.len())
                    && bounded(r.detach_chainage_m, 0., 1e9)
                    && bounded(r.pickup_chainage_m, 0., 1e9)
                    && r.legs.iter().all(|l| {
                        !l.edges.is_empty()
                            && l.edges.len() <= 4096
                            && l.edges.iter().all(|s| text(s))
                            && bounded(l.start_offset_m, 0., 1e9)
                            && bounded(l.end_chainage_m, l.start_offset_m, 1e9)
                    })
            })
            && self.gravity_releases.len() <= 128
            && unique(&self.gravity_releases, |x| &x.id)
            && self.gravity_releases.iter().all(|r| {
                text(&r.service)
                    && bounded(r.release_s, 0., 604800.)
                    && bounded(r.initial_brake_fraction, 0., 1.)
                    && r.initial_handbrakes.len() <= 256
                    && r.initial_handbrakes.iter().all(|i| *i < 256)
                    && r.routes.len() <= 16
                    && unique(&r.routes, |p| &p.id)
                    && r.routes.iter().all(|p| {
                        !p.edges.is_empty()
                            && p.edges.len() <= 4096
                            && p.edges.iter().all(|id| text(id))
                    })
            })
            && self.retarders.len() <= 128
            && unique(&self.retarders, |x| &x.id)
            && self.retarders.iter().all(|r| {
                text(&r.edge)
                    && bounded(r.start_m, 0., 1e9)
                    && bounded(r.end_m, r.start_m + 0.01, 1e9)
                    && bounded(r.max_force_n, 0., 2e6)
                    && bounded(r.default_fraction, 0., 1.)
            })
            && self.recovery_exercises.len() <= 128
            && unique(&self.recovery_exercises, |x| &x.id)
            && self.recovery_exercises.iter().all(|r| {
                text(&r.service)
                    && text(&r.protection_edge)
                    && text(&r.stop_edge)
                    && bounded(r.stop_start_m, 0., 1e9)
                    && bounded(r.stop_end_m, r.stop_start_m + 0.01, 1e9)
                    && bounded(r.protect_before_s, 0., 604800.)
                    && bounded(r.max_safe_contact_mps, 0.05, 5.)
            })
    }
}
