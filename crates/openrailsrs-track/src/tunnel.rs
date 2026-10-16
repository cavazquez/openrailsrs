//! Original tunnel groups, not inferred from scenery meshes or measured forces.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTunnel {
    pub start_m: f64,
    pub end_m: f64,
    /// Original Single sum, kept independently of mirrored edge coordinates.
    pub length_m: f64,
    pub num_paths: u32,
    pub area_m2: f64,
    pub perimeter_m: f64,
}
impl NativeTunnel {
    pub fn valid(&self) -> bool {
        [
            self.start_m,
            self.end_m,
            self.length_m,
            self.area_m2,
            self.perimeter_m,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.start_m >= 0.
            && self.end_m > self.start_m
            && self.length_m > 0.
            && self.area_m2 > 0.
            && self.perimeter_m >= 0.
    }
    pub fn mirrored(&self, edge_length_m: f64) -> Self {
        Self {
            start_m: (edge_length_m - self.end_m).max(0.),
            end_m: (edge_length_m - self.start_m).min(edge_length_m),
            ..self.clone()
        }
    }
}

/// TrainCar.Initialize, OR1.6.1: zero area replaces both authored values.
/// TRK route speed selects defaults; local speed restrictions do not.
pub fn effective_geometry(
    speed_mps: f64,
    num_paths: u32,
    area_m2: f64,
    perimeter_m: f64,
) -> (f64, f64) {
    if area_m2 != 0. {
        return (area_m2, perimeter_m);
    }
    let speed = f64::from(speed_mps as f32);
    let pair: (f32, f32) = if num_paths >= 2 {
        if speed >= 69.4 {
            (100., 37.5)
        } else if speed >= 55.5 {
            (90., 35.)
        } else if speed >= 44.4 {
            (80., 34.5)
        } else if speed >= 33.3 {
            (76., 31.)
        } else {
            (41.8, 25.01)
        }
    } else if speed >= 69.4 {
        (70., 32.)
    } else if speed >= 55.5 {
        (58., 28.)
    } else if speed >= 44.4 {
        (50., 25.5)
    } else if speed >= 33.3 {
        (42., 22.5)
    } else {
        (21., 17.8)
    };
    (f64::from(pair.0), f64::from(pair.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_defaults_use_route_speed_and_area_controls_the_pair() {
        assert_eq!(effective_geometry(223.52, 1, 0., 999.), (70., 32.));
        assert_eq!(effective_geometry(223.52, 2, 0., 999.), (100., 37.5));
        assert_eq!(effective_geometry(30., 1, 80., 0.), (80., 0.));
        assert_eq!(effective_geometry(40., 1, 0., 0.), (42., 22.5));
    }
}
