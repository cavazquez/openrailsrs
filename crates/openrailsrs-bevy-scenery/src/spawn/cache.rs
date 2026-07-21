//! Session-scoped shape GPU cache shared by viewer3d and render3d (#114).
//!
//! Tracks hit/miss/eviction telemetry and per-tile references so unload can drop
//! unreferenced entries without each app reinventing the bookkeeping. Value types
//! stay app-specific via [`SessionShapeCache`]'s type parameters
//! (`ShapeRenderAsset` vs `Vec<PartHandles>`, etc.).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub use crate::stream::TileCoord;

/// Discriminator for a cached shape variant (identity + LOD + material env).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeCacheKey {
    pub shape: String,
    pub lod: String,
    pub material: String,
}

impl ShapeCacheKey {
    pub fn new(
        shape: impl Into<String>,
        lod: impl Into<String>,
        material: impl Into<String>,
    ) -> Self {
        Self {
            shape: shape.into(),
            lod: lod.into(),
            material: material.into(),
        }
    }

    /// Key with empty LOD/material (primary-asset path keyed only by shape identity).
    pub fn shape_only(shape: impl Into<String>) -> Self {
        Self::new(shape, "", "")
    }
}

impl std::fmt::Display for ShapeCacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:lod={}:{}", self.shape, self.lod, self.material)
    }
}

/// Aggregate hit / miss / eviction counters for a session cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionShapeCacheTelemetry {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

#[derive(Clone, Debug)]
struct CacheEntry<V> {
    value: V,
    tiles: HashSet<TileCoord>,
    /// True once any tile has retained this entry (avoids evicting fresh inserts).
    ever_retained: bool,
}

/// Generic session shape cache: keys + values are app-specific; ref/telemetry are shared.
#[derive(Clone, Debug)]
pub struct SessionShapeCache<K, V>
where
    K: Clone + Eq + Hash,
{
    entries: HashMap<K, CacheEntry<V>>,
    tile_keys: HashMap<TileCoord, HashSet<K>>,
    telemetry: SessionShapeCacheTelemetry,
}

impl<K, V> Default for SessionShapeCache<K, V>
where
    K: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            tile_keys: HashMap::new(),
            telemetry: SessionShapeCacheTelemetry::default(),
        }
    }
}

impl<K, V> SessionShapeCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn telemetry(&self) -> &SessionShapeCacheTelemetry {
        &self.telemetry
    }

    pub fn hits(&self) -> u64 {
        self.telemetry.hits
    }

    pub fn misses(&self) -> u64 {
        self.telemetry.misses
    }

    pub fn evictions(&self) -> u64 {
        self.telemetry.evictions
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Peek without updating telemetry.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|e| &e.value)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries.get_mut(key).map(|e| &mut e.value)
    }

    /// Lookup that counts as a cache hit when present.
    pub fn get_hit(&mut self, key: &K) -> Option<&V> {
        if self.entries.contains_key(key) {
            self.telemetry.hits = self.telemetry.hits.saturating_add(1);
            self.entries.get(key).map(|e| &e.value)
        } else {
            None
        }
    }

    pub fn record_miss(&mut self) {
        self.telemetry.misses = self.telemetry.misses.saturating_add(1);
    }

    /// Hit when present; otherwise insert `f()` and count a miss.
    pub fn get_or_insert_with(&mut self, key: K, f: impl FnOnce() -> V) -> &V {
        match self.entries.entry(key) {
            Entry::Occupied(o) => {
                self.telemetry.hits = self.telemetry.hits.saturating_add(1);
                &o.into_mut().value
            }
            Entry::Vacant(v) => {
                self.telemetry.misses = self.telemetry.misses.saturating_add(1);
                &v.insert(CacheEntry {
                    value: f(),
                    tiles: HashSet::new(),
                    ever_retained: false,
                })
                .value
            }
        }
    }

    /// Insert or replace value; preserves existing tile refs when replacing.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.entries.entry(key) {
            Entry::Occupied(mut o) => Some(std::mem::replace(&mut o.get_mut().value, value)),
            Entry::Vacant(v) => {
                v.insert(CacheEntry {
                    value,
                    tiles: HashSet::new(),
                    ever_retained: false,
                });
                None
            }
        }
    }

    pub fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
    {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }

    /// Remove without counting an eviction (manual drop / remap).
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let entry = self.entries.remove(key)?;
        self.unlink_tiles(key, &entry.tiles);
        Some(entry.value)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.values().map(|e| &e.value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, e)| (k, &e.value))
    }

    /// Mark `tile` as using `key` (no-op if key is absent).
    pub fn retain_for_tile(&mut self, tile: TileCoord, key: &K) {
        let Some(entry) = self.entries.get_mut(key) else {
            return;
        };
        entry.ever_retained = true;
        if entry.tiles.insert(tile) {
            self.tile_keys.entry(tile).or_default().insert(key.clone());
        }
    }

    /// Drop all refs owned by `tile`. Returns keys whose refcount hit zero.
    pub fn release_tile(&mut self, tile: TileCoord) -> Vec<K> {
        let Some(keys) = self.tile_keys.remove(&tile) else {
            return Vec::new();
        };
        let mut zeroed = Vec::new();
        for key in keys {
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.tiles.remove(&tile);
                if entry.tiles.is_empty() {
                    zeroed.push(key);
                }
            }
        }
        zeroed
    }

    /// Evict entries that were retained at least once and now have zero tile refs.
    pub fn evict_unreferenced(&mut self) -> Vec<(K, V)> {
        let stale: Vec<K> = self
            .entries
            .iter()
            .filter(|(_, e)| e.ever_retained && e.tiles.is_empty())
            .map(|(k, _)| k.clone())
            .collect();
        self.take_keys(stale)
    }

    /// Evict every key not present in `live` (viewer/render3d live-handle paths).
    pub fn evict_except(&mut self, live: &HashSet<K>) -> Vec<(K, V)> {
        let stale: Vec<K> = self
            .entries
            .keys()
            .filter(|k| !live.contains(*k))
            .cloned()
            .collect();
        self.take_keys(stale)
    }

    /// Tiles currently referencing `key`.
    pub fn tiles_for(&self, key: &K) -> Option<&HashSet<TileCoord>> {
        self.entries.get(key).map(|e| &e.tiles)
    }

    fn take_keys(&mut self, keys: Vec<K>) -> Vec<(K, V)> {
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(entry) = self.entries.remove(&key) {
                self.unlink_tiles(&key, &entry.tiles);
                self.telemetry.evictions = self.telemetry.evictions.saturating_add(1);
                out.push((key, entry.value));
            }
        }
        out
    }

    fn unlink_tiles(&mut self, key: &K, tiles: &HashSet<TileCoord>) {
        for tile in tiles {
            if let Some(set) = self.tile_keys.get_mut(tile) {
                set.remove(key);
                if set.is_empty() {
                    self.tile_keys.remove(tile);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_insert_tracks_hit_and_miss() {
        let mut cache = SessionShapeCache::<&str, i32>::new();
        assert_eq!(*cache.get_or_insert_with("a", || 1), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.hits(), 0);
        assert_eq!(*cache.get_or_insert_with("a", || 99), 1);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn retain_release_and_evict_unreferenced() {
        let mut cache = SessionShapeCache::new();
        cache.insert(ShapeCacheKey::shape_only("shared.s"), 10u32);
        cache.retain_for_tile(TileCoord::new(0, 0), &ShapeCacheKey::shape_only("shared.s"));
        cache.retain_for_tile(TileCoord::new(1, 0), &ShapeCacheKey::shape_only("shared.s"));

        let zeroed = cache.release_tile(TileCoord::new(0, 0));
        assert!(zeroed.is_empty(), "still referenced by tile (1,0)");
        assert!(cache.evict_unreferenced().is_empty());
        assert!(cache.contains_key(&ShapeCacheKey::shape_only("shared.s")));

        let zeroed = cache.release_tile(TileCoord::new(1, 0));
        assert_eq!(zeroed.len(), 1);
        let evicted = cache.evict_unreferenced();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].1, 10);
        assert!(cache.is_empty());
        assert_eq!(cache.evictions(), 1);
    }

    #[test]
    fn fresh_insert_without_retain_survives_evict_unreferenced() {
        let mut cache = SessionShapeCache::new();
        cache.insert("new.s", 1);
        assert!(cache.evict_unreferenced().is_empty());
        assert!(cache.contains_key(&"new.s"));
    }

    #[test]
    fn evict_except_drops_keys_not_in_live_set() {
        let mut cache = SessionShapeCache::new();
        cache.insert("keep", 1);
        cache.insert("drop", 2);
        let live = HashSet::from(["keep"]);
        let evicted = cache.evict_except(&live);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, "drop");
        assert!(cache.contains_key(&"keep"));
        assert_eq!(cache.evictions(), 1);
    }

    #[test]
    fn eviction_n_cycles_stabilizes_unreferenced_entries() {
        let mut cache = SessionShapeCache::<String, u32>::new();
        const CYCLES: i32 = 8;
        let keep = "keep.s".to_string();
        cache.insert(keep.clone(), 0);

        for cycle in 0..CYCLES {
            let tile = TileCoord::new(cycle, 0);
            let drop = format!("drop-{cycle}.s");
            cache.retain_for_tile(tile, &keep);
            cache.insert(drop.clone(), cycle as u32);
            cache.retain_for_tile(tile, &drop);

            if cycle > 0 {
                cache.release_tile(TileCoord::new(cycle - 1, 0));
                let evicted = cache.evict_unreferenced();
                assert!(
                    evicted
                        .iter()
                        .any(|(k, _)| k == &format!("drop-{}.s", cycle - 1)),
                    "cycle {cycle}: prior drop key should evict"
                );
                assert!(
                    !evicted.iter().any(|(k, _)| k == &keep),
                    "cycle {cycle}: shared keep must not evict while live"
                );
            }
            assert!(cache.contains_key(&keep));
            assert_eq!(cache.len(), 2, "keep + current drop only");
        }

        cache.release_tile(TileCoord::new(CYCLES - 1, 0));
        let final_evicted = cache.evict_unreferenced();
        assert_eq!(final_evicted.len(), 2);
        assert!(cache.is_empty());
        // (CYCLES-1) prior drops + keep + last drop
        assert_eq!(cache.evictions(), CYCLES as u64 + 1);
    }

    #[test]
    fn shape_cache_key_display_encodes_lod_and_material() {
        let key = ShapeCacheKey::new("foo.s", "1", "env=3");
        assert_eq!(key.to_string(), "foo.s:lod=1:env=3");
    }
}
