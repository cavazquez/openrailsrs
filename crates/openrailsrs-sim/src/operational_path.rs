//! Ordered native subpaths. A reversal exchanges the travelling front and
//! rear reference; the physical vehicles and their mutable systems stay put.
use crate::{LiveDriveSession, path_data::PathData};
use openrailsrs_track::SwitchPosition;

/// OR Train.TestAbsDelay / CompareTimes.LatestTime, including midnight.
fn native_wait_delay(encoded: u32, clock: u32) -> f64 {
    if !(30000..40000).contains(&encoded) {
        return f64::from(encoded);
    }
    let until = ((encoded / 100) % 100) * 3600 + (encoded % 100) * 60;
    let latest = if until > 16 * 3600 && clock < 8 * 3600 {
        clock
    } else if until < 8 * 3600 && clock > 16 * 3600 {
        until
    } else {
        until.max(clock)
    };
    if latest == until && until >= clock {
        f64::from(until - clock)
    } else if latest == clock {
        1.0
    } else {
        f64::from(until + 86400 - clock)
    }
}

impl LiveDriveSession {
    pub fn oriented_car_offset(&self, offset: f64) -> f64 {
        if self.state.path_reversed {
            -self.formation.length_m() - offset
        } else {
            offset
        }
    }

    pub(crate) fn parked_car_offset(&self, offset: f64) -> f64 {
        if self
            .formation
            .parked_path_reversed
            .unwrap_or(self.state.path_reversed)
        {
            -self
                .formation
                .cars
                .iter()
                .map(|car| car.length_m)
                .sum::<f64>()
                - offset
        } else {
            offset
        }
    }

    pub fn gameplay_chainage_m(&self) -> f64 {
        if self.operational_legs.is_empty() {
            self.head_chainage_m()
        } else {
            self.operational_prefix_m + self.head_chainage_m() - self.operational_start_m
        }
    }

    pub fn operational_distance_m(&self) -> Option<f64> {
        self.operational_legs
            .get(self.operational_leg)
            .map(|leg| (leg.end_chainage_m - self.head_chainage_m()).max(0.0))
    }

    pub fn operational_status(&self) -> Option<String> {
        let leg = self.operational_legs.get(self.operational_leg)?;
        if let Some(operation) = leg.operations.get(self.operational_operation) {
            let distance = operation.chainage_m - self.head_chainage_m();
            if distance.abs() <= 50.0 {
                return Some(match operation.action {
                    openrailsrs_scenarios::model::TimetableAction::Detach { keep } => format!(
                        "F9 · asegurar y separar después del coche {keep} · {}",
                        operation.section
                    ),
                    openrailsrs_scenarios::model::TimetableAction::PickUp { .. } => format!(
                        "F9 · acoplar {}, conectar frenos y soltar frenos de mano",
                        operation.section
                    ),
                });
            }
        }
        if self.operational_distance_m()? > 10.0 {
            return None;
        }
        if self.operational_leg + 1 == self.operational_legs.len() {
            return Some("Fin del itinerario: detener el tren".into());
        }
        if let Some(start) = self.operational_wait_started {
            let delay = native_wait_delay(
                leg.wait_s as u32,
                ((self.service_clock_start_s + start) % 86400.0) as u32,
            );
            let remaining = (delay - (self.time_s() - start)).max(0.0);
            if remaining > 0.0 {
                return Some(format!("Espera del itinerario: {remaining:.0} s"));
            }
        }
        if let Some(time) = self.operational_legs[self.operational_leg + 1].not_before_s
            && time > self.time_s()
        {
            return Some(format!("Próximo servicio en {:.0} s", time - self.time_s()));
        }
        Some("Detener el tren y seleccionar el inversor del próximo tramo".into())
    }

    pub(crate) fn tick_operational_path(&mut self, automatic: bool) -> bool {
        self.tick_timetable_operation(automatic);
        let Some(leg) = self.operational_legs.get(self.operational_leg) else {
            return false;
        };
        if self.operational_operation < leg.operations.len() {
            return false;
        }
        let delta = self.head_chainage_m() - leg.end_chainage_m;
        if delta.abs() > 10.0 || self.velocity_mps() > 0.1 {
            self.operational_wait_started = None;
            return false;
        }
        if matches!(
            self.gameplay.phase,
            crate::service::ServicePhase::Boarding | crate::service::ServicePhase::ReadyToDepart
        ) {
            return false;
        }
        let started = *self
            .operational_wait_started
            .get_or_insert(self.state.time.0);
        let delay = native_wait_delay(
            leg.wait_s as u32,
            ((self.service_clock_start_s + started) % 86400.0) as u32,
        );
        if self.time_s() - started < delay {
            return false;
        }
        let Some(next) = self.operational_legs.get(self.operational_leg + 1).cloned() else {
            return true;
        };
        if next.not_before_s.is_some_and(|time| self.time_s() < time) {
            return false;
        }
        let direction = if next.reversed { 0.0 } else { 1.0 };
        if !automatic && (self.driver_direction - direction).abs() > 0.1 {
            return false;
        }
        // Do not change points granted to another train.
        for switch in &next.switches {
            let position = match switch.position {
                openrailsrs_scenarios::model::SwitchPositionDef::Straight => {
                    SwitchPosition::Straight
                }
                openrailsrs_scenarios::model::SwitchPositionDef::Diverging => {
                    SwitchPosition::Diverging
                }
            };
            if self
                .dispatcher
                .other_locks
                .iter()
                .any(|lock| lock.node == switch.node && lock.position != position)
            {
                return false;
            }
        }
        let reverse = next.reversed != self.state.path_reversed;
        let expected_anchor = next.start_offset_m + if reverse { -delta } else { delta };
        let pd = PathData::from_path(&next.edges, &self.graph);
        let front_offset = if next.reversed {
            -self.formation.length_m()
        } else {
            0.0
        };
        let Some((edge, position)) = self.position_at_head_offset(front_offset) else {
            return false;
        };
        let base = edge.strip_suffix("_r").unwrap_or(&edge);
        let native_position = if edge.ends_with("_r") {
            self.graph.edge(&edge).unwrap().length_m - position
        } else {
            position
        };
        let mut before = 0.0;
        let mut anchors = vec![];
        for (id, data) in next.edges.iter().zip(&pd.edges) {
            if id.strip_suffix("_r").unwrap_or(id) == base {
                anchors.push(
                    before
                        + if id.ends_with("_r") {
                            data.length_m - native_position
                        } else {
                            native_position
                        },
                );
            }
            before += data.length_m;
        }
        let Some(anchor) = anchors.into_iter().min_by(|a, b| {
            (a - expected_anchor)
                .abs()
                .total_cmp(&(b - expected_anchor).abs())
        }) else {
            self.gameplay
                .fail("El tramo siguiente no contiene el extremo de la formación");
            return true;
        };
        if !(0.0..pd.total_length_m()).contains(&anchor) {
            return false;
        }
        for i in 0..self.formation.coupled_count {
            let offset = self.formation.center_offset_m(i);
            let after_offset = if next.reversed {
                -self.formation.length_m() - offset
            } else {
                offset
            };
            let after =
                PathData::position_at_odometer(&next.edges, &pd.edges, anchor + after_offset);
            let before = self.position_at_head_offset(offset);
            let normalized = |(edge, position): (String, f64)| {
                let length = self.graph.edge(&edge).map_or(0.0, |e| e.length_m);
                if let Some(base) = edge.strip_suffix("_r") {
                    (base.to_string(), length - position)
                } else {
                    (edge, position)
                }
            };
            if !before.zip(after).is_some_and(|(a, b)| {
                let a = normalized(a);
                let b = normalized(b);
                a.0 == b.0 && (a.1 - b.1).abs() <= 1.0
            }) {
                self.gameplay.fail(format!(
                    "El tramo siguiente no conserva la posición del vehículo {}",
                    i + 1
                ));
                return true;
            }
        }
        let prefix = self.gameplay_chainage_m();
        let correction = delta + next.start_offset_m - anchor;
        for target in self
            .gameplay
            .stop_targets
            .iter_mut()
            .skip(self.gameplay.next_stop_idx)
        {
            target.cum_dist_m += correction;
        }
        for switch in &next.switches {
            let position = match switch.position {
                openrailsrs_scenarios::model::SwitchPositionDef::Straight => {
                    SwitchPosition::Straight
                }
                openrailsrs_scenarios::model::SwitchPositionDef::Diverging => {
                    SwitchPosition::Diverging
                }
            };
            let _ = self.graph.set_switch(&switch.node, position);
        }
        self.state.path_edges = next.edges;
        self.path_data = pd;
        let odometer = self.state.odometer_m;
        crate::live_drive::apply_start_offset(&mut self.state, &self.path_data, anchor);
        self.state.odometer_m = odometer;
        self.state.path_reversed = next.reversed;
        if let Some(id) = &next.service_id {
            self.scenario_name.clone_from(id);
        }
        self.operational_leg += 1;
        self.operational_operation = 0;
        self.operational_prefix_m = prefix;
        self.operational_start_m = anchor;
        self.operational_wait_started = None;
        self.previous_render_chainage_m = anchor;
        self.driver_direction = direction;
        self.driver_throttle = 0.0;
        self.driver_brake = 1.0;
        self.own_track_reservations.clear();
        self.dispatcher.own_locks.clear();
        self.native_signals.needs_refresh = true;
        self.evaluate_native_signals();
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_waits_match_original_openrails_including_midnight_and_late_arrival() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-parser.json")).unwrap();
        for case in reference["absolute_waits"].as_array().unwrap() {
            assert_eq!(
                native_wait_delay(
                    case["encoded"].as_u64().unwrap() as u32,
                    case["clock_s"].as_u64().unwrap() as u32
                ),
                case["delay_s"].as_f64().unwrap()
            );
        }
    }
}
