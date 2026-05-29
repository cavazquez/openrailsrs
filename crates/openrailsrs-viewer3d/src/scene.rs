//! Reference scene: ground plane, world-space grid, RGB axes and lighting.
//!
//! The grid is spawned as a static line-list mesh at startup (no per-frame
//! Gizmo cost). The ground plane is sized to fit the loaded [`TrackScene`]
//! bounds when present.

use bevy::asset::RenderAssetUsages;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

use crate::launch::{LIVE_GROUND_HALF_MAX_M, ViewerLaunchOpts};
use crate::terrain::TerrainScene;
use crate::track::TrackScene;

/// Spacing between minor grid lines (m).
const GRID_MINOR_STEP: f32 = 10.0;

/// Every Nth line is drawn brighter (acts as a major grid line).
const GRID_MAJOR_EVERY: i32 = 10;

const COLOR_GROUND: Color = Color::srgb(0.18, 0.20, 0.22);
const COLOR_AXIS_X: Color = Color::srgb(0.95, 0.20, 0.20);
const COLOR_AXIS_Y: Color = Color::srgb(0.20, 0.95, 0.30);
const COLOR_AXIS_Z: Color = Color::srgb(0.25, 0.50, 1.00);
const AXIS_LENGTH: f32 = 5.0;

/// One-shot startup: spawn the reference ground/grid and the lights.
///
/// When [`TerrainScene`] has tiles, the flat placeholder plane and grid are omitted.
pub fn spawn_ground_and_lights(
    scene: Res<TrackScene>,
    _focus: Res<crate::world::RouteFocus>,
    terrain: Res<TerrainScene>,
    opts: Res<ViewerLaunchOpts>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut half = scene.bounds.ground_half();
    if opts.live {
        half = half.min(LIVE_GROUND_HALF_MAX_M);
    }
    let center = Vec3::ZERO;

    if terrain.is_empty() {
        let mesh = meshes.add(Plane3d::default().mesh().size(half * 2.0, half * 2.0));
        let material = materials.add(StandardMaterial {
            base_color: COLOR_GROUND,
            perceptual_roughness: 0.95,
            metallic: 0.0,
            ..default()
        });

        let mut ground = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(center.x, -0.001, center.z),
            Name::new("ground"),
        ));
        if opts.live {
            ground.insert(NotShadowCaster);
            ground.insert(NotShadowReceiver);
        }
    }

    if terrain.is_empty() {
        spawn_grid_mesh(
            &mut commands,
            &mut meshes,
            &mut materials,
            &scene,
            center,
            opts.live,
        );
    }

    let light_pos = center + Vec3::new(half * 0.2, half * 0.4, half * 0.3);
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadows_enabled: !opts.live,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: if opts.live { 2 } else { 4 },
            minimum_distance: 0.1,
            maximum_distance: if opts.live { 120.0 } else { 200.0 },
            first_cascade_far_bound: 10.0,
            overlap_proportion: 0.2,
        }
        .build(),
        Transform::from_translation(light_pos).looking_at(center, Vec3::Y),
        Name::new("sun"),
    ));
    commands.insert_resource(DirectionalLightShadowMap {
        size: if opts.live { 1024 } else { 2048 },
    });
}

fn spawn_grid_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scene: &TrackScene,
    center: Vec3,
    live: bool,
) {
    let mut half = scene.bounds.ground_half();
    if live {
        half = half.min(LIVE_GROUND_HALF_MAX_M);
    }
    let step = grid_step_for_extent(half);
    let n = (half / step) as i32;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();

    let minor: [f32; 4] = Color::srgb(0.30, 0.33, 0.36).to_srgba().to_f32_array();
    let major: [f32; 4] = Color::srgb(0.55, 0.58, 0.62).to_srgba().to_f32_array();

    for i in -n..=n {
        let v = i as f32 * step;
        let c = if i.rem_euclid(GRID_MAJOR_EVERY) == 0 {
            major
        } else {
            minor
        };
        let x = center.x + v;
        let z = center.z + v;
        // Vertical line (along Z)
        positions.push([x, 0.0, center.z - half]);
        positions.push([x, 0.0, center.z + half]);
        colors.push(c);
        colors.push(c);
        // Horizontal line (along X)
        positions.push([center.x - half, 0.0, z]);
        positions.push([center.x + half, 0.0, z]);
        colors.push(c);
        colors.push(c);
    }

    let axis_len = (half * 0.05).clamp(AXIS_LENGTH, 200.0);
    let ax: [f32; 4] = COLOR_AXIS_X.to_srgba().to_f32_array();
    let ay: [f32; 4] = COLOR_AXIS_Y.to_srgba().to_f32_array();
    let az: [f32; 4] = COLOR_AXIS_Z.to_srgba().to_f32_array();
    positions.push([center.x, center.y, center.z]);
    positions.push([center.x + axis_len, center.y, center.z]);
    colors.push(ax);
    colors.push(ax);
    positions.push([center.x, center.y, center.z]);
    positions.push([center.x, center.y + axis_len, center.z]);
    colors.push(ay);
    colors.push(ay);
    positions.push([center.x, center.y, center.z]);
    positions.push([center.x, center.y, center.z + axis_len]);
    colors.push(az);
    colors.push(az);

    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);

    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });

    let mut grid = commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, 0.05, 0.0),
        Name::new("grid"),
    ));
    if live {
        grid.insert(NotShadowCaster);
    }
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
