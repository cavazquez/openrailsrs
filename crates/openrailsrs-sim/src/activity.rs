//! Native ACT conditions/outcomes are independent of reaching the PAT's last point.
use crate::LiveDriveSession;
use openrailsrs_core::activity::{ActivityCondition, ActivityDefinition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct ActivityGroup {
    pub uids: Vec<u32>,
    pub front: Option<(String, f64)>,
    pub rear: Option<(String, f64)>,
    pub is_static: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventProgress {
    pub activation: i32,
    pub times_triggered: u32,
    pub disabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityMessage {
    pub event: i32,
    pub time_s: f64,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityRuntime {
    pub definition: ActivityDefinition,
    pub progress: BTreeMap<i32, EventProgress>,
    pub messages: Vec<ActivityMessage>,
    pub successful: bool,
    pub failure: Option<String>,
}
impl ActivityRuntime {
    pub fn new(definition: ActivityDefinition) -> Self {
        let progress = definition
            .events
            .iter()
            .map(|e| {
                (
                    e.id,
                    EventProgress {
                        activation: e.activation,
                        ..Default::default()
                    },
                )
            })
            .collect();
        Self {
            definition,
            progress,
            messages: vec![],
            successful: false,
            failure: None,
        }
    }
    pub(crate) fn valid(&self, expected: &ActivityDefinition, time: f64) -> bool {
        self.definition == *expected
            && self.progress.len() == expected.events.len()
            && expected.events.iter().all(|e| {
                self.progress.get(&e.id).is_some_and(|p| {
                    p.times_triggered < 1_000_000
                        && (-1_000_000..=1_000_000).contains(&p.activation)
                        && (!p.disabled || e.reversible && p.times_triggered > 0)
                        && (p.times_triggered == 0
                            || !matches!(e.condition, ActivityCondition::Unsupported { .. })
                                && e.outcomes.unsupported.is_empty())
                })
            })
            && (!self.successful
                || expected.events.iter().any(|e| {
                    e.outcomes.success
                        && self
                            .progress
                            .get(&e.id)
                            .is_some_and(|p| p.times_triggered > 0)
                }))
            && self.failure.as_ref().is_none_or(|failure| {
                expected.events.iter().any(|e| {
                    e.outcomes.failure.as_ref() == Some(failure)
                        && self
                            .progress
                            .get(&e.id)
                            .is_some_and(|p| p.times_triggered > 0)
                })
            })
            && self.messages.len() <= 64
            && self.messages.iter().all(|m| {
                m.time_s.is_finite()
                    && (0. ..=time).contains(&m.time_s)
                    && m.text.len() <= 65536
                    && self
                        .progress
                        .get(&m.event)
                        .is_some_and(|p| p.times_triggered > 0)
            })
    }
}
fn in_siding(player: &ActivityGroup, id: u32, definition: &ActivityDefinition) -> bool {
    let Some(s) = definition.sidings.iter().find(|s| s.id == id) else {
        return false;
    };
    // Pinned OR atSiding checks either travelling end, despite its old comment.
    [&player.front, &player.rear]
        .into_iter()
        .flatten()
        .any(|(edge, at)| edge == &s.edge && *at >= s.start_m && *at <= s.end_m)
}
// Keep the native event's inputs explicit; none may be inferred from PAT end.
#[allow(clippy::too_many_arguments)]
fn condition(
    condition: &ActivityCondition,
    event: i32,
    definition: &ActivityDefinition,
    player: &ActivityGroup,
    groups: &[ActivityGroup],
    time: f64,
    speed: f64,
    all_stops: bool,
    locations: &HashSet<i32>,
) -> bool {
    match condition {
        ActivityCondition::Time { seconds } => time.floor() >= *seconds,
        ActivityCondition::Location { on_stop, .. } => {
            locations.contains(&event) && (!on_stop || speed.abs() <= 0.032)
        }
        ActivityCondition::AllStops => all_stops,
        ActivityCondition::ReachSpeed { speed_mps } => speed.abs() >= *speed_mps,
        ActivityCondition::PickUpWagons { uids } => {
            speed.abs() <= 1. && uids.iter().all(|id| player.uids.contains(id))
        }
        ActivityCondition::AssembleTrain { uids, siding } => {
            siding.is_none_or(|id| in_siding(player, id, definition))
                && groups.iter().any(|g| {
                    g.uids.len() == uids.len()
                        && (g.uids == *uids || g.uids.iter().rev().eq(uids.iter()))
                })
        }
        // OR1.6.1 matchesConsistNoOrder accepts one listed UID in a non-static
        // train, and tests the player's siding. Preserve this native peculiarity.
        ActivityCondition::DropOffWagons { uids, siding } => {
            siding.is_none_or(|id| in_siding(player, id, definition))
                && groups
                    .iter()
                    .any(|g| !g.is_static && uids.iter().any(|id| g.uids.contains(id)))
        }
        ActivityCondition::Unsupported { .. } => false,
    }
}
impl LiveDriveSession {
    fn normalize_activity_position(&self, p: Option<(String, f64)>) -> Option<(String, f64)> {
        let (edge, at) = p?;
        let length = self.graph.edge(&edge)?.length_m;
        Some((
            edge.strip_suffix("_r").unwrap_or(&edge).into(),
            if edge.ends_with("_r") {
                length - at
            } else {
                at
            },
        ))
    }
    pub fn activity_group(&self) -> ActivityGroup {
        ActivityGroup {
            uids: self.formation.cars[..self.formation.coupled_count]
                .iter()
                .filter_map(|c| c.activity_uid)
                .collect(),
            front: self.normalize_activity_position(self.position_at_head_offset(0.)),
            rear: self.normalize_activity_position(
                self.position_at_head_offset(-self.formation.length_m()),
            ),
            is_static: false,
        }
    }
    pub fn activity_groups(&self) -> Vec<ActivityGroup> {
        let mut result = vec![self.activity_group()];
        for s in self.detached_sections() {
            result.push(ActivityGroup {
                uids: self.formation.cars[s.start..s.start + s.count]
                    .iter()
                    .filter_map(|c| c.activity_uid)
                    .collect(),
                front: self.normalize_activity_position(s.front),
                rear: self.normalize_activity_position(s.rear),
                is_static: true,
            });
        }
        result
    }
    pub fn set_activity_external_groups(&mut self, groups: Vec<ActivityGroup>) {
        self.activity_external_groups = groups;
    }
    pub fn native_activity_status(&self) -> Option<String> {
        let a = self.state.activity.as_ref()?;
        if a.definition.events.is_empty() {
            return None;
        }
        if let Some(f) = &a.failure {
            return Some(format!("ACT fallida: {f}"));
        }
        if a.successful {
            return Some("ACT completada por sus objetivos originales".into());
        }
        let pending = a
            .definition
            .events
            .iter()
            .filter(|e| {
                a.progress
                    .get(&e.id)
                    .is_some_and(|p| p.activation > 0 && p.times_triggered == 0)
            })
            .map(|e| format!("{} ({})", e.name, e.id))
            .take(4)
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("ACT pendiente: {pending}"))
    }
    pub(crate) fn has_native_activity_events(&self) -> bool {
        self.state
            .activity
            .as_ref()
            .is_some_and(|a| !a.definition.events.is_empty())
    }
    pub(crate) fn tick_activity(&mut self) {
        let Some(mut runtime) = self.state.activity.take() else {
            return;
        };
        if runtime.successful || runtime.failure.is_some() {
            self.state.activity = Some(runtime);
            return;
        }
        let player = self.activity_group();
        let mut groups = self.activity_groups();
        groups.extend(self.activity_external_groups.iter().cloned());
        let mut locations = HashSet::new();
        for e in &runtime.definition.events {
            if let ActivityCondition::Location {
                edge: Some(target),
                position_m: Some(position),
                radius_m,
                ..
            } = &e.condition
            {
                let mut before = 0.;
                for (id, data) in self.state.path_edges.iter().zip(&self.path_data.edges) {
                    if id.strip_suffix("_r").unwrap_or(id) == target {
                        let at = before
                            + if id.ends_with("_r") {
                                data.length_m - *position
                            } else {
                                *position
                            };
                        if (self.head_chainage_m() - at).abs() < *radius_m {
                            locations.insert(e.id);
                        }
                    }
                    before += data.length_m;
                }
            }
        }
        let all_stops = !self.gameplay.stop_targets.is_empty()
            && self.gameplay.next_stop_idx == self.gameplay.stop_targets.len();
        for event in &runtime.definition.events {
            let Some(state) = runtime.progress.get_mut(&event.id) else {
                continue;
            };
            if state.activation <= 0 {
                continue;
            }
            let triggered = condition(
                &event.condition,
                event.id,
                &runtime.definition,
                &player,
                &groups,
                self.time_s(),
                self.velocity_mps(),
                all_stops,
                &locations,
            );
            if !triggered {
                if event.reversible {
                    state.disabled = false;
                }
                continue;
            }
            if state.disabled || !event.outcomes.unsupported.is_empty() {
                continue;
            }
            state.times_triggered = state.times_triggered.saturating_add(1);
            if event.reversible {
                state.disabled = true;
            } else {
                state.activation = 0;
            }
            if state.times_triggered > 1 {
                continue;
            }
            if let Some(message) = &event.outcomes.message {
                runtime.messages.push(ActivityMessage {
                    event: event.id,
                    time_s: self.time_s(),
                    text: message.clone(),
                });
                if runtime.messages.len() > 64 {
                    runtime.messages.remove(0);
                }
            }
            for id in &event.outcomes.activate {
                if let Some(s) = runtime.progress.get_mut(id) {
                    s.activation = 1;
                }
            }
            for id in &event.outcomes.restore {
                if let Some(s) = runtime.progress.get_mut(id) {
                    s.activation = runtime
                        .definition
                        .events
                        .iter()
                        .find(|e| e.id == *id)
                        .map_or(0, |e| e.activation);
                }
            }
            for (ids, delta) in [
                (&event.outcomes.increment, 1),
                (&event.outcomes.decrement, -1),
            ] {
                for id in ids {
                    if let Some(s) = runtime.progress.get_mut(id) {
                        s.activation = s.activation.saturating_add(delta);
                    }
                }
            }
            if let Some(failure) = &event.outcomes.failure {
                runtime.failure = Some(failure.clone());
                self.gameplay.fail(failure.clone());
                break;
            }
            if event.outcomes.success {
                runtime.successful = true;
                self.gameplay.phase = crate::ServicePhase::Completed;
                break;
            }
        }
        self.state.activity = Some(runtime);
    }
}
impl crate::LiveTraffic {
    pub(crate) fn sync_activity_context(&self, player: &mut LiveDriveSession) {
        let groups = self
            .services
            .iter()
            .filter(|s| s.departed)
            .flat_map(|s| {
                let mut g = s.session.activity_groups();
                if s.parked {
                    for p in &mut g {
                        p.is_static = true;
                    }
                }
                g
            })
            .collect();
        player.set_activity_external_groups(groups);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_composition_conditions_keep_uids_order_siding_and_native_dropoff_semantics() {
        let player = ActivityGroup {
            uids: vec![0, 1, 2],
            front: Some(("e1".into(), 100.)),
            rear: Some(("e1".into(), 60.)),
            is_static: false,
        };
        let loose = ActivityGroup {
            uids: vec![8, 9],
            is_static: true,
            ..Default::default()
        };
        let definition = ActivityDefinition {
            sidings: vec![openrailsrs_core::activity::ActivitySiding {
                id: 7,
                edge: "e1".into(),
                start_m: 90.,
                end_m: 110.,
            }],
            ..Default::default()
        };
        let locations = HashSet::new();
        let groups = [player.clone(), loose.clone()];
        let test = |c, speed| {
            condition(
                &c,
                1,
                &definition,
                &player,
                &groups,
                1000.,
                speed,
                false,
                &locations,
            )
        };
        assert!(test(
            ActivityCondition::PickUpWagons { uids: vec![1, 2] },
            1.
        ));
        assert!(!test(
            ActivityCondition::PickUpWagons { uids: vec![1, 2] },
            1.01
        ));
        assert!(!test(ActivityCondition::PickUpWagons { uids: vec![9] }, 0.));
        assert!(test(
            ActivityCondition::AssembleTrain {
                uids: vec![2, 1, 0],
                siding: Some(7)
            },
            0.
        ));
        assert!(!test(
            ActivityCondition::AssembleTrain {
                uids: vec![0, 2, 1],
                siding: None
            },
            0.
        ));
        assert!(!test(
            ActivityCondition::AssembleTrain {
                uids: vec![0, 1],
                siding: None
            },
            0.
        ));
        assert!(!test(
            ActivityCondition::AssembleTrain {
                uids: vec![0, 1, 2],
                siding: Some(99)
            },
            0.
        ));
        assert!(test(
            ActivityCondition::AssembleTrain {
                uids: vec![8, 9],
                siding: None
            },
            0.
        ));
        assert!(test(
            ActivityCondition::DropOffWagons {
                uids: vec![1, 99],
                siding: Some(7)
            },
            0.
        ));
        assert!(!test(
            ActivityCondition::DropOffWagons {
                uids: vec![8, 9],
                siding: Some(7)
            },
            0.
        ));
        assert!(!test(
            ActivityCondition::Unsupported {
                token: "unknown".into()
            },
            0.
        ));
        assert!(!test(ActivityCondition::AllStops, 0.));
    }
}
