//! Simple rain streaks around the route centre (order 11 / issue #8).

use bevy::prelude::*;

use crate::track::TrackScene;

const RAIN_DROP_COUNT: usize = 160;
const RAIN_STREAK_HEIGHT: f32 = 1.2;
const RAIN_STREAK_RADIUS: f32 = 0.04;

/// Toggle with `P`. Enabled by default for the smoke demo.
#[derive(Resource, Clone, Debug)]
pub struct PrecipitationState {
    pub enabled: bool,
    pub area_half: f32,
    pub ceiling: f32,
}

impl Default for PrecipitationState {
    fn default() -> Self {
        Self {
            enabled: true,
            area_half: 220.0,
            ceiling: 90.0,
        }
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct RainDrop {
    speed: f32,
    seed: u32,
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

pub fn spawn_precipitation(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    state: Res<PrecipitationState>,
) {
    if !state.enabled {
        return;
    }

    let streak = meshes.add(Cuboid::new(
        RAIN_STREAK_RADIUS,
        RAIN_STREAK_HEIGHT,
        RAIN_STREAK_RADIUS,
    ));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.88, 1.0, 0.55),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        ..default()
    });

    let center = scene.bounds.center;
    for i in 0..RAIN_DROP_COUNT {
        let seed = i as u32 + 1;
        let (x, z) = rain_offset_xz(center, seed, state.area_half);
        let y = center.y + rain_rng01(seed, 2) * state.ceiling;
        let speed = 18.0 + rain_rng01(seed, 3) * 14.0;
        commands.spawn((
            RainDrop { speed, seed },
            Mesh3d(streak.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(x, y, z),
            Visibility::Inherited,
            Name::new(format!("rain:{i}")),
        ));
    }

    eprintln!("openrailsrs-viewer3d: precipitation on ({RAIN_DROP_COUNT} streaks, P toggles)");
}

pub(crate) fn toggle_precipitation(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<PrecipitationState>,
    mut drops: Query<&mut Visibility, With<RainDrop>>,
) {
    if keys.just_pressed(KeyCode::KeyP) {
        state.enabled = !state.enabled;
        let vis = if state.enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for mut visibility in &mut drops {
            *visibility = vis;
        }
    }
}

pub(crate) fn update_precipitation(
    time: Res<Time>,
    scene: Res<TrackScene>,
    state: Res<PrecipitationState>,
    mut drops: Query<(&mut Transform, &RainDrop)>,
) {
    if !state.enabled {
        return;
    }

    let center = scene.bounds.center;
    let floor = center.y - 2.0;
    let top = center.y + state.ceiling;

    for (mut transform, drop) in &mut drops {
        transform.translation.y -= drop.speed * time.delta_secs();
        if transform.translation.y < floor {
            let (x, z) = rain_offset_xz(center, drop.seed, state.area_half);
            transform.translation.x = x;
            transform.translation.z = z;
            transform.translation.y = top;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
