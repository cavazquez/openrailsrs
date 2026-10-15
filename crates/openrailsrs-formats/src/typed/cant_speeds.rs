//! Cant is a property of the rail: use the higher class speed in either direction.
//! OR 1.6.1 SuperElevation.DetermineTrackSpeeds, after resolving backing signs.
use crate::{TrackDbFile, TrackNodeKind};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CantSpeedLimit {
    pub position_m: f64,
    pub direction: i8,
    pub passenger_mps: Option<f32>,
    pub freight_mps: Option<f32>,
}
pub fn speeds_at(limits: &[CantSpeedLimit], position: f64, route_max: f32) -> [f32; 2] {
    let class = |passenger: bool| {
        let value = |p: &CantSpeedLimit| {
            if passenger {
                p.passenger_mps
            } else {
                p.freight_mps
            }
        };
        let forward = limits
            .iter()
            .filter(|p| p.direction >= 0 && p.position_m <= position && value(p).is_some())
            .max_by(|a, b| a.position_m.total_cmp(&b.position_m))
            .and_then(value);
        let backward = limits
            .iter()
            .filter(|p| p.direction <= 0 && p.position_m >= position && value(p).is_some())
            .min_by(|a, b| a.position_m.total_cmp(&b.position_m))
            .and_then(value);
        match (forward, backward) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    };
    let (mut passenger, mut freight) = (class(true), class(false));
    if passenger.is_none() {
        passenger = freight;
    }
    if freight.is_none() {
        freight = passenger;
    }
    [
        passenger.unwrap_or(route_max).min(route_max),
        freight.unwrap_or(route_max).min(route_max),
    ]
}

/// Shared native backing-sign traversal for rendered and physical cant. Sign
/// headings are resolved by each geometry loader before entering this core.
pub fn backing_speed(
    tdb: &TrackDbFile,
    signs: &HashMap<u32, Vec<CantSpeedLimit>>,
    default_branches: &HashMap<u32, usize>,
    first: u32,
    direction: i8,
    passenger: bool,
) -> Option<f32> {
    let mut visited = HashSet::new();
    let (mut node_id, mut travel) = (first, direction);
    let mut previous = None;
    for _ in 0..4096 {
        if !visited.insert(node_id) {
            return None;
        }
        let node = tdb.node_by_id(node_id)?;
        match &node.kind {
            TrackNodeKind::End => return None,
            TrackNodeKind::Junction { pins } => {
                let next = if previous == pins.first().map(|p| p.node_id) {
                    pins.get(1 + default_branches.get(&node_id).copied().unwrap_or(0))
                } else {
                    pins.first()
                }?;
                previous = Some(node_id);
                node_id = next.node_id;
            }
            TrackNodeKind::Vector { pins, .. } => {
                if let Some(previous) = previous {
                    travel = if pins.1 == previous {
                        1
                    } else if pins.0 == previous {
                        -1
                    } else {
                        return None;
                    };
                    let values = signs
                        .get(&node_id)
                        .into_iter()
                        .flatten()
                        .filter(|s| s.direction == 0 || s.direction == travel)
                        .filter_map(|s| {
                            (if passenger {
                                s.passenger_mps
                            } else {
                                s.freight_mps
                            })
                            .map(|v| (s.position_m, v))
                        });
                    let value = if travel > 0 {
                        values.max_by(|a, b| a.0.total_cmp(&b.0))
                    } else {
                        values.min_by(|a, b| a.0.total_cmp(&b.0))
                    };
                    if let Some((_, speed)) = value {
                        return Some(speed);
                    }
                }
                let next = if travel > 0 { pins.0 } else { pins.1 };
                previous = Some(node_id);
                node_id = next;
            }
        }
    }
    None
}

pub fn limits_for_node(
    tdb: &TrackDbFile,
    signs: &HashMap<u32, Vec<CantSpeedLimit>>,
    default_branches: &HashMap<u32, usize>,
    node: u32,
    length_m: f64,
) -> Vec<CantSpeedLimit> {
    let mut result = signs.get(&node).cloned().unwrap_or_default();
    for direction in [1, -1] {
        let passenger = backing_speed(tdb, signs, default_branches, node, direction, true);
        let freight = backing_speed(tdb, signs, default_branches, node, direction, false);
        if passenger.is_some() || freight.is_some() {
            result.push(CantSpeedLimit {
                position_m: if direction > 0 {
                    -0.001
                } else {
                    length_m + 0.001
                },
                direction,
                passenger_mps: passenger,
                freight_mps: freight,
            });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_and_direction_limits_choose_native_maximum_and_keep_route_cap() {
        let limits = vec![
            CantSpeedLimit {
                position_m: 0.,
                direction: 1,
                passenger_mps: Some(25.),
                freight_mps: Some(15.),
            },
            CantSpeedLimit {
                position_m: 200.,
                direction: -1,
                passenger_mps: Some(30.),
                freight_mps: Some(18.),
            },
            CantSpeedLimit {
                position_m: 250.,
                direction: 1,
                passenger_mps: Some(20.),
                freight_mps: None,
            },
        ];
        assert_eq!(speeds_at(&limits, 100., 40.), [30., 18.]);
        assert_eq!(speeds_at(&limits, 100., 27.), [27., 18.]);
        assert_eq!(speeds_at(&limits, 300., 40.), [20., 15.]);
        assert_eq!(speeds_at(&[], 100., 40.), [40., 40.]);
    }
}
