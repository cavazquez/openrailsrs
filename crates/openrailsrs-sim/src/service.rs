//! Station service state machine, shared by the Bevy player and headless checks.
//!
//! Distances are absolute path chainage, unlike the journey odometer which starts
//! at zero after a scenario spawn offset. No position or velocity is snapped.

use serde::{Deserialize, Serialize};

use crate::exterior::DoorState;

pub const STOP_POSITION_TOLERANCE_M: f64 = 10.0;
pub const STOP_SPEED_TOLERANCE_MPS: f64 = 0.1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveStopTarget {
    pub node_id: String,
    pub cum_dist_m: f64,
    pub arrive_s: f64,
    pub depart_s: f64,
    pub dwell_s: f64,
    pub name: String,
    pub is_terminal: bool,
    pub passengers_on: u32,
    pub passengers_off: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    #[default]
    Approaching,
    Boarding,
    ReadyToDepart,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStopResult {
    pub name: String,
    pub node: String,
    pub scheduled_arrive_s: f64,
    pub actual_arrive_s: f64,
    pub actual_depart_s: f64,
    pub position_error_m: f64,
    pub arrival_speed_mps: f64,
    pub dwell_s: f64,
    pub delay_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveGameplay {
    pub destination: String,
    pub destination_node: String,
    pub penalty_per_second_late: f64,
    pub stop_targets: Vec<LiveStopTarget>,
    pub next_stop_idx: usize,
    pub accrued_penalty: f64,
    /// Successfully served stops, retained for the HUD and markers.
    pub passed_stops: Vec<(String, f64)>,
    pub overspeed_active: bool,
    pub phase: ServicePhase,
    pub stop_results: Vec<ServiceStopResult>,
    pub failure: Option<String>,
    boarding_elapsed_s: f64,
    arrival: Option<(f64, f64, f64)>,
}

impl LiveGameplay {
    pub fn new(destination_node: String, penalty: f64, stops: Vec<LiveStopTarget>) -> Self {
        let destination = stops
            .iter()
            .find(|s| s.is_terminal)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| destination_node.clone());
        Self {
            destination,
            destination_node,
            penalty_per_second_late: penalty,
            stop_targets: stops,
            next_stop_idx: 0,
            accrued_penalty: 0.0,
            passed_stops: Vec::new(),
            overspeed_active: false,
            phase: ServicePhase::Approaching,
            stop_results: Vec::new(),
            failure: None,
            boarding_elapsed_s: 0.0,
            arrival: None,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.phase, ServicePhase::Completed | ServicePhase::Failed)
    }

    pub fn current_arrival_s(&self) -> Option<f64> {
        self.arrival.map(|a| a.0)
    }

    pub fn remaining_dwell_s(&self, time_s: f64) -> f64 {
        self.stop_targets
            .get(self.next_stop_idx)
            .map(|stop| {
                (stop.dwell_s - self.boarding_elapsed_s)
                    .max(stop.depart_s - time_s)
                    .max(0.0)
            })
            .unwrap_or(0.0)
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = ServicePhase::Failed;
        self.failure = Some(message.into());
    }

    /// Returns passenger changes once, after boarding and door closure.
    pub fn tick(
        &mut self,
        time_s: f64,
        chainage_m: f64,
        velocity_mps: f64,
        door: DoorState,
        dt: f64,
    ) -> Option<(u32, u32)> {
        if self.is_finished() {
            return None;
        }
        let stop = self.stop_targets.get(self.next_stop_idx)?;
        let error = chainage_m - stop.cum_dist_m;
        if error > STOP_POSITION_TOLERANCE_M {
            self.fail(format!(
                "Parada omitida: {} ({error:.1} m después del punto de parada)",
                stop.name
            ));
            return None;
        }
        if error.abs() > STOP_POSITION_TOLERANCE_M || velocity_mps.abs() > STOP_SPEED_TOLERANCE_MPS
        {
            self.phase = ServicePhase::Approaching;
            self.boarding_elapsed_s = 0.0;
            self.arrival = None;
            return None;
        }
        let arrival = *self
            .arrival
            .get_or_insert((time_s, error.abs(), velocity_mps.abs()));
        if self.phase != ServicePhase::ReadyToDepart {
            self.phase = ServicePhase::Boarding;
            if stop.dwell_s == 0.0 || door == DoorState::Open {
                self.boarding_elapsed_s += dt;
            }
            if self.boarding_elapsed_s + 1e-9 >= stop.dwell_s && time_s + 1e-9 >= stop.depart_s {
                self.phase = ServicePhase::ReadyToDepart;
            }
        }
        if self.phase != ServicePhase::ReadyToDepart || door != DoorState::Closed {
            return None;
        }
        let delay = (arrival.0 - stop.arrive_s).max(0.0);
        self.accrued_penalty += delay * self.penalty_per_second_late;
        self.stop_results.push(ServiceStopResult {
            name: stop.name.clone(),
            node: stop.node_id.clone(),
            scheduled_arrive_s: stop.arrive_s,
            actual_arrive_s: arrival.0,
            actual_depart_s: time_s,
            position_error_m: arrival.1,
            arrival_speed_mps: arrival.2,
            dwell_s: self.boarding_elapsed_s,
            delay_s: delay,
        });
        self.passed_stops.push((stop.name.clone(), delay));
        let passengers = (stop.passengers_off, stop.passengers_on);
        let terminal = stop.is_terminal;
        self.next_stop_idx += 1;
        self.boarding_elapsed_s = 0.0;
        self.arrival = None;
        self.phase = if terminal {
            ServicePhase::Completed
        } else {
            ServicePhase::Approaching
        };
        Some(passengers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(terminal: bool) -> LiveGameplay {
        LiveGameplay::new(
            "end".into(),
            8.0,
            vec![LiveStopTarget {
                node_id: "end".into(),
                cum_dist_m: 100.0,
                arrive_s: 10.0,
                depart_s: 15.0,
                dwell_s: 3.0,
                name: "Station".into(),
                is_terminal: terminal,
                passengers_on: 10,
                passengers_off: 0,
            }],
        )
    }

    #[test]
    fn passing_a_station_at_speed_never_counts_as_a_stop() {
        let mut s = service(true);
        s.tick(10.0, 100.0, 12.0, DoorState::Closed, 1.0);
        assert!(s.passed_stops.is_empty());
        s.tick(11.0, 111.0, 12.0, DoorState::Closed, 1.0);
        assert_eq!(s.phase, ServicePhase::Failed);
        assert!(s.passed_stops.is_empty());
    }

    #[test]
    fn station_requires_boarding_schedule_and_closed_doors() {
        let mut s = service(true);
        for t in 10..15 {
            s.tick(t as f64, 95.0, 0.05, DoorState::Closed, 1.0);
        }
        assert_eq!(s.phase, ServicePhase::Boarding);
        for t in 15..18 {
            s.tick(t as f64, 95.0, 0.05, DoorState::Open, 1.0);
        }
        assert_eq!(s.phase, ServicePhase::ReadyToDepart);
        assert!(s.passed_stops.is_empty());
        assert_eq!(
            s.tick(18.0, 95.0, 0.0, DoorState::Closed, 1.0),
            Some((0, 10))
        );
        assert_eq!(s.phase, ServicePhase::Completed);
        assert_eq!(s.stop_results[0].dwell_s, 3.0);
        assert_eq!(s.stop_results[0].position_error_m, 5.0);
        assert_eq!(s.tick(19.0, 95.0, 0.0, DoorState::Closed, 1.0), None);
    }

    #[test]
    fn moving_during_boarding_restarts_the_dwell() {
        let mut s = service(false);
        s.tick(15.0, 100.0, 0.0, DoorState::Open, 2.0);
        s.tick(17.0, 102.0, 0.2, DoorState::Open, 1.0);
        assert_eq!(s.phase, ServicePhase::Approaching);
        s.tick(18.0, 103.0, 0.0, DoorState::Open, 1.0);
        assert_eq!(s.remaining_dwell_s(18.0), 2.0);
    }
}
