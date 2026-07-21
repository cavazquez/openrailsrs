//! Tile → WORLD entity index for O(candidates) unload (#75).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::world::{WorldSceneryLod, WorldTileBound};
use crate::world_instancing::WorldInstancedGroup;

/// Maps WORLD tiles to spawned entities that carry [`WorldTileBound`].
#[derive(Resource, Default, Debug)]
pub struct WorldTileEntityIndex {
    by_tile: HashMap<(i32, i32), Vec<Entity>>,
    entity_tile: HashMap<Entity, (i32, i32)>,
}

impl WorldTileEntityIndex {
    pub fn insert(&mut self, entity: Entity, tile_x: i32, tile_z: i32) {
        let tile = (tile_x, tile_z);
        if let Some(old) = self.entity_tile.insert(entity, tile) {
            if old == tile {
                return;
            }
            self.remove_from_bucket(entity, old);
        }
        self.by_tile.entry(tile).or_default().push(entity);
    }

    pub fn remove_entity(&mut self, entity: Entity) {
        if let Some(tile) = self.entity_tile.remove(&entity) {
            self.remove_from_bucket(entity, tile);
        }
    }

    fn remove_from_bucket(&mut self, entity: Entity, tile: (i32, i32)) {
        let Some(bucket) = self.by_tile.get_mut(&tile) else {
            return;
        };
        bucket.retain(|e| *e != entity);
        if bucket.is_empty() {
            self.by_tile.remove(&tile);
        }
    }

    /// Remove and return all entities indexed on `tiles` (unload candidates).
    pub fn take_tiles(&mut self, tiles: &HashSet<(i32, i32)>) -> Vec<Entity> {
        let mut out = Vec::new();
        for tile in tiles {
            if let Some(bucket) = self.by_tile.remove(tile) {
                for entity in &bucket {
                    self.entity_tile.remove(entity);
                }
                out.extend(bucket);
            }
        }
        out
    }

    /// Entities currently indexed on `tiles` (does not mutate).
    pub fn entities_on_tiles(&self, tiles: &HashSet<(i32, i32)>) -> Vec<Entity> {
        let mut out = Vec::new();
        for tile in tiles {
            if let Some(bucket) = self.by_tile.get(tile) {
                out.extend(bucket.iter().copied());
            }
        }
        out
    }

    pub fn entity_count(&self) -> usize {
        self.entity_tile.len()
    }

    pub fn tile_count(&self) -> usize {
        self.by_tile.len()
    }
}

/// Live WORLD shape-path reference counts for asset eviction without global scans (#75 / #51).
#[derive(Resource, Default, Debug)]
pub struct WorldShapeLiveRefs {
    counts: HashMap<PathBuf, u32>,
    entity_path: HashMap<Entity, PathBuf>,
}

impl WorldShapeLiveRefs {
    pub fn retain_entity(&mut self, entity: Entity, path: &Path) {
        if path.as_os_str().is_empty() {
            return;
        }
        if let Some(old) = self.entity_path.insert(entity, path.to_path_buf()) {
            if old == path {
                return;
            }
            self.release_path(&old);
        }
        *self.counts.entry(path.to_path_buf()).or_insert(0) += 1;
    }

    pub fn release_entity(&mut self, entity: Entity) {
        if let Some(path) = self.entity_path.remove(&entity) {
            self.release_path(&path);
        }
    }

    fn release_path(&mut self, path: &Path) {
        let Some(count) = self.counts.get_mut(path) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.counts.remove(path);
        }
    }

    /// Paths that remain live after `releasing` entities are despawned this frame.
    pub fn live_paths_after_releasing(&self, releasing: &[Entity]) -> HashSet<PathBuf> {
        let mut counts = self.counts.clone();
        for entity in releasing {
            let Some(path) = self.entity_path.get(entity) else {
                continue;
            };
            if let Some(count) = counts.get_mut(path) {
                *count = count.saturating_sub(1);
            }
        }
        counts
            .into_iter()
            .filter(|(_, c)| *c > 0)
            .map(|(p, _)| p)
            .collect()
    }

    pub fn live_count(&self) -> usize {
        self.counts.values().filter(|c| **c > 0).count()
    }
}

pub fn index_world_tile_bound_added(
    mut index: ResMut<WorldTileEntityIndex>,
    q: Query<(Entity, &WorldTileBound), Added<WorldTileBound>>,
) {
    for (entity, bound) in &q {
        index.insert(entity, bound.tile_x, bound.tile_z);
    }
}

pub fn index_world_tile_bound_removed(
    mut index: ResMut<WorldTileEntityIndex>,
    mut removed: RemovedComponents<WorldTileBound>,
) {
    for entity in removed.read() {
        index.remove_entity(entity);
    }
}

pub fn track_world_shape_live_refs_added(
    mut refs: ResMut<WorldShapeLiveRefs>,
    lod: Query<(Entity, &WorldSceneryLod), Added<WorldSceneryLod>>,
    instanced: Query<(Entity, &WorldInstancedGroup), Added<WorldInstancedGroup>>,
) {
    for (entity, lod) in &lod {
        refs.retain_entity(entity, &lod.shape_path);
    }
    for (entity, group) in &instanced {
        refs.retain_entity(entity, &group.shape_path);
    }
}

pub fn track_world_shape_live_refs_removed(
    mut refs: ResMut<WorldShapeLiveRefs>,
    mut removed_lod: RemovedComponents<WorldSceneryLod>,
    mut removed_instanced: RemovedComponents<WorldInstancedGroup>,
) {
    for entity in removed_lod.read() {
        refs.release_entity(entity);
    }
    for entity in removed_instanced.read() {
        refs.release_entity(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(n: u64) -> Entity {
        Entity::from_bits(n)
    }

    #[test]
    fn take_tiles_only_returns_candidate_entities() {
        let mut index = WorldTileEntityIndex::default();
        let a = entity(1);
        let b = entity(2);
        let c = entity(3);
        index.insert(a, 0, 0);
        index.insert(b, 0, 0);
        index.insert(c, 1, 0);

        let unload = HashSet::from([(0, 0)]);
        let visited = index.entities_on_tiles(&unload);
        assert_eq!(visited.len(), 2);
        assert!(visited.contains(&a) && visited.contains(&b));
        assert!(!visited.contains(&c));

        let taken = index.take_tiles(&unload);
        assert_eq!(taken.len(), 2);
        assert_eq!(index.entity_count(), 1);
        assert_eq!(index.entities_on_tiles(&HashSet::from([(1, 0)])), vec![c]);
        // Idempotent remove after take.
        index.remove_entity(a);
        assert_eq!(index.entity_count(), 1);
    }

    #[test]
    fn live_paths_ignore_pending_despawn_entities() {
        let mut refs = WorldShapeLiveRefs::default();
        let keep = entity(10);
        let drop = entity(11);
        let path_keep = PathBuf::from("keep.s");
        let path_drop = PathBuf::from("drop.s");
        refs.retain_entity(keep, &path_keep);
        refs.retain_entity(drop, &path_drop);
        refs.retain_entity(entity(12), &path_drop); // second ref on drop.s

        let live = refs.live_paths_after_releasing(&[drop]);
        assert!(live.contains(&path_keep));
        assert!(live.contains(&path_drop)); // still one live user
        let live2 = refs.live_paths_after_releasing(&[drop, entity(12)]);
        assert!(live2.contains(&path_keep));
        assert!(!live2.contains(&path_drop));
    }
}
