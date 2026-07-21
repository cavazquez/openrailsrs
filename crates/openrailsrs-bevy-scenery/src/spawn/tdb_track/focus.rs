//! Injectable horizontal focus for TDB chord / path collection.

use bevy::prelude::Vec3;

/// World-space query window: centre + horizontal radius (metres).
///
/// Apps adapt their own focus type (`RouteFocus`, tile centre, …) into this
/// before calling [`super::collect_tdb_chords`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusQuery {
    pub center: Vec3,
    pub radius_m: f32,
}

impl FocusQuery {
    pub fn new(center: Vec3, radius_m: f32) -> Self {
        Self { center, radius_m }
    }

    /// Horizontal (XZ) distance from `center` to `world`.
    pub fn horizontal_distance(&self, world: Vec3) -> f32 {
        let dx = world.x - self.center.x;
        let dz = world.z - self.center.z;
        (dx * dx + dz * dz).sqrt()
    }

    /// True when either endpoint lies inside the radius.
    pub fn reaches_segment(&self, a: Vec3, b: Vec3) -> bool {
        self.horizontal_distance(a) <= self.radius_m || self.horizontal_distance(b) <= self.radius_m
    }

    /// Focus covering a tile of `tile_size_m` plus `margin_m` and optional
    /// `extra_radius_m` (e.g. neighbour grid × tile size).
    pub fn for_tile(
        tile_x: i32,
        tile_z: i32,
        tile_size_m: f32,
        margin_m: f32,
        extra_radius_m: f32,
    ) -> Self {
        Self {
            center: Vec3::new(
                tile_x as f32 * tile_size_m,
                0.0,
                -(tile_z as f32 * tile_size_m),
            ),
            radius_m: tile_size_m * 0.5 + margin_m + extra_radius_m,
        }
    }
}

/// Caps for optional `TrPins` branch walking (viewer `--track-dev`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChordCollectLimits {
    /// When `tdb.nodes.len() <= this`, try branch walk first (0 = never).
    pub branch_walk_max_nodes: usize,
    /// Max branches collected during a walk.
    pub max_branches: usize,
}

impl Default for ChordCollectLimits {
    fn default() -> Self {
        Self {
            branch_walk_max_nodes: 800,
            max_branches: 512,
        }
    }
}

impl ChordCollectLimits {
    /// Per-vector + junction bridges only (render3d tile path).
    pub const PER_VECTOR_ONLY: Self = Self {
        branch_walk_max_nodes: 0,
        max_branches: 0,
    };
}
