//! Simple rain streaks around the camera (order 11 / issue #8, PR2 billboards).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::track::TrackScene;

const RAIN_DROP_COUNT: usize = 420;
const RAIN_STREAK_HEIGHT: f32 = 3.4;
const RAIN_STREAK_WIDTH: f32 = 0.14;

/// Toggle with `P`. Enabled by default for the smoke demo.
#[derive(Resource, Clone, Debug)]
pub struct PrecipitationState {
    pub enabled: bool,
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
            enabled: true,
            area_half: 260.0,
            ceiling: 95.0,
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

/// Y-axis billboard rotation so a vertical streak quad faces the camera.
pub fn rain_billboard_yaw(drop_pos: Vec3, camera_pos: Vec3) -> f32 {
    let dx = camera_pos.x - drop_pos.x;
    let dz = camera_pos.z - drop_pos.z;
    if dx * dx + dz * dz < 1e-8 {
        return 0.0;
    }
    dx.atan2(dz)
}

fn rain_streak_mesh(width: f32, height: f32) -> Mesh {
    let hw = width * 0.5;
    let positions = vec![
        [-hw, 0.0, 0.0],
        [hw, 0.0, 0.0],
        [hw, height, 0.0],
        [-hw, height, 0.0],
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = Indices::U32(vec![0, 1, 2, 0, 2, 3]);
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(indices);
    mesh
}

pub fn spawn_precipitation(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    state: Res<PrecipitationState>,
) {
    let initial_vis = if state.enabled {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    spawn_rain_entities(
        &mut commands,
        &mut meshes,
        &mut materials,
        scene.bounds.center,
        &state,
        initial_vis,
    );
}

fn spawn_rain_entities(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    origin: Vec3,
    state: &PrecipitationState,
    visibility: Visibility,
) {
    let streak = meshes.add(rain_streak_mesh(RAIN_STREAK_WIDTH, RAIN_STREAK_HEIGHT));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.94, 0.97, 1.0, 0.9),
        emissive: LinearRgba::from(Color::srgb(0.55, 0.72, 1.0)) * 0.35,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });

    for i in 0..RAIN_DROP_COUNT {
        let seed = i as u32 + 1;
        let (x, z) = rain_offset_xz(origin, seed, state.area_half);
        let y = origin.y + rain_rng01(seed, 2) * state.ceiling;
        let speed = 20.0 + rain_rng01(seed, 3) * 16.0;
        commands.spawn((
            RainDrop { speed, seed },
            Mesh3d(streak.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(x, y, z),
            visibility,
            Name::new(format!("rain:{i}")),
        ));
    }

    eprintln!(
        "openrailsrs-viewer3d: precipitation {} ({RAIN_DROP_COUNT} billboards, P toggles)",
        if matches!(visibility, Visibility::Inherited) {
            "on"
        } else {
            "off"
        }
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn toggle_precipitation(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<PrecipitationState>,
    mut drops: Query<&mut Visibility, With<RainDrop>>,
    mut commands: Commands,
    scene: Res<TrackScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !keys.just_pressed(KeyCode::KeyP) {
        return;
    }
    state.enabled = !state.enabled;
    if state.enabled && drops.is_empty() {
        spawn_rain_entities(
            &mut commands,
            &mut meshes,
            &mut materials,
            scene.bounds.center,
            &state,
            Visibility::Inherited,
        );
        return;
    }
    let vis = if state.enabled {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut drops {
        *visibility = vis;
    }
}

pub(crate) fn update_precipitation(
    time: Res<Time>,
    state: Res<PrecipitationState>,
    camera: Query<&Transform, With<Camera3d>>,
    mut drops: Query<(&mut Transform, &RainDrop), Without<Camera3d>>,
) {
    if !state.enabled {
        return;
    }

    let Ok(cam) = camera.single() else {
        return;
    };
    let origin = cam.translation;
    let floor = origin.y - 30.0;
    let top = origin.y + state.ceiling;
    let cam_pos = cam.translation;

    for (mut transform, drop) in &mut drops {
        let dx = transform.translation.x - origin.x;
        let dz = transform.translation.z - origin.z;
        if dx.abs() > state.area_half || dz.abs() > state.area_half {
            let (x, z) = rain_offset_xz(origin, drop.seed, state.area_half);
            transform.translation.x = x;
            transform.translation.z = z;
        }

        transform.translation.y -= drop.speed * time.delta_secs();
        if transform.translation.y < floor {
            let (x, z) = rain_offset_xz(origin, drop.seed, state.area_half);
            transform.translation.x = x;
            transform.translation.z = z;
            transform.translation.y = top;
        }

        let yaw = rain_billboard_yaw(transform.translation, cam_pos);
        transform.rotation = Quat::from_rotation_y(yaw);
    }
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
