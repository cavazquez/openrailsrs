//! Explicit control protocols. Delays apply on railway time, not UI frames.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultipleUnitState {
    #[serde(default)]
    pub queue: std::collections::VecDeque<[f64; 4]>,
    pub reverser: f64,
    pub throttle: f64,
    pub dynamic_brake: f64,
    pub pending: Option<[f64; 4]>,
    pub compatible: bool,
}
impl Default for MultipleUnitState {
    fn default() -> Self {
        Self {
            queue: Default::default(),
            reverser: 0.5,
            throttle: 0.,
            dynamic_brake: 0.,
            pending: None,
            compatible: false,
        }
    }
}
impl MultipleUnitState {
    pub fn valid(&self) -> bool {
        [self.throttle, self.dynamic_brake, self.reverser]
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && self.queue.len() <= 8192
            && self.queue.iter().all(|p| {
                p.iter().all(|v| v.is_finite() && *v >= 0.)
                    && p[..3].iter().all(|v| *v <= 1.)
                    && p[3] <= 1e10
            })
            && self
                .queue
                .iter()
                .zip(self.queue.iter().skip(1))
                .all(|(a, b)| a[3] <= b[3])
            && self.pending.is_none_or(|p| {
                p.iter().all(|v| v.is_finite() && *v >= 0.)
                    && p[0] <= 1.
                    && p[1] <= 1.
                    && p[2] <= 1.
            })
    }
}
pub fn advance(state: &mut crate::TrainSimState, train: &crate::physics::TrainPhysics, dt: f64) {
    let master = state.active_cab_vehicle.unwrap_or(0);
    let protocol = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == master)
        .and_then(|c| c.definition.multiple_unit.clone());
    let now = state.time_s();
    for c in &mut state.vehicle_systems {
        let Some(p) = &c.definition.multiple_unit else {
            continue;
        };
        let index = c.definition.vehicle;
        let connected = train
            .power_supply
            .cars
            .iter()
            .any(|c| c.vehicle == index && c.enabled);
        c.mu.compatible = index == master
            || protocol.as_ref().is_some_and(|m| {
                m.protocol == p.protocol
                    || m.compatible_protocols.contains(&p.protocol)
                        && p.compatible_protocols.contains(&m.protocol)
            });
        let enabled = connected && c.mu.compatible;
        let target = [
            if enabled && (index == master || p.throttle) {
                state.throttle
            } else {
                0.
            },
            if enabled && (index == master || p.dynamic_brake) {
                state.electric_brake_command
            } else {
                0.
            },
            if enabled && (index == master || p.reverser) {
                state.driver_reverser
            } else {
                c.mu.reverser
            },
        ];
        if !enabled {
            c.mu.throttle = 0.;
            c.mu.dynamic_brake = 0.;
            c.mu.reverser = 0.5;
            c.mu.pending = None;
            c.mu.queue.clear();
            continue;
        }
        if dt <= 0. {
            continue;
        }
        if c.mu.queue.is_empty()
            && let Some(pending) = c.mu.pending.take()
        {
            c.mu.queue.push_back(pending);
        }
        let current =
            c.mu.queue
                .back()
                .map_or([c.mu.throttle, c.mu.dynamic_brake, c.mu.reverser], |v| {
                    [v[0], v[1], v[2]]
                });
        if current != target {
            if c.mu.queue.len() >= 8192 {
                c.mu.throttle = 0.;
                c.mu.dynamic_brake = 0.;
                c.mu.reverser = 0.5;
                c.mu.queue.clear();
                c.mu.pending = None;
                continue;
            }
            c.mu.queue.push_back([
                target[0],
                target[1],
                target[2],
                now + if index == master { 0. } else { p.delay_s },
            ]);
        }
        while c.mu.queue.front().is_some_and(|v| v[3] <= now + 1e-9) {
            let packet = c.mu.queue.pop_front().unwrap();
            c.mu.throttle = packet[0];
            c.mu.dynamic_brake = packet[1];
            c.mu.reverser = packet[2];
        }
        c.mu.pending = c.mu.queue.front().copied();
    }
}
pub fn throttle(state: &crate::TrainSimState, index: usize, fallback: f64) -> f64 {
    state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == index && c.definition.multiple_unit.is_some())
        .map_or(fallback, |c| {
            if (c.mu.reverser - state.driver_reverser).abs() < 0.01 {
                c.mu.throttle
            } else {
                0.
            }
        })
}
pub fn brake(state: &crate::TrainSimState, index: usize) -> f64 {
    state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == index && c.definition.multiple_unit.is_some())
        .map_or(state.electric_brake_command, |c| c.mu.dynamic_brake)
}

/// A gearbox command follows the same declared MU protocol as traction. Old
/// stock without a protocol declaration retains its connected legacy control.
pub fn accepts_gear_command(state: &crate::TrainSimState, index: usize) -> bool {
    let master = state.active_cab_vehicle.unwrap_or(0);
    if master == index {
        return true;
    }
    let remote = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == index)
        .and_then(|c| c.definition.multiple_unit.as_ref());
    let Some(remote) = remote else {
        return true;
    };
    let leader = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == master)
        .and_then(|c| c.definition.multiple_unit.as_ref());
    remote.throttle
        && leader.is_some_and(|leader| {
            leader.protocol == remote.protocol
                || (leader.compatible_protocols.contains(&remote.protocol)
                    && remote.compatible_protocols.contains(&leader.protocol))
        })
}
