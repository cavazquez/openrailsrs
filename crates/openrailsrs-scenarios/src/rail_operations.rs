//! Authored service connections, temporary works and dispatch decisions.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RailOperationsDef {
    pub passing_plans: Vec<PassingPlanDef>,
    pub yard_deliveries: Vec<YardDeliveryDef>,
    pub depots: Vec<DepotDef>,
    pub connections: Vec<ConnectionDef>,
    pub works: Vec<TrackWorkDef>,
    pub incidents: Vec<IncidentDef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PassingPlanDef {
    pub id: String,
    pub waiting_service: String,
    pub passing_service: String,
    pub siding_edge: String,
    pub position_m: f64,
    pub clear_edge: String,
    /// Complete directed alternatives. The dispatcher preserves pending stops.
    pub alternatives: Vec<Vec<String>>,
}
impl PassingPlanDef {
    pub fn valid(&self) -> bool {
        [
            &self.id,
            &self.waiting_service,
            &self.passing_service,
            &self.siding_edge,
            &self.clear_edge,
        ]
        .into_iter()
        .all(|s| text(s))
            && self.waiting_service != self.passing_service
            && self.position_m.is_finite()
            && self.position_m > 0.
            && !self.alternatives.is_empty()
            && self.alternatives.len() <= 8
            && self
                .alternatives
                .iter()
                .all(|path| !path.is_empty() && path.len() <= 4096 && path.iter().all(|e| text(e)))
    }
}
/// Exact wagon identities and a physical siding interval, independent of camera
/// labels. Vehicle numbers refer to the scenario's consist order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct YardDeliveryDef {
    #[serde(default)]
    pub service: Option<String>,
    pub id: String,
    pub destination: String,
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    pub vehicles: Vec<usize>,
}
impl YardDeliveryDef {
    pub fn valid(&self) -> bool {
        self.service.as_ref().is_none_or(|s| text(s))
            && text(&self.id)
            && text(&self.destination)
            && text(&self.edge)
            && self.start_m.is_finite()
            && self.end_m.is_finite()
            && self.start_m >= 0.
            && self.end_m > self.start_m
            && !self.vehicles.is_empty()
            && self.vehicles.len() <= 256
            && self
                .vehicles
                .iter()
                .enumerate()
                .all(|(i, v)| *v < 256 && !self.vehicles[..i].contains(v))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DepotDef {
    pub id: String,
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    pub maintenance_s: f64,
    pub oil_l: f64,
    pub coolant_l: f64,
}
impl DepotDef {
    pub fn valid(&self) -> bool {
        text(&self.id)
            && text(&self.edge)
            && [
                self.start_m,
                self.end_m,
                self.maintenance_s,
                self.oil_l,
                self.coolant_l,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            && self.end_m > self.start_m
            && (30. ..=1800.).contains(&self.maintenance_s)
            && self.oil_l <= 10000.
            && self.coolant_l <= 10000.
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConnectionDef {
    pub stop: usize,
    pub service: String,
    pub service_stop: usize,
    pub max_wait_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrackWorkDef {
    pub id: String,
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    pub start_s: f64,
    pub end_s: f64,
    /// +1 forward, −1 reverse, 0 both directions in the resolved itinerary.
    pub direction: i8,
    pub speed_kmh: f64,
    /// A physical closure blocks both directions and lets the shared dispatcher
    /// seek an existing forward alternative. Speed-only works remain directional.
    #[serde(default)]
    pub closed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IncidentDef {
    pub id: String,
    pub service: String,
    pub stop: usize,
    pub wait_s: f64,
    pub penalty: f64,
}
impl RailOperationsDef {
    pub fn valid(&self) -> bool {
        self.passing_plans.len() <= 64
            && self
                .passing_plans
                .iter()
                .enumerate()
                .all(|(i, p)| p.valid() && self.passing_plans[..i].iter().all(|q| q.id != p.id))
            && self.yard_deliveries.len() <= 64
            && self.yard_deliveries.iter().enumerate().all(|(i, d)| {
                d.valid()
                    && self.yard_deliveries[..i].iter().all(|other| {
                        other.id != d.id
                            && (other.service != d.service
                                || !other.vehicles.iter().any(|v| d.vehicles.contains(v)))
                    })
            })
            && self.connections.len() <= 128
            && self.depots.len() <= 64
            && self
                .depots
                .iter()
                .enumerate()
                .all(|(i, d)| d.valid() && self.depots[..i].iter().all(|p| p.id != d.id))
            && self.works.len() <= 256
            && self.incidents.len() <= 128
            && self.connections.iter().all(|c| {
                c.stop < 4096
                    && c.service_stop < 4096
                    && text(&c.service)
                    && c.max_wait_s.is_finite()
                    && (1. ..=1800.).contains(&c.max_wait_s)
            })
            && self.works.iter().enumerate().all(|(i, w)| {
                text(&w.id)
                    && text(&w.edge)
                    && self.works[..i].iter().all(|p| p.id != w.id)
                    && [w.start_m, w.end_m, w.start_s, w.end_s, w.speed_kmh]
                        .iter()
                        .all(|v| v.is_finite())
                    && w.start_m >= 0.
                    && w.end_m > w.start_m
                    && w.start_s >= 0.
                    && w.end_s > w.start_s
                    && (-1..=1).contains(&w.direction)
                    && (!w.closed || w.direction == 0)
                    && (1. ..=300.).contains(&w.speed_kmh)
            })
            && self.incidents.iter().enumerate().all(|(i, c)| {
                text(&c.id)
                    && text(&c.service)
                    && self.incidents[..i].iter().all(|p| p.id != c.id)
                    && c.stop < 4096
                    && c.wait_s.is_finite()
                    && (1. ..=1800.).contains(&c.wait_s)
                    && c.penalty.is_finite()
                    && (0. ..=10000.).contains(&c.penalty)
            })
    }
}
fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
