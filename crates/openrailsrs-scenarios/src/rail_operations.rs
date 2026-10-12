//! Authored service connections, temporary works and dispatch decisions.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RailOperationsDef {
    pub connections: Vec<ConnectionDef>,
    pub works: Vec<TrackWorkDef>,
    pub incidents: Vec<IncidentDef>,
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
        self.connections.len() <= 128
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
