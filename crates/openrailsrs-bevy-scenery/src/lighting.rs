//! Shared daylight / night sun + ambient defaults (#124).

use std::f32::consts::{FRAC_PI_4, PI};

use bevy::prelude::*;
use openrailsrs_formats::{EnvironmentSun, geography::GeographicPosition};

/// Open Rails' representative date for an MSTS season (0=spring, 1=summer).
pub fn season_ordinal(season: &str, latitude: f64) -> u16 {
    let index = match season.trim().to_ascii_lowercase().as_str() {
        "summer" => 1,
        "autumn" | "fall" => 2,
        "winter" => 3,
        _ => 0,
    };
    if latitude >= 0.0 {
        82 + index * 91
    } else {
        (82 + (index + 2) * 91) % 365
    }
}

/// Native solar vector in the XNA/Bevy frame (+X east, +Z south).
/// Adapted from Open Rails 1.6.1 `SunMoonPos.SolarAngle`, GPL-3.0-or-later.
/// The route's `.env` sunrise/sunset override the astronomical hour angle.
/// We evaluate the equation continuously instead of copying OR's 20-minute cache.
pub fn solar_direction(
    position: GeographicPosition,
    ordinal: u16,
    time_s: f64,
    sun: Option<EnvironmentSun>,
) -> Vec3 {
    let clock = (time_s.rem_euclid(86400.0) / 86400.0) as f32;
    let year =
        f64::from((std::f32::consts::TAU / 365.0) * (f32::from(ordinal) - 1.0 + clock - 0.5));
    let declination = 0.006918 - 0.399912 * year.cos() + 0.070257 * year.sin()
        - 0.006758 * (2.0 * year).cos()
        + 0.000907 * (2.0 * year).sin()
        - 0.002697 * (3.0 * year).cos()
        + 0.001480 * (3.0 * year).sin();
    let (sin_lat, cos_lat) = position.latitude.sin_cos();
    let (sin_decl, cos_decl) = declination.sin_cos();
    let horizon = (-sin_lat * sin_decl / (cos_lat * cos_decl)).acos();
    let hour_angle = if let Some(sun) =
        sun.filter(|s| s.rise_time_s > 0 && s.set_time_s > s.rise_time_s && horizon.is_finite())
    {
        let noon = (sun.rise_time_s as f32 + sun.set_time_s as f32) / 2.0 / 86400.0;
        let scale = 90.0 / (noon - sun.rise_time_s as f32 / 86400.0);
        f64::from(
            ((clock - noon) * scale * (horizon / f64::from(std::f32::consts::FRAC_PI_2)) as f32)
                .to_radians(),
        )
    } else {
        let west_longitude = -(position.longitude as f32).to_degrees();
        let equation = 229.18
            * (0.000075 + 0.001868 * year.cos()
                - 0.032077 * year.sin()
                - 0.014615 * (2.0 * year).cos()
                - 0.040849 * (2.0 * year).sin());
        let offset = equation - f64::from(4.0 * west_longitude)
            + 60.0 * f64::from((west_longitude / 15.0).round_ties_even());
        let solar_minutes = f64::from(clock * 24.0 * 60.0) + offset;
        f64::from(((solar_minutes / 4.0) as f32 - 180.0).to_radians())
    };
    let (sin_hour, cos_hour) = hour_angle.sin_cos();
    // Algebraic form avoids the native azimuth's 0/0 at zenith / poles.
    Vec3::new(
        (-sin_hour * cos_decl) as f32,
        (sin_lat * sin_decl + cos_lat * cos_decl * cos_hour) as f32,
        (sin_lat * cos_decl * cos_hour - cos_lat * sin_decl) as f32,
    )
    .normalize_or_zero()
}

/// Descriptor for a directional sun + ambient fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSunLight {
    pub illuminance: f32,
    pub color: Color,
    pub ambient_brightness: f32,
    pub ambient_color: Color,
    /// Yaw (around +Y) and pitch (around local X) in radians for the light direction.
    pub yaw_rad: f32,
    pub pitch_rad: f32,
}

impl SceneSunLight {
    pub fn day() -> Self {
        Self {
            illuminance: 55_000.0,
            color: Color::srgb(1.0, 0.97, 0.90),
            ambient_brightness: 0.28,
            ambient_color: Color::srgb(0.78, 0.84, 0.95),
            yaw_rad: -0.65,
            pitch_rad: -0.95,
        }
    }

    pub fn night() -> Self {
        Self {
            illuminance: 2_500.0,
            color: Color::srgb(0.55, 0.62, 0.85),
            ambient_brightness: 0.08,
            ambient_color: Color::srgb(0.12, 0.14, 0.22),
            yaw_rad: 0.4,
            pitch_rad: -0.55,
        }
    }

    pub fn for_night(night: bool) -> Self {
        if night { Self::night() } else { Self::day() }
    }

    /// Sun / moon from MSTS activity `StartTime` (seconds since midnight).
    ///
    /// Night uses a fixed moon pose; day interpolates pitch/yaw and colors by daylight fraction.
    pub fn from_msts_start_time(start_time_s: f64, night: bool) -> Self {
        if night {
            return Self {
                illuminance: 800.0,
                color: Color::srgb(0.75, 0.78, 0.95),
                ambient_brightness: 40.0,
                ambient_color: Color::srgb(0.08, 0.10, 0.18),
                yaw_rad: 0.0,
                pitch_rad: -PI * 0.85,
            };
        }

        let hour = (start_time_s / 3600.0).rem_euclid(24.0) as f32;
        let daylight = ((hour - 6.0) / 14.0).clamp(0.0, 1.0);
        let pitch = -0.25 - daylight * 1.05;
        let yaw = FRAC_PI_4 + (hour - 12.0) * 0.04;
        Self {
            illuminance: 4_000.0 + daylight * 8_000.0,
            color: Color::srgb(1.0, 0.96 + daylight * 0.02, 0.88 + daylight * 0.08),
            ambient_brightness: 160.0,
            ambient_color: Color::srgb(0.45 + daylight * 0.15, 0.52 + daylight * 0.18, 0.65),
            yaw_rad: yaw,
            pitch_rad: pitch,
        }
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw_rad, self.pitch_rad, 0.0)
    }

    /// Legacy tuple used by render3d before #124 (`rotation, illuminance, sun, ambient`).
    pub fn as_legacy_tuple(&self) -> (Quat, f32, Color, Color) {
        (
            self.rotation(),
            self.illuminance,
            self.color,
            self.ambient_color,
        )
    }
}

/// Rotation transform for a directional sun entity.
pub fn sun_transform(sun: &SceneSunLight) -> Transform {
    Transform::from_rotation(sun.rotation())
}

/// Build a Bevy [`DirectionalLight`] from the descriptor (shadows left to the caller).
pub fn directional_light_from_sun(sun: &SceneSunLight, shadows: bool) -> DirectionalLight {
    DirectionalLight {
        color: sun.color,
        illuminance: sun.illuminance,
        shadow_maps_enabled: shadows,
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geography_and_solar_vectors_match_unmodified_or161_assemblies() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/solar-or161.json")).unwrap();
        let mut compared = 0;
        let mut max_error = 0.0f32;
        for sample in oracle["samples"].as_array().unwrap() {
            let position = openrailsrs_formats::geography::geographic_position(
                sample["tile_x"].as_i64().unwrap() as i32,
                sample["tile_z"].as_i64().unwrap() as i32,
                sample["local_x"].as_f64().unwrap(),
                sample["local_z"].as_f64().unwrap(),
            );
            if sample["status"].as_i64().unwrap() != 1 {
                assert!(position.is_none(), "native interrupted region: {sample}");
                continue;
            }
            let position = position.unwrap();
            assert!((position.latitude - sample["latitude"].as_f64().unwrap()).abs() < 1e-11);
            assert!((position.longitude - sample["longitude"].as_f64().unwrap()).abs() < 1e-11);
            let season = ["spring", "summer", "autumn", "winter"]
                [sample["season_index"].as_u64().unwrap() as usize];
            let ordinal = season_ordinal(season, position.latitude);
            assert_eq!(u64::from(ordinal), sample["ordinal"].as_u64().unwrap());
            let sun = EnvironmentSun {
                rise_time_s: sample["rise_time_s"].as_u64().unwrap() as u32,
                set_time_s: sample["set_time_s"].as_u64().unwrap() as u32,
            };
            let actual = solar_direction(
                position,
                ordinal,
                sample["time_s"].as_f64().unwrap(),
                Some(sun),
            );
            let native = sample["direction"].as_array().unwrap();
            let expected = Vec3::new(
                native[0].as_f64().unwrap() as f32,
                native[1].as_f64().unwrap() as f32,
                native[2].as_f64().unwrap() as f32,
            );
            let error = actual.distance(expected);
            assert!(error < 1e-5, "solar vector error {error}: {sample}");
            max_error = max_error.max(error);
            compared += 1;
        }
        assert!(
            compared >= 60,
            "native oracle must include both hemispheres and .env / astronomical paths"
        );
        eprintln!("Native OR 1.6.1 solar samples: {compared}, maximum vector error {max_error:.9}");
    }

    #[test]
    fn continuous_sun_handles_midnight_and_poles_without_nonfinite_rotations() {
        for latitude in [
            -std::f64::consts::FRAC_PI_2,
            0.0,
            std::f64::consts::FRAC_PI_2,
        ] {
            let position = GeographicPosition {
                latitude,
                longitude: 0.0,
            };
            for time in [0.0, 43200.0, 86400.0] {
                let direction = solar_direction(position, 173, time, None);
                assert!(direction.is_finite());
                assert!((direction.length() - 1.0).abs() < 1e-6);
            }
            assert_eq!(
                solar_direction(position, 173, 0.0, None),
                solar_direction(position, 173, 86400.0, None)
            );
        }
    }

    #[test]
    fn noon_and_midnight_differ() {
        let day = SceneSunLight::from_msts_start_time(12.0 * 3600.0, false);
        let night = SceneSunLight::from_msts_start_time(0.0, true);
        assert!(day.illuminance > night.illuminance);
        assert_ne!(day.rotation(), night.rotation());
    }

    #[test]
    fn same_hour_is_deterministic() {
        let a = SceneSunLight::from_msts_start_time(15.5 * 3600.0, false);
        let b = SceneSunLight::from_msts_start_time(15.5 * 3600.0, false);
        assert_eq!(a, b);
    }
}
