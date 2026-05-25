//! Reference scene: ground plane, world-space grid, RGB axes and lighting.
//!
//! The grid is drawn every frame with [`Gizmos`]. The ground plane is sized to
//! fit the loaded [`TrackScene`] bounds when present.

use bevy::prelude::*;

use crate::terrain::TerrainScene;
use crate::track::TrackScene;

/// Spacing between minor grid lines (m).
const GRID_MINOR_STEP: f32 = 10.0;

/// Every Nth line is drawn brighter (acts as a major grid line).
const GRID_MAJOR_EVERY: i32 = 10;

const COLOR_GROUND: Color = Color::srgb(0.18, 0.20, 0.22);
const COLOR_GRID_MINOR: Color = Color::srgb(0.30, 0.33, 0.36);
const COLOR_GRID_MAJOR: Color = Color::srgb(0.55, 0.58, 0.62);
const COLOR_AXIS_X: Color = Color::srgb(0.95, 0.20, 0.20);
const COLOR_AXIS_Y: Color = Color::srgb(0.20, 0.95, 0.30);
const COLOR_AXIS_Z: Color = Color::srgb(0.25, 0.50, 1.00);
const AXIS_LENGTH: f32 = 5.0;

/// One-shot startup: spawn the ground plane and the lights.
///
/// When [`TerrainScene`] has tiles, the flat placeholder plane is omitted.
pub fn spawn_ground_and_lights(
    scene: Res<TrackScene>,
    terrain: Res<TerrainScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let half = scene.bounds.ground_half();
    let center = scene.bounds.center;

    if terrain.is_empty() {
        let mesh = meshes.add(Plane3d::default().mesh().size(half * 2.0, half * 2.0));
        let material = materials.add(StandardMaterial {
            base_color: COLOR_GROUND,
            perceptual_roughness: 0.95,
            metallic: 0.0,
            ..default()
        });

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(center.x, -0.001, center.z),
            Name::new("ground"),
        ));
    }

    let light_pos = center + Vec3::new(half * 0.2, half * 0.4, half * 0.3);
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_translation(light_pos).looking_at(center, Vec3::Y),
        Name::new("sun"),
    ));
}

/// Update-loop gizmos: ground grid + RGB world axes at the route centre.
pub fn draw_grid_and_axes(scene: Res<TrackScene>, mut gizmos: Gizmos) {
    let half = scene.bounds.ground_half();
    let center = scene.bounds.center;
    let step = grid_step_for_extent(half);
    let n = (half / step) as i32;

    for i in -n..=n {
        let v = i as f32 * step;
        let color = if i.rem_euclid(GRID_MAJOR_EVERY) == 0 {
            COLOR_GRID_MAJOR
        } else {
            COLOR_GRID_MINOR
        };
        let x = center.x + v;
        let z = center.z + v;
        gizmos.line(
            Vec3::new(x, 0.0, center.z - half),
            Vec3::new(x, 0.0, center.z + half),
            color,
        );
        gizmos.line(
            Vec3::new(center.x - half, 0.0, z),
            Vec3::new(center.x + half, 0.0, z),
            color,
        );
    }

    let axis_len = (half * 0.05).clamp(AXIS_LENGTH, 200.0);
    gizmos.line(center, center + Vec3::X * axis_len, COLOR_AXIS_X);
    gizmos.line(center, center + Vec3::Y * axis_len, COLOR_AXIS_Y);
    gizmos.line(center, center + Vec3::Z * axis_len, COLOR_AXIS_Z);
}

/// Pick a grid step that keeps line count reasonable on large imported routes.
fn grid_step_for_extent(half: f32) -> f32 {
    let mut step = GRID_MINOR_STEP;
    while half / step > 200.0 {
        step *= 5.0;
    }
    step
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_step_scales_up_for_large_routes() {
        assert!(grid_step_for_extent(100.0) <= GRID_MINOR_STEP);
        assert!(grid_step_for_extent(50_000.0) > GRID_MINOR_STEP);
    }
}
