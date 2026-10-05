//! Weather selection and deterministic particle seeds.
//! The bounded CPU/GPU renderer is in `weather_particles`.

use bevy::prelude::*;

/// Activated by selected weather; the standalone scenery viewer also uses `P`.
#[derive(Resource, Clone, Debug)]
pub struct PrecipitationState {
    pub enabled: bool,
    pub snow: bool,
    pub area_half: f32,
    pub ceiling: f32,
}

impl PrecipitationState {
    pub fn hud_label(&self) -> &'static str {
        if self.enabled { "on" } else { "off" }
    }
}

impl Default for PrecipitationState {
    fn default() -> Self {
        Self {
            enabled: false,
            snow: false,
            area_half: 35.0,
            ceiling: 45.0,
        }
    }
}

/// Deterministic [0, 1) helper for drop placement.
pub fn rain_rng01(seed: u32, channel: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ channel.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 16;
    (x as f32) / (u32::MAX as f32)
}

/// Horizontal spawn offset for one rain streak.
pub fn rain_offset_xz(center: Vec3, seed: u32, area_half: f32) -> (f32, f32) {
    let rx = rain_rng01(seed, 0) * 2.0 - 1.0;
    let rz = rain_rng01(seed, 1) * 2.0 - 1.0;
    (center.x + rx * area_half, center.z + rz * area_half)
}

/// Y-axis billboard rotation so a vertical streak quad faces the camera.
pub fn rain_billboard_yaw(drop_pos: Vec3, camera_pos: Vec3) -> f32 {
    let dx = camera_pos.x - drop_pos.x;
    let dz = camera_pos.z - drop_pos.z;
    if dx * dx + dz * dz < 1e-8 {
        return 0.0;
    }
    dx.atan2(dz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_label_reflects_enabled_flag() {
        assert_eq!(
            PrecipitationState {
                enabled: true,
                ..Default::default()
            }
            .hud_label(),
            "on"
        );
        assert_eq!(
            PrecipitationState {
                enabled: false,
                snow: false,
                ..Default::default()
            }
            .hud_label(),
            "off"
        );
    }

    #[test]
    fn rain_rng_is_deterministic() {
        assert_eq!(rain_rng01(7, 1), rain_rng01(7, 1));
        assert_ne!(rain_rng01(7, 1), rain_rng01(8, 1));
    }

    #[test]
    fn rain_offset_stays_in_patch() {
        let center = Vec3::new(100.0, 0.0, 50.0);
        let (x, z) = rain_offset_xz(center, 12, 80.0);
        assert!((x - center.x).abs() <= 80.0);
        assert!((z - center.z).abs() <= 80.0);
    }

    #[test]
    fn billboard_yaw_faces_camera_on_xz() {
        let drop = Vec3::new(0.0, 10.0, 0.0);
        let cam = Vec3::new(10.0, 10.0, 0.0);
        let yaw = rain_billboard_yaw(drop, cam);
        assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 0.05);
    }
}
