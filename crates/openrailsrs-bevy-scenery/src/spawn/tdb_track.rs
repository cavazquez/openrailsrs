//! `.tdb` vector-graph chord types and pure world-space helpers.
//!
//! Branch walking (`collect_tdb_chords`, junction bridges, …) remains in
//! `openrailsrs-viewer3d::tdb_track` until `RouteFocus` moves here.

use bevy::prelude::*;
use openrailsrs_formats::{
    TSectionCatalog, TrVectorSectionRecord, TrackProceduralLink, TrackVectorPoint,
};

pub use crate::spawn::dyntrack::{
    MSTS_DEFAULT_SECTION_LENGTH_M, MSTS_STANDARD_HALF_GAUGE_M, ProceduralTrackSegment,
};

/// Sentinel `section_index` for junction bridge chords (excluded from intra-node gap stats).
pub const TDB_JUNCTION_BRIDGE_SECTION: usize = usize::MAX;

#[derive(Clone, Copy, Debug)]
pub struct TdbChord {
    pub node_id: u32,
    pub section_index: usize,
    pub shape_idx: u32,
    pub start_world: Vec3,
    pub end_world: Vec3,
}

pub fn chord_heading_and_length(from: Vec3, to: Vec3) -> Option<(f64, f32)> {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    let len = (dx * dx + dz * dz).sqrt();
    if len < 0.5 {
        return None;
    }
    Some((f64::from(dx).atan2(f64::from(dz)).to_degrees(), len))
}

pub fn end_from_heading(start: Vec3, heading_deg: f64, length_m: f32) -> Vec3 {
    let yaw = heading_deg.to_radians() as f32;
    start + Vec3::new(yaw.sin() * length_m, 0.0, yaw.cos() * length_m)
}

pub fn single_section_length(node_length_m: f64, _shape_idx: u32) -> f32 {
    if node_length_m > 0.5 {
        return node_length_m as f32;
    }
    MSTS_DEFAULT_SECTION_LENGTH_M
}

pub fn section_shape_length_m(
    tsection: Option<&TSectionCatalog>,
    shape_idx: u32,
    node_length_m: f64,
    section_count: usize,
) -> f32 {
    if let Some(cat) = tsection {
        if let Some(dims) = cat.procedural_dims(shape_idx) {
            if dims.length_m > 0.5 {
                return dims.length_m as f32;
            }
        }
    }
    if section_count <= 1 {
        return single_section_length(node_length_m, shape_idx);
    }
    if node_length_m > 0.5 {
        return (node_length_m / section_count as f64) as f32;
    }
    MSTS_DEFAULT_SECTION_LENGTH_M
}

pub fn straight_segment_from_tsection_link(
    position: Vec3,
    rotation: Quat,
    length_m: f32,
    link: Option<&TrackProceduralLink>,
) -> ProceduralTrackSegment {
    let half_gauge = link
        .map(|l| l.dims.half_gauge_m as f32)
        .or(Some(MSTS_STANDARD_HALF_GAUGE_M));

    ProceduralTrackSegment {
        position,
        rotation,
        length_m: Some(length_m),
        half_gauge_m: half_gauge,
        curve_radius_m: None,
        curve_angle_deg: None,
    }
}

pub fn section_world_vec3(section: TrVectorSectionRecord, near_hint: Option<Vec3>) -> Vec3 {
    let (dx, _, dz) = section.start.bevy_position();
    let (near_x, near_z) = near_hint.map(|h| (h.x, h.z)).unwrap_or((dx, dz));
    let (x, y, z) = section.bevy_position_nearest_to(
        near_x,
        near_z,
        Some((section.header_tile_x, section.header_tile_z)),
    );
    Vec3::new(x, y, z)
}

pub fn point_world_vec3(
    point: TrackVectorPoint,
    header_tile: (i32, i32),
    near_hint: Option<Vec3>,
) -> Vec3 {
    let (dx, _, dz) = point.bevy_position();
    let (near_x, near_z) = near_hint.map(|h| (h.x, h.z)).unwrap_or((dx, dz));
    let (x, y, z) =
        point.bevy_position_nearest_to(near_x, near_z, Some(header_tile), Some(header_tile));
    Vec3::new(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section_at(x: f64, z: f64, shape_idx: u32) -> TrVectorSectionRecord {
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 0.0,
            z,
        };
        TrVectorSectionRecord {
            shape_idx,
            aux_shape_idx: 0,
            header_tile_x: start.tile_x,
            header_tile_z: start.tile_z,
            start,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
        }
    }

    #[test]
    fn chord_heading_turns_at_right_angle() {
        let (h, len) =
            chord_heading_and_length(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 100.0))
                .unwrap();
        assert!((len - 141.42).abs() < 0.1);
        assert!((h - 45.0).abs() < 0.1);
    }

    #[test]
    fn chained_rebase_keeps_consecutive_chord_endpoints_aligned() {
        let s0 = section_at(0.0, 0.0, 1);
        let s1 = section_at(100.0, 0.0, 2);
        let s2 = section_at(200.0, 0.0, 3);
        let start0 = section_world_vec3(s0, None);
        let end0 = section_world_vec3(s1, Some(start0));
        let end1 = section_world_vec3(s2, Some(end0));
        assert!((end0 - section_world_vec3(s1, Some(start0))).length() < 1e-4);
        assert!((end1 - section_world_vec3(s2, Some(end0))).length() < 1e-4);
    }
}
