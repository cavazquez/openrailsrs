//! Cant is a property of the rail: use the higher class speed in either direction.
//! OR 1.6.1 SuperElevation.DetermineTrackSpeeds, after resolving backing signs.
use serde::{Deserialize, Serialize};
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
