//! Shared temperature and wind: analytic in railway time and independent of FPS.
use crate::{environment::RouteLocation, player_launch::PlayerWeather};
use bevy::prelude::*;

pub fn temperature(
    location: Option<RouteLocation>,
    season: &str,
    clock_s: f64,
    weather: PlayerWeather,
) -> f32 {
    let latitude = location.map_or(45.0, |p| p.latitude.abs()) as f32;
    let seasonal = match season {
        "winter" => -9.0,
        "autumn" => -3.0,
        "spring" => 0.0,
        _ => 7.0,
    };
    let daylight =
        ((clock_s.rem_euclid(86400.0) / 86400.0 * std::f64::consts::TAU - 3.7).cos() * 4.0) as f32;
    let value = 26.0 - latitude * 0.28 + seasonal + daylight;
    match weather {
        PlayerWeather::Snow => value.min(-1.0),
        PlayerWeather::Rain | PlayerWeather::Storm => value - 3.0,
        _ => value,
    }
    .clamp(-45.0, 45.0)
}
pub fn gust(base: Vec3, seed: u32, seconds: f64) -> Vec3 {
    let phase = crate::precipitation::rain_rng01(seed, 991) * std::f32::consts::TAU;
    let t = seconds.rem_euclid(86400.0) as f32;
    let strength = 1.0 + 0.22 * (t * 0.21 + phase).sin() + 0.12 * (t * 0.073 + phase * 1.7).sin();
    let yaw = 0.12 * (t * 0.083 + phase).sin();
    Quat::from_rotation_y(yaw) * base * strength
}
pub fn condensation(temperature_c: f32) -> f32 {
    ((26.0 - temperature_c) / 30.0).clamp(0.08, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn climate_uses_location_and_season_and_gusts_are_reproducible() {
        let london = Some(RouteLocation {
            latitude: 51.5,
            longitude: -0.1,
        });
        let argentina = Some(RouteLocation {
            latitude: -34.6,
            longitude: -58.4,
        });
        assert!(
            temperature(london, "winter", 43000.0, PlayerWeather::Clear)
                < temperature(london, "summer", 43000.0, PlayerWeather::Clear)
        );
        assert_ne!(
            temperature(london, "summer", 43000.0, PlayerWeather::Clear),
            temperature(argentina, "summer", 43000.0, PlayerWeather::Clear)
        );
        assert!(temperature(argentina, "summer", 43000.0, PlayerWeather::Snow) <= -1.0);
        let wind = Vec3::new(9.0, 0.0, 3.0);
        assert_eq!(gust(wind, 3, 60.0), gust(wind, 3, 60.0));
        assert_ne!(gust(wind, 3, 60.0), gust(wind, 4, 60.0));
        for tick in 0..10000 {
            assert!(gust(wind, 3, tick as f64 / 30.0).length() <= wind.length() * 1.35);
        }
        assert!(condensation(-10.0) > condensation(30.0));
    }
}
