//! Procedural overhead contact wire (#36), mirroring Open Rails `Wire.cs`.
//!
//! Extrudes a thin rectangular profile along [`ProceduralTrackSegment`] centreline
//! at `OverheadWireHeight`. Optional messenger wire + droppers when double-wire
//! is enabled in the route `.trk`.

#![allow(clippy::too_many_arguments)]

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::dyntrack::{
    MSTS_DEFAULT_SECTION_LENGTH_M, ProceduralTrackSegment, arc_local_frame, part_transform,
};

/// Contact-wire cross-section (metres) — matches OR `WireProfile` ~2 cm.
const WIRE_HALF_WIDTH_M: f32 = 0.01;
const WIRE_HEIGHT_M: f32 = 0.02;
/// Messenger (upper) wire slightly thinner.
const MESSENGER_HALF_WIDTH_M: f32 = 0.008;
const MESSENGER_HEIGHT_M: f32 = 0.016;
/// Vertical dropper spacing when double-wire is on (OR `expectedSegmentLength`).
const DROPPER_SPACING_M: f32 = 40.0;
const DROPPER_HALF_SIZE_M: f32 = 0.008;

/// Visual parameters for overhead wire generation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverheadWireStyle {
    pub height_m: f32,
    pub double_wire: bool,
    pub double_wire_height_m: f32,
}

impl Default for OverheadWireStyle {
    fn default() -> Self {
        Self {
            height_m: 6.0,
            double_wire: false,
            double_wire_height_m: 1.0,
        }
    }
}

impl OverheadWireStyle {
    pub fn contact_y(self) -> f32 {
        self.height_m
    }

    pub fn messenger_y(self) -> f32 {
        self.height_m + self.double_wire_height_m.max(0.1)
    }
}

/// Append one contact-wire (and optional messenger/droppers) for a track segment.
pub fn append_overhead_wire_segment(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    segment: ProceduralTrackSegment,
    style: OverheadWireStyle,
) {
    let length = segment
        .length_m
        .unwrap_or(MSTS_DEFAULT_SECTION_LENGTH_M)
        .max(0.05);
    if !length.is_finite() || !style.height_m.is_finite() {
        return;
    }

    append_wire_run(
        positions,
        normals,
        uvs,
        indices,
        segment,
        length,
        style.contact_y(),
        WIRE_HALF_WIDTH_M,
        WIRE_HEIGHT_M,
    );

    if style.double_wire {
        let messenger_y = style.messenger_y();
        append_wire_run(
            positions,
            normals,
            uvs,
            indices,
            segment,
            length,
            messenger_y,
            MESSENGER_HALF_WIDTH_M,
            MESSENGER_HEIGHT_M,
        );
        append_droppers(
            positions,
            normals,
            uvs,
            indices,
            segment,
            length,
            style.contact_y(),
            messenger_y,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn append_wire_run(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    segment: ProceduralTrackSegment,
    length: f32,
    y: f32,
    half_width: f32,
    height: f32,
) {
    if segment.is_curved() {
        let radius = segment.curve_radius_m.unwrap();
        let angle = segment.curve_angle_deg.unwrap();
        let pieces = ((length / 2.0).ceil() as usize).clamp(1, 256);
        let piece_len = length / pieces as f32;
        for piece in 0..pieces {
            let f_mid = (piece as f32 + 0.5) / pieces as f32;
            let (local_pos, local_rot) = arc_local_frame(radius, angle, f_mid);
            let tf = part_transform(
                segment.position,
                segment.rotation * local_rot,
                local_pos + local_rot * Vec3::new(0.0, y + height * 0.5, 0.0),
                Vec3::new(half_width * 2.0, height, piece_len),
            );
            push_cuboid(positions, normals, uvs, indices, &tf, Vec3::splat(1.0));
        }
        return;
    }

    let half_len = length * 0.5;
    let tf = part_transform(
        segment.position,
        segment.rotation,
        Vec3::new(0.0, y + height * 0.5, half_len),
        Vec3::new(half_width * 2.0, height, length),
    );
    push_cuboid(positions, normals, uvs, indices, &tf, Vec3::splat(1.0));
}

#[allow(clippy::too_many_arguments)]
fn append_droppers(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    segment: ProceduralTrackSegment,
    length: f32,
    contact_y: f32,
    messenger_y: f32,
) {
    let drop_len = (messenger_y - contact_y).abs().max(0.05);
    let mid_y = (contact_y + messenger_y) * 0.5;
    let mut d = DROPPER_SPACING_M * 0.5;
    while d < length {
        let (local_pos, local_rot) = if segment.is_curved() {
            let radius = segment.curve_radius_m.unwrap();
            let angle = segment.curve_angle_deg.unwrap();
            arc_local_frame(radius, angle, (d / length).clamp(0.0, 1.0))
        } else {
            (Vec3::new(0.0, 0.0, d), Quat::IDENTITY)
        };
        let rot = segment.rotation * local_rot;
        let tf = part_transform(
            segment.position,
            rot,
            local_pos + local_rot * Vec3::new(0.0, mid_y, 0.0),
            Vec3::new(
                DROPPER_HALF_SIZE_M * 2.0,
                drop_len,
                DROPPER_HALF_SIZE_M * 2.0,
            ),
        );
        push_cuboid(positions, normals, uvs, indices, &tf, Vec3::splat(1.0));
        d += DROPPER_SPACING_M;
    }
}

/// Spawn a single merged mesh for many overhead-wire segments.
pub fn spawn_overhead_wire_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    segments: &[ProceduralTrackSegment],
    style: OverheadWireStyle,
    label: &str,
) {
    if segments.is_empty() {
        return;
    }

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for segment in segments {
        append_overhead_wire_segment(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            *segment,
            style,
        );
    }

    if positions.is_empty() {
        return;
    }

    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.12, 0.14),
        emissive: LinearRgba::new(0.04, 0.04, 0.05, 1.0),
        perceptual_roughness: 0.45,
        metallic: 0.85,
        cull_mode: None,
        ..default()
    });

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    let count = segments.len();
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::default(),
        Name::new(format!("{label}:overhead_wire:{count}")),
    ));
}

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

    for (i0, i1, i2, i3, local_n) in face_defs {
        let base = positions.len() as u32;
        let n = (tf.rotation * local_n).normalize_or_zero();
        for idx in [i0, i1, i2, i3] {
            positions.push(world[idx].to_array());
            normals.push(n.to_array());
            uvs.push([0.25, 0.25]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_wire_produces_vertices_above_rail() {
        let mut pos = Vec::new();
        let mut nrm = Vec::new();
        let mut uv = Vec::new();
        let mut idx = Vec::new();
        let segment = ProceduralTrackSegment {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            length_m: Some(25.0),
            half_gauge_m: Some(0.7175),
            curve_radius_m: None,
            curve_angle_deg: None,
        };
        let style = OverheadWireStyle {
            height_m: 6.0,
            ..Default::default()
        };
        append_overhead_wire_segment(&mut pos, &mut nrm, &mut uv, &mut idx, segment, style);
        assert!(!pos.is_empty());
        assert!(!idx.is_empty());
        let ys: Vec<f32> = pos.iter().map(|p| p[1]).collect();
        let min_y = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(min_y > 5.9, "min_y={min_y}");
        assert!(max_y < 6.1, "max_y={max_y}");
        assert!(pos.iter().all(|p| p.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn curved_wire_has_no_nan() {
        let mut pos = Vec::new();
        let mut nrm = Vec::new();
        let mut uv = Vec::new();
        let mut idx = Vec::new();
        let segment = ProceduralTrackSegment {
            position: Vec3::new(10.0, 0.0, -5.0),
            rotation: Quat::from_rotation_y(0.3),
            length_m: Some(31.4),
            half_gauge_m: Some(0.7175),
            curve_radius_m: Some(200.0),
            curve_angle_deg: Some(9.0),
        };
        append_overhead_wire_segment(
            &mut pos,
            &mut nrm,
            &mut uv,
            &mut idx,
            segment,
            OverheadWireStyle::default(),
        );
        assert!(!pos.is_empty());
        assert!(pos.iter().all(|p| p.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn double_wire_adds_messenger_above_contact() {
        let mut pos = Vec::new();
        let mut nrm = Vec::new();
        let mut uv = Vec::new();
        let mut idx = Vec::new();
        let segment = ProceduralTrackSegment {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            length_m: Some(50.0),
            half_gauge_m: None,
            curve_radius_m: None,
            curve_angle_deg: None,
        };
        append_overhead_wire_segment(
            &mut pos,
            &mut nrm,
            &mut uv,
            &mut idx,
            segment,
            OverheadWireStyle {
                height_m: 5.5,
                double_wire: true,
                double_wire_height_m: 1.0,
            },
        );
        let max_y = pos.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
        assert!(max_y > 6.4, "max_y={max_y}");
    }
}
