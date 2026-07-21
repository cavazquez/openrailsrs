//! Shared daylight / night sun + ambient defaults (#124).

use std::f32::consts::{FRAC_PI_4, PI};

use bevy::prelude::*;

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
        if night {
            Self::night()
        } else {
            Self::day()
        }
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
        (self.rotation(), self.illuminance, self.color, self.ambient_color)
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
