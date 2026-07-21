//! Shared WORLD shape-part → entity spawn core (#115).
//!
//! Converts a WORLD object pose + already-resolved shape parts + a
//! [`PlacementAdapter`] into planned part spawns (and optionally entities).
//!
//! Apps keep orchestration (progressive budgets, LOD, instancing, VSM, …).
//! Mesh/material *building* stays in each app; this module owns the common
//! resolve→plan→entity→tile-binding spine and stable part identities.
//!
//! Session shape GPU handles are looked up via [`crate::spawn::SessionShapeCache`] (#114).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_formats::WorldItem;
use openrailsrs_or_shader::coordinates::msts_tile_local_to_bevy;

use crate::spawn::cache::{SessionShapeCache, ShapeCacheKey};
use crate::stream::{TileBound, TileCoord};
use crate::tile::item_transform;

/// Canonical WORLD object pose before app-specific adapters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldObjectPose {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl WorldObjectPose {
    pub fn transform(&self) -> Transform {
        Transform {
            translation: self.position,
            rotation: self.rotation,
            scale: self.scale,
        }
    }
}

/// Placement of one WORLD object instance (canonical, pre-adapter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldObjectPlacement {
    pub pose: WorldObjectPose,
    pub tile: TileBound,
}

impl WorldObjectPlacement {
    pub fn new(pose: WorldObjectPose, tile: TileBound) -> Self {
        Self { pose, tile }
    }

    pub fn transform(&self) -> Transform {
        self.pose.transform()
    }

    pub fn tile_coord(self) -> TileCoord {
        self.tile.coord()
    }

    pub fn cache_tile(self) -> TileCoord {
        self.tile_coord()
    }
}

/// Stable identity for a resolved shape part (cache / telemetry / tests).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapePartId {
    pub shape_key: PathBuf,
    pub part_index: usize,
    pub prim_state_idx: i32,
}

impl ShapePartId {
    pub fn new(shape_key: impl Into<PathBuf>, part_index: usize, prim_state_idx: i32) -> Self {
        Self {
            shape_key: shape_key.into(),
            part_index,
            prim_state_idx,
        }
    }
}

/// Mesh + material handles for one part, with a stable id.
#[derive(Clone, Debug)]
pub struct ResolvedShapePart {
    pub id: ShapePartId,
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

impl ResolvedShapePart {
    pub fn new(
        shape_key: impl Into<PathBuf>,
        part_index: usize,
        prim_state_idx: i32,
        mesh: Handle<Mesh>,
        material: Handle<StandardMaterial>,
    ) -> Self {
        Self {
            id: ShapePartId::new(shape_key, part_index, prim_state_idx),
            mesh,
            material,
        }
    }

    /// Asset ids used for equality checks before adapters run.
    pub fn asset_ids(&self) -> (AssetId<Mesh>, AssetId<StandardMaterial>) {
        (self.mesh.id(), self.material.id())
    }
}

/// Planned entity spawn after applying a [`PlacementAdapter`].
#[derive(Clone, Debug)]
pub struct PlannedShapePartSpawn {
    pub part: ResolvedShapePart,
    /// Canonical placement (pre-adapter).
    pub placement: WorldObjectPlacement,
    /// Transform after the adapter (equals [`WorldObjectPlacement::transform`] for identity).
    pub transform: Transform,
}

/// App-specific placement transform (floating origin, tile offset, lab cameras, …).
///
/// Adapters must not change mesh/material identities — only the final transform
/// (and optionally tile remapping via [`PlacementAdapter::adapt_placement`]).
pub trait PlacementAdapter {
    fn adapt_transform(&self, placement: &WorldObjectPlacement) -> Transform {
        placement.transform()
    }

    fn adapt_placement(&self, placement: WorldObjectPlacement) -> WorldObjectPlacement {
        let tf = self.adapt_transform(&placement);
        WorldObjectPlacement {
            pose: WorldObjectPose {
                position: tf.translation,
                rotation: tf.rotation,
                scale: tf.scale,
            },
            tile: placement.tile,
        }
    }
}

/// No-op adapter — final transform equals the canonical placement.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityPlacementAdapter;

impl PlacementAdapter for IdentityPlacementAdapter {}

/// Adds a constant world translation (render3d tile origin offset).
#[derive(Clone, Copy, Debug)]
pub struct TileOffsetPlacementAdapter {
    pub tile_offset: Vec3,
}

impl PlacementAdapter for TileOffsetPlacementAdapter {
    fn adapt_transform(&self, placement: &WorldObjectPlacement) -> Transform {
        let mut tf = placement.transform();
        tf.translation += self.tile_offset;
        tf
    }
}

/// Closure-based adapter for viewer floating-origin / render-space remaps.
pub struct FnPlacementAdapter<F>(pub F);

impl<F> PlacementAdapter for FnPlacementAdapter<F>
where
    F: Fn(&WorldObjectPlacement) -> Transform,
{
    fn adapt_transform(&self, placement: &WorldObjectPlacement) -> Transform {
        (self.0)(placement)
    }
}

/// Rotation/scale from a `.w` [`WorldItem`] (`Matrix3x3` or `QDirection`).
#[inline]
pub fn world_item_rotation_scale(item: &WorldItem) -> (Quat, Vec3) {
    item_transform(item)
}

/// Canonical placement from a `.w` item. Returns `None` when the item has no position.
pub fn world_item_placement(
    tile_x: i32,
    tile_z: i32,
    item: &WorldItem,
) -> Option<WorldObjectPlacement> {
    let local = item.position()?;
    let position = msts_tile_local_to_bevy(tile_x, tile_z, local);
    let (rotation, scale) = world_item_rotation_scale(item);
    Some(WorldObjectPlacement {
        pose: WorldObjectPose {
            position,
            rotation,
            scale,
        },
        tile: TileBound::new(tile_x, tile_z),
    })
}

/// Canonical placement from an already-resolved Bevy pose + tile.
pub fn object_placement(
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
    tile_x: i32,
    tile_z: i32,
) -> WorldObjectPlacement {
    WorldObjectPlacement {
        pose: WorldObjectPose {
            position,
            rotation,
            scale,
        },
        tile: TileBound::new(tile_x, tile_z),
    }
}

/// Build [`ResolvedShapePart`]s from mesh/material handles and part metadata.
pub fn resolve_shape_parts(
    shape_key: &Path,
    parts: impl IntoIterator<Item = (usize, i32, Handle<Mesh>, Handle<StandardMaterial>)>,
) -> Vec<ResolvedShapePart> {
    parts
        .into_iter()
        .map(|(part_index, prim_state_idx, mesh, material)| {
            ResolvedShapePart::new(shape_key, part_index, prim_state_idx, mesh, material)
        })
        .collect()
}

/// Plan part spawns: same input → same part ids + canonical transform; adapter
/// only affects [`PlannedShapePartSpawn::transform`].
pub fn plan_shape_part_spawns(
    parts: &[ResolvedShapePart],
    placement: WorldObjectPlacement,
    adapter: &impl PlacementAdapter,
) -> Vec<PlannedShapePartSpawn> {
    let transform = adapter.adapt_transform(&placement);
    parts
        .iter()
        .cloned()
        .map(|part| PlannedShapePartSpawn {
            part,
            placement,
            transform,
        })
        .collect()
}

/// Generic plan over app-owned part payloads (e.g. render3d `PartHandles`).
pub fn plan_parts_with_ids<P: Clone>(
    shape_key: &Path,
    parts: &[P],
    prim_state_idx: impl Fn(usize, &P) -> i32,
    placement: WorldObjectPlacement,
    adapter: &impl PlacementAdapter,
) -> Vec<(ShapePartId, P, Transform, TileBound)> {
    let transform = adapter.adapt_transform(&placement);
    parts
        .iter()
        .enumerate()
        .map(|(part_index, part)| {
            let id = ShapePartId::new(shape_key, part_index, prim_state_idx(part_index, part));
            (id, part.clone(), transform, placement.tile)
        })
        .collect()
}

/// Look up (or insert) resolved parts in a [`SessionShapeCache`], retaining the tile ref.
pub fn cached_shape_parts<'a, V>(
    cache: &'a mut SessionShapeCache<ShapeCacheKey, V>,
    key: ShapeCacheKey,
    tile: TileCoord,
    build: impl FnOnce() -> V,
) -> &'a V {
    let _ = cache.get_or_insert_with(key.clone(), build);
    cache.retain_for_tile(tile, &key);
    cache
        .get(&key)
        .expect("asset just inserted or already present")
}

/// Spawn StandardMaterial parts with a cloned tile/membership component.
pub fn spawn_standard_shape_parts<T>(
    commands: &mut Commands,
    planned: &[PlannedShapePartSpawn],
    name: &str,
    tile_component: T,
) -> usize
where
    T: Component + Clone,
{
    let mut count = 0usize;
    for entry in planned {
        commands.spawn((
            Mesh3d(entry.part.mesh.clone()),
            MeshMaterial3d(entry.part.material.clone()),
            entry.transform,
            tile_component.clone(),
            Name::new(name.to_string()),
        ));
        count += 1;
    }
    count
}

/// Spawn StandardMaterial parts using shared [`TileBound`] membership.
pub fn spawn_standard_shape_parts_bound(
    commands: &mut Commands,
    planned: &[PlannedShapePartSpawn],
    name: &str,
) -> usize {
    let mut count = 0usize;
    for entry in planned {
        commands.spawn((
            Mesh3d(entry.part.mesh.clone()),
            MeshMaterial3d(entry.part.material.clone()),
            entry.transform,
            entry.placement.tile,
            Name::new(name.to_string()),
        ));
        count += 1;
    }
    count
}

/// Spawn parts when the material handle type is chosen per-part by the caller.
///
/// `spawn_one` receives `(mesh, transform, tile)` and must insert the material
/// bundle itself (Standard vs OrScenery, etc.).
pub fn spawn_shape_parts_with<F>(
    parts_and_transforms: impl IntoIterator<Item = (Handle<Mesh>, Transform, TileBound)>,
    mut spawn_one: F,
) -> usize
where
    F: FnMut(Handle<Mesh>, Transform, TileBound),
{
    let mut count = 0usize;
    for (mesh, transform, tile) in parts_and_transforms {
        spawn_one(mesh, transform, tile);
        count += 1;
    }
    count
}

/// Convenience: plan + spawn StandardMaterial parts in one call.
pub fn spawn_resolved_shape_parts<T>(
    commands: &mut Commands,
    parts: &[ResolvedShapePart],
    placement: WorldObjectPlacement,
    adapter: &impl PlacementAdapter,
    name: &str,
    tile_component: T,
) -> usize
where
    T: Component + Clone,
{
    let planned = plan_shape_part_spawns(parts, placement, adapter);
    spawn_standard_shape_parts(commands, &planned, name, tile_component)
}

/// Plan + spawn with shared [`TileBound`] (preferred when apps adopt stream membership).
pub fn spawn_resolved_shape_parts_bound(
    commands: &mut Commands,
    parts: &[ResolvedShapePart],
    placement: WorldObjectPlacement,
    adapter: &impl PlacementAdapter,
    name: &str,
) -> usize {
    let planned = plan_shape_part_spawns(parts, placement, adapter);
    spawn_standard_shape_parts_bound(commands, &planned, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_placement_transform_is_canonical() {
        let p = object_placement(
            Vec3::new(10.0, 2.0, -3.0),
            Quat::from_rotation_y(0.5),
            Vec3::new(1.0, 2.0, 1.0),
            4,
            -7,
        );
        let tf = p.transform();
        assert_eq!(tf.translation, Vec3::new(10.0, 2.0, -3.0));
        assert_eq!(tf.scale, Vec3::new(1.0, 2.0, 1.0));
        assert_eq!(p.tile, TileBound::new(4, -7));
    }

    #[test]
    fn same_input_yields_same_part_ids_and_pre_adapter_transform() {
        let shape = Path::new("/route/SHAPES/bench.s");
        let mesh_a = Handle::<Mesh>::default();
        let mesh_b = Handle::<Mesh>::default();
        let mat_a = Handle::<StandardMaterial>::default();
        let mat_b = Handle::<StandardMaterial>::default();
        let parts = resolve_shape_parts(
            shape,
            [
                (0, 3, mesh_a.clone(), mat_a.clone()),
                (1, 7, mesh_b.clone(), mat_b.clone()),
            ],
        );
        let placement = object_placement(Vec3::new(1.0, 0.0, 2.0), Quat::IDENTITY, Vec3::ONE, 0, 0);

        let plan_a = plan_shape_part_spawns(&parts, placement, &IdentityPlacementAdapter);
        let plan_b = plan_shape_part_spawns(&parts, placement, &IdentityPlacementAdapter);

        assert_eq!(plan_a.len(), 2);
        for (a, b) in plan_a.iter().zip(plan_b.iter()) {
            assert_eq!(a.part.id, b.part.id);
            assert_eq!(a.part.asset_ids(), b.part.asset_ids());
            assert_eq!(a.placement.transform(), b.placement.transform());
            assert_eq!(a.transform, a.placement.transform());
            assert_eq!(b.transform, b.placement.transform());
        }
        assert_eq!(plan_a[0].part.id, ShapePartId::new(shape, 0, 3));
        assert_eq!(plan_a[1].part.id, ShapePartId::new(shape, 1, 7));
    }

    #[test]
    fn placement_adapter_changes_transform_not_part_ids() {
        let shape = Path::new("signal.s");
        let parts = resolve_shape_parts(
            shape,
            [(
                0,
                0,
                Handle::<Mesh>::default(),
                Handle::<StandardMaterial>::default(),
            )],
        );
        let placement = object_placement(Vec3::new(5.0, 1.0, 0.0), Quat::IDENTITY, Vec3::ONE, 1, 2);
        let adapter = TileOffsetPlacementAdapter {
            tile_offset: Vec3::new(100.0, 0.0, -50.0),
        };

        let planned = plan_shape_part_spawns(&parts, placement, &adapter);
        assert_eq!(planned[0].part.id, ShapePartId::new(shape, 0, 0));
        assert_eq!(
            planned[0].placement.transform().translation,
            Vec3::new(5.0, 1.0, 0.0)
        );
        assert_eq!(
            planned[0].transform.translation,
            Vec3::new(105.0, 1.0, -50.0)
        );
        assert_eq!(planned[0].placement.tile, TileBound::new(1, 2));
    }

    #[test]
    fn plan_parts_with_ids_is_stable() {
        #[derive(Clone)]
        struct DummyPart;
        let parts = [DummyPart, DummyPart];
        let placement = object_placement(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE, 3, 4);
        let a = plan_parts_with_ids(
            Path::new("a.s"),
            &parts,
            |i, _| (i as i32) * 10,
            placement,
            &IdentityPlacementAdapter,
        );
        let b = plan_parts_with_ids(
            Path::new("a.s"),
            &parts,
            |i, _| (i as i32) * 10,
            placement,
            &IdentityPlacementAdapter,
        );
        assert_eq!(a[0].0, b[0].0);
        assert_eq!(a[1].0, b[1].0);
        assert_eq!(a[0].0.prim_state_idx, 0);
        assert_eq!(a[1].0.prim_state_idx, 10);
        assert_eq!(a[0].2, placement.transform());
        assert_eq!(a[0].3, TileBound::new(3, 4));
    }

    #[test]
    fn session_shape_cache_retains_tile_on_spawn_lookup() {
        let mut cache = SessionShapeCache::<ShapeCacheKey, Vec<u32>>::new();
        let key = ShapeCacheKey::shape_only("bench.s");
        let parts = cached_shape_parts(&mut cache, key.clone(), TileCoord::new(0, 0), || {
            vec![1, 2, 3]
        });
        assert_eq!(parts, &[1, 2, 3]);
        assert_eq!(cache.misses(), 1);
        let again = cached_shape_parts(&mut cache, key.clone(), TileCoord::new(1, 0), || vec![9]);
        assert_eq!(again, &[1, 2, 3]);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.tiles_for(&key).map(|t| t.len()), Some(2));
    }

    #[test]
    fn world_item_placement_static_qdir() {
        let item = WorldItem::Static {
            uid: 1,
            file_name: Some("bench.s".into()),
            position: openrailsrs_formats::Vec3 {
                x: 10.0,
                y: 2.0,
                z: 30.0,
            },
            qdir: Some([0.0, 0.0, 0.0, 1.0]),
            matrix3x3: None,
        };
        let p = world_item_placement(0, 0, &item).expect("placement");
        assert_eq!(p.pose.position, Vec3::new(10.0, 2.0, -30.0));
        assert_eq!(p.pose.scale, Vec3::ONE);
        assert_eq!(p.tile, TileBound::new(0, 0));
        let again = world_item_placement(0, 0, &item).expect("placement");
        assert_eq!(p.transform(), again.transform());
    }
}
