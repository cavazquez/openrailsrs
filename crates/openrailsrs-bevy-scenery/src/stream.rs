//! Shared tile-window policy: desired set, load/unload hysteresis, and `TileBound`.
//!
//! Apps keep their own spawn/despawn/FSM; this module only decides *which* tiles
//! enter or leave the loaded set (Chebyshev tile indices, Open Rails–style).

use std::collections::HashSet;

use bevy::prelude::*;

/// MSTS / Open Rails terrain tile edge length (metres).
pub const TILE_SIZE_M: f32 = 2048.0;

/// Integer tile grid coordinate (`tile_x`, `tile_z`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCoord {
    pub x: i32,
    pub z: i32,
}

impl TileCoord {
    #[inline]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Chebyshev distance in tile indices: `max(|dx|, |dz|)`.
    #[inline]
    pub fn chebyshev_distance(self, other: Self) -> u32 {
        let dx = (self.x - other.x).unsigned_abs();
        let dz = (self.z - other.z).unsigned_abs();
        dx.max(dz)
    }
}

impl From<(i32, i32)> for TileCoord {
    #[inline]
    fn from(value: (i32, i32)) -> Self {
        Self::new(value.0, value.1)
    }
}

impl From<TileCoord> for (i32, i32) {
    #[inline]
    fn from(value: TileCoord) -> Self {
        (value.x, value.z)
    }
}

/// Generic WORLD/terrain membership marker for unload indexing (#62 / #113).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileBound {
    pub tile_x: i32,
    pub tile_z: i32,
}

impl TileBound {
    #[inline]
    pub const fn new(tile_x: i32, tile_z: i32) -> Self {
        Self { tile_x, tile_z }
    }

    #[inline]
    pub const fn coord(self) -> TileCoord {
        TileCoord::new(self.tile_x, self.tile_z)
    }
}

impl From<TileCoord> for TileBound {
    #[inline]
    fn from(c: TileCoord) -> Self {
        Self::new(c.x, c.z)
    }
}

impl From<TileBound> for TileCoord {
    #[inline]
    fn from(b: TileBound) -> Self {
        b.coord()
    }
}

/// Load radius + unload hysteresis in Chebyshev tile indices.
///
/// - **Load** a candidate when `chebyshev(center, tile) <= load_radius`.
/// - **Keep** an already-loaded tile while `chebyshev <= unload_radius`
///   (`load_radius + unload_hysteresis`).
/// - **Unload** only when beyond `unload_radius` (avoids thrash at the boundary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamWindowPolicy {
    pub load_radius: u32,
    pub unload_hysteresis: u32,
}

impl StreamWindowPolicy {
    #[inline]
    pub const fn chebyshev(load_radius: u32, unload_hysteresis: u32) -> Self {
        Self {
            load_radius,
            unload_hysteresis,
        }
    }

    /// Map metre radii to Chebyshev tile radii (`ceil(m / tile_size)`).
    #[inline]
    pub fn chebyshev_from_meters(
        load_radius_m: f32,
        unload_hysteresis_m: f32,
        tile_size_m: f32,
    ) -> Self {
        Self::chebyshev(
            meters_to_tile_radius(load_radius_m, tile_size_m),
            meters_to_tile_radius(unload_hysteresis_m, tile_size_m),
        )
    }

    /// Convenience: load metres + hysteresis metres at [`TILE_SIZE_M`].
    #[inline]
    pub fn chebyshev_from_meters_default_tile(
        load_radius_m: f32,
        unload_hysteresis_m: f32,
    ) -> Self {
        Self::chebyshev_from_meters(load_radius_m, unload_hysteresis_m, TILE_SIZE_M)
    }

    #[inline]
    pub fn unload_radius(self) -> u32 {
        self.load_radius.saturating_add(self.unload_hysteresis)
    }

    #[inline]
    pub fn should_load(self, center: TileCoord, tile: TileCoord) -> bool {
        center.chebyshev_distance(tile) <= self.load_radius
    }

    #[inline]
    pub fn should_keep_loaded(self, center: TileCoord, tile: TileCoord) -> bool {
        center.chebyshev_distance(tile) <= self.unload_radius()
    }

    /// All tile coords in the inclusive Chebyshev square around `center`.
    pub fn chebyshev_disk(center: TileCoord, radius: u32) -> impl Iterator<Item = TileCoord> {
        let r = radius as i32;
        (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| TileCoord::new(center.x + dx, center.z + dz)))
    }

    /// Candidates inside the load window (order not guaranteed).
    pub fn desired_set(
        self,
        center: TileCoord,
        candidates: impl IntoIterator<Item = TileCoord>,
    ) -> HashSet<TileCoord> {
        candidates
            .into_iter()
            .filter(|t| self.should_load(center, *t))
            .collect()
    }

    /// Diff load/unload against `loaded` using hysteresis.
    ///
    /// - `to_load`: in `candidates`, inside load radius, not yet loaded
    /// - `to_unload`: currently loaded and beyond unload radius
    ///
    /// Tiles in the hysteresis band stay loaded and are not listed in either set.
    pub fn diff(
        self,
        center: TileCoord,
        loaded: &HashSet<TileCoord>,
        candidates: impl IntoIterator<Item = TileCoord>,
    ) -> StreamDiff {
        let desired = self.desired_set(center, candidates);
        let mut to_load: Vec<TileCoord> = desired
            .iter()
            .copied()
            .filter(|t| !loaded.contains(t))
            .collect();
        let mut to_unload: Vec<TileCoord> = loaded
            .iter()
            .copied()
            .filter(|t| !self.should_keep_loaded(center, *t))
            .collect();
        to_load.sort_unstable();
        to_unload.sort_unstable();
        StreamDiff { to_load, to_unload }
    }

    /// Like [`Self::diff`], but candidates are the Chebyshev load disk (no catalog filter).
    pub fn diff_disk(self, center: TileCoord, loaded: &HashSet<TileCoord>) -> StreamDiff {
        self.diff(center, loaded, Self::chebyshev_disk(center, self.load_radius))
    }
}

/// Result of comparing the desired window to the currently loaded set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamDiff {
    pub to_load: Vec<TileCoord>,
    pub to_unload: Vec<TileCoord>,
}

/// Convert a metre radius to an inclusive Chebyshev tile radius.
#[inline]
pub fn meters_to_tile_radius(meters: f32, tile_size_m: f32) -> u32 {
    if !meters.is_finite() || meters <= 0.0 || !tile_size_m.is_finite() || tile_size_m <= 0.0 {
        return 0;
    }
    (meters / tile_size_m).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(tiles: &[(i32, i32)]) -> HashSet<TileCoord> {
        tiles.iter().copied().map(TileCoord::from).collect()
    }

    #[test]
    fn chebyshev_distance_is_max_abs() {
        let a = TileCoord::new(0, 0);
        assert_eq!(a.chebyshev_distance(TileCoord::new(2, 1)), 2);
        assert_eq!(a.chebyshev_distance(TileCoord::new(-3, 2)), 3);
    }

    #[test]
    fn meters_to_tiles_ceils() {
        assert_eq!(meters_to_tile_radius(2048.0, TILE_SIZE_M), 1);
        assert_eq!(meters_to_tile_radius(2048.1, TILE_SIZE_M), 2);
        assert_eq!(meters_to_tile_radius(1024.0, TILE_SIZE_M), 1);
        assert_eq!(meters_to_tile_radius(0.0, TILE_SIZE_M), 0);
    }

    #[test]
    fn desired_set_matches_load_radius() {
        let policy = StreamWindowPolicy::chebyshev(1, 1);
        let center = TileCoord::new(10, 20);
        let candidates = [
            TileCoord::new(10, 20),
            TileCoord::new(11, 20),
            TileCoord::new(12, 20),
            TileCoord::new(10, 22),
        ];
        let desired = policy.desired_set(center, candidates);
        assert!(desired.contains(&TileCoord::new(10, 20)));
        assert!(desired.contains(&TileCoord::new(11, 20)));
        assert!(!desired.contains(&TileCoord::new(12, 20)));
        assert!(!desired.contains(&TileCoord::new(10, 22)));
    }

    #[test]
    fn hysteresis_keeps_band_loaded_without_reloading() {
        // load ≤1, unload ≤2 (hysteresis 1)
        let policy = StreamWindowPolicy::chebyshev(1, 1);
        let center = TileCoord::new(0, 0);
        let loaded = set(&[(0, 0), (2, 0)]); // (2,0) is in hysteresis band
        let candidates = StreamWindowPolicy::chebyshev_disk(center, 3);
        let diff = policy.diff(center, &loaded, candidates);

        assert!(!diff.to_unload.contains(&TileCoord::new(2, 0)));
        assert!(!diff.to_load.contains(&TileCoord::new(2, 0)));
        // Far tile must unload.
        let loaded_far = set(&[(0, 0), (3, 0)]);
        let diff_far = policy.diff_disk(center, &loaded_far);
        assert_eq!(diff_far.to_unload, vec![TileCoord::new(3, 0)]);
        // Neighbour inside load radius still loads.
        assert!(diff.to_load.contains(&TileCoord::new(1, 0)));
    }

    #[test]
    fn diff_load_and_unload_sets() {
        let policy = StreamWindowPolicy::chebyshev(1, 0);
        let center = TileCoord::new(0, 0);
        let loaded = set(&[(0, 0), (5, 5)]);
        let diff = policy.diff_disk(center, &loaded);
        assert!(diff.to_unload.contains(&TileCoord::new(5, 5)));
        assert!(!diff.to_unload.contains(&TileCoord::new(0, 0)));
        assert!(diff.to_load.contains(&TileCoord::new(1, 0)));
        assert!(diff.to_load.contains(&TileCoord::new(0, 1)));
        assert!(!diff.to_load.contains(&TileCoord::new(0, 0)));
    }

    #[test]
    fn equivalent_chebyshev_policies_same_desired() {
        let a = StreamWindowPolicy::chebyshev(2, 1);
        let b = StreamWindowPolicy::chebyshev_from_meters(4096.0, 2048.0, TILE_SIZE_M);
        assert_eq!(a, b);
        let center = TileCoord::new(0, 0);
        let candidates: Vec<_> = StreamWindowPolicy::chebyshev_disk(center, 4).collect();
        assert_eq!(
            a.desired_set(center, candidates.iter().copied()),
            b.desired_set(center, candidates.iter().copied())
        );
    }

    #[test]
    fn zero_hysteresis_unloads_at_load_boundary() {
        let policy = StreamWindowPolicy::chebyshev(1, 0);
        let center = TileCoord::new(0, 0);
        let loaded = set(&[(0, 0), (2, 0)]);
        let diff = policy.diff_disk(center, &loaded);
        assert_eq!(diff.to_unload, vec![TileCoord::new(2, 0)]);
    }
}
