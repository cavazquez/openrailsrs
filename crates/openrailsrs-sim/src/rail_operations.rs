//! Railway operations evaluated on physical time and shared traffic, never FPS.
use crate::{LiveDriveSession, LiveTraffic};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RailOperationsState {
    pub definition: openrailsrs_scenarios::rail_operations::RailOperationsDef,
    pub connection_started: BTreeMap<usize, f64>,
    pub connections: BTreeMap<usize, String>,
    pub active_works: BTreeSet<String>,
    pub pending_incident: Option<String>,
    pub decisions: BTreeMap<String, IncidentChoice>,
    pub decision_started: BTreeMap<String, f64>,
    pub notice: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentChoice {
    Wait,
    Continue,
}
impl RailOperationsState {
    pub fn valid(&self) -> bool {
        self.definition.valid()
            && self.connection_started.len() <= 128
            && self.connections.len() <= 128
            && self.decisions.len() <= 128
            && self.decision_started.len() <= 128
            && self.notice.len() <= 1024
            && self
                .connection_started
                .values()
                .chain(self.decision_started.values())
                .all(|v| v.is_finite() && *v >= 0.)
            && self.connections.values().all(|v| v.len() <= 256)
            && self
                .active_works
                .iter()
                .all(|id| self.definition.works.iter().any(|w| &w.id == id))
            && self
                .pending_incident
                .as_ref()
                .is_none_or(|id| self.definition.incidents.iter().any(|i| &i.id == id))
            && self
                .decisions
                .keys()
                .all(|id| self.definition.incidents.iter().any(|i| &i.id == id))
    }
}
impl LiveDriveSession {
    pub(crate) fn closed_work_occupancy(
        &self,
        traffic_footprints: &[crate::native_signals::TrackOccupancy],
    ) -> Vec<crate::native_signals::TrackOccupancy> {
        self.state
            .rail_operations
            .definition
            .works
            .iter()
            .filter_map(|work| {
                if !work.closed || self.time_s() < work.start_s {
                    return None;
                }
                let edge = self.graph.edge(&work.edge)?;
                let (start_m, end_m) = if work.edge.ends_with("_r") {
                    (edge.length_m - work.end_m, edge.length_m - work.start_m)
                } else {
                    (work.start_m, work.end_m)
                };
                let interval = crate::native_signals::TrackOccupancy {
                    edge: work.edge.strip_suffix("_r").unwrap_or(&work.edge).into(),
                    start_m,
                    end_m,
                    owner: format!("Obra: {}", work.id),
                };
                // Ending the timetable window cannot remove protection underneath
                // any vehicle, including a parked section or the other service's tail.
                (self.time_s() < work.end_s
                    || traffic_footprints
                        .iter()
                        .any(|p| crate::track_reservations::overlaps(&interval, p)))
                .then_some(interval)
            })
            .collect()
    }
    pub fn decide_incident(&mut self, choice: IncidentChoice) -> Result<(), String> {
        let id = self
            .state
            .rail_operations
            .pending_incident
            .take()
            .ok_or("No hay una decisión pendiente")?;
        let def = self
            .state
            .rail_operations
            .definition
            .incidents
            .iter()
            .find(|i| i.id == id)
            .ok_or("Incidente desconocido")?;
        if choice == IncidentChoice::Continue {
            self.gameplay.accrued_penalty += def.penalty;
        }
        let now = self.time_s();
        let ops = &mut self.state.rail_operations;
        ops.decisions.insert(id.clone(), choice);
        ops.decision_started.insert(id, now);
        Ok(())
    }
    pub fn temporary_speed_limit(&self, base: f64) -> f64 {
        if self.state.rail_operations.definition.works.is_empty() {
            return base;
        }
        let head = self.head_chainage_m();
        let tail = head - self.formation.length_m();
        let now = self.time_s();
        let mut cap = base;
        let mut before = 0.;
        let direction = if self.driver_direction <= 0.25 { -1 } else { 1 };
        for (id, edge) in self.state.path_edges.iter().zip(&self.path_data.edges) {
            for work in &self.state.rail_operations.definition.works {
                if &work.edge != id || work.direction != 0 && work.direction != direction {
                    continue;
                }
                let occupied = head >= before + work.start_m && tail <= before + work.end_m;
                let retained = self.state.rail_operations.active_works.contains(&work.id);
                if occupied && (retained || now >= work.start_s && now < work.end_s) {
                    cap = cap.min(work.speed_kmh / 3.6);
                }
            }
            before += edge.length_m;
        }
        cap
    }
    pub fn update_work_occupancy(&mut self) {
        if self.state.rail_operations.definition.works.is_empty() {
            return;
        }
        let head = self.head_chainage_m();
        let tail = head - self.formation.length_m();
        let now = self.time_s();
        let mut before = 0.;
        let ops = &mut self.state.rail_operations;
        for (id, edge) in self.state.path_edges.iter().zip(&self.path_data.edges) {
            for work in ops.definition.works.iter().filter(|w| &w.edge == id) {
                let overlap = head >= before + work.start_m && tail <= before + work.end_m;
                if overlap && now >= work.start_s && now < work.end_s {
                    ops.active_works.insert(work.id.clone());
                }
                if !overlap {
                    ops.active_works.remove(&work.id);
                }
            }
            before += edge.length_m;
        }
    }
}
/// Holds are finite and release independently of rendering/menus. A connection
/// is made only after actual arrival of the declared service at its stop.
pub fn coordinate(player: &mut LiveDriveSession, traffic: &LiveTraffic) {
    let now = player.time_s();
    let stop = player.gameplay.next_stop_idx;
    let at_stop = player.gameplay.stop_targets.get(stop).is_some_and(|s| {
        (s.cum_dist_m - player.gameplay_chainage_m()).abs()
            <= crate::service::STOP_POSITION_TOLERANCE_M
            && player.velocity_mps() < crate::service::STOP_SPEED_TOLERANCE_MPS
    }) && !player.gameplay.is_finished();
    let ops = &mut player.state.rail_operations;
    let mut hold = false;
    let mut notice = String::new();
    for (index, c) in ops
        .definition
        .connections
        .iter()
        .enumerate()
        .filter(|(_, c)| c.stop == stop)
    {
        if !at_stop || ops.connections.contains_key(&index) {
            continue;
        }
        let start = *ops.connection_started.entry(index).or_insert(now);
        let service = traffic.services.iter().find(|s| s.id == c.service);
        let arrived = service.is_some_and(|s| {
            s.session.gameplay.stop_results.len() > c.service_stop
                || s.session.gameplay.next_stop_idx == c.service_stop
                    && s.session.gameplay.current_arrival_s().is_some()
        });
        if arrived {
            ops.connections.insert(index, "Realizada".into());
        } else if now - start >= c.max_wait_s || service.is_none() {
            ops.connections.insert(index, "Perdida".into());
        } else {
            hold = true;
            notice = format!(
                "Conexión con {}: espera máxima restante {:.0} s",
                c.service,
                c.max_wait_s - (now - start)
            );
        }
    }
    for c in ops
        .definition
        .incidents
        .iter()
        .filter(|c| c.stop == stop && at_stop)
    {
        let blocked = traffic
            .services
            .iter()
            .find(|s| s.id == c.service)
            .is_some_and(|s| {
                !s.departed
                    || s.session.distance_to_occupied_block_m().is_some()
                    || s.session.velocity_mps() < 0.1 && !s.session.gameplay.is_finished()
            });
        if !blocked {
            continue;
        }
        match ops.decisions.get(&c.id) {
            Some(IncidentChoice::Continue) => {}
            Some(IncidentChoice::Wait) => {
                if now - ops.decision_started.get(&c.id).copied().unwrap_or(now) < c.wait_s {
                    hold = true;
                    notice = format!("Esperando {} por decisión del conductor", c.service);
                }
            }
            None => {
                hold = true;
                ops.pending_incident = Some(c.id.clone());
                notice = format!(
                    "{} demorado: esperar hasta {:.0} s o continuar con penalización {:.0}",
                    c.service, c.wait_s, c.penalty
                );
            }
        }
    }
    ops.notice = notice;
    player.gameplay.departure_hold = hold;
}
