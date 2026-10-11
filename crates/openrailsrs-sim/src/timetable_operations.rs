//! Native detach/static and pickup operations reuse the physical F9 machinery.
use crate::LiveDriveSession;
use openrailsrs_scenarios::model::TimetableAction;

impl LiveDriveSession {
    pub(crate) fn tick_timetable_operation(&mut self, automatic: bool) {
        let Some(operation) = self
            .operational_legs
            .get(self.operational_leg)
            .and_then(|leg| leg.operations.get(self.operational_operation))
            .cloned()
        else {
            return;
        };
        let operation_head = if matches!(operation.action, TimetableAction::Detach { .. })
            && self.formation.parked_path_edges == self.state.path_edges
        {
            self.formation
                .parked_head_chainage_m
                .unwrap_or(self.head_chainage_m())
        } else {
            self.head_chainage_m()
        };
        if self.velocity_mps() > 0.1 || (operation_head - operation.chainage_m).abs() > 20.0 {
            return;
        }
        let automatic = automatic || !operation.manual;
        let finished = match operation.action {
            TimetableAction::Detach { keep } => {
                if automatic
                    && self.formation.parked_head_chainage_m.is_none()
                    && keep > 0
                    && keep < self.formation.coupled_count
                {
                    for car in &mut self.formation.cars[keep..] {
                        car.handbrake = true;
                    }
                    if self.uncouple_after(keep - 1).is_err() {
                        return;
                    }
                }
                let detached_length: f64 =
                    self.formation.cars[keep..].iter().map(|c| c.length_m).sum();
                let head_after_cut = operation_head
                    - if self.state.path_reversed {
                        detached_length
                    } else {
                        0.0
                    };
                self.formation.parked_head_chainage_m.is_some()
                    && self.formation.parked_path_edges == self.state.path_edges
                    && self.formation.coupled_count == keep
                    && (self.head_chainage_m() - head_after_cut).abs() <= 1.0
            }
            TimetableAction::PickUp { count } => {
                if automatic && self.formation.parked_head_chainage_m.is_some() {
                    if self.recouple().is_err() {
                        return;
                    }
                    for car in &mut self.formation.cars[..self.formation.coupled_count] {
                        car.handbrake = false;
                        car.hose_connected = true;
                        car.front_cock_open = true;
                        car.rear_cock_open = true;
                    }
                    self.sync_operating_brakes();
                }
                self.formation.parked_head_chainage_m.is_none()
                    && self.formation.coupled_count == count
                    && self.formation.cars[..count].iter().all(|c| {
                        !c.handbrake && c.hose_connected && c.front_cock_open && c.rear_cock_open
                    })
            }
        };
        if finished {
            self.operational_operation += 1;
        }
    }
}
