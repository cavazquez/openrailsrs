//! Double precision, saved movement shared by clouds and local fog banks.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WeatherAdvection {
    pub offset_m: [f64; 2],
    pub velocity_mps: [f64; 2],
    pub clock_s: Option<f64>,
}
impl WeatherAdvection {
    pub fn advance(&mut self, clock: f64, wind: Vec3) {
        if !clock.is_finite() || !wind.is_finite() {
            return;
        }
        if let Some(previous) = self.clock_s {
            let dt = (clock - previous).max(0.);
            if clock < previous {
                self.offset_m = [0.; 2];
            } else {
                for i in 0..2 {
                    self.offset_m[i] += self.velocity_mps[i] * dt;
                }
            }
        }
        self.velocity_mps = [f64::from(wind.x), f64::from(wind.z)];
        self.clock_s = Some(clock);
    }
    pub fn phase(&self, origin: Vec3, period: f64) -> Vec2 {
        Vec2::new(
            (f64::from(origin.x) - self.offset_m[0]).rem_euclid(period) as f32,
            (f64::from(origin.z) - self.offset_m[1]).rem_euclid(period) as f32,
        )
    }
    pub fn valid(&self) -> bool {
        self.offset_m
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1e12)
            && self
                .velocity_mps
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 150.)
            && self
                .clock_s
                .is_none_or(|v| v.is_finite() && (0. ..=1e10).contains(&v))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wind_changes_pause_save_and_rebase_do_not_jump_the_field() {
        let mut a = WeatherAdvection::default();
        a.advance(0., Vec3::X * 3.);
        a.advance(10., Vec3::X * 3.);
        assert_eq!(a.offset_m, [30., 0.]);
        a.advance(10., Vec3::Z * 4.);
        assert_eq!(a.offset_m, [30., 0.]);
        let mut b: WeatherAdvection =
            serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        a.advance(12., Vec3::Z * 4.);
        b.advance(12., Vec3::Z * 4.);
        assert_eq!(a.offset_m, b.offset_m);
        let p = a.phase(Vec3::new(10000., 0., 30000.), 8192.);
        let rebased = a.phase(Vec3::new(12048., 0., 30000.), 8192.);
        assert!(((p.x + 2048.).rem_euclid(8192.) - rebased.x).abs() < 0.001);
    }
}
