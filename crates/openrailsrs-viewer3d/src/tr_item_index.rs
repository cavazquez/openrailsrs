//! Index `.w` objects by `TrItemId` (TSRE `fillWorldObjectsByTrackItemIds`).

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::world::{TrItemIndexDelta, WorldObject};

/// Kind of world object linked to a TDB `TrItem`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrItemWorldKind {
    Signal,
    Speedpost,
    SoundRegion,
}

/// One `.w` object referencing a `TrItemId`.
#[derive(Clone, Debug)]
pub struct TrItemWorldRef {
    pub tile_x: i32,
    pub tile_z: i32,
    pub uid: u32,
    pub kind: TrItemWorldKind,
    pub position_msts: Vec3,
}

/// Stats from an incremental sync (#61).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrItemIndexSyncStats {
    pub tiles_added: usize,
    pub tiles_removed: usize,
    pub objects_inserted: usize,
    pub full_rebuild: bool,
}

/// Map `TrItemId` → world objects in loaded tiles, plus WORLD coverage.
#[derive(Resource, Clone, Debug, Default)]
pub struct TrItemWorldIndex {
    by_item: HashMap<u32, Vec<TrItemWorldRef>>,
    /// Tiles successfully loaded/parsed into the WORLD scene used to build this index.
    loaded_tiles: HashSet<(i32, i32)>,
}

impl TrItemWorldIndex {
    pub fn from_world_objects(items: &[WorldObject]) -> Self {
        let loaded_tiles = items.iter().map(|o| (o.tile_x, o.tile_z)).collect();
        Self::from_world_objects_with_coverage(items, loaded_tiles)
    }

    pub fn from_world_objects_with_coverage(
        items: &[WorldObject],
        loaded_tiles: HashSet<(i32, i32)>,
    ) -> Self {
        let mut index = Self {
            by_item: HashMap::new(),
            loaded_tiles: HashSet::new(),
        };
        for tile in loaded_tiles {
            index.loaded_tiles.insert(tile);
        }
        for obj in items {
            index.insert_object(obj);
        }
        index
    }

    pub fn rebuild_from_scene(world: &crate::world::WorldScene) -> Self {
        let coverage = if world.loaded_tiles.is_empty() {
            world.items.iter().map(|o| (o.tile_x, o.tile_z)).collect()
        } else {
            world.loaded_tiles.clone()
        };
        Self::from_world_objects_with_coverage(&world.items, coverage)
    }

    pub fn objects_for_item(&self, item_id: u32) -> &[TrItemWorldRef] {
        self.by_item
            .get(&item_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn loaded_tiles(&self) -> &HashSet<(i32, i32)> {
        &self.loaded_tiles
    }

    pub fn covers_tile(&self, tile_x: i32, tile_z: i32) -> bool {
        self.loaded_tiles.contains(&(tile_x, tile_z))
    }

    /// True when a `.w` object already represents this `TrItem` near `msts` (XZ).
    pub fn has_world_object_near(&self, item_id: u32, msts: Vec3, radius_m: f32) -> bool {
        self.by_item.get(&item_id).is_some_and(|refs| {
            refs.iter().any(|r| {
                Vec2::new(r.position_msts.x - msts.x, r.position_msts.z - msts.z).length()
                    <= radius_m
            })
        })
    }

    /// Insert TrItem links for one WORLD object (idempotent only if caller avoids dupes).
    pub fn insert_object(&mut self, obj: &WorldObject) {
        for (item_id, kind) in tr_item_ids_for_object(obj) {
            self.by_item
                .entry(item_id)
                .or_default()
                .push(TrItemWorldRef {
                    tile_x: obj.tile_x,
                    tile_z: obj.tile_z,
                    uid: obj.uid.unwrap_or(0),
                    kind,
                    position_msts: obj.position,
                });
        }
    }

    /// Drop all refs and coverage for one tile (#61).
    pub fn remove_tile(&mut self, tile_x: i32, tile_z: i32) {
        if !self.loaded_tiles.remove(&(tile_x, tile_z)) {
            return;
        }
        self.by_item.retain(|_, refs| {
            refs.retain(|r| r.tile_x != tile_x || r.tile_z != tile_z);
            !refs.is_empty()
        });
    }

    /// Apply a stream/unload delta without scanning unrelated tiles (#61).
    pub fn apply_delta(&mut self, delta: &TrItemIndexDelta) -> TrItemIndexSyncStats {
        let mut stats = TrItemIndexSyncStats {
            tiles_added: delta.added_tiles.len(),
            tiles_removed: delta.removed_tiles.len(),
            objects_inserted: 0,
            full_rebuild: false,
        };
        for &(tile_x, tile_z) in &delta.removed_tiles {
            self.remove_tile(tile_x, tile_z);
        }
        for &(tile_x, tile_z) in &delta.added_tiles {
            // Replacing a tile: clear stale refs before inserting the new batch.
            if self.loaded_tiles.contains(&(tile_x, tile_z)) {
                self.remove_tile(tile_x, tile_z);
            }
            self.loaded_tiles.insert((tile_x, tile_z));
        }
        for obj in &delta.added_objects {
            self.insert_object(obj);
            stats.objects_inserted += 1;
        }
        stats
    }
}

fn tr_item_ids_for_object(obj: &WorldObject) -> Vec<(u32, TrItemWorldKind)> {
    match obj.kind {
        "Signal" => obj
            .tr_item_ids
            .iter()
            .map(|id| (*id, TrItemWorldKind::Signal))
            .collect(),
        "Speedpost" => obj
            .tr_item_ids
            .iter()
            .map(|id| (*id, TrItemWorldKind::Speedpost))
            .collect(),
        "SoundRegion" => obj
            .tr_item_ids
            .iter()
            .map(|id| (*id, TrItemWorldKind::SoundRegion))
            .collect(),
        _ => Vec::new(),
    }
}

/// Apply stream/unload deltas; full rebuild only as recovery when coverage drifts (#61).
pub fn sync_tr_item_world_index(
    mut world: ResMut<crate::world::WorldScene>,
    mut index: ResMut<TrItemWorldIndex>,
) {
    if !world.tr_item_delta.is_empty() {
        let stats = index.apply_delta(&world.tr_item_delta);
        world.tr_item_delta.clear();
        if stats.tiles_added > 0 || stats.tiles_removed > 0 {
            crate::viewer_log!(
                "openrailsrs-viewer3d: tr_item index delta — +{} tile(s)/{} obj(s), -{} tile(s)",
                stats.tiles_added,
                stats.objects_inserted,
                stats.tiles_removed
            );
        }
        return;
    }

    // Recovery only when coverage cardinality drifts (retain/manual edits without delta).
    let scene_tile_count = if world.loaded_tiles.is_empty() {
        world
            .items
            .iter()
            .map(|o| (o.tile_x, o.tile_z))
            .collect::<HashSet<_>>()
            .len()
    } else {
        world.loaded_tiles.len()
    };
    if index.loaded_tiles.len() == scene_tile_count {
        return;
    }
    let scene_tiles = if world.loaded_tiles.is_empty() {
        world.items.iter().map(|o| (o.tile_x, o.tile_z)).collect()
    } else {
        world.loaded_tiles.clone()
    };
    if index.loaded_tiles != scene_tiles {
        *index = TrItemWorldIndex::from_world_objects_with_coverage(&world.items, scene_tiles);
        crate::viewer_log!(
            "openrailsrs-viewer3d: tr_item index full rebuild ({} tile(s), recovery)",
            index.loaded_tiles.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(tile_x: i32, tile_z: i32, uid: u32, item_ids: &[u32], pos: Vec3) -> WorldObject {
        WorldObject {
            kind: "Signal",
            uid: Some(uid),
            label: "sig".into(),
            shape_file: Some("sig.s".into()),
            section_idx: None,
            position: pos,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            tile_x,
            tile_z,
            forest: None,
            water: None,
            transfer: None,
            car_spawner: None,
            tr_item_ids: item_ids.to_vec(),
        }
    }

    #[test]
    fn index_signal_tr_item_ids() {
        let items = vec![signal(
            -6080,
            14925,
            42,
            &[39, 40],
            Vec3::new(1.0, 2.0, 3.0),
        )];
        let index = TrItemWorldIndex::from_world_objects(&items);
        assert_eq!(index.objects_for_item(39).len(), 1);
        assert!(index.has_world_object_near(39, Vec3::new(1.0, 2.0, 3.0), 1.0));
        assert!(index.covers_tile(-6080, 14925));
        assert!(!index.covers_tile(-6081, 14925));
    }

    #[test]
    fn incremental_add_remove_matches_full_rebuild() {
        let tile_a = (-6080, 14925);
        let tile_b = (-6081, 14925);
        let a = signal(tile_a.0, tile_a.1, 1, &[10], Vec3::new(0.0, 0.0, 0.0));
        let b = signal(tile_b.0, tile_b.1, 2, &[20], Vec3::new(100.0, 0.0, 0.0));
        let c = signal(tile_a.0, tile_a.1, 3, &[10, 11], Vec3::new(5.0, 0.0, 0.0));

        let mut incremental = TrItemWorldIndex::default();
        let add_a = TrItemIndexDelta {
            added_tiles: HashSet::from([tile_a]),
            removed_tiles: HashSet::new(),
            added_objects: vec![a.clone(), c.clone()],
        };
        incremental.apply_delta(&add_a);

        let add_b = TrItemIndexDelta {
            added_tiles: HashSet::from([tile_b]),
            removed_tiles: HashSet::new(),
            added_objects: vec![b.clone()],
        };
        incremental.apply_delta(&add_b);

        let full = TrItemWorldIndex::from_world_objects_with_coverage(
            &[a.clone(), c.clone(), b.clone()],
            HashSet::from([tile_a, tile_b]),
        );
        assert_eq!(
            incremental.objects_for_item(10).len(),
            full.objects_for_item(10).len()
        );
        assert_eq!(
            incremental.objects_for_item(20).len(),
            full.objects_for_item(20).len()
        );
        assert_eq!(incremental.loaded_tiles(), full.loaded_tiles());

        let remove_a = TrItemIndexDelta {
            added_tiles: HashSet::new(),
            removed_tiles: HashSet::from([tile_a]),
            added_objects: Vec::new(),
        };
        incremental.apply_delta(&remove_a);
        let full_b = TrItemWorldIndex::from_world_objects_with_coverage(
            std::slice::from_ref(&b),
            HashSet::from([tile_b]),
        );
        assert!(!incremental.covers_tile(tile_a.0, tile_a.1));
        assert_eq!(incremental.objects_for_item(10).len(), 0);
        assert_eq!(
            incremental.objects_for_item(20).len(),
            full_b.objects_for_item(20).len()
        );
        assert_eq!(incremental.loaded_tiles(), full_b.loaded_tiles());
    }

    #[test]
    fn apply_delta_does_not_require_foreign_tile_objects() {
        let mut index = TrItemWorldIndex::default();
        // Only the new tile's objects are supplied — no scan of a 35k-item scene.
        let stats = index.apply_delta(&TrItemIndexDelta {
            added_tiles: HashSet::from([(-1, 2)]),
            removed_tiles: HashSet::new(),
            added_objects: vec![signal(-1, 2, 9, &[99], Vec3::ZERO)],
        });
        assert_eq!(stats.tiles_added, 1);
        assert_eq!(stats.objects_inserted, 1);
        assert!(!stats.full_rebuild);
        assert_eq!(index.objects_for_item(99).len(), 1);
    }
}
