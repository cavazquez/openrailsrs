//! Via dinamica MSTS (`Dyntrack` en `.w`): durmientes + rieles procedurales.
//!
//! Paridad visual con `openrailsrs-viewer3d/src/dyntrack.rs` (sin `tsection.dat` aun).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_formats::TSectionCatalog;

use crate::coords::msts_local_offset_to_bevy;
use crate::objects::{ObjectKind, ObjectMarker};
use crate::stream::TileContent;

const COLOR_SLEEPER: Color = Color::srgb(0.20, 0.14, 0.10);
const COLOR_RAIL: Color = Color::srgb(0.78, 0.86, 0.98);

/// Medio ancho de via UIC (1.435 m).
pub const MSTS_STANDARD_HALF_GAUGE_M: f32 = 0.7175;
/// Longitud de seccion MSTS por defecto cuando no hay `tsection.dat`.
pub const MSTS_DEFAULT_SECTION_LENGTH_M: f32 = 25.0;

/// Dimensiones visuales de un segmento dyntrack.
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

/// Segmento orientado para geometria procedural.
#[derive(Clone, Copy, Debug)]
pub struct ProceduralTrackSegment {
    pub position: Vec3,
    pub rotation: Quat,
    pub length_m: Option<f32>,
    pub half_gauge_m: Option<f32>,
    pub curve_radius_m: Option<f32>,
    pub curve_angle_deg: Option<f32>,
}

impl ProceduralTrackSegment {
    fn is_curved(&self) -> bool {
        matches!(
            (self.curve_radius_m, self.curve_angle_deg),
            (Some(r), Some(a)) if r.abs() > 1e-6 && a.abs() > 1e-6
        )
    }
}

/// Rota el eje local **+Z** (tangente del segmento procedural) hacia `direction`.
pub fn quat_align_positive_z_to(direction: Vec3) -> Quat {
    let dir = direction.normalize_or_zero();
    if dir.length_squared() < 1e-8 {
        return Quat::IDENTITY;
    }
    let dot = Vec3::Z.dot(dir);
    if dot > 0.9999 {
        return Quat::IDENTITY;
    }
    if dot < -0.9999 {
        return Quat::from_rotation_y(std::f32::consts::PI);
    }
    Quat::from_rotation_arc(Vec3::Z, dir)
}

fn procedural_segment_visual_dims(segment: ProceduralTrackSegment) -> DyntrackDimensions {
    msts_track_visual_dims(
        segment.half_gauge_m.unwrap_or(MSTS_STANDARD_HALF_GAUGE_M),
        segment.length_m.unwrap_or(MSTS_DEFAULT_SECTION_LENGTH_M),
    )
}

fn sleeper_path_distances(path_length: f32, spacing: f32) -> Vec<f32> {
    if spacing <= 0.0 || path_length <= 0.0 {
        return Vec::new();
    }
    let mut positions = Vec::new();
    let mut z = spacing * 0.5;
    while z < path_length {
        positions.push(z);
        z += spacing;
    }
    positions
}

fn part_transform(anchor: Vec3, rotation: Quat, local_center: Vec3, scale: Vec3) -> Transform {
    Transform {
        translation: anchor + rotation * local_center,
        rotation,
        scale,
    }
}

/// Posicion + tangente en un arco MSTS (origen al inicio, tangente +Z).
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

#[allow(clippy::too_many_arguments)]
fn append_procedural_track_segment(
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
) {
    let length = segment.length_m.unwrap_or(dims.length);
    let half_gauge = segment.half_gauge_m.unwrap_or(dims.half_gauge);
    let mut seg_dims = dims;
    seg_dims.length = length;
    seg_dims.half_gauge = half_gauge;
    let rail_y = seg_dims.sleeper_height + seg_dims.rail_height * 0.5;
    let half_len = seg_dims.length * 0.5;

    if segment.is_curved() {
        let radius = segment.curve_radius_m.unwrap();
        let angle = segment.curve_angle_deg.unwrap();
        append_curved_segment(
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
        );
        return;
    }

    for local_z in sleeper_path_distances(seg_dims.length, seg_dims.sleeper_spacing) {
        let tf = part_transform(
            segment.position,
            segment.rotation,
            Vec3::new(0.0, seg_dims.sleeper_height * 0.5, local_z),
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
            Vec3::ONE,
        );
    }

    for side in [-half_gauge, half_gauge] {
        let tf = part_transform(
            segment.position,
            segment.rotation,
            Vec3::new(side, rail_y, half_len),
            Vec3::new(seg_dims.rail_width, seg_dims.rail_height, seg_dims.length),
        );
        push_cuboid(rail_pos, rail_nrm, rail_uv, rail_idx, &tf, Vec3::ONE);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_curved_segment(
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
) {
    for distance in sleeper_path_distances(arc_length, seg_dims.sleeper_spacing) {
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
            Vec3::ONE,
        );
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
            push_cuboid(rail_pos, rail_nrm, rail_uv, rail_idx, &tf, Vec3::ONE);
        }
    }
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

/// Segmentos procedurales para un `TrackObj` sin `.s` resoluble (desde `tsection.dat`).
pub fn trackobj_procedural_segments(
    obj: &ObjectMarker,
    tile_offset: Vec3,
    tsection: &TSectionCatalog,
    rotation: Quat,
) -> Vec<ProceduralTrackSegment> {
    if obj.kind != ObjectKind::Track {
        return Vec::new();
    }
    let render_pos = obj.position + tile_offset;
    let Some(shape_idx) = obj.section_idx else {
        return vec![ProceduralTrackSegment {
            position: render_pos,
            rotation,
            length_m: Some(MSTS_DEFAULT_SECTION_LENGTH_M),
            half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m: None,
            curve_angle_deg: None,
        }];
    };
    let links = tsection.procedural_links(shape_idx);
    if links.is_empty() {
        return Vec::new();
    }
    links
        .into_iter()
        .map(|link| {
            let offset = msts_local_offset_to_bevy(
                link.shape_local_offset[0] as f32,
                link.shape_local_offset[1] as f32,
                link.shape_local_offset[2] as f32,
            );
            let link_rot = Quat::from_rotation_y(link.shape_local_yaw_deg.to_radians() as f32);
            ProceduralTrackSegment {
                position: render_pos + rotation * offset,
                rotation: rotation * link_rot,
                length_m: Some(link.dims.length_m as f32),
                half_gauge_m: Some(link.dims.half_gauge_m as f32),
                curve_radius_m: link.dims.curve_radius_m.map(|v| v as f32),
                curve_angle_deg: link.dims.curve_angle_deg.map(|v| v as f32),
            }
        })
        .collect()
}

/// Durmientes + rieles fusionados para una lista de segmentos (TrackObj fallback / Dyntrack).
#[allow(clippy::too_many_arguments)]
pub fn spawn_procedural_track_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    segments: &[ProceduralTrackSegment],
    materials_lit: bool,
    tile_x: i32,
    tile_z: i32,
    label: &str,
) -> usize {
    if segments.is_empty() {
        return 0;
    }
    let count = segments.len();
    let sleeper_material = materials.add(StandardMaterial {
        base_color: COLOR_SLEEPER,
        emissive: if materials_lit {
            LinearRgba::new(0.0, 0.0, 0.0, 1.0)
        } else {
            LinearRgba::new(0.04, 0.03, 0.02, 1.0)
        },
        perceptual_roughness: 0.95,
        unlit: !materials_lit,
        ..default()
    });
    let rail_material = materials.add(StandardMaterial {
        base_color: COLOR_RAIL,
        emissive: if materials_lit {
            LinearRgba::new(0.0, 0.0, 0.0, 1.0)
        } else {
            LinearRgba::from(COLOR_RAIL) * 0.12
        },
        perceptual_roughness: if materials_lit { 0.4 } else { 0.35 },
        metallic: if materials_lit { 0.85 } else { 0.75 },
        unlit: !materials_lit,
        ..default()
    });

    let mut sleeper_pos: Vec<[f32; 3]> = Vec::new();
    let mut sleeper_nrm: Vec<[f32; 3]> = Vec::new();
    let mut sleeper_uv: Vec<[f32; 2]> = Vec::new();
    let mut sleeper_idx: Vec<u32> = Vec::new();
    let mut rail_pos: Vec<[f32; 3]> = Vec::new();
    let mut rail_nrm: Vec<[f32; 3]> = Vec::new();
    let mut rail_uv: Vec<[f32; 2]> = Vec::new();
    let mut rail_idx: Vec<u32> = Vec::new();

    for &segment in segments {
        let dims = procedural_segment_visual_dims(segment);
        append_procedural_track_segment(
            &mut sleeper_pos,
            &mut sleeper_nrm,
            &mut sleeper_uv,
            &mut sleeper_idx,
            &mut rail_pos,
            &mut rail_nrm,
            &mut rail_uv,
            &mut rail_idx,
            segment,
            dims,
        );
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
            TileContent { tile_x, tile_z },
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
            TileContent { tile_x, tile_z },
            Name::new(format!("{label}:rails:{count}")),
        ));
    }

    count
}

/// Spawnea durmientes + rieles fusionados para todos los `Dyntrack` del tile.
#[allow(clippy::too_many_arguments)]
pub fn spawn_tile_dyntrack(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    objects: &[ObjectMarker],
    tile_offset: Vec3,
    materials_lit: bool,
    tile_x: i32,
    tile_z: i32,
) -> usize {
    let segments: Vec<ProceduralTrackSegment> = objects
        .iter()
        .filter(|obj| obj.kind == ObjectKind::Dyntrack)
        .map(|obj| ProceduralTrackSegment {
            position: obj.position + tile_offset,
            rotation: obj.rotation,
            length_m: None,
            half_gauge_m: None,
            curve_radius_m: None,
            curve_angle_deg: None,
        })
        .collect();

    if segments.is_empty() {
        return 0;
    }

    spawn_procedural_track_batch(
        commands,
        meshes,
        materials,
        &segments,
        materials_lit,
        tile_x,
        tile_z,
        "dyntrack",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quat_align_z_follows_uphill() {
        let q = quat_align_positive_z_to(Vec3::new(0.0, 10.0, 100.0));
        let forward = q * Vec3::Z;
        assert!(forward.y > 0.05, "y={}", forward.y);
        assert!(forward.z > 0.9, "z={}", forward.z);
        assert!((forward.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn msts_visual_dims_match_gauge() {
        let dims = msts_track_visual_dims(0.7175, 25.0);
        assert!((dims.half_gauge - 0.7175).abs() < 1e-3);
        assert!((dims.length - 25.0).abs() < 1e-3);
        assert!(dims.sleeper_width > dims.half_gauge * 2.0);
    }

    #[test]
    fn sleepers_repeat_along_segment() {
        let positions = sleeper_path_distances(72.0, 0.55);
        assert!(positions.len() >= 10);
        assert!(*positions.last().unwrap() < 72.0);
    }

    #[test]
    fn arc_end_distance_matches_msts_formula() {
        let radius = 500.0_f32;
        let angle = -5.0_f32;
        let (end, _) = arc_local_frame(radius, angle, 1.0);
        let expected = radius * angle.abs().to_radians();
        assert!((end.length() - expected).abs() < 0.05);
    }

    #[test]
    fn dyntrack_from_smoke_world_tile() {
        use crate::objects::load_objects;
        use std::path::PathBuf;

        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let objs = load_objects(&route_dir, 0, 0, 0.0);
        let count = objs
            .iter()
            .filter(|o| o.kind == ObjectKind::Dyntrack)
            .count();
        assert!(count >= 1, "smoke route should contain dyntrack");
    }
}
