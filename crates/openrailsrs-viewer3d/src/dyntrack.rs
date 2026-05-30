//! MSTS dynamic track segments from `.w` `Dyntrack` items (order 9 / issue #8).
//!
//! All sleeper and rail geometry for a route is merged into two consolidated
//! meshes (one for sleepers, one for rails) to minimise draw calls.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::track::{SceneBounds, TrackScene};
use crate::viewer_log;
use crate::world::WorldScene;

/// Dark weathered wood — deliberately far from graph orange (`1.0, 0.667, 0.2`).
const COLOR_SLEEPER: Color = Color::srgb(0.20, 0.14, 0.10);
/// Cool steel so rails read clearly against edge cylinders and brown sleepers.
const COLOR_RAIL: Color = Color::srgb(0.78, 0.86, 0.98);

/// Visual dimensions for a dyntrack segment, scaled like other world placeholders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DyntrackDimensions {
    pub length: f32,
    pub sleeper_width: f32,
    pub sleeper_height: f32,
    pub sleeper_spacing: f32,
    pub sleeper_depth: f32,
    pub half_gauge: f32,
    pub rail_width: f32,
    pub rail_height: f32,
}

/// Match the `base` used by [`crate::world::spawn_world_boxes`] so dyntrack is visible
/// next to scaled graph cylinders and world cuboids.
pub fn dyntrack_dimensions(bounds: &SceneBounds) -> DyntrackDimensions {
    let base = bounds.edge_radius().max(2.0) * 1.5;
    let spacing = (base * 0.16).clamp(3.0, 12.0);
    DyntrackDimensions {
        length: base * 2.4,
        sleeper_width: base * 1.2,
        sleeper_height: base * 0.12,
        sleeper_spacing: spacing,
        sleeper_depth: spacing * 0.55,
        half_gauge: base * 0.35 * 0.5,
        rail_width: base * 0.07,
        rail_height: base * 0.09,
    }
}

/// Local +Z positions (metres from segment start) for repeated sleepers.
pub fn sleeper_local_z_positions(length: f32, spacing: f32) -> Vec<f32> {
    if spacing <= 0.0 || length <= 0.0 {
        return Vec::new();
    }
    let mut positions = Vec::new();
    let mut z = spacing * 0.5;
    while z < length {
        positions.push(z);
        z += spacing;
    }
    positions
}

/// World-space end point of a segment anchored at `position` with `rotation`.
pub fn segment_end_world(position: Vec3, rotation: Quat, length_m: f32) -> Vec3 {
    position + rotation * Vec3::new(0.0, 0.0, length_m)
}

/// Transform for a unit cube scaled and placed in the segment's local frame.
pub fn part_transform(anchor: Vec3, rotation: Quat, local_center: Vec3, scale: Vec3) -> Transform {
    Transform {
        translation: anchor + rotation * local_center,
        rotation,
        scale,
    }
}

/// Spawn all dyntrack geometry as two merged meshes (sleepers + rails).
pub fn spawn_dyntrack_segments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    track: Res<TrackScene>,
    focus: Res<crate::world::RouteFocus>,
) {
    let dyntracks: Vec<_> = world
        .items
        .iter()
        .filter(|obj| obj.kind == "Dyntrack")
        .collect();
    if dyntracks.is_empty() {
        return;
    }

    let count = dyntracks.len();
    let dims = dyntrack_dimensions(&track.bounds);
    let sleeper_material = materials.add(StandardMaterial {
        base_color: COLOR_SLEEPER,
        perceptual_roughness: 0.92,
        metallic: 0.02,
        ..default()
    });
    let rail_material = materials.add(StandardMaterial {
        base_color: COLOR_RAIL,
        emissive: LinearRgba::from(COLOR_RAIL) * 0.08,
        perceptual_roughness: 0.35,
        metallic: 0.75,
        ..default()
    });

    let rail_y = dims.sleeper_height + dims.rail_height * 0.5;
    let half_len = dims.length * 0.5;
    let sleeper_positions = sleeper_local_z_positions(dims.length, dims.sleeper_spacing);

    let mut sleeper_pos: Vec<[f32; 3]> = Vec::new();
    let mut sleeper_nrm: Vec<[f32; 3]> = Vec::new();
    let mut sleeper_uv: Vec<[f32; 2]> = Vec::new();
    let mut sleeper_idx: Vec<u32> = Vec::new();

    let mut rail_pos: Vec<[f32; 3]> = Vec::new();
    let mut rail_nrm: Vec<[f32; 3]> = Vec::new();
    let mut rail_uv: Vec<[f32; 2]> = Vec::new();
    let mut rail_idx: Vec<u32> = Vec::new();

    for obj in &dyntracks {
        let pos = focus.to_render(obj.position);
        for local_z in &sleeper_positions {
            let tf = part_transform(
                pos,
                obj.rotation,
                Vec3::new(0.0, dims.sleeper_height * 0.5, *local_z),
                Vec3::new(dims.sleeper_width, dims.sleeper_height, dims.sleeper_depth),
            );
            push_cuboid(
                &mut sleeper_pos,
                &mut sleeper_nrm,
                &mut sleeper_uv,
                &mut sleeper_idx,
                &tf,
                Vec3::splat(1.0),
            );
        }

        for side in [-dims.half_gauge, dims.half_gauge] {
            let tf = part_transform(
                pos,
                obj.rotation,
                Vec3::new(side, rail_y, half_len),
                Vec3::new(dims.rail_width, dims.rail_height, dims.length),
            );
            push_cuboid(
                &mut rail_pos,
                &mut rail_nrm,
                &mut rail_uv,
                &mut rail_idx,
                &tf,
                Vec3::splat(1.0),
            );
        }
    }

    if !sleeper_pos.is_empty() {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, sleeper_pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, sleeper_nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, sleeper_uv);
        mesh.insert_indices(Indices::U32(sleeper_idx));
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(sleeper_material),
            Transform::IDENTITY,
            Name::new("dyntrack:sleepers"),
        ));
    }

    if !rail_pos.is_empty() {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, rail_pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, rail_nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, rail_uv);
        mesh.insert_indices(Indices::U32(rail_idx));
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(rail_material),
            Transform::IDENTITY,
            Name::new("dyntrack:rails"),
        ));
    }

    viewer_log!("openrailsrs-viewer3d: {count} dyntrack segment(s)");
}

/// Bake a unit cuboid (optionally scaled by `size`) transformed by `tf` into aggregate buffers.
fn push_cuboid(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    tf: &Transform,
    size: Vec3,
) {
    let hx = size.x * 0.5;
    let hy = size.y * 0.5;
    let hz = size.z * 0.5;

    let local = [
        Vec3::new(-hx, -hy, -hz),
        Vec3::new(hx, -hy, -hz),
        Vec3::new(hx, hy, -hz),
        Vec3::new(-hx, hy, -hz),
        Vec3::new(-hx, -hy, hz),
        Vec3::new(hx, -hy, hz),
        Vec3::new(hx, hy, hz),
        Vec3::new(-hx, hy, hz),
    ];
    let world: [Vec3; 8] = local.map(|c| tf.transform_point(c));

    let face_defs: [(usize, usize, usize, usize, Vec3); 6] = [
        (4, 5, 6, 7, Vec3::new(0.0, 0.0, 1.0)),
        (1, 0, 3, 2, Vec3::new(0.0, 0.0, -1.0)),
        (3, 7, 6, 2, Vec3::new(0.0, 1.0, 0.0)),
        (0, 1, 5, 4, Vec3::new(0.0, -1.0, 0.0)),
        (1, 2, 6, 5, Vec3::new(1.0, 0.0, 0.0)),
        (0, 4, 7, 3, Vec3::new(-1.0, 0.0, 0.0)),
    ];

    for (v0, v1, v2, v3, normal) in &face_defs {
        let face_base = positions.len() as u32;
        let wn = tf.rotation * *normal;
        let wn_arr = [wn.x, wn.y, wn.z];
        positions.push(world[*v0].to_array());
        positions.push(world[*v1].to_array());
        positions.push(world[*v2].to_array());
        positions.push(world[*v3].to_array());
        for _ in 0..4 {
            normals.push(wn_arr);
        }
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        uvs.push([1.0, 1.0]);
        uvs.push([0.0, 1.0]);
        indices.extend([
            face_base,
            face_base + 1,
            face_base + 2,
            face_base,
            face_base + 2,
            face_base + 3,
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::world::load_world_from_route_dir;

    #[test]
    fn dimensions_scale_with_route_bounds() {
        let small = dyntrack_dimensions(&SceneBounds::default_sandbox());
        assert!(small.length > 5.0);

        let large = dyntrack_dimensions(&SceneBounds {
            half_extent: 5_000.0,
            ..SceneBounds::default_sandbox()
        });
        assert!(large.length > small.length);
        assert!(large.half_gauge > small.half_gauge);
    }

    #[test]
    fn sleepers_repeat_along_segment() {
        let positions = sleeper_local_z_positions(72.0, 4.8);
        assert!(positions.len() >= 10);
        assert!((positions[0] - 2.4).abs() < 1e-4);
        assert!(*positions.last().unwrap() < 72.0);
    }

    #[test]
    fn segment_extends_along_local_z() {
        let end = segment_end_world(Vec3::new(10.0, 0.0, 5.0), Quat::IDENTITY, 20.0);
        assert_eq!(end, Vec3::new(10.0, 0.0, 25.0));
    }

    #[test]
    fn segment_respects_yaw_rotation() {
        let yaw = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let end = segment_end_world(Vec3::ZERO, yaw, 10.0);
        assert!((end.x - 10.0).abs() < 1e-4);
        assert!(end.z.abs() < 1e-4);
    }

    #[test]
    fn smoke_route_has_dyntrack_on_e1() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        let dyntrack = scene
            .items
            .iter()
            .find(|o| o.kind == "Dyntrack")
            .expect("dyntrack");
        assert!((dyntrack.position.x - 80.0).abs() < 0.1);
        assert!((dyntrack.position.z - 0.8).abs() < 0.1);
    }
}
