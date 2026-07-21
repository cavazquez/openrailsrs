//! Shared `.tdb` vector-graph geometry (SSOT for viewer3d + render3d).
//!
//! - [`geometry`]: TrackSection spans, poses, nearest-track
//! - [`collect`]: chord / path collection via injectable [`FocusQuery`]
//! - [`ukfs`]: UKFS placement + procedural fallback filter
//! - [`transforms`]: metric tile / scene XZ helpers + ribbon clip

mod collect;
mod focus;
mod geometry;
mod transforms;
mod ukfs;

pub use collect::{
    AnchorPoint, collect_tdb_chords, collect_tdb_path_segments, inter_node_junction_gap_m,
    nearest_oriented_anchor, vector_oriented_anchors,
};
pub use focus::{ChordCollectLimits, FocusQuery};
pub use geometry::*;
pub use transforms::{
    MSTS_TILE_SIZE_M, clip_segment_to_box, ribbon_scene_segment, scene_xz_to_world,
    world_to_scene_xz, world_to_tile_local, world_to_tile_local_centered,
};
pub use ukfs::{
    UkfsWorldPlacement, procedural_fallback_shaped_chords, route_has_ukfs_tsection,
    shaped_chords_from_tdb, shape_uses_ukfs_mesh, ukfs_placements_world,
};

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::prelude::Vec3;

use crate::spawn::dyntrack::ProceduralTrackSegment as DynProceduralTrackSegment;

/// Stable fingerprint of chord endpoints + indices (cm quantization).
pub fn tdb_chord_geometry_hash(chords: &[TdbChord]) -> u64 {
    let mut h = DefaultHasher::new();
    chords.len().hash(&mut h);
    for c in chords {
        quant_cm(c.start_world).hash(&mut h);
        quant_cm(c.end_world).hash(&mut h);
        c.node_id.hash(&mut h);
        c.section_index.hash(&mut h);
        c.span_index.hash(&mut h);
        c.shape_idx.hash(&mut h);
    }
    h.finish()
}

/// Stable fingerprint of procedural segment poses (cm + millidegree yaw).
pub fn tdb_segment_geometry_hash(segments: &[DynProceduralTrackSegment]) -> u64 {
    let mut h = DefaultHasher::new();
    segments.len().hash(&mut h);
    for s in segments {
        quant_cm(s.position).hash(&mut h);
        let (yaw, _, _) = s.rotation.to_euler(bevy::math::EulerRot::YXZ);
        ((yaw.to_degrees() * 1000.0).round() as i32).hash(&mut h);
        s.length_m
            .map(|l| (l * 100.0).round() as i32)
            .unwrap_or(0)
            .hash(&mut h);
    }
    h.finish()
}

fn quant_cm(v: Vec3) -> (i32, i32, i32) {
    (
        (v.x * 100.0).round() as i32,
        (v.y * 100.0).round() as i32,
        (v.z * 100.0).round() as i32,
    )
}

#[cfg(test)]
mod hash_tests {
    use super::*;

    #[test]
    fn chord_hash_stable_for_same_geometry() {
        let c = TdbChord {
            node_id: 1,
            section_index: 0,
            span_index: 0,
            shape_idx: 9,
            start_world: Vec3::new(1.234, 0.0, 5.678),
            end_world: Vec3::new(10.0, 0.0, 5.678),
            curve_radius_m: None,
            curve_angle_deg: None,
        };
        assert_eq!(
            tdb_chord_geometry_hash(&[c]),
            tdb_chord_geometry_hash(&[c])
        );
    }
}
