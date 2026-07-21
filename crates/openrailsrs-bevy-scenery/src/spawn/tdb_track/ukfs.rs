//! UKFS shape placement along TDB chords (world space) and procedural fallback filter.

use bevy::prelude::{Quat, Vec3};
use openrailsrs_formats::TSectionCatalog;

use crate::spawn::tdb_track::geometry::TdbChord;

/// One UKFS shape instance along a chord (world XZ; Y from chord lerp).
#[derive(Clone, Copy, Debug)]
pub struct UkfsWorldPlacement {
    pub shape_idx: u32,
    pub position: Vec3,
    pub rotation: Quat,
}

/// Heuristic: large native UKFS catalogues expose thousands of shapes.
pub fn route_has_ukfs_tsection(tsection: &TSectionCatalog) -> bool {
    tsection.shapes.len() > 500
}

/// True when `shape_idx` should be instanced as a UKFS `.s` along the chord.
pub fn shape_uses_ukfs_mesh(shape_idx: u32, tsection: &TSectionCatalog) -> bool {
    shape_idx != 0
        && !tsection.is_road_shape(shape_idx)
        && tsection.shape_file_name(shape_idx).is_some()
}

/// Chords that should fall back to procedural rail (road / missing shape file).
pub fn procedural_fallback_shaped_chords(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
) -> Vec<(Vec3, Vec3, u32)> {
    shaped_chords
        .iter()
        .copied()
        .filter(|(_, _, shape_idx)| !shape_uses_ukfs_mesh(*shape_idx, tsection))
        .filter(|(_, _, shape_idx)| *shape_idx != 0)
        .collect()
}

/// Place UKFS shapes along world-space chords (no tile / height transform).
pub fn ukfs_placements_world(
    shaped_chords: &[(Vec3, Vec3, u32)],
    tsection: &TSectionCatalog,
    default_section_len_m: f32,
) -> Vec<UkfsWorldPlacement> {
    let mut out = Vec::new();
    for (start, end, shape_idx) in shaped_chords {
        if !shape_uses_ukfs_mesh(*shape_idx, tsection) {
            continue;
        }
        let dx = end.x - start.x;
        let dz = end.z - start.z;
        let chord_len = (dx * dx + dz * dz).sqrt();
        if chord_len < 0.5 {
            continue;
        }
        let dir = Vec3::new(dx / chord_len, 0.0, dz / chord_len);
        let heading = dx.atan2(dz);
        let rot = Quat::from_rotation_y(heading);
        let section_len = tsection
            .procedural_dims(*shape_idx)
            .map(|d| d.length_m as f32)
            .filter(|l| *l > 0.5)
            .unwrap_or(default_section_len_m);
        let mut dist = 0.0f32;
        while dist + 0.25 <= chord_len {
            let t = dist / chord_len;
            let position = Vec3::new(
                start.x + dir.x * dist,
                start.y + (end.y - start.y) * t,
                start.z + dir.z * dist,
            );
            out.push(UkfsWorldPlacement {
                shape_idx: *shape_idx,
                position,
                rotation: rot,
            });
            dist += section_len;
        }
    }
    out
}

/// Map [`TdbChord`] list to `(start, end, shape_idx)` tuples (skips junction bridges if desired).
pub fn shaped_chords_from_tdb(
    chords: &[TdbChord],
    include_junction_bridges: bool,
) -> Vec<(Vec3, Vec3, u32)> {
    use crate::spawn::tdb_track::geometry::TDB_JUNCTION_BRIDGE_SECTION;
    chords
        .iter()
        .filter(|c| include_junction_bridges || c.section_index != TDB_JUNCTION_BRIDGE_SECTION)
        .map(|c| (c.start_world, c.end_world, c.shape_idx))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::typed::{TrackSectionDef, TrackShapeDef, TrackShapePath};

    fn cat_with_shape(idx: u32, file: &str, road: bool) -> TSectionCatalog {
        let mut cat = TSectionCatalog::default();
        cat.sections.insert(
            idx,
            TrackSectionDef {
                gauge_m: 1.435,
                length_m: 25.0,
                curve_radius_m: None,
                curve_angle_deg: None,
                skew_deg: None,
            },
        );
        cat.shapes.insert(
            idx,
            TrackShapeDef {
                file_name: file.into(),
                road_shape: road,
                paths: vec![TrackShapePath {
                    offset: [0.0, 0.0, 0.0],
                    angle_deg: 0.0,
                    num_sections: 1,
                    section_indices: vec![idx],
                }],
                main_route: Some(0),
                clearance_dist_m: None,
            },
        );
        cat
    }

    #[test]
    fn ukfs_placements_step_along_chord() {
        let cat = cat_with_shape(10, "ukfs_s_1x25m.s", false);
        let chords = [(Vec3::ZERO, Vec3::new(0.0, 0.0, 100.0), 10u32)];
        let places = ukfs_placements_world(&chords, &cat, 25.0);
        assert_eq!(places.len(), 4);
        assert!((places[0].position.z - 0.0).abs() < 1e-3);
        assert!((places[1].position.z - 25.0).abs() < 1e-3);
    }

    #[test]
    fn road_shape_is_procedural_fallback() {
        let cat = cat_with_shape(3, "road.s", true);
        assert!(!shape_uses_ukfs_mesh(3, &cat));
        let fb = procedural_fallback_shaped_chords(&[(Vec3::ZERO, Vec3::X * 10.0, 3)], &cat);
        assert_eq!(fb.len(), 1);
    }
}
