//! Simple rain streaks around the camera (order 11 / issue #8, PR2 billboards).
//!
//! All drops are merged into a single mesh entity, updated each frame from
//! a [`RainState`] resource. This avoids hundreds of individual draw calls.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::track::TrackScene;

const RAIN_DROP_COUNT: usize = 200;
const RAIN_STREAK_HEIGHT: f32 = 3.4;
const RAIN_STREAK_WIDTH: f32 = 0.14;

/// Toggle with `P`. Disabled by default for performance.
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
            enabled: false,
            area_half: 260.0,
            ceiling: 95.0,
        }
    }
}

/// Per-drop state (seed determines position deterministically).
#[derive(Clone, Copy, Debug)]
struct RainDropState {
    seed: u32,
    #[allow(dead_code)]
    speed: f32,
}

/// Resource holding the merged rain mesh handle and drop states.
#[derive(Resource, Default)]
pub(crate) struct RainState {
    drops: Vec<RainDropState>,
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

fn _rain_streak_mesh(width: f32, height: f32) -> Mesh {
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

#[derive(Component)]
pub(crate) struct RainMeshMarker;

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
    let origin = scene.bounds.center;
    let mut drops = Vec::with_capacity(RAIN_DROP_COUNT);
    for i in 0..RAIN_DROP_COUNT {
        let seed = i as u32 + 1;
        let speed = 20.0 + rain_rng01(seed, 3) * 16.0;
        drops.push(RainDropState { seed, speed });
    }
    let mesh = build_rain_mesh(&drops, origin, state.area_half, state.ceiling);
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.94, 0.97, 1.0, 0.9),
        emissive: LinearRgba::from(Color::srgb(0.55, 0.72, 1.0)) * 0.35,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let handle = meshes.add(mesh);
    commands.spawn((
        RainMeshMarker,
        Mesh3d(handle),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Name::new("rain"),
    ));
    commands.insert_resource(RainState { drops });
    eprintln!(
        "openrailsrs-viewer3d: precipitation on ({RAIN_DROP_COUNT} billboards merged, P toggles)"
    );
}

fn build_rain_mesh(drops: &[RainDropState], origin: Vec3, area_half: f32, ceiling: f32) -> Mesh {
    let hw = RAIN_STREAK_WIDTH * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(drops.len() * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(drops.len() * 4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(drops.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(drops.len() * 6);

    for drop in drops {
        let (x, z) = rain_offset_xz(origin, drop.seed, area_half);
        let y = origin.y + rain_rng01(drop.seed, 2) * ceiling;
        let yaw = rain_billboard_yaw(Vec3::new(x, y, z), origin);
        let rot = Quat::from_rotation_y(yaw);
        let corners_local = [
            Vec3::new(-hw, 0.0, 0.0),
            Vec3::new(hw, 0.0, 0.0),
            Vec3::new(hw, RAIN_STREAK_HEIGHT, 0.0),
            Vec3::new(-hw, RAIN_STREAK_HEIGHT, 0.0),
        ];
        let base = positions.len() as u32;
        for c in &corners_local {
            let w = Vec3::new(x, y, z) + rot * *c;
            positions.push(w.to_array());
        }
        // Billboard normal: rotate forward face toward camera
        let wn = rot * Vec3::new(0.0, 0.0, 1.0);
        for _ in 0..4 {
            normals.push([wn.x, wn.y, wn.z]);
        }
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        uvs.push([1.0, 1.0]);
        uvs.push([0.0, 1.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn toggle_precipitation(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<PrecipitationState>,
    existing: Query<Entity, With<RainMeshMarker>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
) {
    if !keys.just_pressed(KeyCode::KeyP) {
        return;
    }
    state.enabled = !state.enabled;
    if state.enabled {
        if existing.is_empty() {
            let origin = scene.bounds.center;
            let mut drops = Vec::with_capacity(RAIN_DROP_COUNT);
            for i in 0..RAIN_DROP_COUNT {
                let seed = i as u32 + 1;
                let speed = 20.0 + rain_rng01(seed, 3) * 16.0;
                drops.push(RainDropState { seed, speed });
            }
            let mesh = build_rain_mesh(&drops, origin, state.area_half, state.ceiling);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(0.94, 0.97, 1.0, 0.9),
                emissive: LinearRgba::from(Color::srgb(0.55, 0.72, 1.0)) * 0.35,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            let handle = meshes.add(mesh);
            commands.spawn((
                RainMeshMarker,
                Mesh3d(handle),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Name::new("rain"),
            ));
            commands.insert_resource(RainState { drops });
        }
    } else {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        commands.remove_resource::<RainState>();
    }
}

pub(crate) fn update_precipitation(
    time: Res<Time>,
    state: Res<PrecipitationState>,
    camera: Query<&Transform, With<Camera3d>>,
    rain_state: Option<ResMut<RainState>>,
    mut mesh_assets: ResMut<Assets<Mesh>>,
    mesh_query: Query<&Mesh3d, With<RainMeshMarker>>,
) {
    if !state.enabled {
        return;
    }
    let Ok(cam) = camera.single() else {
        return;
    };
    let Some(rain) = rain_state else {
        return;
    };
    let Ok(mesh_handle) = mesh_query.single() else {
        return;
    };

    let origin = cam.translation;
    let _dt = time.delta_secs();

    let mesh = build_rain_mesh(&rain.drops, origin, state.area_half, state.ceiling);
    if let Some(existing) = mesh_assets.get_mut(&mesh_handle.0) {
        *existing = mesh;
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
