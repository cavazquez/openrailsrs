//! Reference scene: ground plane, world-space grid, RGB axes and lighting.
//!
//! The grid is drawn every frame with [`Gizmos`] (no extra mesh asset). The
//! ground is a single PBR plane spawned once in `spawn_ground_and_lights`.

use bevy::prelude::*;

/// Half-extent of the ground plane and the grid (m). The plane is
/// `2 * GROUND_HALF` wide on each axis and the grid covers the same area.
const GROUND_HALF: f32 = 100.0;

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
pub fn spawn_ground_and_lights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(
        Plane3d::default()
            .mesh()
            .size(GROUND_HALF * 2.0, GROUND_HALF * 2.0),
    );
    let material = materials.add(StandardMaterial {
        base_color: COLOR_GROUND,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, -0.001, 0.0),
        Name::new("ground"),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(20.0, 40.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        Name::new("sun"),
    ));
}

/// Update-loop gizmos: ground grid + RGB world axes.
pub fn draw_grid_and_axes(mut gizmos: Gizmos) {
    let half = GROUND_HALF;
    let step = GRID_MINOR_STEP;
    let n = (half / step) as i32;

    for i in -n..=n {
        let v = i as f32 * step;
        let color = if i.rem_euclid(GRID_MAJOR_EVERY) == 0 {
            COLOR_GRID_MAJOR
        } else {
            COLOR_GRID_MINOR
        };
        gizmos.line(Vec3::new(v, 0.0, -half), Vec3::new(v, 0.0, half), color);
        gizmos.line(Vec3::new(-half, 0.0, v), Vec3::new(half, 0.0, v), color);
    }

    gizmos.line(Vec3::ZERO, Vec3::X * AXIS_LENGTH, COLOR_AXIS_X);
    gizmos.line(Vec3::ZERO, Vec3::Y * AXIS_LENGTH, COLOR_AXIS_Y);
    gizmos.line(Vec3::ZERO, Vec3::Z * AXIS_LENGTH, COLOR_AXIS_Z);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_steps_evenly_divide_half_extent() {
        let n = GROUND_HALF / GRID_MINOR_STEP;
        assert!(
            (n - n.round()).abs() < 1e-6,
            "GROUND_HALF must be a multiple of GRID_MINOR_STEP"
        );
    }
}
