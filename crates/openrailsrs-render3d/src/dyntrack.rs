//! Via dinamica MSTS (`Dyntrack` en `.w`): durmientes + rieles procedurales.
//!
//! Geometry lives in [`openrailsrs_bevy_scenery::spawn::dyntrack`]; this module
//! wires render3d-specific spawn (materials_lit, [`TileContent`]).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_formats::TSectionCatalog;
use openrailsrs_or_shader::coordinates::msts_local_offset_to_bevy;

use crate::objects::{ObjectKind, ObjectMarker};
use crate::stream::TileContent;

pub use openrailsrs_bevy_scenery::spawn::dyntrack::{
    DyntrackDimensions, MSTS_DEFAULT_SECTION_LENGTH_M, MSTS_STANDARD_HALF_GAUGE_M,
    ProceduralTrackSegment, ProceduralTrackStyle, append_procedural_track_segment, arc_local_frame,
    dyntrack_dimensions_from_edge_radius, msts_track_visual_dims, part_transform,
    procedural_segment_visual_dims, segment_end_world, sleeper_local_z_positions,
    sleeper_path_distances,
};

const COLOR_SLEEPER: Color = Color::srgb(0.20, 0.14, 0.10);
const COLOR_RAIL: Color = Color::srgb(0.78, 0.86, 0.98);

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

/// Durmientes + rieles fusionados (geometria canónica + materiales / tile de render3d).
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
    spawn_procedural_track_batch_styled(
        commands,
        meshes,
        materials,
        segments,
        materials_lit,
        tile_x,
        tile_z,
        label,
        ProceduralTrackStyle::Full,
    )
}

/// Same as [`spawn_procedural_track_batch`] with an explicit draw style.
#[allow(clippy::too_many_arguments)]
pub fn spawn_procedural_track_batch_styled(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    segments: &[ProceduralTrackSegment],
    materials_lit: bool,
    tile_x: i32,
    tile_z: i32,
    label: &str,
    style: ProceduralTrackStyle,
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
    let rail_material = materials.add(if style == ProceduralTrackStyle::RailsOnly {
        StandardMaterial {
            base_color: Color::srgb(0.35, 0.38, 0.42),
            emissive: if materials_lit {
                LinearRgba::new(0.0, 0.0, 0.0, 1.0)
            } else {
                LinearRgba::new(0.15, 0.16, 0.18, 1.0)
            },
            perceptual_roughness: 0.35,
            metallic: 0.75,
            unlit: !materials_lit,
            ..default()
        }
    } else {
        StandardMaterial {
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

    for &segment in segments {
        let mut dims = procedural_segment_visual_dims(segment);
        if style == ProceduralTrackStyle::RailsOnly {
            dims.rail_width *= 3.0;
            dims.rail_height *= 2.5;
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
            segment,
            dims,
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
        .flat_map(|obj| {
            openrailsrs_bevy_scenery::spawn::dyntrack::procedural_segments_from_dyntrack_sections(
                obj.position + tile_offset,
                obj.rotation,
                &obj.dyntrack_sections,
            )
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
    fn straight_and_curved_buffers_match_canonical_builder() {
        let straight = ProceduralTrackSegment {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            length_m: Some(25.0),
            half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m: None,
            curve_angle_deg: None,
        };
        let curved = ProceduralTrackSegment {
            position: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            length_m: Some(43.633),
            half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m: Some(500.0),
            curve_angle_deg: Some(-5.0),
        };

        for segment in [straight, curved] {
            let dims = procedural_segment_visual_dims(segment);
            let mut a_sleeper_pos = Vec::new();
            let mut a_sleeper_nrm = Vec::new();
            let mut a_sleeper_uv = Vec::new();
            let mut a_sleeper_idx = Vec::new();
            let mut a_rail_pos = Vec::new();
            let mut a_rail_nrm = Vec::new();
            let mut a_rail_uv = Vec::new();
            let mut a_rail_idx = Vec::new();
            append_procedural_track_segment(
                &mut a_sleeper_pos,
                &mut a_sleeper_nrm,
                &mut a_sleeper_uv,
                &mut a_sleeper_idx,
                &mut a_rail_pos,
                &mut a_rail_nrm,
                &mut a_rail_uv,
                &mut a_rail_idx,
                segment,
                dims,
                ProceduralTrackStyle::Full,
            );
            assert!(!a_sleeper_pos.is_empty());
            assert!(!a_rail_pos.is_empty());
            assert_eq!(a_sleeper_pos.len(), a_sleeper_nrm.len());
            assert_eq!(a_sleeper_pos.len(), a_sleeper_uv.len());
            assert_eq!(a_rail_pos.len(), a_rail_uv.len());
            assert!(!a_sleeper_idx.is_empty());
            assert!(!a_rail_idx.is_empty());
        }
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
