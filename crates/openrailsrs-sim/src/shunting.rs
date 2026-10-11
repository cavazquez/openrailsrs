//! Ordered yard objectives use actual formation/points state and positions.
use crate::LiveDriveSession;
use openrailsrs_scenarios::model::{ShuntingKind, SwitchPositionDef};
use openrailsrs_track::SwitchPosition;

impl LiveDriveSession {
    pub fn shunting_instruction(&self) -> Option<String> {
        let done = self.gameplay.shunting_completed_s.len();
        self.gameplay.shunting_steps.get(done).map(|s| {
            let position = match s.kind {
                ShuntingKind::MoveClear => Some(s.position_m),
                ShuntingKind::PickUp => self.formation.parked_head_chainage_m,
                _ => None,
            }
            .map_or(String::new(), |target| {
                format!(
                    " · posición {:.1} m / objetivo {:.1} m",
                    self.head_chainage_m(),
                    target
                )
            });
            format!(
                "Maniobra {}/{}: {}{}",
                done + 1,
                self.gameplay.shunting_steps.len(),
                s.instruction,
                position
            )
        })
    }
    pub(crate) fn update_shunting(&mut self) {
        let Some(goal) = self
            .gameplay
            .shunting_steps
            .get(self.gameplay.shunting_completed_s.len())
        else {
            return;
        };
        let stopped = self.velocity_mps() <= 0.1;
        let parked = self.formation.parked_head_chainage_m.is_some();
        let ready = self
            .formation
            .cars
            .iter()
            .take(self.formation.coupled_count)
            .enumerate()
            .all(|(i, c)| {
                !c.handbrake
                    && (i == 0 || c.hose_connected && c.front_cock_open)
                    && (i + 1 == self.formation.coupled_count || c.rear_cock_open)
            });
        let passed = match goal.kind {
            ShuntingKind::Cut => stopped && parked && self.formation.coupled_count == goal.count,
            ShuntingKind::MoveClear => {
                stopped && parked && (self.head_chainage_m() - goal.position_m).abs() <= 5.0
            }
            ShuntingKind::Switch => {
                self.graph.switch_position(&goal.node)
                    == Some(match goal.switch_position {
                        SwitchPositionDef::Straight => SwitchPosition::Straight,
                        SwitchPositionDef::Diverging => SwitchPosition::Diverging,
                    })
            }
            ShuntingKind::PickUp => {
                stopped && !parked && ready && self.formation.coupled_count == goal.count
            }
            ShuntingKind::Depart => {
                !parked
                    && ready
                    && self.head_chainage_m() >= goal.position_m
                    && self.velocity_mps() > 0.1
            }
        };
        if passed {
            self.gameplay.shunting_completed_s.push(self.time_s());
        }
    }
}
