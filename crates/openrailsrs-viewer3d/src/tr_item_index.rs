//! Index `.w` objects by `TrItemId` (TSRE `fillWorldObjectsByTrackItemIds`).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::world::WorldObject;

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

/// Map `TrItemId` → world objects in loaded tiles.
#[derive(Resource, Clone, Debug, Default)]
pub struct TrItemWorldIndex {
    by_item: HashMap<u32, Vec<TrItemWorldRef>>,
}

impl TrItemWorldIndex {
    pub fn from_world_objects(items: &[WorldObject]) -> Self {
        let mut by_item: HashMap<u32, Vec<TrItemWorldRef>> = HashMap::new();
        for obj in items {
            for (item_id, kind) in tr_item_ids_for_object(obj) {
                by_item.entry(item_id).or_default().push(TrItemWorldRef {
                    tile_x: obj.tile_x,
                    tile_z: obj.tile_z,
                    uid: obj.uid.unwrap_or(0),
                    kind,
                    position_msts: obj.position,
                });
            }
        }
        Self { by_item }
    }

    pub fn rebuild_from_scene(world: &crate::world::WorldScene) -> Self {
        Self::from_world_objects(&world.items)
    }

    pub fn objects_for_item(&self, item_id: u32) -> &[TrItemWorldRef] {
        self.by_item
            .get(&item_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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

/// Rebuild when streamed `.w` tiles add objects (live mode).
pub fn sync_tr_item_world_index(
    world: Res<crate::world::WorldScene>,
    mut index: ResMut<TrItemWorldIndex>,
    mut last_len: Local<usize>,
) {
    if world.items.len() != *last_len {
        *index = TrItemWorldIndex::rebuild_from_scene(&world);
        *last_len = world.items.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_signal_tr_item_ids() {
        let items = vec![WorldObject {
            kind: "Signal",
            uid: Some(42),
            label: "sig".into(),
            shape_file: Some("sig.s".into()),
            section_idx: None,
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            tile_x: -6080,
            tile_z: 14925,
            forest: None,
            water: None,
            tr_item_ids: vec![39, 40],
        }];
        let index = TrItemWorldIndex::from_world_objects(&items);
        assert_eq!(index.objects_for_item(39).len(), 1);
        assert!(index.has_world_object_near(39, Vec3::new(1.0, 2.0, 3.0), 1.0));
    }
}
