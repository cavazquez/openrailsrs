//! MSTS dynamic track segment geometry (sleepers + rails merged meshes).
//!
//! Shared procedural track builders used by viewer3d and render3d. App-specific
//! spawn systems (WORLD tile stream, `.tdb` graph) stay in each frontend.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Dark weathered wood — deliberately far from graph orange (`1.0, 0.667, 0.2`).
const COLOR_SLEEPER: Color = Color::srgb(0.20, 0.14, 0.10);
/// Cool steel so rails read clearly against edge cylinders and brown sleepers.
const COLOR_RAIL: Color = Color::srgb(0.78, 0.86, 0.98);

/// Standard UIC gauge half-width (1.435 m track).
pub const MSTS_STANDARD_HALF_GAUGE_M: f32 = 0.7175;
/// Default MSTS section length when `tsection.dat` has no entry.
pub const MSTS_DEFAULT_SECTION_LENGTH_M: f32 = 25.0;

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

/// Scale dyntrack from a route bbox edge radius (metres).
pub fn dyntrack_dimensions_from_edge_radius(edge_radius: f32) -> DyntrackDimensions {
    let base = edge_radius.max(2.0) * 1.5;
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

/// Realistic sleeper/rail cross-section from track gauge (independent of route bbox).
pub fn msts_track_visual_dims(half_gauge_m: f32, length_m: f32) -> DyntrackDimensions {
    let half_gauge = half_gauge_m.max(0.35);
    let gauge = half_gauge * 2.0;
    DyntrackDimensions {
        length: length_m.max(0.5),
        sleeper_width: gauge * 2.2,
        sleeper_height: 0.14,
        sleeper_spacing: 0.55,
        sleeper_depth: 0.26,
        half_gauge,
        rail_width: 0.072,
        rail_height: 0.16,
    }
}

pub fn procedural_segment_visual_dims(segment: ProceduralTrackSegment) -> DyntrackDimensions {
    msts_track_visual_dims(
        segment.half_gauge_m.unwrap_or(MSTS_STANDARD_HALF_GAUGE_M),
        segment.length_m.unwrap_or(MSTS_DEFAULT_SECTION_LENGTH_M),
    )
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
pub fn segment_end_world(
    position: Vec3,
    rotation: Quat,
    length_m: f32,
    curve_radius_m: Option<f32>,
    curve_angle_deg: Option<f32>,
) -> Vec3 {
    if let (Some(r), Some(a)) = (curve_radius_m, curve_angle_deg) {
        if r.abs() > 1e-6 && a.abs() > 1e-6 {
            let (local, _) = arc_local_frame(r, a, 1.0);
            return position + rotation * local;
        }
    }
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

/// One oriented track segment for merged procedural geometry (dyntrack / TrackObj fallback).
#[derive(Clone, Copy, Debug)]
pub struct ProceduralTrackSegment {
    pub position: Vec3,
    pub rotation: Quat,
    /// When set, overrides default dyntrack segment length from route bounds.
    pub length_m: Option<f32>,
    /// When set, overrides default half-gauge from route bounds.
    pub half_gauge_m: Option<f32>,
    /// MSTS `SectionCurve` radius (metres); paired with [`Self::curve_angle_deg`].
    pub curve_radius_m: Option<f32>,
    /// MSTS `SectionCurve` angle (degrees, signed left/right).
    pub curve_angle_deg: Option<f32>,
}

/// How procedural track meshes are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProceduralTrackStyle {
    #[default]
    Full,
    /// Two rails per segment, no sleepers (clearer for `.tdb` debug).
    RailsOnly,
}

impl ProceduralTrackSegment {
    pub fn is_curved(&self) -> bool {
        matches!(
            (self.curve_radius_m, self.curve_angle_deg),
            (Some(r), Some(a)) if r.abs() > 1e-6 && a.abs() > 1e-6
        )
    }
}

/// Expand WORLD Dyntrack `TrackSections` into chained procedural segments (#87).
///
/// Empty `sections` → one fallback straight of [`MSTS_DEFAULT_SECTION_LENGTH_M`].
pub fn procedural_segments_from_dyntrack_sections(
    start: Vec3,
    rotation: Quat,
    sections: &[openrailsrs_formats::DyntrackSection],
) -> Vec<ProceduralTrackSegment> {
    if sections.is_empty() {
        return vec![ProceduralTrackSegment {
            position: start,
            rotation,
            length_m: None,
            half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m: None,
            curve_angle_deg: None,
        }];
    }
    let mut out = Vec::with_capacity(sections.len());
    let mut pos = start;
    let mut rot = rotation;
    for sec in sections {
        let travel = sec.travel_length_m();
        if travel < 0.05 {
            continue;
        }
        let (curve_radius_m, curve_angle_deg) = if sec.is_curve() {
            (sec.curve_radius_m(), sec.curve_angle_deg())
        } else {
            (None, None)
        };
        out.push(ProceduralTrackSegment {
            position: pos,
            rotation: rot,
            length_m: Some(travel),
            half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m,
            curve_angle_deg,
        });
        pos = segment_end_world(pos, rot, travel, curve_radius_m, curve_angle_deg);
        if let (Some(r), Some(a)) = (curve_radius_m, curve_angle_deg) {
            let (_, drot) = arc_local_frame(r, a, 1.0);
            rot *= drot;
        }
    }
    if out.is_empty() {
        return procedural_segments_from_dyntrack_sections(start, rotation, &[]);
    }
    out
}

/// Local position + tangent rotation along an MSTS circular arc (start at origin, tangent +Z).
pub fn arc_local_frame(radius_m: f32, total_angle_deg: f32, fraction: f32) -> (Vec3, Quat) {
    let theta_rad = total_angle_deg.to_radians();
    let r = radius_m.abs();
    let sign = if total_angle_deg >= 0.0 { 1.0 } else { -1.0 };
    let center = Vec3::new(sign * r, 0.0, 0.0);
    let phi = theta_rad * fraction.clamp(0.0, 1.0);
    let from_center = Vec3::new(-sign * r, 0.0, 0.0);
    let rotated = Quat::from_rotation_y(-phi) * from_center;
    let pos = center + rotated;
    (pos, Quat::from_rotation_y(-phi))
}

/// Distances along a centreline for repeated sleepers (straight or curved).
pub fn sleeper_path_distances(path_length: f32, spacing: f32) -> Vec<f32> {
    sleeper_local_z_positions(path_length, spacing)
}

/// Append sleepers + rails for one segment into aggregate mesh buffers.
#[allow(clippy::too_many_arguments)]
pub fn append_procedural_track_segment(
    sleeper_pos: &mut Vec<[f32; 3]>,
    sleeper_nrm: &mut Vec<[f32; 3]>,
    sleeper_uv: &mut Vec<[f32; 2]>,
    sleeper_idx: &mut Vec<u32>,
    rail_pos: &mut Vec<[f32; 3]>,
    rail_nrm: &mut Vec<[f32; 3]>,
    rail_uv: &mut Vec<[f32; 2]>,
    rail_idx: &mut Vec<u32>,
    segment: ProceduralTrackSegment,
    dims: DyntrackDimensions,
    style: ProceduralTrackStyle,
) {
    let length = segment.length_m.unwrap_or(dims.length);
    let half_gauge = segment.half_gauge_m.unwrap_or(dims.half_gauge);
    let mut seg_dims = dims;
    seg_dims.length = length;
    seg_dims.half_gauge = half_gauge;
    let rail_y = seg_dims.sleeper_height + seg_dims.rail_height * 0.5;
    let draw_sleepers = style == ProceduralTrackStyle::Full;

    if segment.is_curved() {
        let radius = segment.curve_radius_m.unwrap();
        let angle = segment.curve_angle_deg.unwrap();
        append_curved_procedural_segment(
            sleeper_pos,
            sleeper_nrm,
            sleeper_uv,
            sleeper_idx,
            rail_pos,
            rail_nrm,
            rail_uv,
            rail_idx,
            segment,
            &seg_dims,
            radius,
            angle,
            length,
            half_gauge,
            rail_y,
            draw_sleepers,
        );
        return;
    }

    let half_len = seg_dims.length * 0.5;

    if draw_sleepers {
        let sleeper_positions = sleeper_path_distances(seg_dims.length, seg_dims.sleeper_spacing);
        for local_z in &sleeper_positions {
            let tf = part_transform(
                segment.position,
                segment.rotation,
                Vec3::new(0.0, seg_dims.sleeper_height * 0.5, *local_z),
                Vec3::new(
                    seg_dims.sleeper_width,
                    seg_dims.sleeper_height,
                    seg_dims.sleeper_depth,
                ),
            );
            push_cuboid(
                sleeper_pos,
                sleeper_nrm,
                sleeper_uv,
                sleeper_idx,
                &tf,
                Vec3::splat(1.0),
            );
        }
    }

    if style == ProceduralTrackStyle::RailsOnly {
        for side in [-half_gauge, half_gauge] {
            let tf = part_transform(
                segment.position,
                segment.rotation,
                Vec3::new(side, rail_y, half_len),
                Vec3::new(seg_dims.rail_width, seg_dims.rail_height, seg_dims.length),
            );
            push_cuboid(rail_pos, rail_nrm, rail_uv, rail_idx, &tf, Vec3::splat(1.0));
        }
        return;
    }

    for side in [-half_gauge, half_gauge] {
        let tf = part_transform(
            segment.position,
            segment.rotation,
            Vec3::new(side, rail_y, half_len),
            Vec3::new(seg_dims.rail_width, seg_dims.rail_height, seg_dims.length),
        );
        push_cuboid(rail_pos, rail_nrm, rail_uv, rail_idx, &tf, Vec3::splat(1.0));
    }
}

#[allow(clippy::too_many_arguments)]
fn append_curved_procedural_segment(
    sleeper_pos: &mut Vec<[f32; 3]>,
    sleeper_nrm: &mut Vec<[f32; 3]>,
    sleeper_uv: &mut Vec<[f32; 2]>,
    sleeper_idx: &mut Vec<u32>,
    rail_pos: &mut Vec<[f32; 3]>,
    rail_nrm: &mut Vec<[f32; 3]>,
    rail_uv: &mut Vec<[f32; 2]>,
    rail_idx: &mut Vec<u32>,
    segment: ProceduralTrackSegment,
    seg_dims: &DyntrackDimensions,
    radius_m: f32,
    angle_deg: f32,
    arc_length: f32,
    half_gauge: f32,
    rail_y: f32,
    draw_sleepers: bool,
) {
    if draw_sleepers {
        let sleeper_distances = sleeper_path_distances(arc_length, seg_dims.sleeper_spacing);
        for distance in sleeper_distances {
            let fraction = distance / arc_length;
            let (local_pos, local_rot) = arc_local_frame(radius_m, angle_deg, fraction);
            let tf = part_transform(
                segment.position,
                segment.rotation * local_rot,
                local_pos + Vec3::new(0.0, seg_dims.sleeper_height * 0.5, 0.0),
                Vec3::new(
                    seg_dims.sleeper_width,
                    seg_dims.sleeper_height,
                    seg_dims.sleeper_depth,
                ),
            );
            push_cuboid(
                sleeper_pos,
                sleeper_nrm,
                sleeper_uv,
                sleeper_idx,
                &tf,
                Vec3::splat(1.0),
            );
        }
    }

    let rail_pieces = ((arc_length / 2.0).ceil() as usize).clamp(1, 256);
    let piece_len = arc_length / rail_pieces as f32;
    for piece in 0..rail_pieces {
        let f_mid = (piece as f32 + 0.5) / rail_pieces as f32;
        let (local_pos, local_rot) = arc_local_frame(radius_m, angle_deg, f_mid);
        for side in [-half_gauge, half_gauge] {
            let tf = part_transform(
                segment.position,
                segment.rotation * local_rot,
                local_pos + local_rot * Vec3::new(side, rail_y, 0.0),
                Vec3::new(seg_dims.rail_width, seg_dims.rail_height, piece_len),
            );
            push_cuboid(rail_pos, rail_nrm, rail_uv, rail_idx, &tf, Vec3::splat(1.0));
        }
    }
}

/// Spawn merged sleepers + rails for arbitrary oriented segments.
pub fn spawn_procedural_track_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    segments: &[ProceduralTrackSegment],
    label: &str,
    style: ProceduralTrackStyle,
) {
    if segments.is_empty() {
        return;
    }

    let count = segments.len();
    let sleeper_material = materials.add(StandardMaterial {
        base_color: COLOR_SLEEPER,
        perceptual_roughness: 0.92,
        metallic: 0.02,
        ..default()
    });
    let rail_material = materials.add(if style == ProceduralTrackStyle::RailsOnly {
        StandardMaterial {
            base_color: Color::srgb(0.35, 0.38, 0.42),
            emissive: LinearRgba::new(0.15, 0.16, 0.18, 1.0),
            perceptual_roughness: 0.35,
            metallic: 0.75,
            ..default()
        }
    } else {
        StandardMaterial {
            base_color: COLOR_RAIL,
            emissive: LinearRgba::from(COLOR_RAIL) * 0.08,
            perceptual_roughness: 0.35,
            metallic: 0.75,
            ..default()
        }
    });

    let mut sleeper_pos: Vec<[f32; 3]> = Vec::new();
    let mut sleeper_nrm: Vec<[f32; 3]> = Vec::new();
    let mut sleeper_uv: Vec<[f32; 2]> = Vec::new();
    let mut sleeper_idx: Vec<u32> = Vec::new();

    let mut rail_pos: Vec<[f32; 3]> = Vec::new();
    let mut rail_nrm: Vec<[f32; 3]> = Vec::new();
    let mut rail_uv: Vec<[f32; 2]> = Vec::new();
    let mut rail_idx: Vec<u32> = Vec::new();

    for segment in segments {
        let mut seg_dims = procedural_segment_visual_dims(*segment);
        if style == ProceduralTrackStyle::RailsOnly {
            seg_dims.rail_width *= 3.0;
            seg_dims.rail_height *= 2.5;
        }
        append_procedural_track_segment(
            &mut sleeper_pos,
            &mut sleeper_nrm,
            &mut sleeper_uv,
            &mut sleeper_idx,
            &mut rail_pos,
            &mut rail_nrm,
            &mut rail_uv,
            &mut rail_idx,
            *segment,
            seg_dims,
            style,
        );
    }

    if style == ProceduralTrackStyle::Full && !sleeper_pos.is_empty() {
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
            Visibility::default(),
            Name::new(format!("{label}:sleepers:{count}")),
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
            Visibility::default(),
            Name::new(format!("{label}:rails:{count}")),
        ));
    }
}

/// Spawn one procedural segment as a single rail mesh entity (for mobile TDB streaming).
pub fn spawn_procedural_track_single(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    _materials: &mut Assets<StandardMaterial>,
    segment: ProceduralTrackSegment,
    label: &str,
    style: ProceduralTrackStyle,
    rail_material: &Handle<StandardMaterial>,
) -> Entity {
    let mut seg_dims = procedural_segment_visual_dims(segment);
    if style == ProceduralTrackStyle::RailsOnly {
        seg_dims.rail_width *= 3.0;
        seg_dims.rail_height *= 2.5;
    }
    let mut rail_pos: Vec<[f32; 3]> = Vec::new();
    let mut rail_nrm: Vec<[f32; 3]> = Vec::new();
    let mut rail_uv: Vec<[f32; 2]> = Vec::new();
    let mut rail_idx: Vec<u32> = Vec::new();
    append_procedural_track_segment(
        &mut Vec::new(),
        &mut Vec::new(),
        &mut Vec::new(),
        &mut Vec::new(),
        &mut rail_pos,
        &mut rail_nrm,
        &mut rail_uv,
        &mut rail_idx,
        segment,
        seg_dims,
        style,
    );
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, rail_pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, rail_nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, rail_uv);
    mesh.insert_indices(Indices::U32(rail_idx));
    commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(rail_material.clone()),
            Transform::IDENTITY,
            Visibility::default(),
            Name::new(format!("{label}:rail")),
        ))
        .id()
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

    #[test]
    fn msts_visual_dims_match_gauge_not_route_bbox() {
        let dims = msts_track_visual_dims(0.7175, 25.0);
        assert!((dims.half_gauge - 0.7175).abs() < 1e-3);
        assert!((dims.length - 25.0).abs() < 1e-3);
        assert!(
            dims.sleeper_width < 3.5,
            "sleepers should be ~3m not route-scaled"
        );
        assert!(dims.sleeper_width > dims.half_gauge * 2.0);
    }

    #[test]
    fn dimensions_scale_with_edge_radius() {
        let small = dyntrack_dimensions_from_edge_radius(2.0);
        assert!(small.length > 5.0);

        let large = dyntrack_dimensions_from_edge_radius(7_500.0);
        assert!(large.length > small.length);
        assert!(large.half_gauge > small.half_gauge);
    }

    #[test]
    fn sleepers_repeat_along_segment() {
        let positions = sleeper_path_distances(72.0, 4.8);
        assert!(positions.len() >= 10);
        assert!((positions[0] - 2.4).abs() < 1e-4);
        assert!(*positions.last().unwrap() < 72.0);
    }

    #[test]
    fn segment_extends_along_local_z() {
        let end = segment_end_world(Vec3::new(10.0, 0.0, 5.0), Quat::IDENTITY, 20.0, None, None);
        assert_eq!(end, Vec3::new(10.0, 0.0, 25.0));
    }

    #[test]
    fn segment_respects_yaw_rotation() {
        let yaw = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let end = segment_end_world(Vec3::ZERO, yaw, 10.0, None, None);
        assert!((end.x - 10.0).abs() < 1e-4);
        assert!(end.z.abs() < 1e-4);
    }

    #[test]
    fn arc_end_distance_matches_msts_formula() {
        let radius = 500.0_f32;
        let angle = -5.0_f32;
        let (end, _) = arc_local_frame(radius, angle, 1.0);
        let expected = radius * angle.abs().to_radians();
        assert!(
            (end.length() - expected).abs() < 0.05,
            "got {}",
            end.length()
        );
    }

    #[test]
    fn arc_start_is_origin_facing_plus_z() {
        let (pos, rot) = arc_local_frame(500.0, -5.0, 0.0);
        assert!(pos.length() < 1e-4);
        let tangent = rot * Vec3::Z;
        assert!((tangent.z - 1.0).abs() < 1e-4);
        assert!(tangent.x.abs() < 1e-4);
    }
}
