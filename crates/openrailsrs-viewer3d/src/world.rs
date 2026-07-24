//! MSTS world tiles (`.w`) as coloured placeholder boxes (order 5 / issue #8).
//!
//! TODO(#112): materialize [`WorldScene`] / [`WorldObject`] from
//! `openrailsrs_bevy_scenery::MstsTileSnapshot` (classify + coords), keeping
//! RouteFocus / [`WorldItemWindow`] filtering as a viewer-only adapter.
//! Stream hot path uses [`crate::tile_bundle`] / AssetServer (#111); bootstrap
//! (`load_world_from_route_dir_*`) still parses via `WorldFile::from_path`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::MeshAabb;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    ShapeAnimBinding, ShapeAnimState, animation_playback_speed, apply_shape_auto_z_bias,
    lod_level_index_for_distance, primary_texture_filename, shape_has_loop_animation,
    world_mesh_options_for_shape,
};
use openrailsrs_bevy_scenery::stream::{StreamWindowPolicy, TILE_SIZE_M, TileBound, TileCoord};
use openrailsrs_bevy_scenery::{
    LoadFailure, MstsAssetKind, MstsLoadCause, MstsLoadDiagnostics, world_item_placement,
};
use openrailsrs_formats::{
    ShapeFile, WorldFile, WorldItem, msts_tile_world_origin, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord, parse_world_w_tile_xz,
};

use crate::camera::CameraFollowMode;
#[cfg(test)]
use crate::coordinates::qdir_to_quat;
use crate::coordinates::{
    linear_requires_affine, matrix3x3_to_rotation_scale, msts_local_offset_to_bevy,
    msts_tile_local_to_bevy,
};
use crate::floating_origin::{FloatingOrigin, view_transform, view_translation};
use crate::launch::ViewerSceneryMode;
use crate::shapes::{
    RouteAssets, ShapeRenderAsset, apply_shape_descriptor_to_asset,
    collect_loaded_shape_texture_paths, collect_pbr_normal_map_texture_paths,
    load_shape_file_and_loaded, load_shape_pbr_sidecar, prefetch_ace_textures,
    reset_shape_file_parse_count, shape_file_parse_count, shape_part_visible_for_day_night,
    shape_render_asset_from_loaded_with_ace_cache, texture_search_dirs_for_shape,
};

/// WORLD-tile membership for scenery unload (shapes LOD, Transfer, road cars, …) (#62 / #113).
pub type WorldTileBound = TileBound;

/// Chebyshev stream policy from viewing metres (load = radius + one tile; hysteresis env/CLI).
pub fn view_stream_window_policy(view_radius_m: f32) -> StreamWindowPolicy {
    let tile = TILE_SIZE_M;
    StreamWindowPolicy::chebyshev_from_meters(
        view_radius_m + tile,
        crate::launch::VIEW_UNLOAD_HYSTERESIS_M,
        tile,
    )
}

/// Tracks which LOD level a spawned world shape part is using (runtime swap).
#[derive(Component, Clone, Debug)]
pub struct WorldSceneryLod {
    pub enabled: bool,
    pub shape_path: PathBuf,
    /// Stable identity within a shape. LOD bands may omit or reorder parts.
    pub sub_object_idx: u32,
    pub prim_state_idx: i32,
    /// Current vector position, retained for diagnostics only.
    pub part_index: usize,
    pub lod_idx: usize,
}

/// Skip full LOD scan when camera/focus move less than this (metres, render space) (#62).
pub const WORLD_LOD_EPS_M: f32 = 0.5;

/// Last camera/focus positions used by [`update_world_scenery_lod`] early-out (#62).
#[derive(Resource, Default, Debug)]
pub struct WorldLodCameraState {
    pub last_cam: Option<Vec3>,
    pub last_focus: Option<Vec3>,
}

/// `true` when LOD should rescan (camera or focus moved ≥ ε) (#62).
pub fn lod_camera_needs_update(
    state: &WorldLodCameraState,
    cam_pos: Vec3,
    focus_pos: Vec3,
    eps: f32,
) -> bool {
    let cam_moved = state
        .last_cam
        .map(|c| c.distance(cam_pos) >= eps)
        .unwrap_or(true);
    let focus_moved = state
        .last_focus
        .map(|f| f.distance(focus_pos) >= eps)
        .unwrap_or(true);
    cam_moved || focus_moved
}

/// Camera→entity/group distance for WORLD LOD selection (render / floating-origin space) (#74).
///
/// Must be `length(cam - center)`, not `dist(cam, focus) + dist(center, focus)`.
pub fn world_lod_distance_m(cam_pos: Vec3, entity_or_group_center: Vec3) -> f32 {
    cam_pos.distance(entity_or_group_center)
}

/// Find the same logical shape part in another LOD band.
///
/// Vector positions are not stable in MSTS shapes: a band can remove an earlier
/// primitive group and shift every later part. `sub_object_idx + prim_state_idx`
/// is the stable identity used by Open Rails' primitive groups.
pub fn shape_lod_part_by_identity(
    asset: &ShapeRenderAsset,
    sub_object_idx: u32,
    prim_state_idx: i32,
) -> Option<(usize, &crate::shapes::ShapePartAsset)> {
    asset.parts.iter().enumerate().find(|(_, part)| {
        part.sub_object_idx == sub_object_idx && part.prim_state_idx == prim_state_idx
    })
}

/// Unload decision for a WORLD scenery entity (#62).
pub fn scenery_entity_should_unload(
    bound: Option<WorldTileBound>,
    unloaded_tiles: &HashSet<(i32, i32)>,
    dist_to_center: f32,
    unload_radius: f32,
) -> bool {
    if let Some(b) = bound {
        unloaded_tiles.contains(&(b.tile_x, b.tile_z))
    } else {
        dist_to_center > unload_radius
    }
}

/// One WORLD shape instance queued for spawn, with tile membership (#62).
#[derive(Clone, Copy, Debug)]
pub struct ShapeInstancePlacement {
    pub transform: Transform,
    /// Full Matrix3x3 linear when present (shear); overrides TRS in GPU instancing (#139).
    pub linear: Option<Mat3>,
    pub tile_x: i32,
    pub tile_z: i32,
    /// Open Rails `ShapeFlags.AutoZBias` (TrackObj / switch track) (#103).
    pub auto_z_bias: bool,
    /// WORLD `SignalSubObj` bitmask when this instance is a Signal mesh (#80).
    pub signal_sub_obj: Option<u32>,
}

/// True shear: `linear` does not round-trip via Quat×scale (#139 / #174).
///
/// Ordinary Matrix3x3 (rotation/scale/reflection) must stay on the TRS entity path —
/// do **not** treat `linear.is_some()` as shear.
fn placement_has_shear(p: &ShapeInstancePlacement) -> bool {
    p.linear
        .is_some_and(|lin| linear_requires_affine(lin, p.transform.rotation, p.transform.scale))
}

/// Bake XNA linear into mesh positions/normals; Transform keeps translation only (#139).
fn bake_linear_into_mesh(mesh: &Mesh, linear: Mat3) -> Mesh {
    let mut out = mesh.clone();
    if let Some(VertexAttributeValues::Float32x3(positions)) =
        out.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            let v = linear * Vec3::from_array(*p);
            *p = v.to_array();
        }
    }
    // Normal transform: inverse-transpose of linear (safe for shear).
    let normal_m = if linear.determinant().abs() > 1e-8 {
        linear.inverse().transpose()
    } else {
        linear
    };
    if let Some(VertexAttributeValues::Float32x3(normals)) =
        out.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
    {
        for n in normals.iter_mut() {
            let v = (normal_m * Vec3::from_array(*n)).normalize_or_zero();
            *n = if v.length_squared() > 1e-12 {
                v.to_array()
            } else {
                *n
            };
        }
    }
    out
}

/// View Transform + mesh handle: bake shear into mesh when needed; else TRS as authored.
fn view_mesh_for_placement(
    meshes: &mut Assets<Mesh>,
    part_mesh: &Handle<Mesh>,
    p: &ShapeInstancePlacement,
    origin: &FloatingOrigin,
) -> (Transform, Handle<Mesh>) {
    let mut tf = view_transform(p.transform, origin);
    if let Some(linear) = p.linear.filter(|_| placement_has_shear(p)) {
        let Some(src) = meshes.get(part_mesh) else {
            return (tf, part_mesh.clone());
        };
        let baked = bake_linear_into_mesh(src, linear);
        tf.rotation = Quat::IDENTITY;
        tf.scale = Vec3::ONE;
        (tf, meshes.add(baked))
    } else {
        (tf, part_mesh.clone())
    }
}

/// Clone material with OR AutoZBias when the shared cache asset has ZBias≈0 (#103).
fn material_with_auto_z_bias(
    materials: &mut Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
    auto_z_bias: bool,
) -> Handle<StandardMaterial> {
    if !auto_z_bias {
        return handle.clone();
    }
    let Some(base) = materials.get(handle) else {
        return handle.clone();
    };
    let effective = apply_shape_auto_z_bias(base.depth_bias, true);
    if (effective - base.depth_bias).abs() < 1e-6 {
        return handle.clone();
    }
    let mut cloned = base.clone();
    cloned.depth_bias = effective;
    materials.add(cloned)
}

/// Session-long WORLD shape/texture cache shared across tile streams (#50 / #114).
///
/// Shape GPU assets use shared [`SessionShapeCache`] (hit/miss/ref/evict telemetry).
/// Also backs runtime LOD swaps (`update_world_scenery_lod`). Unused entries are
/// evicted on tile unload so GPU `Assets` can drop (#51).
#[derive(Resource, Default)]
pub struct WorldShapeLodCache {
    pub shapes: HashMap<PathBuf, ShapeFile>,
    pub assets_by_lod: HashMap<PathBuf, Vec<ShapeRenderAsset>>,
    /// Primary spawn assets (Bevy handles) keyed by canonical `.s` path.
    pub shape_assets: openrailsrs_bevy_scenery::SessionShapeCache<PathBuf, ShapeRenderAsset>,
    /// Decoded ACE → `Handle<Image>` shared across builds/streams.
    pub texture_images: HashMap<(PathBuf, i32), Handle<Image>>,
}

impl WorldShapeLodCache {
    pub fn session_hits(&self) -> u64 {
        self.shape_assets.hits()
    }

    pub fn session_misses(&self) -> u64 {
        self.shape_assets.misses()
    }

    pub fn session_evictions(&self) -> u64 {
        self.shape_assets.evictions()
    }
}
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::{log_step, viewer_log};
use rayon::prelude::*;

/// MSTS / Open Rails world tile size (metres).
/// Re-exported from [`crate::coordinates`] for callers that only import `world`.
pub use crate::coordinates::MSTS_TILE_SIZE_M;

/// Default mobile view radius (alias of [`crate::launch::VIEW_RADIUS_M`]).
pub const VISIBLE_RADIUS_M: f32 = crate::launch::VIEW_RADIUS_M;

/// Visible scenery/terrain radius. Prefer `OPENRAILSRS_VIEW_RADIUS_M`; legacy alias
/// `OPENRAILSRS_VISIBLE_RADIUS_M`.
pub fn visible_radius_m() -> f32 {
    crate::launch::view_radius_m()
}

/// Open Rails `WorldObjectDensity` default (0–99). Higher keeps more detail levels.
pub const DEFAULT_WORLD_OBJECT_DENSITY: u32 = 49;

/// Prefer `OPENRAILSRS_WORLD_OBJECT_DENSITY` (clamped 0–99); else [`DEFAULT_WORLD_OBJECT_DENSITY`].
pub fn world_object_density() -> u32 {
    std::env::var("OPENRAILSRS_WORLD_OBJECT_DENSITY")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_WORLD_OBJECT_DENSITY)
        .min(99)
}

/// Keep WORLD item when its `StaticDetailLevel` is within the configured density (#141).
pub fn keep_by_world_object_density(static_detail_level: u32, density: u32) -> bool {
    static_detail_level <= density
}

/// Within this radius, spawn real `.s` meshes when the file resolves; matches [`visible_radius_m`].
pub fn shape_mesh_radius_m() -> f32 {
    visible_radius_m()
}

/// Legacy bake-merge (disabled). GPU instancing (#58) replaces this path.
const ENABLE_SHAPE_INSTANCE_MERGE: bool = false;

/// Only bake merged instance meshes when the source part has at most this many vertices.
#[allow(dead_code)]
const SHAPE_INSTANCE_MERGE_MAX_VERTS: usize = 256;

/// Minimum instances before bake-merge is considered (unused while bake is off).
#[allow(dead_code)]
const SHAPE_INSTANCE_MERGE_MIN: usize = 12;

/// Procedural sleepers/rails for TrackObj without a resolvable `.s` mesh (Open Rails TSection fallback).
const SPAWN_TRACKOBJ_PROCEDURAL: bool = true;

/// Opt-in debug cuboids for TrackObj that failed mesh+procedural (`OPENRAILSRS_TRACKOBJ_PLACEHOLDERS=1`).
fn trackobj_placeholders_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_TRACKOBJ_PLACEHOLDERS").is_some_and(|v| {
        let v = v.to_string_lossy();
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

/// Why a TrackObj did not get a real `.s` mesh (#35).
#[derive(Clone, Debug)]
pub struct TrackObjFailure {
    pub tile_x: i32,
    pub tile_z: i32,
    pub uid: Option<u32>,
    pub file_name: Option<String>,
    pub section_idx: Option<u32>,
    pub reason: &'static str,
}

/// World items classified per frame during progressive spawn.
const CLASSIFY_ITEMS_PER_FRAME: usize = 12_000;

/// Unique `.s` files parsed per frame during progressive scenery spawn.
const SHAPE_PARSE_PER_FRAME: usize = 16;

/// ACE texture files decoded per frame after shape parse.
const ACE_TEXTURES_PER_FRAME: usize = 32;

/// Shape assets converted to Bevy handles per frame.
const SHAPE_ASSETS_PER_FRAME: usize = 64;

/// Shape groups turned into spawn bundles per frame.
const BUILD_QUEUE_SHAPES_PER_FRAME: usize = 32;

/// Mesh entities spawned per frame.
const SPAWN_ENTITIES_PER_FRAME: usize = 600;

/// Fraction of normal world spawn rate while in driver view (VRAM). Set `OPENRAILSRS_CAB_PAUSE_WORLD=1` for full pause.
fn driver_world_spawn_scale() -> f32 {
    if matches!(
        std::env::var("OPENRAILSRS_CAB_PAUSE_WORLD").ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    ) {
        return 0.0;
    }
    0.2
}

fn scaled_usize(base: usize, scale: f32) -> usize {
    if scale <= 0.0 {
        0
    } else {
        ((base as f32) * scale).ceil().max(1.0) as usize
    }
}

/// Forest patch metadata from a `.w` `Forest` item.
#[derive(Clone, Debug, PartialEq)]
pub struct ForestPatch {
    pub uid: u32,
    pub tree_texture: Option<String>,
    pub scale_min: f32,
    pub scale_max: f32,
    pub population: u32,
    /// Half-width of scatter patch in metres (`Area` / 2, or 0 → viewer default).
    pub patch_half_x: f32,
    pub patch_half_z: f32,
    /// Base billboard width in metres from `TreeSize`.
    pub tree_width: f32,
    /// Base billboard height in metres from `TreeSize`.
    pub tree_height: f32,
}

/// Horizontal water metadata from a `.w` `HWater` item.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterPatch {
    pub uid: u32,
    pub half_x: f32,
    pub half_z: f32,
    pub surface_y: f32,
    pub texture_file: Option<String>,
}

/// Ground decal metadata from a `.w` `Transfer` item (`FileName` is a texture).
#[derive(Clone, Debug, PartialEq)]
pub struct TransferPatch {
    pub uid: u32,
    pub width: f32,
    pub height: f32,
    pub texture: Option<String>,
}

/// Road traffic spawner from a `.w` `CarSpawner` item (poses from RDB TrItems).
#[derive(Clone, Debug, PartialEq)]
pub struct CarSpawnerPatch {
    pub uid: u32,
    pub car_frequency: f32,
    pub car_av_speed: f32,
    pub list_name: Option<String>,
    /// RDB `TrItemId`s (typically start, end).
    pub rdb_tr_item_ids: Vec<u32>,
}

/// One scenery object from a loaded `.w` tile, ready for 3D spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldObject {
    pub kind: &'static str,
    pub uid: Option<u32>,
    pub label: String,
    /// Shape filename from the world item (`FileName`), if any.
    pub shape_file: Option<String>,
    /// `TrackObj` → `TrackShape` index in `tsection.dat`.
    pub section_idx: Option<u32>,
    /// Authored Dyntrack subsections (#87).
    pub dyntrack_sections: Vec<openrailsrs_formats::DyntrackSection>,
    /// Quantized absolute Bevy position, retained for world/TDB indexing.
    pub position: Vec3,
    /// Sub-metre remainder lost when a ~10,000 km MSTS absolute coordinate is
    /// converted to `f32`. Apply only after subtracting the render focus.
    pub position_precision_offset: Vec3,
    pub rotation: Quat,
    /// Non-uniform scale from `.w` `Matrix3x3` when present.
    pub scale: Vec3,
    /// Full XNA linear from Matrix3x3 (shear); `None` for QDirection (#139).
    pub linear: Option<Mat3>,
    pub tile_x: i32,
    pub tile_z: i32,
    pub forest: Option<ForestPatch>,
    pub water: Option<WaterPatch>,
    pub transfer: Option<TransferPatch>,
    pub car_spawner: Option<CarSpawnerPatch>,
    /// Signal head units / bitmask for lamp spawn (#37).
    pub signal: Option<SignalPatch>,
    /// TDB `TrItemId`s when this object references track items (Signal, Speedpost, …).
    pub tr_item_ids: Vec<u32>,
    /// From `.w` `Tr_Watermark` — HideWire uses levels 2/3 (#36).
    pub static_detail_level: u32,
}

impl WorldObject {
    /// Rebase first, then restore the sub-metre component of the WORLD position.
    pub fn render_position(&self, focus: &RouteFocus) -> Vec3 {
        focus.scenery_to_render(self.position) + self.position_precision_offset
    }
}

/// WORLD Signal metadata for specialised lamp rendering (#37).
#[derive(Clone, Debug, PartialEq)]
pub struct SignalPatch {
    pub uid: u32,
    pub signal_sub_obj: u32,
    pub units: Vec<openrailsrs_formats::SignalUnitRef>,
}

/// Optional XZ window applied while materializing `.w` items (#59).
#[derive(Clone, Copy, Debug)]
pub struct WorldItemWindow {
    pub center: Vec3,
    pub radius_m: f32,
}

impl WorldItemWindow {
    #[inline]
    pub fn contains_xz(&self, world: Vec3) -> bool {
        let dx = world.x - self.center.x;
        let dz = world.z - self.center.z;
        dx * dx + dz * dz <= self.radius_m * self.radius_m
    }
}

/// Keep radius for WORLD *items* given the tile-stream radius (matches
/// [`crate::launch::scenery_content_radius_m`] relative to viewing distance).
pub fn world_item_keep_radius_m(tile_radius_m: f32) -> f32 {
    if tile_radius_m.is_finite() && tile_radius_m < f32::MAX / 4.0 {
        tile_radius_m + MSTS_TILE_SIZE_M as f32
    } else {
        f32::MAX
    }
}

/// Pending TrItem index updates produced by WORLD tile stream/unload (#61).
#[derive(Clone, Debug, Default)]
pub struct TrItemIndexDelta {
    /// Tiles newly present in [`WorldScene::loaded_tiles`] (may have zero TrItem objects).
    pub added_tiles: std::collections::HashSet<(i32, i32)>,
    pub removed_tiles: std::collections::HashSet<(i32, i32)>,
    /// Signal/Speedpost/SoundRegion objects appended with those tiles (cloned, small set).
    pub added_objects: Vec<WorldObject>,
}

impl TrItemIndexDelta {
    pub fn is_empty(&self) -> bool {
        self.added_tiles.is_empty()
            && self.removed_tiles.is_empty()
            && self.added_objects.is_empty()
    }

    pub fn clear(&mut self) {
        self.added_tiles.clear();
        self.removed_tiles.clear();
        self.added_objects.clear();
    }
}

/// All world objects discovered under a route's `WORLD/` (or `world/`) folder.
#[derive(Resource, Clone, Default)]
pub struct WorldScene {
    pub tiles_loaded: usize,
    /// Tiles successfully parsed into this scene (coverage for TrItem audits).
    pub loaded_tiles: std::collections::HashSet<(i32, i32)>,
    pub items: Vec<WorldObject>,
    /// Items skipped during materialization because they were outside [`WorldItemWindow`].
    pub items_skipped_out_of_window: usize,
    /// WORLD objects omitted at parse time (no Position / no Matrix3x3|QDirection) (#140).
    pub items_skipped_invalid_pose: usize,
    /// WORLD objects omitted by `StaticDetailLevel` vs [`world_object_density`] (#141).
    pub items_skipped_by_density: usize,
    /// Stream/unload deltas for incremental [`crate::tr_item_index::TrItemWorldIndex`] sync.
    pub tr_item_delta: TrItemIndexDelta,
    /// `.w` load outcomes for shared [`MstsLoadDiagnostics`] (#54).
    pub load_diag: MstsLoadDiagnostics,
}

impl WorldScene {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// World-space centre of loaded scenery (for culling / terrain when the track graph has no `x_m`/`y_m`).
    pub fn position_center(&self) -> Option<Vec3> {
        if self.items.is_empty() {
            return None;
        }
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for obj in &self.items {
            min = min.min(obj.position);
            max = max.max(obj.position);
        }
        Some((min + max) * 0.5)
    }

    /// Drop objects outside the visible radius before progressive spawn (avoids classifying distant items).
    pub fn retain_within_visible_radius(&mut self, focus: &RouteFocus, radius_m: f32) {
        let before = self.items.len();
        self.items
            .retain(|obj| focus.horizontal_distance(obj.position) <= radius_m);
        let removed = before.saturating_sub(self.items.len());
        if removed > 0 {
            viewer_log!(
                "openrailsrs-viewer3d: dropped {removed} world object(s) at load (>{radius_m:.0}m from focus)"
            );
        }
    }

    /// Record tiles/objects added by streaming (not used for the initial full load).
    pub fn note_tr_item_tiles_added(&mut self, tiles: impl IntoIterator<Item = (i32, i32)>) {
        self.tr_item_delta.added_tiles.extend(tiles);
    }

    pub fn note_tr_item_objects_added(&mut self, objects: impl IntoIterator<Item = WorldObject>) {
        self.tr_item_delta.added_objects.extend(objects);
    }

    pub fn note_tr_item_tiles_removed(&mut self, tiles: impl IntoIterator<Item = (i32, i32)>) {
        for tile in tiles {
            self.tr_item_delta.added_tiles.remove(&tile);
            self.tr_item_delta
                .added_objects
                .retain(|o| (o.tile_x, o.tile_z) != tile);
            self.tr_item_delta.removed_tiles.insert(tile);
        }
    }
}

/// View/cull centre: MSTS world bbox when present, else track graph centre.
#[derive(Resource, Clone, Copy, Debug)]
pub struct RouteFocus {
    pub center: Vec3,
    /// Terrain MSL (metres) at route centre; use with [`Self::to_render_surface`] only.
    pub height_origin: f32,
}

impl RouteFocus {
    pub fn from_scene_and_world(scene: &TrackScene, world: &WorldScene) -> Self {
        Self::from_scene_world_and_elevation(scene, world, None)
    }

    pub fn from_scene_world_and_elevation(
        scene: &TrackScene,
        world: &WorldScene,
        elevation: Option<&TerrainElevation>,
    ) -> Self {
        let center = world.position_center().unwrap_or(scene.bounds.center);
        let height_origin = elevation
            .and_then(|t| t.sample_world_y(center.x, center.z))
            .unwrap_or(center.y);
        Self {
            center,
            height_origin,
        }
    }

    /// General world-space position to Bevy render space using the scenery bbox centre.
    /// For Y this subtracts `center.y`; prefer [`Self::to_render_surface`] (uses
    /// `height_origin`) for consistent height with terrain tiles.
    pub fn to_render(&self, world: Vec3) -> Vec3 {
        Vec3::new(
            world.x - self.center.x,
            world.y - self.center.y,
            world.z - self.center.z,
        )
    }

    /// World points on the terrain surface (MSL from `_Y.RAW` / [`crate::terrain::ground_y_at`]).
    pub fn to_render_surface(&self, world: Vec3) -> Vec3 {
        Vec3::new(
            world.x - self.center.x,
            world.y - self.height_origin,
            world.z - self.center.z,
        )
    }

    /// Absolute MSTS / Open Rails height for a `.w` `Position.Y` (#64).
    ///
    /// WORLD Y shares the same vertical datum as TDB and terrain MSL samples.
    /// Do **not** remap through `(scenery_y - center.y)` — that flattens embankments
    /// and platforms onto the RAW heightfield (Chiltern rail ~35.8 vs terrain ~28.5).
    pub fn scenery_y_to_msl(&self, scenery_y: f32) -> f32 {
        let _ = self;
        scenery_y
    }

    /// MSTS world position from a `.w` item → Bevy render space.
    ///
    /// XZ are recentred on [`Self::center`]; Y is absolute height minus
    /// [`Self::height_origin`] (terrain sample at focus), preserving authored
    /// vertical offsets vs the ground plane (#64).
    pub fn scenery_to_render(&self, scenery_world: Vec3) -> Vec3 {
        self.to_render_surface(scenery_world)
    }

    /// Build focus at an explicit MSTS world centre, sampling terrain MSL when available.
    pub fn at_world_center(center: Vec3, elevation: Option<&TerrainElevation>) -> Self {
        let height_origin = elevation
            .and_then(|t| t.sample_world_y(center.x, center.z))
            .unwrap_or(center.y);
        Self {
            center,
            height_origin,
        }
    }

    /// Horizontal distance from route centre in render space (for culling).
    pub fn horizontal_distance(&self, world: Vec3) -> f32 {
        horizontal_distance_xz(self.center, world)
    }
}

/// Horizontal XZ distance from an arbitrary world-space centre (view window / camera).
#[inline]
pub fn horizontal_distance_xz(center: Vec3, world: Vec3) -> f32 {
    Vec2::new(world.x - center.x, world.z - center.z).length()
}

/// Whether a world object should be culled for being outside [`VISIBLE_RADIUS_M`].
#[inline]
pub fn should_cull_world_object(focus: &RouteFocus, world: Vec3) -> bool {
    should_cull_world_object_at(focus.center, world)
}

/// Cull against a mobile centre (live [`crate::view_window::ViewWindow`], free camera, …).
#[inline]
pub fn should_cull_world_object_at(center: Vec3, world: Vec3) -> bool {
    horizontal_distance_xz(center, world) > visible_radius_m()
}

/// Translates abstract graph coordinates into MSTS world space when the two diverge.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RouteWorldOffset {
    pub delta: Vec3,
}

impl RouteWorldOffset {
    /// Minimum horizontal (XZ) separation before applying a graph→scenery shift.
    const ALIGN_THRESHOLD_XZ_M: f32 = 10000.0;

    pub fn from_scene_and_world(scene: &TrackScene, world: &WorldScene) -> Self {
        let graph_center = scene.bounds.center;
        let Some(world_center) = world.position_center() else {
            return Self::default();
        };
        let delta_xz = Vec2::new(
            world_center.x - graph_center.x,
            world_center.z - graph_center.z,
        );
        if delta_xz.length() <= Self::ALIGN_THRESHOLD_XZ_M {
            return Self::default();
        }
        let delta = Vec3::new(delta_xz.x, 0.0, delta_xz.y);
        viewer_log!(
            "openrailsrs-viewer3d: aligning track/train to MSTS scenery (offset {:.0}, {:.0}, {:.0} m)",
            delta.x,
            delta.y,
            delta.z
        );
        Self { delta }
    }
}

/// Convert MSTS tile-local coordinates to Bevy world space (Y up).
///
/// Delegates to [`crate::coordinates::msts_to_bevy`]; kept here as a public
/// re-export so existing callers in `world.rs` and other modules don't break.
pub fn msts_to_bevy(tile_x: i32, tile_z: i32, local: openrailsrs_formats::Vec3) -> Vec3 {
    msts_tile_local_to_bevy(tile_x, tile_z, local)
}

/// Remainder lost by the absolute `f64 → f32` MSTS conversion.
///
/// Near Chiltern (`|x| ≈ 12.5 Mm`, `|z| ≈ 30.6 Mm`) an absolute `f32` advances
/// in 1–2 metre steps. Keeping this remainder until after rebasing prevents
/// adjacent authored track pieces from separating.
fn msts_position_precision_offset(
    tile_x: i32,
    tile_z: i32,
    local: openrailsrs_formats::Vec3,
    quantized: Vec3,
) -> Vec3 {
    let precise = DVec3::new(
        tile_x as f64 * MSTS_TILE_SIZE_M + local.x,
        local.y,
        -(tile_z as f64 * MSTS_TILE_SIZE_M + local.z),
    );
    (precise - quantized.as_dvec3()).as_vec3()
}

/// MSTS `Matrix3x3` → Bevy rotation.
pub fn matrix3x3_to_quat(m: &[f64; 9]) -> Quat {
    matrix3x3_to_rotation_scale(m).0
}

// `qdir_to_quat` and `matrix3x3_to_rotation_scale` are imported from `crate::coordinates`.
// `.w` placement: shared [`world_item_placement`] (#115).

fn object_label(item: &WorldItem) -> String {
    item.file_name()
        .map(str::to_string)
        .unwrap_or_else(|| item.kind().to_string())
}

/// Why [`try_object_from_item`] rejected a WORLD item during materialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectSkip {
    OutOfWindow,
    Density,
}

/// Materialize a `.w` item, optionally rejecting it before heavy forest/water payloads (#59).
///
/// Returns `Ok(None)` when the item has no position; `Err` when skipped by window/density.
fn try_object_from_item(
    tile_x: i32,
    tile_z: i32,
    item: &WorldItem,
    window: Option<WorldItemWindow>,
    density: u32,
) -> Result<Option<WorldObject>, ObjectSkip> {
    if !keep_by_world_object_density(item.static_detail_level(), density) {
        return Err(ObjectSkip::Density);
    }
    let Some(local_position) = item.position() else {
        return Ok(None);
    };
    let Some(placement) = world_item_placement(tile_x, tile_z, item) else {
        return Ok(None);
    };
    let position = placement.pose.position;
    let position_precision_offset =
        msts_position_precision_offset(tile_x, tile_z, local_position, position);
    if let Some(window) = window {
        if !window.contains_xz(position) {
            return Err(ObjectSkip::OutOfWindow);
        }
    }
    let rotation = placement.pose.rotation;
    let scale = placement.pose.scale;
    let linear = placement.pose.linear;
    let forest = match item {
        WorldItem::Forest {
            uid,
            tree_texture,
            scale_range,
            patch_size,
            tree_size,
            population,
            ..
        } => {
            let (scale_min, scale_max) = scale_range
                .map(|r| (r[0] as f32, r[1] as f32))
                .unwrap_or((0.8, 1.2));
            let (patch_half_x, patch_half_z) = patch_size
                .map(|a| ((a[0] * 0.5) as f32, (a[1] * 0.5) as f32))
                .unwrap_or((0.0, 0.0));
            let (tree_width, tree_height) = tree_size
                .map(|s| (s[0] as f32, s[1] as f32))
                .unwrap_or((0.0, 0.0));
            Some(ForestPatch {
                uid: *uid,
                tree_texture: tree_texture.clone(),
                scale_min,
                scale_max,
                population: *population,
                patch_half_x,
                patch_half_z,
                tree_width,
                tree_height,
            })
        }
        _ => None,
    };
    let water = match item {
        WorldItem::HWater {
            uid,
            file_name,
            position,
            size,
            ..
        } => Some(WaterPatch {
            uid: *uid,
            half_x: (size[0] * 0.5) as f32,
            half_z: (size[1] * 0.5) as f32,
            surface_y: position.y as f32,
            texture_file: file_name.clone(),
        }),
        _ => None,
    };
    let transfer = match item {
        WorldItem::Transfer {
            uid,
            file_name,
            width,
            height,
            ..
        } => Some(TransferPatch {
            uid: *uid,
            width: (*width).max(0.5) as f32,
            height: (*height).max(0.5) as f32,
            texture: file_name.clone(),
        }),
        _ => None,
    };
    let car_spawner = match item {
        WorldItem::CarSpawner {
            uid,
            car_frequency,
            car_av_speed,
            list_name,
            rdb_tr_item_ids,
            ..
        } => Some(CarSpawnerPatch {
            uid: *uid,
            car_frequency: *car_frequency as f32,
            car_av_speed: *car_av_speed as f32,
            list_name: list_name.clone(),
            rdb_tr_item_ids: rdb_tr_item_ids.clone(),
        }),
        _ => None,
    };
    let signal = match item {
        WorldItem::Signal {
            uid,
            signal_sub_obj,
            signal_units,
            ..
        } => Some(SignalPatch {
            uid: *uid,
            signal_sub_obj: *signal_sub_obj,
            units: signal_units.clone(),
        }),
        _ => None,
    };
    Ok(Some(WorldObject {
        kind: item.kind(),
        uid: item.uid(),
        label: object_label(item),
        shape_file: item.file_name().map(str::to_string),
        section_idx: item.section_idx(),
        dyntrack_sections: item.dyntrack_sections().to_vec(),
        position,
        position_precision_offset,
        rotation,
        scale,
        linear,
        tile_x,
        tile_z,
        forest,
        water,
        transfer,
        car_spawner,
        signal,
        tr_item_ids: item.tr_item_ids(),
        static_detail_level: item.static_detail_level(),
    }))
}

/// Scan `route_dir/WORLD` and `route_dir/world` for `.w` files and parse them.
pub fn load_world_from_route_dir(route_dir: &Path) -> WorldScene {
    load_world_from_route_dir_near(route_dir, None, f32::MAX)
}

pub fn tile_center_distance_m(tile_x: i32, tile_z: i32, center: Vec3) -> f32 {
    let tile = MSTS_TILE_SIZE_M as f32;
    let half = tile * 0.5;
    let (ox, oz) = msts_tile_world_origin(tile_x, tile_z);
    let tcx = ox + half;
    let tcz = oz + half;
    Vec2::new(tcx - center.x, tcz - center.z).length()
}

/// `(display_tile_x, display_tile_z, path)` for each `.w` tile within `radius_m` of `center`.
pub fn discover_world_tile_entries(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
) -> Vec<(i32, i32, PathBuf)> {
    let tile = MSTS_TILE_SIZE_M as f32;
    let extra = tile;
    if let Some(c) = center {
        let center_tx = msts_tile_x_index_for_coord(c.x);
        let center_tz = msts_tile_z_index_for_coord(c.z);
        let radius_tiles = (radius_m / tile).ceil() as i32 + 1;
        let mut out = Vec::new();
        for dtx in -radius_tiles..=radius_tiles {
            for dtz in -radius_tiles..=radius_tiles {
                let tile_x = center_tx + dtx;
                let tile_z = center_tz + dtz;
                if tile_center_distance_m(tile_x, tile_z, c) > radius_m + extra {
                    continue;
                }
                if let Some(path) = world_tile_path_for_coords(route_dir, tile_x, tile_z) {
                    out.push((tile_x, tile_z, path));
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    discover_world_files(route_dir)
        .into_iter()
        .filter_map(|path| {
            let (tile_x, tile_z) = parse_world_w_tile_xz(&path)?;
            if let Some(c) = center {
                if tile_center_distance_m(tile_x, tile_z, c) > radius_m + extra {
                    return None;
                }
            }
            Some((tile_x, tile_z, path))
        })
        .collect()
}

fn world_tile_path_for_coords(route_dir: &Path, tile_x: i32, tile_z: i32) -> Option<PathBuf> {
    openrailsrs_formats::resolve_world_tile_file(route_dir, tile_x, tile_z)
}

/// MSTS world XZ centre from `.w` filenames (no parse) — used before the route anchor is known.
pub fn world_tile_center_hint(route_dir: &Path) -> Option<Vec3> {
    let paths = discover_world_files(route_dir);
    let mut count = 0usize;
    let mut sum_x = 0.0f64;
    let mut sum_z = 0.0f64;
    let half = MSTS_TILE_SIZE_M * 0.5;
    for path in paths {
        let Some((tile_x, tile_z)) = parse_world_w_tile_xz(&path) else {
            continue;
        };
        let (ox, oz) = msts_tile_world_origin(tile_x, tile_z);
        sum_x += f64::from(ox) + half;
        sum_z += f64::from(oz) + half;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let n = count as f64;
    Some(Vec3::new((sum_x / n) as f32, 0.0, (sum_z / n) as f32))
}

/// Parse only `.w` tiles within `radius_m` of `center` (Open Rails tile streaming at load).
///
/// When `center` is set and `radius_m` is finite, items outside
/// [`world_item_keep_radius_m`]`(radius_m)` are never materialized into [`WorldScene`] (#59).
pub fn load_world_from_route_dir_near(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
) -> WorldScene {
    load_world_from_route_dir_near_filtered(route_dir, center, radius_m, true)
}

/// Same as [`load_world_from_route_dir_near`], with optional early item-window filter.
pub fn load_world_from_route_dir_near_filtered(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
    filter_items: bool,
) -> WorldScene {
    let mut entries = discover_world_tile_entries(route_dir, center, radius_m);
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let item_window = if filter_items {
        center
            .filter(|_| radius_m.is_finite() && radius_m < f32::MAX / 2.0)
            .map(|c| WorldItemWindow {
                center: c,
                radius_m: world_item_keep_radius_m(radius_m),
            })
    } else {
        None
    };

    let mut scene = WorldScene::default();
    let mut skip_count = 0usize;
    let mut skip_sample: Option<String> = None;
    for (_display_x, _display_z, path) in entries {
        match append_world_tile_file(&mut scene, &path, item_window) {
            Ok(()) => {
                scene
                    .load_diag
                    .record_path_loaded(&path, MstsAssetKind::World);
            }
            Err(err) => {
                skip_count += 1;
                scene.load_diag.record_path_failed(
                    &path,
                    MstsAssetKind::World,
                    MstsLoadCause::Parse,
                    err,
                );
                if skip_sample.is_none() {
                    skip_sample = Some(path.display().to_string());
                }
            }
        }
    }
    if let Some(c) = center.filter(|_| radius_m.is_finite() && radius_m < f32::MAX / 2.0) {
        viewer_log!(
            "openrailsrs-viewer3d: loaded {} world tile(s) ({} item(s), {} skipped out of window @{:.0}m) within {:.0}m of ({:.0},{:.0})",
            scene.tiles_loaded,
            scene.items.len(),
            scene.items_skipped_out_of_window,
            item_window.map(|w| w.radius_m).unwrap_or(0.0),
            radius_m,
            c.x,
            c.z
        );
    }
    if skip_count > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: skipped {skip_count} world tile(s){}",
            skip_sample
                .as_ref()
                .map(|s| format!(" (e.g. {s})"))
                .unwrap_or_default()
        );
    }
    scene
}

fn append_world_tile_file(
    scene: &mut WorldScene,
    path: &Path,
    item_window: Option<WorldItemWindow>,
) -> Result<(), String> {
    let world = WorldFile::from_path(path).map_err(|e| e.to_string())?;
    append_world_tile(scene, &world, item_window);
    Ok(())
}

/// Append objects from an already-parsed [`WorldFile`] (AssetServer / bootstrap).
pub(crate) fn append_world_tile(
    scene: &mut WorldScene,
    world: &WorldFile,
    item_window: Option<WorldItemWindow>,
) {
    append_world_tile_with_density(scene, world, item_window, world_object_density());
}

pub(crate) fn append_world_tile_with_density(
    scene: &mut WorldScene,
    world: &WorldFile,
    item_window: Option<WorldItemWindow>,
    density: u32,
) {
    let key = (world.tile_x, world.tile_z);
    if scene.loaded_tiles.insert(key) {
        scene.tiles_loaded = scene.loaded_tiles.len();
    }
    scene.items_skipped_invalid_pose += world.skipped_invalid_pose;
    for item in &world.items {
        match try_object_from_item(world.tile_x, world.tile_z, item, item_window, density) {
            Ok(Some(obj)) => scene.items.push(obj),
            Ok(None) => {}
            Err(ObjectSkip::OutOfWindow) => scene.items_skipped_out_of_window += 1,
            Err(ObjectSkip::Density) => scene.items_skipped_by_density += 1,
        }
    }
}

fn discover_world_files(route_dir: &Path) -> Vec<PathBuf> {
    openrailsrs_formats::scan_world_tile_files(route_dir)
        .into_iter()
        .map(|(_, _, path)| path)
        .collect()
}

fn kind_color(kind: &str) -> Color {
    match kind {
        "Static" => Color::srgb(0.6, 0.65, 0.75),
        "Forest" => Color::srgb(0.22, 0.72, 0.28),
        "TrackObj" => Color::srgb(0.78, 0.48, 0.18),
        "Signal" => Color::srgb(1.0, 0.85, 0.2),
        "Dyntrack" => Color::srgb(0.58, 0.32, 0.82),
        "Pickup" => Color::srgb(0.55, 0.45, 0.35),
        "Hazard" => Color::srgb(0.85, 0.35, 0.25),
        _ => Color::srgb(0.45, 0.45, 0.5),
    }
}

fn box_size_for_kind(kind: &str, base: f32) -> Vec3 {
    match kind {
        "Forest" => Vec3::new(base * 1.6, base * 2.4, base * 1.6),
        "Static" => Vec3::new(base * 1.4, base * 1.8, base * 1.4),
        "TrackObj" | "Dyntrack" => Vec3::new(base * 2.4, base * 0.35, base * 0.35),
        _ => Vec3::splat(base),
    }
}

/// Open Rails treats Platform/Siding as TrItem labels only (`Scenery.cs`), not scenery meshes.
/// Spawning them as placeholders/shapes produces giant grey cubes after #86.
fn is_tr_item_label_only(kind: &str) -> bool {
    matches!(kind, "Platform" | "Siding")
}

/// Kinds that may load a `.s` mesh but must never fall back to a debug cuboid.
fn suppress_world_placeholder(kind: &str) -> bool {
    matches!(
        kind,
        "Platform" | "Siding" | "Other" | "Speedpost" | "SoundRegion"
    )
}

fn shape_eligible(obj: &WorldObject) -> bool {
    if is_tr_item_label_only(obj.kind) {
        return false;
    }
    let Some(f) = trackobj_effective_shape_file(obj) else {
        return false;
    };
    let lower = f.to_ascii_lowercase();
    if obj.kind == "Hazard" {
        return lower.ends_with(".haz") || lower.ends_with(".s");
    }
    // CollideObject/LevelCr/Gantry arrive as `Other` with a `.s` FileName (#86).
    lower.ends_with(".s")
}

fn trackobj_effective_shape_file(obj: &WorldObject) -> Option<String> {
    obj.shape_file.as_ref().filter(|f| !f.is_empty()).cloned()
}

fn trackobj_procedural_links(
    assets: &RouteAssets,
    shape_idx: u32,
    mode: ViewerSceneryMode,
) -> Vec<openrailsrs_formats::TrackProceduralLink> {
    let catalog = assets.tsection();
    if mode.is_track_focused() {
        if catalog
            .shapes
            .get(&shape_idx)
            .is_some_and(|s| s.main_route.is_some())
        {
            return catalog.procedural_links_all_paths(shape_idx);
        }
        return catalog.procedural_links_primary_path(shape_idx);
    }
    catalog.procedural_links(shape_idx)
}

fn trackobj_procedural_segments(
    obj: &WorldObject,
    render_pos: Vec3,
    assets: &RouteAssets,
    mode: ViewerSceneryMode,
) -> Vec<crate::dyntrack::ProceduralTrackSegment> {
    let rotation = assets.refine_trackobj_rotation(obj.section_idx, obj.position, obj.rotation);
    let Some(shape_idx) = obj.section_idx else {
        if mode.is_track_focused() {
            return Vec::new();
        }
        return vec![crate::dyntrack::ProceduralTrackSegment {
            position: render_pos,
            rotation,
            length_m: Some(crate::dyntrack::MSTS_DEFAULT_SECTION_LENGTH_M),
            half_gauge_m: Some(crate::dyntrack::MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m: None,
            curve_angle_deg: None,
        }];
    };
    let links = trackobj_procedural_links(assets, shape_idx, mode);
    if links.is_empty() {
        return Vec::new();
    }
    links
        .into_iter()
        .map(|link| {
            // `shape_local_offset` comes from tsection.dat and is in MSTS local space (+Z forward).
            // It must be Z-flipped before being used as a Bevy-space offset vector.
            let offset = msts_local_offset_to_bevy(
                link.shape_local_offset[0] as f32,
                link.shape_local_offset[1] as f32,
                link.shape_local_offset[2] as f32,
            );
            let link_rot = Quat::from_rotation_y(link.shape_local_yaw_deg.to_radians() as f32);
            crate::dyntrack::ProceduralTrackSegment {
                position: render_pos + rotation * offset,
                rotation: rotation * link_rot,
                length_m: Some(link.dims.length_m as f32),
                half_gauge_m: Some(link.dims.half_gauge_m as f32),
                curve_radius_m: link.dims.curve_radius_m.map(|v| v as f32),
                curve_angle_deg: link.dims.curve_angle_deg.map(|v| v as f32),
            }
        })
        .collect()
}

fn resolve_object_shape_path(obj: &WorldObject, assets: &RouteAssets) -> Option<PathBuf> {
    if obj.kind == "TrackObj" {
        return assets.resolve_trackobj_shape(obj.shape_file.as_deref(), obj.section_idx);
    }
    let file_name = trackobj_effective_shape_file(obj).or(obj.shape_file.clone())?;
    assets.resolve_world_shape(obj.kind, &file_name)
}

struct MergedBoxGroup {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    color: Color,
}

fn push_cuboid(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    tf: &Transform,
    size: Vec3,
) {
    let hx = size.x * 0.5;
    let hy = size.y * 0.5;
    let hz = size.z * 0.5;
    let local = [
        Vec3::new(-hx, -hy, -hz),
        Vec3::new(hx, -hy, -hz),
        Vec3::new(hx, hy, -hz),
        Vec3::new(-hx, hy, -hz),
        Vec3::new(-hx, -hy, hz),
        Vec3::new(hx, -hy, hz),
        Vec3::new(hx, hy, hz),
        Vec3::new(-hx, hy, hz),
    ];
    let world: [Vec3; 8] = local.map(|c| tf.transform_point(c));
    let faces: [(usize, usize, usize, usize, Vec3); 6] = [
        (4, 5, 6, 7, Vec3::new(0.0, 0.0, 1.0)),
        (1, 0, 3, 2, Vec3::new(0.0, 0.0, -1.0)),
        (3, 7, 6, 2, Vec3::new(0.0, 1.0, 0.0)),
        (0, 1, 5, 4, Vec3::new(0.0, -1.0, 0.0)),
        (1, 2, 6, 5, Vec3::new(1.0, 0.0, 0.0)),
        (0, 4, 7, 3, Vec3::new(-1.0, 0.0, 0.0)),
    ];
    for (v0, v1, v2, v3, normal) in &faces {
        let base = positions.len() as u32;
        let wn = tf.rotation * *normal;
        let wn_arr = [wn.x, wn.y, wn.z];
        positions.push(world[*v0].to_array());
        positions.push(world[*v1].to_array());
        positions.push(world[*v2].to_array());
        positions.push(world[*v3].to_array());
        for _ in 0..4 {
            normals.push(wn_arr);
        }
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        uvs.push([1.0, 1.0]);
        uvs.push([0.0, 1.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn append_mesh_with_transform(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    src: &Mesh,
    tf: &Transform,
) {
    let Some(verts) = src
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|attribute| attribute.as_float3())
    else {
        return;
    };
    let norm_attr = src
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(|attribute| attribute.as_float3());
    let uv_attr = src.attribute(Mesh::ATTRIBUTE_UV_0);
    let base = positions.len() as u32;
    for v in verts {
        positions.push(tf.transform_point(Vec3::from(*v)).to_array());
    }
    match norm_attr {
        Some(norms) if norms.len() == verts.len() => {
            for n in norms {
                let wn = (tf.rotation * Vec3::from(*n)).normalize_or_zero();
                normals.push(wn.to_array());
            }
        }
        _ => normals.extend(std::iter::repeat_n([0.0, 1.0, 0.0], verts.len())),
    }
    match uv_attr {
        Some(VertexAttributeValues::Float32x2(uvs_src)) if uvs_src.len() == verts.len() => {
            uvs.extend(uvs_src.iter().copied());
        }
        _ => uvs.extend(std::iter::repeat_n([0.0, 0.0], verts.len())),
    }
    match src.indices() {
        Some(Indices::U32(idxs)) => {
            let vert_count = positions.len() as u32 - base;
            indices.extend(
                idxs.iter()
                    .filter_map(|&i| (i < vert_count).then_some(base + i)),
            );
        }
        Some(Indices::U16(idxs)) => {
            let vert_count = positions.len() as u32 - base;
            indices.extend(
                idxs.iter()
                    .filter_map(|&i| (u32::from(i) < vert_count).then_some(base + u32::from(i))),
            );
        }
        None => {}
    }
}

fn build_merged_instance_mesh(
    meshes: &Assets<Mesh>,
    part_mesh: &Handle<Mesh>,
    transforms: &[Transform],
) -> Option<Mesh> {
    let src = meshes.get(part_mesh)?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for tf in transforms {
        append_mesh_with_transform(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            src,
            tf,
        );
    }
    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

type ShapeSpawnBundle = (
    Transform,
    Mesh3d,
    MeshMaterial3d<StandardMaterial>,
    Name,
    WorldSceneryLod,
    WorldTileBound,
);

/// WORLD shape part with loop animation (#34). Spawned individually (not `spawn_batch`).
type AnimatedShapeSpawnBundle = (
    Transform,
    Mesh3d,
    MeshMaterial3d<StandardMaterial>,
    Name,
    WorldSceneryLod,
    WorldTileBound,
    ShapeAnimState,
    ShapeAnimBinding,
);

/// GPU-instanced static opaque WORLD group (#58).
type InstancedShapeSpawnBundle = (
    Transform,
    Mesh3d,
    Name,
    crate::world_instancing::WorldInstancedGroup,
    WorldTileBound,
    crate::world_instancing::WorldInstanceBuffer,
    crate::world_instancing::WorldInstanceAppearance,
    bevy::camera::primitives::Aabb,
);

#[derive(Resource)]
pub struct WorldSpawnProgress {
    phase: WorldSpawnPhase,
    started: Instant,
    item_index: usize,
    shape_path_cache: std::collections::HashMap<String, Option<PathBuf>>,
    shape_instances: std::collections::HashMap<PathBuf, Vec<ShapeInstancePlacement>>,
    merged_boxes: std::collections::HashMap<String, MergedBoxGroup>,
    culled_count: usize,
    trackobj_seen: usize,
    trackobj_resolved: usize,
    trackobj_procedural_objects: usize,
    trackobj_failed: usize,
    trackobj_failures: Vec<TrackObjFailure>,
    trackobj_procedural: Vec<crate::dyntrack::ProceduralTrackSegment>,
    /// Overhead wire centreline segments (#36).
    trackobj_wire: Vec<crate::dyntrack::ProceduralTrackSegment>,
    placeholder_base: f32,
    shape_fallback_color: Color,
    shape_fallback_material: Option<Handle<StandardMaterial>>,
    parsed_shapes: Vec<(PathBuf, Option<crate::shapes::LoadedShape>)>,
    shape_load_paths: Vec<PathBuf>,
    shape_parse_index: usize,
    texture_paths: Vec<PathBuf>,
    texture_prefetch_index: usize,
    ace_cache: std::collections::HashMap<PathBuf, openrailsrs_ace::AceFile>,
    shape_cache: std::collections::HashMap<PathBuf, ShapeRenderAsset>,
    parsed_shape_files: std::collections::HashMap<PathBuf, ShapeFile>,
    shape_lod_assets: std::collections::HashMap<PathBuf, Vec<ShapeRenderAsset>>,
    texture_image_cache: std::collections::HashMap<(PathBuf, i32), Handle<Image>>,
    asset_build_index: usize,
    instance_paths: Vec<PathBuf>,
    build_queue_index: usize,
    spawn_queue: Vec<ShapeSpawnBundle>,
    anim_spawn_queue: Vec<AnimatedShapeSpawnBundle>,
    instanced_spawn_queue: Vec<InstancedShapeSpawnBundle>,
    spawn_index: usize,
    shape_mesh_count: usize,
    shape_texture_count: usize,
    merged_shape_groups: usize,
    instanced_groups: usize,
    instanced_instances: usize,
    loading_shapes_started: Option<Instant>,
    build_queue_started: Option<Instant>,
    scenery_audit: Option<crate::scenery_audit::ShapeAuditSummary>,
    paused_for_driver_log: bool,
    /// True after [`hydrate_spawn_from_session`] ran for this cycle.
    session_hydrated: bool,
    cache_hits: usize,
    cache_misses: usize,
    /// Shape/ACE/TrackObj load bag merged into the app Resource at spawn end (#54).
    load_diag: MstsLoadDiagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorldSpawnPhase {
    Classifying,
    LoadingShapes,
    BuildingQueue,
    SpawningEntities,
    SpawningPlaceholders,
}

impl WorldSpawnProgress {
    pub fn status_text(&self) -> String {
        match self.phase {
            WorldSpawnPhase::Classifying => {
                format!("Clasificando objetos 3D ({})", self.item_index)
            }
            WorldSpawnPhase::LoadingShapes => {
                let total = self.shape_load_paths.len();
                match self
                    .shape_parse_index
                    .checked_mul(100)
                    .and_then(|n| n.checked_div(total))
                {
                    Some(pct) => format!(
                        "Cargando mallas 3D y texturas ({}% — {}/{})",
                        pct, self.shape_parse_index, total
                    ),
                    None => "Cargando mallas 3D y texturas...".into(),
                }
            }
            WorldSpawnPhase::BuildingQueue => "Agrupando instancias en GPU...".into(),
            WorldSpawnPhase::SpawningEntities | WorldSpawnPhase::SpawningPlaceholders => {
                let total = (self.spawn_queue.len() + self.instanced_spawn_queue.len()).max(1);
                let current = self.spawn_index.min(total);
                let pct = (current * 100) / total;
                format!(
                    "Instanciando objetos en el mundo ({}% — {}/{})",
                    pct, current, total
                )
            }
        }
    }

    fn new(placeholder_base: f32) -> Self {
        Self::new_from_item_index(placeholder_base, 0)
    }

    fn new_from_item_index(placeholder_base: f32, item_index: usize) -> Self {
        Self {
            phase: WorldSpawnPhase::Classifying,
            started: Instant::now(),
            item_index,
            shape_path_cache: std::collections::HashMap::new(),
            shape_instances: std::collections::HashMap::new(),
            merged_boxes: std::collections::HashMap::new(),
            culled_count: 0,
            trackobj_seen: 0,
            trackobj_resolved: 0,
            trackobj_procedural_objects: 0,
            trackobj_failed: 0,
            trackobj_failures: Vec::new(),
            trackobj_procedural: Vec::new(),
            trackobj_wire: Vec::new(),
            placeholder_base,
            shape_fallback_color: Color::srgb(0.72, 0.55, 0.42),
            shape_fallback_material: None,
            parsed_shapes: Vec::new(),
            shape_load_paths: Vec::new(),
            shape_parse_index: 0,
            texture_paths: Vec::new(),
            texture_prefetch_index: 0,
            ace_cache: std::collections::HashMap::new(),
            shape_cache: std::collections::HashMap::new(),
            parsed_shape_files: std::collections::HashMap::new(),
            shape_lod_assets: std::collections::HashMap::new(),
            texture_image_cache: std::collections::HashMap::new(),
            asset_build_index: 0,
            instance_paths: Vec::new(),
            build_queue_index: 0,
            spawn_queue: Vec::new(),
            anim_spawn_queue: Vec::new(),
            instanced_spawn_queue: Vec::new(),
            spawn_index: 0,
            shape_mesh_count: 0,
            shape_texture_count: 0,
            merged_shape_groups: 0,
            instanced_groups: 0,
            instanced_instances: 0,
            loading_shapes_started: None,
            build_queue_started: None,
            scenery_audit: None,
            paused_for_driver_log: false,
            session_hydrated: false,
            cache_hits: 0,
            cache_misses: 0,
            load_diag: MstsLoadDiagnostics::default(),
        }
    }
}

/// Reuse session shape/texture handles for paths already loaded; leave misses to parse (#50).
fn hydrate_spawn_from_session(progress: &mut WorldSpawnProgress, session: &mut WorldShapeLodCache) {
    if progress.session_hydrated {
        return;
    }
    progress.session_hydrated = true;

    for (key, handle) in &session.texture_images {
        progress
            .texture_image_cache
            .entry(key.clone())
            .or_insert_with(|| handle.clone());
    }

    let mut pending = Vec::with_capacity(progress.shape_load_paths.len());
    let mut hits = 0usize;
    for path in std::mem::take(&mut progress.shape_load_paths) {
        if let Some(asset) = session.shape_assets.get_hit(&path).cloned() {
            progress.shape_cache.insert(path.clone(), asset);
            if let Some(shape) = session.shapes.get(&path) {
                progress
                    .parsed_shape_files
                    .insert(path.clone(), shape.clone());
            }
            if let Some(lods) = session.assets_by_lod.get(&path) {
                progress.shape_lod_assets.insert(path.clone(), lods.clone());
            }
            hits += 1;
        } else {
            pending.push(path);
            session.shape_assets.record_miss();
        }
    }
    progress.cache_hits = hits;
    progress.cache_misses = pending.len();
    progress.shape_load_paths = pending;

    if hits > 0 || progress.cache_misses > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: shape session cache — {} hit(s), {} miss(es) (session {} shape(s), {} texture(s))",
            hits,
            progress.cache_misses,
            session.shape_assets.len(),
            session.texture_images.len()
        );
    }
}

fn standard_material_image_ids(
    mat: &StandardMaterial,
) -> impl Iterator<Item = AssetId<Image>> + '_ {
    [
        mat.base_color_texture.as_ref(),
        mat.emissive_texture.as_ref(),
        mat.metallic_roughness_texture.as_ref(),
        mat.normal_map_texture.as_ref(),
        mat.occlusion_texture.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(Handle::id)
}

fn release_shape_render_asset_gpu(
    asset: &ShapeRenderAsset,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    meshes.remove(asset.combined_mesh.id());
    for part in &asset.parts {
        meshes.remove(part.mesh.id());
        materials.remove(part.material.id());
    }
}

/// Drop session entries (and GPU assets) for shapes with no remaining live entities (#51 / #114).
fn evict_unreferenced_world_shapes(
    session: &mut WorldShapeLodCache,
    live_shape_paths: &HashSet<PathBuf>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (usize, usize) {
    let stale = session.shape_assets.evict_except(live_shape_paths);
    if stale.is_empty() {
        return (0, 0);
    }

    let mut shapes_evicted = 0usize;
    for (path, asset) in stale {
        release_shape_render_asset_gpu(&asset, meshes, materials);
        shapes_evicted += 1;
        if let Some(lods) = session.assets_by_lod.remove(&path) {
            for lod_asset in lods {
                release_shape_render_asset_gpu(&lod_asset, meshes, materials);
            }
        }
        session.shapes.remove(&path);
    }

    let mut still_needed_images = HashSet::new();
    for asset in session
        .shape_assets
        .values()
        .chain(session.assets_by_lod.values().flatten())
    {
        for part in &asset.parts {
            if let Some(mat) = materials.get(&part.material) {
                still_needed_images.extend(standard_material_image_ids(mat));
            }
        }
    }

    let mut textures_evicted = 0usize;
    let stale_textures: Vec<(PathBuf, i32)> = session
        .texture_images
        .iter()
        .filter(|(_, handle)| !still_needed_images.contains(&handle.id()))
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale_textures {
        if let Some(handle) = session.texture_images.remove(&key) {
            images.remove(handle.id());
            textures_evicted += 1;
        }
    }

    (shapes_evicted, textures_evicted)
}

/// Merge this spawn cycle's newly built assets into the session cache (#50).
fn commit_spawn_to_session(session: &mut WorldShapeLodCache, progress: &mut WorldSpawnProgress) {
    let shapes_before = session.shape_assets.len();
    let textures_before = session.texture_images.len();
    session.shapes.extend(progress.parsed_shape_files.drain());
    session
        .assets_by_lod
        .extend(progress.shape_lod_assets.drain());
    session.shape_assets.extend(progress.shape_cache.drain());
    session
        .texture_images
        .extend(progress.texture_image_cache.drain());
    let shapes_added = session.shape_assets.len().saturating_sub(shapes_before);
    let textures_added = session.texture_images.len().saturating_sub(textures_before);
    if shapes_added > 0 || textures_added > 0 || progress.cache_hits > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: shape session cache commit — +{} shape(s)/+{} texture(s) → session {}/{} (hits {} / misses {} this cycle)",
            shapes_added,
            textures_added,
            session.shape_assets.len(),
            session.texture_images.len(),
            progress.cache_hits,
            progress.cache_misses
        );
    }
}

/// True once progressive world spawn has finished (or was never started).
pub fn world_spawn_complete(progress: Option<Res<WorldSpawnProgress>>) -> bool {
    progress.is_none()
}

/// Begin progressive world spawn (continues in [`progressive_world_spawn_system`]).
pub fn init_world_spawn_progress(
    world: Res<WorldScene>,
    scene: Res<TrackScene>,
    mode: Res<ViewerSceneryMode>,
    mut cycle: ResMut<openrailsrs_bevy_scenery::ScenerySpawnCycle>,
    mut commands: Commands,
) {
    if world.is_empty() || !mode.loads_msts_scenery() {
        return;
    }
    viewer_log!(
        "openrailsrs-viewer3d: progressive world spawn — {} item(s)",
        world.items.len()
    );
    let placeholder_base = scene.bounds.edge_radius().max(2.0) * 1.5;
    cycle.begin(openrailsrs_bevy_scenery::ScenerySpawnPhase::Objects);
    commands.insert_resource(WorldSpawnProgress::new(placeholder_base));
}

fn classify_one_object(
    obj: &WorldObject,
    focus: &RouteFocus,
    // World-space XZ centre for distance culling (view window / camera, not only route anchor).
    cull_center: Vec3,
    assets: &RouteAssets,
    mode: ViewerSceneryMode,
    wire: &crate::overhead_wire::RouteWireConfig,
    progress: &mut WorldSpawnProgress,
) {
    if obj.kind == "Dyntrack"
        || obj.kind == "Forest"
        || obj.kind == "HWater"
        || obj.kind == "Transfer"
        || obj.kind == "CarSpawner"
        || is_tr_item_label_only(obj.kind)
    {
        return;
    }
    if should_cull_world_object_at(cull_center, obj.position) {
        progress.culled_count += 1;
        return;
    }

    let dist = horizontal_distance_xz(cull_center, obj.position);

    if !mode.loads_msts_scenery() {
        return;
    }

    // TrackObj: exclusive outcome mesh | procedural | failed (#35). Never silent omit in-radius.
    if obj.kind == "TrackObj" {
        if dist > shape_mesh_radius_m() {
            return;
        }
        progress.trackobj_seen += 1;
        // Wire is independent of mesh vs procedural (#36 / OR Wire.DecomposeStaticWire).
        if crate::overhead_wire::should_draw_wire_for(obj, assets, wire) {
            let render_pos = obj.render_position(focus);
            let segs = trackobj_procedural_segments(obj, render_pos, assets, mode);
            progress.trackobj_wire.extend(segs);
        }
        let cache_key = obj
            .shape_file
            .clone()
            .or_else(|| {
                obj.section_idx
                    .and_then(|idx| assets.tsection().shape_file_name(idx).map(str::to_string))
            })
            .unwrap_or_default();
        let shape_path = if cache_key.is_empty() && !shape_eligible(obj) {
            None
        } else {
            progress
                .shape_path_cache
                .entry(cache_key)
                .or_insert_with(|| resolve_object_shape_path(obj, assets))
                .clone()
        };
        if let Some(shape_path) = shape_path {
            progress.trackobj_resolved += 1;
            let render_pos = obj.render_position(focus);
            let tf = Transform {
                translation: render_pos,
                rotation: obj.rotation,
                scale: obj.scale,
            };
            progress
                .shape_instances
                .entry(shape_path.clone())
                .or_default()
                .push(ShapeInstancePlacement {
                    transform: tf,
                    linear: obj.linear,
                    tile_x: obj.tile_x,
                    tile_z: obj.tile_z,
                    auto_z_bias: true,
                    signal_sub_obj: None,
                });
            return;
        }

        let render_pos = obj.render_position(focus);
        if SPAWN_TRACKOBJ_PROCEDURAL {
            let segs = trackobj_procedural_segments(obj, render_pos, assets, mode);
            if !segs.is_empty() {
                progress.trackobj_procedural_objects += 1;
                progress.trackobj_procedural.extend(segs);
                return;
            }
        }

        let reason =
            if obj.shape_file.as_ref().is_none_or(|s| s.is_empty()) && obj.section_idx.is_none() {
                "no_filename_or_section"
            } else if obj.shape_file.as_ref().is_some_and(|s| !s.is_empty()) {
                "shape_unresolved_and_no_procedural"
            } else {
                "section_without_procedural_links"
            };
        progress.trackobj_failed += 1;
        if progress.trackobj_failures.len() < 64 {
            progress.trackobj_failures.push(TrackObjFailure {
                tile_x: obj.tile_x,
                tile_z: obj.tile_z,
                uid: obj.uid,
                file_name: obj.shape_file.clone(),
                section_idx: obj.section_idx,
                reason,
            });
        }
        if !trackobj_placeholders_enabled() {
            return;
        }
        // Fall through to placeholder cuboid when opt-in.
    } else if shape_eligible(obj) && dist <= shape_mesh_radius_m() {
        let cache_key = obj.shape_file.clone().unwrap_or_default();
        let shape_path = progress
            .shape_path_cache
            .entry(cache_key)
            .or_insert_with(|| resolve_object_shape_path(obj, assets))
            .clone();
        if let Some(shape_path) = shape_path {
            let render_pos = obj.render_position(focus);
            let tf = Transform {
                translation: render_pos,
                rotation: obj.rotation,
                scale: obj.scale,
            };
            progress
                .shape_instances
                .entry(shape_path.clone())
                .or_default()
                .push(ShapeInstancePlacement {
                    transform: tf,
                    linear: obj.linear,
                    tile_x: obj.tile_x,
                    tile_z: obj.tile_z,
                    auto_z_bias: false,
                    signal_sub_obj: (obj.kind == "Signal")
                        .then(|| obj.signal.as_ref().map(|s| s.signal_sub_obj))
                        .flatten(),
                });
            return;
        }
    }
    if dist > shape_mesh_radius_m() {
        return;
    }
    if suppress_world_placeholder(obj.kind) {
        return;
    }

    let size = box_size_for_kind(obj.kind, progress.placeholder_base);
    let translation = obj.render_position(focus) + Vec3::Y * (size.y * 0.5);
    // Size is already applied in local verts; keep transform scale at 1 to avoid size² cubes.
    let tf = Transform {
        translation,
        rotation: obj.rotation,
        scale: Vec3::ONE,
    };
    let kind_entry = progress
        .merged_boxes
        .entry(obj.kind.to_string())
        .or_insert_with(|| MergedBoxGroup {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            color: kind_color(obj.kind),
        });
    push_cuboid(
        &mut kind_entry.positions,
        &mut kind_entry.normals,
        &mut kind_entry.uvs,
        &mut kind_entry.indices,
        &tf,
        size,
    );
}

fn matrix_idx_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> usize {
    shape
        .prim_states
        .get(prim_state_idx.max(0) as usize)
        .and_then(|ps| shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize))
        .map(|vs| vs.matrix_idx.max(0) as usize)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn append_shape_spawn_entries_for_transforms(
    shape_path: &Path,
    asset: &ShapeRenderAsset,
    shape_file: Option<&ShapeFile>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    placements: &[ShapeInstancePlacement],
    spawn_queue: &mut Vec<ShapeSpawnBundle>,
    anim_spawn_queue: &mut Vec<AnimatedShapeSpawnBundle>,
    instanced_spawn_queue: &mut Vec<InstancedShapeSpawnBundle>,
    initial_lod_idx: usize,
    shape_mesh_count: &mut usize,
    shape_texture_count: &mut usize,
    merged_shape_groups: &mut usize,
    instanced_groups: &mut usize,
    instanced_instances: &mut usize,
    origin: &FloatingOrigin,
    sigcfg: Option<&openrailsrs_formats::SigCfgFile>,
) {
    use crate::signal_subobj::{signal_part_visible, signal_subobj_visible};
    use crate::world_instancing::{
        WORLD_INSTANCING_MIN, WorldInstanceAppearance, WorldInstanceBuffer, WorldInstanceData,
        WorldInstancedGroup, appearance_from_standard_material, group_placements_by_tile,
        instances_aabb, instancing_part_supported, world_instancing_enabled,
    };

    // viewer3d scenery uses daylight sun by default (#95).
    let is_day = crate::shapes::scenery_is_day(asset.texture_flags);
    let day_parts: Vec<(usize, &crate::shapes::ShapePartAsset)> = asset
        .parts
        .iter()
        .enumerate()
        .filter(|(_, part)| shape_part_visible_for_day_night(asset, part, is_day))
        .collect();
    let batch_auto_z = placements.iter().any(|p| p.auto_z_bias);
    // Distinct SignalSubObj masks cannot share merged/instanced batches (#80).
    let has_signal_filter = placements.iter().any(|p| p.signal_sub_obj.is_some());
    let shape_file_name = shape_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let signal_vis_for = |mask: Option<u32>| -> Option<Vec<bool>> {
        let mask = mask?;
        let shape = shape_file?;
        let def = sigcfg?.signal_shape(shape_file_name)?;
        signal_subobj_visible(shape, def, mask)
    };
    let parts_for_mask = |mask: Option<u32>| -> Vec<(usize, &crate::shapes::ShapePartAsset)> {
        match signal_vis_for(mask) {
            Some(vis) => day_parts
                .iter()
                .copied()
                .filter(|(_, part)| signal_part_visible(&vis, part.sub_object_idx))
                .collect(),
            None => day_parts.clone(),
        }
    };
    // Default day-only list when no signal filter on the batch.
    let visible_parts = if has_signal_filter {
        Vec::new()
    } else {
        day_parts.clone()
    };

    if asset.has_texture {
        *shape_texture_count += placements.len();
    }
    let animated = shape_file.is_some_and(shape_has_loop_animation);
    let mergeable = !has_signal_filter
        && !animated
        && ENABLE_SHAPE_INSTANCE_MERGE
        && placements.len() >= SHAPE_INSTANCE_MERGE_MIN
        && visible_parts.iter().all(|(_, part)| {
            !part.is_transparent
                && meshes
                    .get(&part.mesh)
                    .map(|mesh| mesh.count_vertices() <= SHAPE_INSTANCE_MERGE_MAX_VERTS)
                    .unwrap_or(false)
        });

    if mergeable {
        *merged_shape_groups += 1;
        *shape_mesh_count += visible_parts.len();
        let view_transforms: Vec<Transform> = placements
            .iter()
            .map(|p| view_transform(p.transform, origin))
            .collect();
        let bound = placements.first().map(|p| WorldTileBound {
            tile_x: p.tile_x,
            tile_z: p.tile_z,
        });
        for (_, part) in &visible_parts {
            if let Some(merged) = build_merged_instance_mesh(meshes, &part.mesh, &view_transforms) {
                let Some(bound) = bound else {
                    continue;
                };
                let material = material_with_auto_z_bias(materials, &part.material, batch_auto_z);
                spawn_queue.push((
                    Transform::IDENTITY,
                    Mesh3d(meshes.add(merged)),
                    MeshMaterial3d(material),
                    Name::new("world:merged"),
                    WorldSceneryLod {
                        enabled: false,
                        shape_path: PathBuf::new(),
                        sub_object_idx: u32::MAX,
                        prim_state_idx: -1,
                        part_index: 0,
                        lod_idx: 0,
                    },
                    bound,
                ));
            }
        }
    } else if !has_signal_filter && !animated && world_instancing_enabled() {
        // GPU instancing (#58): one entity per (part × tile) for opaque static repeats.
        let by_tile = group_placements_by_tile(placements);
        for ((tile_x, tile_z), indices) in by_tile {
            let tile_placements: Vec<&ShapeInstancePlacement> =
                indices.iter().map(|&i| &placements[i]).collect();
            let use_gpu = tile_placements.len() >= WORLD_INSTANCING_MIN;
            let tile_auto_z = tile_placements.iter().any(|p| p.auto_z_bias);
            for &(part_index, part) in &visible_parts {
                let material = material_with_auto_z_bias(materials, &part.material, tile_auto_z);
                let can_instance = use_gpu
                    && !part.is_transparent
                    && part.has_texture
                    && instancing_part_supported(
                        part.shader_name.as_deref(),
                        part.light_mat_idx,
                        materials,
                        &material,
                    );
                if can_instance {
                    let instances: Vec<WorldInstanceData> = tile_placements
                        .iter()
                        .map(|p| {
                            WorldInstanceData::from_view_placement(
                                view_transform(p.transform, origin),
                                p.linear,
                            )
                        })
                        .collect();
                    let count = instances.len() as u32;
                    let local_aabb = meshes.get(&part.mesh).and_then(MeshAabb::compute_aabb);
                    let aabb = instances_aabb(&instances, local_aabb.as_ref());
                    let appearance: WorldInstanceAppearance =
                        appearance_from_standard_material(materials, &material);
                    *instanced_groups += 1;
                    *instanced_instances += instances.len();
                    *shape_mesh_count += 1;
                    instanced_spawn_queue.push((
                        Transform::IDENTITY,
                        Mesh3d(part.mesh.clone()),
                        Name::new("world:instanced"),
                        WorldInstancedGroup {
                            shape_path: shape_path.to_path_buf(),
                            part_index,
                            sub_object_idx: part.sub_object_idx,
                            prim_state_idx: part.prim_state_idx,
                            lod_idx: initial_lod_idx,
                            lod_enabled: true,
                            instance_count: count,
                        },
                        WorldTileBound { tile_x, tile_z },
                        WorldInstanceBuffer(instances.into()),
                        appearance,
                        aabb,
                    ));
                } else {
                    *shape_mesh_count += tile_placements.len();
                    for p in &tile_placements {
                        let (tf, mesh) = view_mesh_for_placement(meshes, &part.mesh, p, origin);
                        let mat =
                            material_with_auto_z_bias(materials, &part.material, p.auto_z_bias);
                        spawn_queue.push((
                            tf,
                            Mesh3d(mesh),
                            MeshMaterial3d(mat),
                            Name::new("world:mesh"),
                            WorldSceneryLod {
                                enabled: true,
                                shape_path: shape_path.to_path_buf(),
                                sub_object_idx: part.sub_object_idx,
                                prim_state_idx: part.prim_state_idx,
                                part_index,
                                lod_idx: initial_lod_idx,
                            },
                            WorldTileBound { tile_x, tile_z },
                        ));
                    }
                }
            }
        }
    } else if animated {
        let shape = shape_file.expect("animated branch requires ShapeFile");
        let speed = animation_playback_speed(shape);
        let frame_count = shape
            .animations
            .first()
            .map(|a| a.frame_count as f32)
            .unwrap_or(0.0);
        for inst in placements {
            let inst_parts = parts_for_mask(inst.signal_sub_obj);
            *shape_mesh_count += inst_parts.len();
            let placement = view_transform(inst.transform, origin);
            let bound = WorldTileBound {
                tile_x: inst.tile_x,
                tile_z: inst.tile_z,
            };
            for &(part_index, part) in &inst_parts {
                let matrix_idx = matrix_idx_for_prim_state(shape, part.prim_state_idx);
                let material =
                    material_with_auto_z_bias(materials, &part.material, inst.auto_z_bias);
                anim_spawn_queue.push((
                    placement,
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(material),
                    Name::new("world:anim"),
                    WorldSceneryLod {
                        // LOD stays on; swap re-applies anim delta without resetting key (#100).
                        enabled: true,
                        shape_path: shape_path.to_path_buf(),
                        sub_object_idx: part.sub_object_idx,
                        prim_state_idx: part.prim_state_idx,
                        part_index,
                        lod_idx: initial_lod_idx,
                    },
                    bound,
                    ShapeAnimState {
                        key: 0.0,
                        matrix_idx,
                    },
                    ShapeAnimBinding {
                        shape: shape.clone(),
                        matrix_idx,
                        speed,
                        frame_count,
                        placement,
                        baked_rest_mesh: true,
                    },
                ));
            }
        }
    } else {
        // Static non-instanced path (#115 / SignalSubObj #80). Shear → bake into mesh (#139).
        for inst in placements {
            let inst_parts = parts_for_mask(inst.signal_sub_obj);
            *shape_mesh_count += inst_parts.len();
            for &(part_index, part) in &inst_parts {
                let (tf, mesh) = view_mesh_for_placement(meshes, &part.mesh, inst, origin);
                let material =
                    material_with_auto_z_bias(materials, &part.material, inst.auto_z_bias);
                spawn_queue.push((
                    tf,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Name::new("world:mesh"),
                    WorldSceneryLod {
                        enabled: true,
                        shape_path: shape_path.to_path_buf(),
                        sub_object_idx: part.sub_object_idx,
                        prim_state_idx: part.prim_state_idx,
                        part_index,
                        lod_idx: initial_lod_idx,
                    },
                    WorldTileBound {
                        tile_x: inst.tile_x,
                        tile_z: inst.tile_z,
                    },
                ));
            }
        }
    }
}

fn append_shape_spawn_entries(
    progress: &mut WorldSpawnProgress,
    shape_path: &Path,
    asset: &ShapeRenderAsset,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    origin: &FloatingOrigin,
    sigcfg: Option<&openrailsrs_formats::SigCfgFile>,
) {
    let Some(placements) = progress.shape_instances.get(shape_path).cloned() else {
        return;
    };
    // Spawn the finest band so every stable part identity exists. The runtime LOD
    // system immediately selects the correct band for each placement independently.
    let initial_lod_idx = 0;
    let shape_file = progress.parsed_shape_files.get(shape_path);
    append_shape_spawn_entries_for_transforms(
        shape_path,
        asset,
        shape_file,
        meshes,
        materials,
        &placements,
        &mut progress.spawn_queue,
        &mut progress.anim_spawn_queue,
        &mut progress.instanced_spawn_queue,
        initial_lod_idx,
        &mut progress.shape_mesh_count,
        &mut progress.shape_texture_count,
        &mut progress.merged_shape_groups,
        &mut progress.instanced_groups,
        &mut progress.instanced_instances,
        origin,
        sigcfg,
    );
}

fn add_shape_fallback_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * 0.35,
        perceptual_roughness: 0.75,
        metallic: 0.1,
        double_sided: true,
        ..default()
    })
}

#[allow(clippy::too_many_arguments)]
fn build_world_shape_asset(
    shape_path: PathBuf,
    loaded: Option<crate::shapes::LoadedShape>,
    route_dir: &Path,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_image_cache: &mut std::collections::HashMap<(PathBuf, i32), Handle<Image>>,
    ace_cache: &std::collections::HashMap<PathBuf, openrailsrs_ace::AceFile>,
    fallback_color: Color,
    fallback_material: &Handle<StandardMaterial>,
) -> (PathBuf, ShapeRenderAsset) {
    let tex_dirs = texture_search_dirs_for_shape(&shape_path, route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let pbr = load_shape_pbr_sidecar(&shape_path);
    let asset = match loaded {
        Some(loaded) => shape_render_asset_from_loaded_with_ace_cache(
            loaded,
            &tex_refs,
            meshes,
            images,
            materials,
            None,
            texture_image_cache,
            ace_cache,
            fallback_color,
            None,
            false,
            false,
            pbr.as_ref(),
        ),
        None => {
            viewer_log!(
                "openrailsrs-viewer3d: shape {} failed, using placeholder cube",
                shape_path.display()
            );
            let unit: Handle<Mesh> = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
            ShapeRenderAsset {
                combined_mesh: unit.clone(),
                parts: vec![crate::shapes::ShapePartAsset {
                    prim_state_idx: -1,
                    sub_object_idx: u32::MAX,
                    sort_index: 0,
                    cab_matrix_idx: None,
                    mesh: unit,
                    material: fallback_material.clone(),
                    or_cab_material: None,
                    has_texture: false,
                    is_transparent: false,
                    texture_name: None,
                    shader_name: None,
                    light_mat_idx: None,
                    solid_color: None,
                    lever_pivot_at_mesh_center: false,
                    lever_local_axis: None,
                    bounds_center: None,
                }],
                has_texture: false,
                has_night_subobj: false,
                texture_flags: openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(
                    openrailsrs_bevy_scenery::textures::TextureFlags::NONE,
                ),
            }
        }
    };
    let mut asset = asset;
    apply_shape_descriptor_to_asset(&shape_path, &mut asset);
    (shape_path, asset)
}

#[allow(clippy::too_many_arguments)]
fn build_shape_lod_assets(
    shape_path: &Path,
    shape: &ShapeFile,
    route_dir: &Path,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_image_cache: &mut std::collections::HashMap<(PathBuf, i32), Handle<Image>>,
    ace_cache: &std::collections::HashMap<PathBuf, openrailsrs_ace::AceFile>,
    fallback_color: Color,
    _fallback_material: &Handle<StandardMaterial>,
) -> Vec<ShapeRenderAsset> {
    let band_count = openrailsrs_bevy_scenery::shapes::lod_band_count(shape);
    if band_count <= 1 {
        return Vec::new();
    }
    let tex_dirs = texture_search_dirs_for_shape(shape_path, route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let pbr = load_shape_pbr_sidecar(shape_path);
    (0..band_count)
        .filter_map(|band| {
            let parts = openrailsrs_bevy_scenery::shapes::build_mesh_parts_for_lod_band(
                shape,
                band,
                world_mesh_options_for_shape(shape_path),
            );
            if parts.is_empty() {
                return None;
            }
            // Bounds/combined mesh: first part is enough for WORLD LOD bookkeeping.
            let mesh = parts.first()?.mesh.clone();
            let loaded = openrailsrs_bevy_scenery::shapes::LoadedShape {
                mesh,
                texture_file: primary_texture_filename(shape),
                parts,
            };
            let mut asset = shape_render_asset_from_loaded_with_ace_cache(
                loaded,
                &tex_refs,
                meshes,
                images,
                materials,
                None,
                texture_image_cache,
                ace_cache,
                fallback_color,
                None,
                false,
                false,
                pbr.as_ref(),
            );
            apply_shape_descriptor_to_asset(shape_path, &mut asset);
            Some(asset)
        })
        .collect()
}

fn prepare_shape_load_paths(progress: &mut WorldSpawnProgress) {
    // After session hydrate, an empty miss-list means "all hits" — do not refill
    // from `shape_instances` or the next frame would reparse every shared path (#50).
    if progress.session_hydrated || !progress.shape_load_paths.is_empty() {
        return;
    }
    progress.shape_load_paths = progress.shape_instances.keys().cloned().collect();
    progress.shape_load_paths.sort_unstable();
    progress.shape_load_paths.dedup();
    progress
        .parsed_shapes
        .reserve(progress.shape_load_paths.len());
}

fn parse_next_shape_batch(progress: &mut WorldSpawnProgress, route_dir: &Path) -> bool {
    if progress.shape_parse_index >= progress.shape_load_paths.len() {
        return false;
    }
    if progress.shape_parse_index == 0 {
        reset_shape_file_parse_count();
    }
    let end =
        (progress.shape_parse_index + SHAPE_PARSE_PER_FRAME).min(progress.shape_load_paths.len());
    let batch: Vec<PathBuf> = progress.shape_load_paths[progress.shape_parse_index..end].to_vec();
    // One `ShapeFile::from_path` per unique path → mesh + LOD/anim file (#57).
    let parsed: Vec<(
        PathBuf,
        Option<ShapeFile>,
        Option<crate::shapes::LoadedShape>,
    )> = batch
        .par_iter()
        .map(|path| match load_shape_file_and_loaded(path, None) {
            Some((shape, loaded)) => (path.clone(), Some(shape), Some(loaded)),
            None => (path.clone(), None, None),
        })
        .collect();
    for (shape_path, shape_file, loaded) in parsed {
        if let Some(shape) = shape_file {
            progress
                .parsed_shape_files
                .insert(shape_path.clone(), shape);
        }
        if let Some(ref loaded) = loaded {
            progress
                .load_diag
                .record_path_loaded(&shape_path, MstsAssetKind::Shape);
            let tex_dirs = texture_search_dirs_for_shape(&shape_path, route_dir);
            let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
            progress
                .texture_paths
                .extend(collect_loaded_shape_texture_paths(loaded, &tex_refs));
            let pbr = load_shape_pbr_sidecar(&shape_path);
            progress
                .texture_paths
                .extend(collect_pbr_normal_map_texture_paths(
                    pbr.as_ref(),
                    &tex_refs,
                ));
        } else {
            progress.load_diag.record_path_failed(
                &shape_path,
                MstsAssetKind::Shape,
                MstsLoadCause::Parse,
                "shape parse/mesh failed",
            );
        }
        progress.parsed_shapes.push((shape_path, loaded));
    }
    progress.shape_parse_index = end;
    if progress.shape_parse_index >= progress.shape_load_paths.len() {
        progress.texture_paths.sort_unstable();
        progress.texture_paths.dedup();
        // Skip ACE decode when the Image handle is already in the session cache (#50).
        progress.texture_paths.retain(|p| {
            !progress
                .texture_image_cache
                .keys()
                .any(|(path, _)| path == p)
        });
        false
    } else {
        true
    }
}

fn prefetch_next_shape_texture_batch(progress: &mut WorldSpawnProgress) -> bool {
    if progress.texture_prefetch_index >= progress.texture_paths.len() {
        return false;
    }
    let end = (progress.texture_prefetch_index + ACE_TEXTURES_PER_FRAME)
        .min(progress.texture_paths.len());
    let batch: Vec<PathBuf> = progress.texture_paths[progress.texture_prefetch_index..end].to_vec();
    let decoded = prefetch_ace_textures(&batch);
    for path in &batch {
        if decoded.contains_key(path) {
            progress
                .load_diag
                .record_path_loaded(path, MstsAssetKind::Ace);
        } else {
            progress.load_diag.record_path_failed(
                path,
                MstsAssetKind::Ace,
                MstsLoadCause::Parse,
                "ace decode failed",
            );
        }
    }
    progress.ace_cache.extend(decoded);
    progress.texture_prefetch_index = end;
    progress.texture_prefetch_index < progress.texture_paths.len()
}

fn finish_shape_loading(
    progress: &mut WorldSpawnProgress,
    route_dir: &Path,
    materials: &mut Assets<StandardMaterial>,
) {
    if progress.shape_fallback_material.is_some() {
        return;
    }
    let color = progress.shape_fallback_color;
    progress.shape_fallback_material = Some(add_shape_fallback_material(materials, color));
    let parses = shape_file_parse_count();
    let unique = progress.shape_load_paths.len();
    viewer_log!(
        "openrailsrs-viewer3d: parsed {} shape(s) ({} ShapeFile parse(s) for {} miss path(s); {} session hit(s)), prefetched {} texture(s)",
        progress.parsed_shapes.len(),
        parses,
        unique,
        progress.cache_hits,
        progress.ace_cache.len()
    );
    if unique > 0 && parses > unique {
        viewer_log!(
            "openrailsrs-viewer3d: WARNING shape parse amplification {parses}/{unique} (expected 1 parse per unique .s)"
        );
    }
    if progress.scenery_audit.is_none() && crate::scenery_audit::scenery_audit_enabled() {
        progress.scenery_audit = Some(crate::scenery_audit::audit_parsed_shapes(
            &progress.parsed_shapes,
            route_dir,
        ));
    }
}

fn finalize_load_diagnostics(
    progress: &WorldSpawnProgress,
    world: Option<&WorldScene>,
    terrain: Option<&crate::terrain::TerrainScene>,
) -> MstsLoadDiagnostics {
    let mut diag = MstsLoadDiagnostics::default();
    if let Some(world) = world {
        diag.merge_from(&world.load_diag);
    }
    if let Some(terrain) = terrain {
        diag.merge_from(&terrain.load_diag);
    }
    diag.merge_from(&progress.load_diag);
    let track_samples = progress.trackobj_failures.iter().map(|f| LoadFailure {
        path: f
            .file_name
            .clone()
            .unwrap_or_else(|| format!("TrackObj:uid={:?}", f.uid)),
        kind: MstsAssetKind::TrackObj,
        cause: MstsLoadCause::Missing,
        detail: f.reason.to_string(),
        tile_x: Some(f.tile_x),
        tile_z: Some(f.tile_z),
    });
    diag.ingest_trackobj_outcomes(
        progress.trackobj_resolved as u64,
        progress.trackobj_procedural_objects as u64,
        progress.trackobj_failed as u64,
        track_samples,
    );
    diag
}

fn log_world_spawn_summary(
    progress: &WorldSpawnProgress,
    world: Option<&WorldScene>,
    terrain: Option<&crate::terrain::TerrainScene>,
) -> MstsLoadDiagnostics {
    if progress.culled_count > 0 {
        let radius_m = visible_radius_m();
        viewer_log!(
            "openrailsrs-viewer3d: {culled} world object(s) culled (>{radius_m:.0}m from centre)",
            culled = progress.culled_count
        );
    }
    if progress.trackobj_seen > 0 {
        let accounted = progress.trackobj_resolved
            + progress.trackobj_procedural_objects
            + progress.trackobj_failed;
        viewer_log!(
            "openrailsrs-viewer3d: TrackObj <={}m: {} seen = {} mesh + {} procedural + {} failed{}",
            shape_mesh_radius_m() as u32,
            progress.trackobj_seen,
            progress.trackobj_resolved,
            progress.trackobj_procedural_objects,
            progress.trackobj_failed,
            if accounted != progress.trackobj_seen {
                format!(" (WARNING accounted {accounted})")
            } else {
                String::new()
            }
        );
        for fail in progress.trackobj_failures.iter().take(12) {
            viewer_log!(
                "openrailsrs-viewer3d: TrackObj failed uid={:?} file={:?} section={:?} tile={},{} — {}",
                fail.uid,
                fail.file_name.as_deref().unwrap_or(""),
                fail.section_idx,
                fail.tile_x,
                fail.tile_z,
                fail.reason
            );
        }
        if progress.trackobj_failed > progress.trackobj_failures.len() {
            viewer_log!(
                "openrailsrs-viewer3d: … {} more TrackObj failure(s) omitted from sample",
                progress.trackobj_failed - progress.trackobj_failures.len()
            );
        }
    }
    if !progress.trackobj_procedural.is_empty() {
        viewer_log!(
            "openrailsrs-viewer3d: {} TrackObj procedural segment(s) (tsection/tdb)",
            progress.trackobj_procedural.len()
        );
    }
    if progress.merged_shape_groups > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: merged {} repeated shape(s) (≥{SHAPE_INSTANCE_MERGE_MIN} instances)",
            progress.merged_shape_groups
        );
    }
    if progress.instanced_groups > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: GPU instanced {} group(s) covering {} instance(s) (#58)",
            progress.instanced_groups,
            progress.instanced_instances
        );
    }
    if progress.shape_mesh_count > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {} world shape part(s) spawned",
            progress.shape_mesh_count
        );
    }
    if progress.shape_texture_count > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {} textured instance(s)",
            progress.shape_texture_count
        );
    }
    if let Some(audit) = &progress.scenery_audit {
        audit.log_report("unique shapes at spawn");
    }
    log_step("spawned world objects (progressive)", progress.started);
    let diag = finalize_load_diagnostics(progress, world, terrain);
    for line in diag.summary_lines() {
        viewer_log!("openrailsrs-viewer3d: {line}");
    }
    diag.maybe_write_audit_env();
    diag
}

/// Index of every `.w` tile on disk; loads additional tiles as the camera moves (OR `SceneryDrawer`).
///
/// Hot path requests `.tilebundle` via AssetServer (#111); materialization is async.
#[derive(Resource, Default)]
pub struct WorldTileStream {
    catalog: std::collections::HashMap<(i32, i32), PathBuf>,
    loaded: std::collections::HashSet<(i32, i32)>,
    /// Requested via AssetServer but not yet materialized into [`WorldScene`].
    pending: std::collections::HashSet<(i32, i32)>,
    route_dir: PathBuf,
    radius_m: f32,
    last_camera_tile: Option<(i32, i32)>,
}

impl WorldTileStream {
    pub fn new(route_dir: &Path, world: &WorldScene, radius_m: f32) -> Self {
        let catalog = openrailsrs_formats::build_world_tile_catalog(route_dir);
        let mut loaded = std::collections::HashSet::new();
        for obj in &world.items {
            loaded.insert((obj.tile_x, obj.tile_z));
        }
        for key in &world.loaded_tiles {
            loaded.insert(*key);
        }
        Self {
            catalog,
            loaded,
            pending: std::collections::HashSet::new(),
            route_dir: route_dir.to_path_buf(),
            radius_m,
            last_camera_tile: None,
        }
    }
}

fn camera_msts_xz(
    focus: &RouteFocus,
    cam: &Transform,
    origin: &crate::floating_origin::FloatingOrigin,
) -> Vec2 {
    Vec2::new(
        cam.translation.x + focus.center.x + origin.shift.x,
        cam.translation.z + focus.center.z + origin.shift.z,
    )
}

/// Tracks how many world items already have forest / water / transfer / dyntrack GPU spawns.
#[derive(Resource, Default)]
pub struct WorldSceneryStreamState {
    pub processed_items: usize,
}

/// After startup (or tile stream), spawn forest / water / transfer / dyntrack for newly loaded items.
pub fn init_scenery_stream_state(
    world: Res<WorldScene>,
    mut state: ResMut<WorldSceneryStreamState>,
) {
    state.processed_items = world.items.len();
}

#[allow(clippy::too_many_arguments)]
pub fn world_stream_scenery_system(
    world: Res<WorldScene>,
    mut state: ResMut<WorldSceneryStreamState>,
    progress: Option<Res<WorldSpawnProgress>>,
    mode: Res<ViewerSceneryMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut forest_materials: ResMut<Assets<openrailsrs_bevy_scenery::OrForestMaterial>>,
    track: Res<TrackScene>,
    terrain: Option<Res<TerrainElevation>>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    window: Res<crate::view_window::ViewWindow>,
    offset: Res<RouteWorldOffset>,
    wire: Res<crate::overhead_wire::RouteWireConfig>,
) {
    if progress.is_some() {
        return;
    }
    if world.items.len() <= state.processed_items {
        return;
    }
    let new_items = &world.items[state.processed_items..];
    let cull_center = window.center_world;
    let forests = if !mode.loads_msts_scenery() {
        0
    } else {
        new_items
            .iter()
            .filter(|obj| obj.kind == "Forest" && obj.forest.is_some())
            .count()
    };
    let waters = if !mode.loads_msts_scenery() {
        0
    } else {
        new_items
            .iter()
            .filter(|obj| obj.kind == "HWater" && obj.water.is_some())
            .count()
    };
    let transfers = if !mode.loads_msts_scenery() {
        0
    } else {
        new_items
            .iter()
            .filter(|obj| obj.kind == "Transfer" && obj.transfer.is_some())
            .count()
    };
    let road_cars = if !mode.loads_msts_scenery() {
        0
    } else {
        new_items
            .iter()
            .filter(|obj| obj.kind == "CarSpawner" && obj.car_spawner.is_some())
            .count()
    };
    let dyntracks = new_items
        .iter()
        .filter(|obj| obj.kind == "Dyntrack")
        .count();
    if forests > 0 {
        crate::forest::spawn_forest_objects(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut forest_materials,
            new_items,
            &track,
            terrain.as_deref(),
            &assets,
            &focus,
            &offset,
            Some(cull_center),
        );
    }
    if waters > 0 {
        crate::water::spawn_water_objects(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut materials,
            new_items,
            &track,
            terrain.as_deref(),
            &assets,
            &focus,
        );
    }
    if transfers > 0 {
        crate::transfer::spawn_transfer_objects(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut materials,
            new_items,
            terrain.as_deref(),
            &assets,
            &focus,
            Some(window.center_world),
        );
    }
    if road_cars > 0 {
        crate::road_cars::spawn_road_car_objects(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut materials,
            new_items,
            &assets,
            &focus,
            Some(cull_center),
        );
    }
    let signal_lamps = if !mode.loads_msts_scenery() {
        0
    } else {
        new_items
            .iter()
            .filter(|obj| obj.kind == "Signal" && obj.signal.is_some())
            .count()
    };
    if signal_lamps > 0 {
        crate::signal_lamps::spawn_signal_lamp_objects(
            &mut commands,
            &mut meshes,
            &mut materials,
            new_items,
            &assets,
            &focus,
            Some(cull_center),
        );
    }
    if dyntracks > 0 {
        crate::dyntrack::spawn_dyntrack_objects_with_wire(
            &mut commands,
            &mut meshes,
            &mut materials,
            new_items,
            &focus,
            Some(&wire),
        );
    }
    if forests + waters + transfers + road_cars + dyntracks > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: streamed scenery — {forests} forest, {waters} water, {transfers} transfer, {road_cars} roadcar, {dyntracks} dyntrack"
        );
    }
    state.processed_items = world.items.len();
}

/// Request `.tilebundle` loads for WORLD tiles near the view window (#111).
///
/// Materialization runs in [`world_tile_bundle_materialize_system`].
#[allow(clippy::too_many_arguments)]
pub fn world_tile_stream_system(
    mut stream: ResMut<WorldTileStream>,
    mut handles: ResMut<crate::tile_bundle::TileBundleHandles>,
    asset_server: Res<AssetServer>,
    focus: Res<RouteFocus>,
    window: Res<crate::view_window::ViewWindow>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    camera: Query<&Transform, With<Camera3d>>,
    progress: Option<Res<WorldSpawnProgress>>,
    mode: Res<crate::launch::ViewerSceneryMode>,
) {
    // Tile-lab keeps exactly the tiles loaded at startup: no streaming.
    if mode.is_tile_lab() || !mode.loads_msts_scenery() {
        return;
    }
    if progress.is_some() {
        return;
    }
    let center = if opts.live {
        window.center_world
    } else {
        let Ok(cam) = camera.single() else {
            return;
        };
        let msts_xz = camera_msts_xz(&focus, cam, &origin);
        Vec3::new(msts_xz.x, focus.center.y, msts_xz.y)
    };
    let cam_tile_x = msts_tile_x_index_for_coord(center.x);
    let cam_tile_z = msts_tile_z_index_for_coord(center.z);
    if stream.last_camera_tile == Some((cam_tile_x, cam_tile_z)) && !opts.live {
        return;
    }
    stream.last_camera_tile = Some((cam_tile_x, cam_tile_z));

    let policy = view_stream_window_policy(stream.radius_m);
    let cam_coord = TileCoord::new(cam_tile_x, cam_tile_z);
    let known: HashSet<TileCoord> = stream
        .loaded
        .iter()
        .chain(stream.pending.iter())
        .copied()
        .map(TileCoord::from)
        .collect();
    let stream_diff = policy.diff_disk(cam_coord, &known);

    let mut requested = 0usize;
    for tile in &stream_diff.to_load {
        let key = (tile.x, tile.z);
        if stream.loaded.contains(&key) || stream.pending.contains(&key) {
            continue;
        }
        let world_path = stream
            .catalog
            .get(&key)
            .cloned()
            .or_else(|| world_tile_path_for_coords(&stream.route_dir, tile.x, tile.z));
        let Some(world_path) = world_path else {
            continue;
        };
        if crate::tile_bundle::ensure_tile_bundle_handle(
            &mut handles,
            &asset_server,
            &stream.route_dir,
            tile.x,
            tile.z,
            Some(world_path.as_path()),
            None,
        )
        .is_some()
        {
            stream.pending.insert(key);
            requested += 1;
        }
    }
    if requested > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: requested {requested} world tilebundle(s) near ({cam_tile_x},{cam_tile_z})"
        );
    }
}

/// Bundle AssetServer + tilebundle asset maps for materialize (keeps ≤16 system params).
#[derive(bevy::ecs::system::SystemParam)]
pub struct WorldTileBundleAssets<'w> {
    handles: Res<'w, crate::tile_bundle::TileBundleHandles>,
    asset_server: Res<'w, AssetServer>,
    bundles: Res<'w, Assets<openrailsrs_bevy_scenery::MstsTileBundleAsset>>,
    worlds: Res<'w, Assets<openrailsrs_bevy_scenery::MstsWorldTileAsset>>,
    terrains: Res<'w, Assets<openrailsrs_bevy_scenery::MstsTerrainTileAsset>>,
}

/// View / mode inputs for WORLD tilebundle materialize (param budget for schedule traits).
#[derive(bevy::ecs::system::SystemParam)]
pub struct WorldTileMaterializeView<'w, 's> {
    focus: Res<'w, RouteFocus>,
    window: Res<'w, crate::view_window::ViewWindow>,
    opts: Res<'w, crate::launch::ViewerLaunchOpts>,
    origin: Res<'w, crate::floating_origin::FloatingOrigin>,
    scene: Res<'w, TrackScene>,
    camera: Query<'w, 's, &'static Transform, With<Camera3d>>,
    progress: Option<Res<'w, WorldSpawnProgress>>,
    mode: Res<'w, crate::launch::ViewerSceneryMode>,
}

/// Materialize Ready/Partial tile bundles into [`WorldScene`] (#111).
pub fn world_tile_bundle_materialize_system(
    mut world: ResMut<WorldScene>,
    mut stream: ResMut<WorldTileStream>,
    assets: WorldTileBundleAssets,
    view: WorldTileMaterializeView,
    mut cycle: ResMut<openrailsrs_bevy_scenery::ScenerySpawnCycle>,
    mut commands: Commands,
) {
    let WorldTileBundleAssets {
        handles,
        asset_server,
        bundles,
        worlds,
        terrains,
    } = assets;
    let WorldTileMaterializeView {
        focus,
        window,
        opts,
        origin,
        scene,
        camera,
        progress,
        mode,
    } = view;
    if mode.is_tile_lab() || !mode.loads_msts_scenery() || progress.is_some() {
        return;
    }
    if stream.pending.is_empty() {
        return;
    }
    let center = if opts.live {
        window.center_world
    } else {
        let Ok(cam) = camera.single() else {
            return;
        };
        let msts_xz = camera_msts_xz(&focus, cam, &origin);
        Vec3::new(msts_xz.x, focus.center.y, msts_xz.y)
    };
    let item_window = Some(WorldItemWindow {
        center,
        radius_m: world_item_keep_radius_m(stream.radius_m),
    });
    let item_base = world.items.len();
    let mut tiles_loaded = 0usize;
    let mut added_tile_keys = Vec::new();
    let pending: Vec<(i32, i32)> = stream.pending.iter().copied().collect();

    for key in pending {
        let Some(handle) = handles.get(key.0, key.1).cloned() else {
            stream.pending.remove(&key);
            continue;
        };
        match crate::tile_bundle::tile_bundle_load_outcome(&asset_server, &handle) {
            crate::tile_bundle::TileBundleLoadOutcome::Pending => continue,
            crate::tile_bundle::TileBundleLoadOutcome::Failed => {
                crate::tile_bundle::record_bundle_load_failure(
                    &mut world.load_diag,
                    key.0,
                    key.1,
                    &format!("tilebundle ({},{})", key.0, key.1),
                );
                stream.pending.remove(&key);
                stream.loaded.insert(key);
                continue;
            }
            crate::tile_bundle::TileBundleLoadOutcome::Loaded => {}
        }
        let Some(bundle) = bundles.get(&handle) else {
            continue;
        };
        if !crate::tile_bundle::try_materialize_world_bundle(
            bundle,
            &worlds,
            &terrains,
            &mut world,
            item_window,
        ) {
            continue;
        }
        stream.pending.remove(&key);
        stream.loaded.insert(key);
        added_tile_keys.push(key);
        tiles_loaded += 1;
    }

    let new_items = world.items.len().saturating_sub(item_base);
    if tiles_loaded > 0 {
        let tr_item_objects: Vec<WorldObject> = world.items[item_base..]
            .iter()
            .filter(|o| matches!(o.kind, "Signal" | "Speedpost" | "SoundRegion"))
            .cloned()
            .collect();
        world.note_tr_item_tiles_added(added_tile_keys);
        world.note_tr_item_objects_added(tr_item_objects);
        viewer_log!(
            "openrailsrs-viewer3d: streamed {tiles_loaded} world tile(s) ({new_items} item(s)) via tilebundle"
        );
        let placeholder_base = scene.bounds.edge_radius().max(2.0) * 1.5;
        cycle.begin(openrailsrs_bevy_scenery::ScenerySpawnPhase::Objects);
        commands.insert_resource(WorldSpawnProgress::new_from_item_index(
            placeholder_base,
            item_base,
        ));
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct WorldUnloadParams<'w, 's> {
    world: ResMut<'w, WorldScene>,
    stream: ResMut<'w, WorldTileStream>,
    handles: ResMut<'w, crate::tile_bundle::TileBundleHandles>,
    tile_index: ResMut<'w, crate::world_tile_index::WorldTileEntityIndex>,
    shape_refs: Res<'w, crate::world_tile_index::WorldShapeLiveRefs>,
    window: Res<'w, crate::view_window::ViewWindow>,
    opts: Res<'w, crate::launch::ViewerLaunchOpts>,
    focus: Res<'w, RouteFocus>,
    origin: Res<'w, crate::floating_origin::FloatingOrigin>,
    mode: Res<'w, ViewerSceneryMode>,
    progress: Option<Res<'w, WorldSpawnProgress>>,
    session: ResMut<'w, WorldShapeLodCache>,
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    legacy_scenery: Query<
        'w,
        's,
        (Entity, &'static Transform, &'static WorldSceneryLod),
        Without<WorldTileBound>,
    >,
    stream_state: ResMut<'w, WorldSceneryStreamState>,
}

/// Unload distant world tiles and despawn scenery meshes in live mode.
///
/// Despawn walks only entities indexed on candidate tiles (#75), not all WORLD entities.
pub fn world_tile_unload_system(mut p: WorldUnloadParams) {
    if !p.opts.live || !p.mode.loads_msts_scenery() || p.mode.is_tile_lab() {
        return;
    }
    // Same gate as stream: mutating `world.items` mid-classify invalidates `item_index`.
    if p.progress.is_some() {
        return;
    }
    let unload_radius = crate::launch::view_unload_radius_m().max(p.window.radius_m);
    let center = p.window.center_world;
    let policy = view_stream_window_policy(p.window.radius_m.max(p.stream.radius_m));
    let cam_coord = TileCoord::new(
        msts_tile_x_index_for_coord(center.x),
        msts_tile_z_index_for_coord(center.z),
    );
    let loaded_coords: HashSet<TileCoord> = p
        .stream
        .loaded
        .iter()
        .chain(p.stream.pending.iter())
        .copied()
        .map(TileCoord::from)
        .collect();
    let stream_diff = policy.diff_disk(cam_coord, &loaded_coords);

    let mut unloaded_tiles = HashSet::new();
    for tile in &stream_diff.to_unload {
        let key = (tile.x, tile.z);
        unloaded_tiles.insert(key);
        p.stream.loaded.remove(&key);
        p.stream.pending.remove(&key);
    }
    if unloaded_tiles.is_empty() {
        return;
    }
    // Drop AssetServer strong handles for unloaded tiles (#111 / #51).
    p.handles.release_all(unloaded_tiles.iter());
    let unload_t0 = Instant::now();
    let before = p.world.items.len();
    p.world
        .note_tr_item_tiles_removed(unloaded_tiles.iter().copied());
    p.world
        .items
        .retain(|obj| !unloaded_tiles.contains(&(obj.tile_x, obj.tile_z)));
    for key in &unloaded_tiles {
        p.world.loaded_tiles.remove(key);
    }
    p.world.tiles_loaded = p.world.loaded_tiles.len();
    p.stream_state.processed_items = p.stream_state.processed_items.min(p.world.items.len());

    // O(candidates): only entities indexed on unloaded tiles (#75).
    let mut to_despawn = p.tile_index.take_tiles(&unloaded_tiles);
    let visited = to_despawn.len();

    // Legacy scenery without WorldTileBound: distance fallback (#62).
    for (entity, tf, _lod) in &p.legacy_scenery {
        let msts_x = tf.translation.x + p.focus.center.x + p.origin.shift.x;
        let msts_z = tf.translation.z + p.focus.center.z + p.origin.shift.z;
        let dist = Vec2::new(msts_x - center.x, msts_z - center.z).length();
        if scenery_entity_should_unload(None, &unloaded_tiles, dist, unload_radius) {
            to_despawn.push(entity);
        }
    }

    let live_shape_paths = p.shape_refs.live_paths_after_releasing(&to_despawn);
    let despawned = to_despawn.len();
    for entity in to_despawn {
        p.commands.entity(entity).despawn();
    }

    let (shapes_evicted, textures_evicted) = evict_unreferenced_world_shapes(
        &mut p.session,
        &live_shape_paths,
        &mut p.meshes,
        &mut p.images,
        &mut p.materials,
    );
    if std::env::var_os("OPENRAILSRS_PERF_DEBUG").is_some() {
        eprintln!(
            "[PERF] world_tile_unload_ms={:.2} tiles={} visited={} despawned={}",
            unload_t0.elapsed().as_secs_f64() * 1000.0,
            unloaded_tiles.len(),
            visited,
            despawned
        );
    }
    viewer_log!(
        "openrailsrs-viewer3d: unloaded {} world tile(s) ({} → {} items; despawned {} entit(ies); evicted {} shape(s)/{} texture(s); session {}/{} )",
        unloaded_tiles.len(),
        before,
        p.world.items.len(),
        despawned,
        shapes_evicted,
        textures_evicted,
        p.session.shape_assets.len(),
        p.session.texture_images.len()
    );
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct WorldSpawnSession<'w> {
    session: ResMut<'w, WorldShapeLodCache>,
    cycle: ResMut<'w, openrailsrs_bevy_scenery::ScenerySpawnCycle>,
}

/// Continue progressive world spawn across frames so the window stays responsive.
#[allow(clippy::too_many_arguments)]
pub fn progressive_world_spawn_system(
    follow: Res<CameraFollowMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    terrain: Option<Res<crate::terrain::TerrainScene>>,
    _scene: Res<TrackScene>,
    focus: Res<RouteFocus>,
    window: Res<crate::view_window::ViewWindow>,
    origin: Res<FloatingOrigin>,
    assets: Res<RouteAssets>,
    mode: Res<ViewerSceneryMode>,
    wire: Res<crate::overhead_wire::RouteWireConfig>,
    spawn_session: WorldSpawnSession,
    progress: Option<ResMut<WorldSpawnProgress>>,
) {
    let Some(mut progress) = progress else {
        return;
    };
    let WorldSpawnSession {
        mut session,
        mut cycle,
    } = spawn_session;
    // Spawn culling must follow the mobile view window (camera/train), not only the
    // startup RouteFocus — otherwise streamed tiles far from the anchor never appear.
    let cull_center = window.center_world;

    let driver_throttle = if *follow == CameraFollowMode::DriverCam {
        driver_world_spawn_scale()
    } else {
        1.0
    };

    if driver_throttle <= 0.0 {
        if !progress.paused_for_driver_log {
            viewer_log!(
                "openrailsrs-viewer3d: pausing world scenery GPU load while in driver view (VRAM; OPENRAILSRS_CAB_PAUSE_WORLD=1)"
            );
            progress.paused_for_driver_log = true;
        }
        return;
    }

    if *follow == CameraFollowMode::DriverCam && !progress.paused_for_driver_log {
        viewer_log!(
            "openrailsrs-viewer3d: throttling world scenery GPU load in driver view ({:.0}% rate; set OPENRAILSRS_CAB_PAUSE_WORLD=1 to pause)",
            driver_throttle * 100.0
        );
        progress.paused_for_driver_log = true;
    } else if *follow != CameraFollowMode::DriverCam {
        progress.paused_for_driver_log = false;
    }

    let classify_batch = scaled_usize(CLASSIFY_ITEMS_PER_FRAME, driver_throttle);
    let asset_batch = scaled_usize(SHAPE_ASSETS_PER_FRAME, driver_throttle);
    let build_batch = scaled_usize(BUILD_QUEUE_SHAPES_PER_FRAME, driver_throttle);
    let spawn_batch = scaled_usize(SPAWN_ENTITIES_PER_FRAME, driver_throttle);

    match progress.phase {
        WorldSpawnPhase::Classifying => {
            // Defense: never slice past `world.items` if another system shrank the vec.
            progress.item_index = progress.item_index.min(world.items.len());
            let end = (progress.item_index + classify_batch).min(world.items.len());
            for obj in &world.items[progress.item_index..end] {
                classify_one_object(
                    obj,
                    &focus,
                    cull_center,
                    &assets,
                    *mode,
                    &wire,
                    &mut progress,
                );
            }
            progress.item_index = end;
            if progress.item_index >= world.items.len() {
                viewer_log!(
                    "openrailsrs-viewer3d: classified {} visible world item(s)",
                    world.items.len().saturating_sub(progress.culled_count)
                );
                if progress.trackobj_seen > 0 {
                    viewer_log!(
                        "openrailsrs-viewer3d: TrackObj <={}m: {} seen = {} mesh + {} procedural + {} failed",
                        shape_mesh_radius_m() as u32,
                        progress.trackobj_seen,
                        progress.trackobj_resolved,
                        progress.trackobj_procedural_objects,
                        progress.trackobj_failed
                    );
                }
                progress.phase = if progress.shape_instances.is_empty() {
                    WorldSpawnPhase::SpawningPlaceholders
                } else {
                    WorldSpawnPhase::LoadingShapes
                };
                progress.loading_shapes_started = Some(Instant::now());
            }
        }
        WorldSpawnPhase::LoadingShapes => {
            prepare_shape_load_paths(&mut progress);
            hydrate_spawn_from_session(&mut progress, &mut session);
            if parse_next_shape_batch(&mut progress, &assets.route_dir) {
                return;
            }
            if prefetch_next_shape_texture_batch(&mut progress) {
                return;
            }
            finish_shape_loading(&mut progress, &assets.route_dir, &mut materials);

            let Some(fallback_material) = progress.shape_fallback_material.clone() else {
                return;
            };
            let fallback_color = progress.shape_fallback_color;
            let ace_cache = progress.ace_cache.clone();
            let end = (progress.asset_build_index + asset_batch).min(progress.parsed_shapes.len());
            let batch: Vec<(PathBuf, Option<crate::shapes::LoadedShape>)> =
                progress.parsed_shapes[progress.asset_build_index..end].to_vec();
            for (shape_path, loaded) in batch {
                let (shape_path, asset) = build_world_shape_asset(
                    shape_path,
                    loaded,
                    &assets.route_dir,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                    &mut progress.texture_image_cache,
                    &ace_cache,
                    fallback_color,
                    &fallback_material,
                );
                if let Some(shape) = progress.parsed_shape_files.get(&shape_path).cloned() {
                    let lod_assets = build_shape_lod_assets(
                        &shape_path,
                        &shape,
                        &assets.route_dir,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                        &mut progress.texture_image_cache,
                        &ace_cache,
                        fallback_color,
                        &fallback_material,
                    );
                    if !lod_assets.is_empty() {
                        progress
                            .shape_lod_assets
                            .insert(shape_path.clone(), lod_assets);
                    }
                }
                progress.shape_cache.insert(shape_path, asset);
            }
            progress.asset_build_index = end;
            if progress.asset_build_index >= progress.parsed_shapes.len() {
                if let Some(start) = progress.loading_shapes_started {
                    log_step("loaded world shape assets", start);
                }
                progress.instance_paths = progress.shape_instances.keys().cloned().collect();
                progress.build_queue_index = 0;
                progress.build_queue_started = Some(Instant::now());
                progress.phase = WorldSpawnPhase::BuildingQueue;
            }
        }
        WorldSpawnPhase::BuildingQueue => {
            let end = (progress.build_queue_index + build_batch).min(progress.instance_paths.len());
            let paths: Vec<PathBuf> =
                progress.instance_paths[progress.build_queue_index..end].to_vec();
            for shape_path in paths {
                let Some(asset) = progress.shape_cache.get(&shape_path).cloned() else {
                    continue;
                };
                append_shape_spawn_entries(
                    &mut progress,
                    &shape_path,
                    &asset,
                    &mut meshes,
                    &mut materials,
                    &origin,
                    Some(assets.sigcfg()),
                );
            }
            progress.build_queue_index = end;
            if progress.build_queue_index >= progress.instance_paths.len() {
                if let Some(start) = progress.build_queue_started {
                    log_step(
                        &format!(
                            "built world spawn queue ({} entit(ies))",
                            progress.spawn_queue.len()
                        ),
                        start,
                    );
                }
                progress.spawn_index = 0;
                progress.phase = WorldSpawnPhase::SpawningEntities;
            }
        }
        WorldSpawnPhase::SpawningEntities => {
            if progress.spawn_index == 0
                && (!progress.spawn_queue.is_empty()
                    || !progress.anim_spawn_queue.is_empty()
                    || !progress.instanced_spawn_queue.is_empty())
            {
                viewer_log!(
                    "openrailsrs-viewer3d: spawning {} world mesh + {} instanced group(s) + {} animated entit(ies) progressively",
                    progress.spawn_queue.len(),
                    progress.instanced_spawn_queue.len(),
                    progress.anim_spawn_queue.len()
                );
            }
            let end = (progress.spawn_index + spawn_batch).min(progress.spawn_queue.len());
            let batch: Vec<ShapeSpawnBundle> =
                progress.spawn_queue[progress.spawn_index..end].to_vec();
            commands.spawn_batch(batch);
            progress.spawn_index = end;
            if progress.spawn_index >= progress.spawn_queue.len() {
                let instanced = std::mem::take(&mut progress.instanced_spawn_queue);
                for bundle in instanced {
                    commands.spawn(bundle);
                }
                // Animated bundles carry cloned ShapeFile — spawn one-by-one.
                let animated = std::mem::take(&mut progress.anim_spawn_queue);
                for bundle in animated {
                    commands.spawn(bundle);
                }
                progress.phase = WorldSpawnPhase::SpawningPlaceholders;
            }
        }
        WorldSpawnPhase::SpawningPlaceholders => {
            let placeholders: Vec<(String, MergedBoxGroup)> =
                progress.merged_boxes.drain().collect();
            for (kind, group) in placeholders {
                let material = materials.add(StandardMaterial {
                    base_color: group.color,
                    perceptual_roughness: 0.85,
                    metallic: 0.05,
                    ..default()
                });
                let mut mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::default(),
                );
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, group.positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, group.normals);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, group.uvs);
                let cuboid_count = group.indices.len() / 6;
                mesh.insert_indices(Indices::U32(group.indices));
                commands.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    Transform::from_translation(-crate::floating_origin::horizontal_shift(
                        origin.shift,
                    )),
                    Name::new(format!("world-boxes:{kind}")),
                ));
                viewer_log!("openrailsrs-viewer3d: merged {cuboid_count} {kind} placeholder(s)");
            }
            if !progress.trackobj_procedural.is_empty() {
                let segments = std::mem::take(&mut progress.trackobj_procedural)
                    .into_iter()
                    .map(|mut seg| {
                        seg.position = view_translation(seg.position, &origin);
                        seg
                    })
                    .collect::<Vec<_>>();
                crate::dyntrack::spawn_procedural_track_batch(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &segments,
                    "trackobj",
                    crate::dyntrack::ProceduralTrackStyle::Full,
                );
            }
            if !progress.trackobj_wire.is_empty() {
                let wire_count = progress.trackobj_wire.len();
                let segments = std::mem::take(&mut progress.trackobj_wire)
                    .into_iter()
                    .map(|mut seg| {
                        seg.position = view_translation(seg.position, &origin);
                        seg
                    })
                    .collect::<Vec<_>>();
                crate::overhead_wire::spawn_overhead_wire_batch(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &segments,
                    wire.style,
                    "trackobj",
                );
                viewer_log!(
                    "openrailsrs-viewer3d: overhead wire {wire_count} segment(s) at {:.2} m",
                    wire.style.height_m
                );
            }
            let load_diag =
                log_world_spawn_summary(&progress, Some(world.as_ref()), terrain.as_deref());
            commands.insert_resource(load_diag);
            commit_spawn_to_session(&mut session, &mut progress);
            cycle.finish();
            commands.remove_resource::<WorldSpawnProgress>();
        }
    }
}

/// Swap world shape meshes when the camera crosses MSTS `dlevel_selection` thresholds.
///
/// Animated WORLD parts keep [`ShapeAnimState::key`] across swaps and re-apply the
/// baked-rest delta on the new mesh (#100).
#[allow(clippy::type_complexity)]
pub fn update_world_scenery_lod(
    cache: Option<Res<WorldShapeLodCache>>,
    progress: Option<Res<WorldSpawnProgress>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    focus: Option<Res<RouteFocus>>,
    mut lod_cam: ResMut<WorldLodCameraState>,
    mut parts: Query<(
        &GlobalTransform,
        &mut WorldSceneryLod,
        &mut Mesh3d,
        &mut Visibility,
        Option<&mut ShapeAnimState>,
        Option<&mut ShapeAnimBinding>,
        Option<&mut Transform>,
    )>,
) {
    let Some(cache) = cache else {
        return;
    };
    let Ok(cam_gt) = camera.single() else {
        return;
    };
    let cam_pos = cam_gt.translation();
    let focus_pos = focus
        .as_ref()
        .map(|f| f.scenery_to_render(f.center))
        .unwrap_or(Vec3::ZERO);

    // A streaming cycle can add parts while the session LOD cache is incomplete.
    // Force a full pass as soon as the cycle commits, even if the camera stayed still.
    if progress.is_some() {
        lod_cam.last_cam = None;
        lod_cam.last_focus = None;
        return;
    }

    if !lod_camera_needs_update(&lod_cam, cam_pos, focus_pos, WORLD_LOD_EPS_M) {
        return;
    }
    lod_cam.last_cam = Some(cam_pos);
    lod_cam.last_focus = Some(focus_pos);

    let lod_t0 = Instant::now();
    let mut scanned = 0u32;
    let mut swapped = 0u32;

    for (gt, mut lod, mut mesh3d, mut visibility, anim_state, anim_binding, transform) in &mut parts
    {
        if !lod.enabled {
            continue;
        }
        scanned += 1;
        let Some(shape) = cache.shapes.get(&lod.shape_path) else {
            continue;
        };
        let Some(lod_assets) = cache.assets_by_lod.get(&lod.shape_path) else {
            continue;
        };
        if lod_assets.is_empty() {
            continue;
        }
        let instance_dist = world_lod_distance_m(cam_pos, gt.translation());
        let new_lod = lod_level_index_for_distance(shape, instance_dist).min(lod_assets.len() - 1);
        if new_lod == lod.lod_idx {
            continue;
        }
        let Some(asset) = lod_assets.get(new_lod) else {
            continue;
        };
        let Some((part_index, part)) =
            shape_lod_part_by_identity(asset, lod.sub_object_idx, lod.prim_state_idx)
        else {
            // The target band intentionally omits this primitive group.
            *visibility = Visibility::Hidden;
            lod.lod_idx = new_lod;
            swapped += 1;
            continue;
        };
        mesh3d.0 = part.mesh.clone();
        *visibility = Visibility::Inherited;
        lod.part_index = part_index;
        lod.lod_idx = new_lod;
        if let (Some(mut state), Some(mut binding), Some(mut tf)) =
            (anim_state, anim_binding, transform)
        {
            let matrix_idx = matrix_idx_for_prim_state(shape, part.prim_state_idx);
            state.matrix_idx = matrix_idx;
            binding.matrix_idx = matrix_idx;
            if binding.baked_rest_mesh {
                let pose = openrailsrs_bevy_scenery::shapes::animation_pose_matrices(
                    &binding.shape,
                    state.key,
                );
                *tf = openrailsrs_bevy_scenery::shapes::world_baked_anim_transform(
                    binding.placement,
                    &binding.shape,
                    matrix_idx,
                    &pose,
                );
            }
        }
        swapped += 1;
    }
    if std::env::var_os("OPENRAILSRS_PERF_DEBUG").is_some() && scanned > 0 {
        eprintln!(
            "[PERF] world_scenery_lod_ms={:.2} scanned={scanned} swapped={swapped}",
            lod_t0.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// Spawn all world objects in one shot (tests and synchronous tooling).
#[allow(clippy::too_many_arguments)]
pub fn spawn_world_boxes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    focus: Res<RouteFocus>,
    origin: Res<FloatingOrigin>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
    mode: Res<ViewerSceneryMode>,
) {
    if world.is_empty() {
        return;
    }

    viewer_log!(
        "openrailsrs-viewer3d: spawning world objects ({} item(s))",
        world.items.len()
    );
    let spawn_start = Instant::now();

    let base = scene.bounds.edge_radius().max(2.0) * 1.5;
    let mut shape_cache: std::collections::HashMap<PathBuf, ShapeRenderAsset> =
        std::collections::HashMap::new();
    let mut texture_image_cache: std::collections::HashMap<(PathBuf, i32), Handle<Image>> =
        std::collections::HashMap::new();
    let mut shape_instances: std::collections::HashMap<PathBuf, Vec<ShapeInstancePlacement>> =
        std::collections::HashMap::new();
    let mut trackobj_procedural: Vec<crate::dyntrack::ProceduralTrackSegment> = Vec::new();

    let mut merged_boxes: std::collections::HashMap<&str, MergedBoxGroup> =
        std::collections::HashMap::new();

    let shape_fallback_color = Color::srgb(0.72, 0.55, 0.42);
    let shape_fallback_material = add_shape_fallback_material(&mut materials, shape_fallback_color);

    let mut shape_mesh_count = 0usize;
    let mut shape_texture_count = 0usize;
    let mut culled_count = 0usize;
    let mut trackobj_seen = 0usize;
    let mut trackobj_resolved = 0usize;
    let mut trackobj_procedural_objects = 0usize;
    let mut trackobj_failed = 0usize;
    let mut shape_spawn_batches: Vec<ShapeSpawnBundle> = Vec::new();
    let mut instanced_spawn_batches: Vec<InstancedShapeSpawnBundle> = Vec::new();
    let mut merged_shape_groups = 0usize;
    let mut instanced_groups = 0usize;
    let mut instanced_instances = 0usize;

    for obj in &world.items {
        if obj.kind == "Dyntrack"
            || obj.kind == "Forest"
            || obj.kind == "HWater"
            || obj.kind == "Transfer"
            || obj.kind == "CarSpawner"
            || is_tr_item_label_only(obj.kind)
        {
            continue;
        }

        if should_cull_world_object(&focus, obj.position) {
            culled_count += 1;
            continue;
        }

        let dist = focus.horizontal_distance(obj.position);

        if !mode.loads_msts_scenery() {
            continue;
        }

        if obj.kind == "TrackObj" {
            if dist > shape_mesh_radius_m() {
                continue;
            }
            trackobj_seen += 1;
            if let Some(shape_path) = resolve_object_shape_path(obj, &assets) {
                trackobj_resolved += 1;
                let render_pos = obj.render_position(&focus);
                shape_instances
                    .entry(shape_path)
                    .or_default()
                    .push(ShapeInstancePlacement {
                        transform: Transform {
                            translation: render_pos,
                            rotation: obj.rotation,
                            scale: obj.scale,
                        },
                        linear: obj.linear,
                        tile_x: obj.tile_x,
                        tile_z: obj.tile_z,
                        auto_z_bias: true,
                        signal_sub_obj: None,
                    });
                continue;
            }
            if SPAWN_TRACKOBJ_PROCEDURAL {
                let segs =
                    trackobj_procedural_segments(obj, obj.render_position(&focus), &assets, *mode);
                if !segs.is_empty() {
                    trackobj_procedural_objects += 1;
                    trackobj_procedural.extend(segs);
                    continue;
                }
            }
            trackobj_failed += 1;
            if !trackobj_placeholders_enabled() {
                continue;
            }
        } else if shape_eligible(obj) && dist <= shape_mesh_radius_m() {
            if let Some(shape_path) = resolve_object_shape_path(obj, &assets) {
                let render_pos = obj.render_position(&focus);
                shape_instances
                    .entry(shape_path)
                    .or_default()
                    .push(ShapeInstancePlacement {
                        transform: Transform {
                            translation: render_pos,
                            rotation: obj.rotation,
                            scale: obj.scale,
                        },
                        linear: obj.linear,
                        tile_x: obj.tile_x,
                        tile_z: obj.tile_z,
                        auto_z_bias: false,
                        signal_sub_obj: (obj.kind == "Signal")
                            .then(|| obj.signal.as_ref().map(|s| s.signal_sub_obj))
                            .flatten(),
                    });
                continue;
            }
        }

        // Placeholders only near the camera; 4–8 km clutter dominated spawn time on large routes.
        if dist > shape_mesh_radius_m() {
            continue;
        }
        if suppress_world_placeholder(obj.kind) {
            continue;
        }

        let size = box_size_for_kind(obj.kind, base);
        let translation = obj.render_position(&focus) + Vec3::Y * (size.y * 0.5);
        // Size is already applied in local verts; keep transform scale at 1 to avoid size² cubes.
        let tf = Transform {
            translation,
            rotation: obj.rotation,
            scale: Vec3::ONE,
        };
        let kind_entry = merged_boxes
            .entry(obj.kind)
            .or_insert_with(|| MergedBoxGroup {
                positions: Vec::new(),
                normals: Vec::new(),
                uvs: Vec::new(),
                indices: Vec::new(),
                color: kind_color(obj.kind),
            });
        push_cuboid(
            &mut kind_entry.positions,
            &mut kind_entry.normals,
            &mut kind_entry.uvs,
            &mut kind_entry.indices,
            &tf,
            size,
        );
    }

    let shape_load_start = Instant::now();
    let unique_shape_paths: Vec<PathBuf> = shape_instances.keys().cloned().collect();
    let parsed_shapes: Vec<(PathBuf, Option<(ShapeFile, crate::shapes::LoadedShape)>)> =
        unique_shape_paths
            .par_iter()
            .map(|path| (path.clone(), load_shape_file_and_loaded(path, None)))
            .collect();
    log_step(
        &format!(
            "parsed {} unique world shape(s) in parallel",
            unique_shape_paths.len()
        ),
        shape_load_start,
    );

    let mut texture_paths = Vec::new();
    for (shape_path, loaded) in &parsed_shapes {
        if let Some((_, loaded)) = loaded {
            let tex_dirs = texture_search_dirs_for_shape(shape_path, &assets.route_dir);
            let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
            texture_paths.extend(collect_loaded_shape_texture_paths(loaded, &tex_refs));
            let pbr = load_shape_pbr_sidecar(shape_path);
            texture_paths.extend(collect_pbr_normal_map_texture_paths(
                pbr.as_ref(),
                &tex_refs,
            ));
        }
    }
    texture_paths.sort_unstable();
    texture_paths.dedup();
    let tex_start = Instant::now();
    let ace_cache = prefetch_ace_textures(&texture_paths);
    log_step(
        &format!(
            "prefetched {} world shape texture(s) in parallel",
            ace_cache.len()
        ),
        tex_start,
    );

    let asset_start = Instant::now();
    let mut parsed_shape_files: HashMap<PathBuf, ShapeFile> = HashMap::new();
    for (shape_path, loaded) in parsed_shapes {
        let loaded_mesh = loaded.map(|(sf, mesh)| {
            parsed_shape_files.insert(shape_path.clone(), sf);
            mesh
        });
        let (shape_path, asset) = build_world_shape_asset(
            shape_path,
            loaded_mesh,
            &assets.route_dir,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_image_cache,
            &ace_cache,
            shape_fallback_color,
            &shape_fallback_material,
        );
        shape_cache.insert(shape_path, asset);
    }
    log_step("built world shape Bevy assets", asset_start);

    let mut anim_spawn_batches: Vec<AnimatedShapeSpawnBundle> = Vec::new();
    for (shape_path, placements) in shape_instances {
        let Some(asset) = shape_cache.get(&shape_path) else {
            continue;
        };
        append_shape_spawn_entries_for_transforms(
            &shape_path,
            asset,
            parsed_shape_files.get(&shape_path),
            &mut meshes,
            &mut materials,
            &placements,
            &mut shape_spawn_batches,
            &mut anim_spawn_batches,
            &mut instanced_spawn_batches,
            0,
            &mut shape_mesh_count,
            &mut shape_texture_count,
            &mut merged_shape_groups,
            &mut instanced_groups,
            &mut instanced_instances,
            &origin,
            Some(assets.sigcfg()),
        );
    }

    if !shape_spawn_batches.is_empty() {
        commands.spawn_batch(shape_spawn_batches);
    }
    for bundle in instanced_spawn_batches {
        commands.spawn(bundle);
    }
    for bundle in anim_spawn_batches {
        commands.spawn(bundle);
    }

    for (kind, group) in merged_boxes {
        let material = materials.add(StandardMaterial {
            base_color: group.color,
            perceptual_roughness: 0.85,
            metallic: 0.05,
            ..default()
        });
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, group.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, group.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, group.uvs);
        let cuboid_count = group.indices.len() / 6;
        mesh.insert_indices(Indices::U32(group.indices));
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(-crate::floating_origin::horizontal_shift(origin.shift)),
            Name::new(format!("world-boxes:{}", kind)),
        ));
        viewer_log!(
            "openrailsrs-viewer3d: merged {cuboid_count} {} placeholder(s)",
            kind
        );
    }

    if !trackobj_procedural.is_empty() {
        let shifted: Vec<_> = trackobj_procedural
            .iter()
            .map(|seg| {
                let mut s = *seg;
                s.position = view_translation(s.position, &origin);
                s
            })
            .collect();
        crate::dyntrack::spawn_procedural_track_batch(
            &mut commands,
            &mut meshes,
            &mut materials,
            &shifted,
            "trackobj",
            crate::dyntrack::ProceduralTrackStyle::Full,
        );
    }

    if culled_count > 0 {
        let radius_m = visible_radius_m();
        viewer_log!(
            "openrailsrs-viewer3d: {culled_count} world object(s) culled (>{radius_m:.0}m from centre)"
        );
    }
    if trackobj_seen > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: TrackObj <={}m: {trackobj_seen} seen = {trackobj_resolved} mesh + {trackobj_procedural_objects} procedural + {trackobj_failed} failed",
            shape_mesh_radius_m() as u32
        );
    }
    if merged_shape_groups > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: merged {merged_shape_groups} repeated shape(s) (≥{SHAPE_INSTANCE_MERGE_MIN} instances)"
        );
    }
    if instanced_groups > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: GPU instanced {instanced_groups} group(s) covering {instanced_instances} instance(s) (#58)"
        );
    }
    if shape_mesh_count > 0 {
        viewer_log!("openrailsrs-viewer3d: {shape_mesh_count} world shape part(s) spawned");
    }
    if shape_texture_count > 0 {
        viewer_log!("openrailsrs-viewer3d: {shape_texture_count} textured instance(s)");
    }
    log_step("spawned world objects", spawn_start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::ShapePartAsset;
    use openrailsrs_formats::Vec3 as FVec3;

    #[test]
    fn qdir_identity_is_identity() {
        let q = qdir_to_quat(&[0.0, 0.0, 0.0, 1.0]);
        assert!((q.length() - 1.0).abs() < 1e-4);
        assert!((q.w.abs() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn qdir_y_90_matches_openrails_xna_rotation() {
        // File stores (0, sin45, 0, cos45) — same numeric values OR uses in XNA.
        let q = qdir_to_quat(&[
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ]);
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!((q.dot(expected).abs() - 1.0) < 1e-3 || (q.dot(-expected).abs() - 1.0) < 1e-3);
    }

    #[test]
    fn matrix3x3_extracts_non_uniform_scale() {
        let m = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 3.0];
        let (_, scale) = matrix3x3_to_rotation_scale(&m);
        assert!((scale.x - 2.0).abs() < 1e-3);
        assert!((scale.y - 1.0).abs() < 1e-3);
        assert!((scale.z - 3.0).abs() < 1e-3);
    }

    #[test]
    fn matrix3x3_rows_match_openrails_xna_layout() {
        // A proper 90° rotation around Y in MSTS row-major convention.
        // MSTS row-major: rows are [X-axis, Y-axis, Z-axis] of the world frame in local space.
        // For 90° CW around Y (MSTS): X→Z, Y→Y, Z→-X
        // Row 0 (MSTS X in local): (0, 0, 1)
        // Row 1 (MSTS Y in local): (0, 1, 0)
        // Row 2 (MSTS Z in local): (-1, 0, 0)
        let m: [f64; 9] = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0];
        let (rot, scale) = crate::coordinates::matrix3x3_to_rotation_scale(&m);
        // Scale should be ~1 for a pure rotation.
        assert!((scale.x - 1.0).abs() < 1e-4, "scale.x={}", scale.x);
        assert!((scale.y - 1.0).abs() < 1e-4, "scale.y={}", scale.y);
        assert!((scale.z - 1.0).abs() < 1e-4, "scale.z={}", scale.z);
        assert!(
            (rot.length() - 1.0).abs() < 1e-4,
            "rot must be unit quat, len={}",
            rot.length()
        );
    }

    #[test]
    fn matrix3x3_identity_is_identity_quat() {
        let m = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let q = matrix3x3_to_quat(&m);
        assert!((q.x - 0.0).abs() < 1e-4);
        assert!((q.y - 0.0).abs() < 1e-4);
        assert!((q.z - 0.0).abs() < 1e-4);
        assert!((q.w - 1.0).abs() < 1e-4 || (q.w + 1.0).abs() < 1e-4);
    }

    #[test]
    fn placement_has_shear_only_for_true_shear() {
        let shear = Mat3::from_cols(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.35, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let sheared = ShapeInstancePlacement {
            transform: Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            linear: Some(shear),
            tile_x: 0,
            tile_z: 0,
            auto_z_bias: false,
            signal_sub_obj: None,
        };
        assert!(placement_has_shear(&sheared));

        let (rot, scale) =
            matrix3x3_to_rotation_scale(&[2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 3.0]);
        let orthogonal = ShapeInstancePlacement {
            transform: Transform {
                translation: Vec3::ZERO,
                rotation: rot,
                scale,
            },
            linear: Some(Mat3::from_quat(rot) * Mat3::from_diagonal(scale)),
            tile_x: 0,
            tile_z: 0,
            auto_z_bias: false,
            signal_sub_obj: None,
        };
        assert!(
            !placement_has_shear(&orthogonal),
            "orthogonal Matrix3x3 must not force shear bake (#174)"
        );
    }

    #[test]
    fn bake_linear_into_mesh_applies_shear_to_positions() {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[1.0_f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vec![[0.0_f32, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        );
        let shear = Mat3::from_cols(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.5, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let baked = bake_linear_into_mesh(&mesh, shear);
        let VertexAttributeValues::Float32x3(pos) =
            baked.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("positions");
        };
        // (0,1,0) → (0.5, 1, 0) under this shear.
        assert!((pos[1][0] - 0.5).abs() < 1e-4);
        assert!((pos[1][1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn msts_tile_zero_uses_local_coords() {
        let p = msts_to_bevy(
            0,
            0,
            FVec3 {
                x: 100.0,
                y: 5.0,
                z: -3.0,
            },
        );
        assert_eq!(p, Vec3::new(100.0, 5.0, 3.0));
    }

    #[test]
    fn msts_tile_offset_scales_by_2048() {
        let p = msts_to_bevy(
            2,
            1,
            FVec3 {
                x: 10.0,
                y: 0.0,
                z: 20.0,
            },
        );
        // z = -(1*2048 + 20) = -2068 (whole-world Z negation).
        assert_eq!(p, Vec3::new(4106.0, 0.0, -2068.0));
    }

    #[test]
    fn world_position_rebase_restores_sub_metre_track_placement() {
        let tile_x = -6079;
        let tile_z = 14925;
        let anchor_local = FVec3 {
            x: -961.3,
            y: 28.5577,
            z: -71.9,
        };
        let object_a_local = FVec3 {
            x: -949.815,
            y: 28.5577,
            z: -125.436,
        };
        let object_b_local = FVec3 {
            x: -947.852,
            y: 28.5577,
            z: -95.5,
        };
        let anchor = msts_to_bevy(tile_x, tile_z, anchor_local);
        let quantized_a = msts_to_bevy(tile_x, tile_z, object_a_local);
        let quantized_b = msts_to_bevy(tile_x, tile_z, object_b_local);
        let precision_a =
            msts_position_precision_offset(tile_x, tile_z, object_a_local, quantized_a);
        let precision_b =
            msts_position_precision_offset(tile_x, tile_z, object_b_local, quantized_b);
        let focus = RouteFocus::at_world_center(anchor, None);

        let quantized_delta =
            focus.scenery_to_render(quantized_b) - focus.scenery_to_render(quantized_a);
        let precise_delta = quantized_delta + precision_b - precision_a;
        let expected = Vec3::new(
            (object_b_local.x - object_a_local.x) as f32,
            0.0,
            (object_a_local.z - object_b_local.z) as f32,
        );

        assert!(
            precise_delta.distance(expected) < 1e-4,
            "precise={precise_delta:?} expected={expected:?}"
        );
        assert!(
            quantized_delta.distance(expected) > 0.01,
            "fixture must expose absolute-f32 quantization"
        );
    }

    #[test]
    fn world_object_density_keeps_levels_at_or_below_threshold() {
        // #141: StaticDetailLevel > density → omit (OR WorldObjectDensity).
        assert!(keep_by_world_object_density(0, 49));
        assert!(keep_by_world_object_density(49, 49));
        assert!(!keep_by_world_object_density(50, 49));
        assert!(!keep_by_world_object_density(99, 10));
        assert!(keep_by_world_object_density(10, 10));
    }

    #[test]
    fn density_filter_selects_uids_from_synthetic_multi_level_world() {
        use openrailsrs_formats::parser::parse_from_first_paren;

        let src = r#"
(Tr_Worldfile
  (Tr_Watermark 5)
  (Static (UiD 1) (FileName "a.s") (Position 0 0 0) (QDirection 0 0 0 1))
  (Tr_Watermark 20)
  (Static (UiD 2) (FileName "b.s") (Position 1 0 0) (QDirection 0 0 0 1))
  (Tr_Watermark 60)
  (Static (UiD 3) (FileName "c.s") (Position 2 0 0) (QDirection 0 0 0 1))
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 3);

        let mut dense = WorldScene::default();
        append_world_tile_with_density(&mut dense, &world, None, 49);
        let uids: Vec<u32> = dense.items.iter().filter_map(|o| o.uid).collect();
        assert_eq!(uids, vec![1, 2]);
        assert_eq!(dense.items_skipped_by_density, 1);

        let mut sparse = WorldScene::default();
        append_world_tile_with_density(&mut sparse, &world, None, 10);
        let uids: Vec<u32> = sparse.items.iter().filter_map(|o| o.uid).collect();
        assert_eq!(uids, vec![1]);
        assert_eq!(sparse.items_skipped_by_density, 2);

        let mut all = WorldScene::default();
        append_world_tile_with_density(&mut all, &world, None, 99);
        assert_eq!(all.items.len(), 3);
        assert_eq!(all.items_skipped_by_density, 0);
    }

    #[test]
    fn load_fixture_world_from_route_dir() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        assert_eq!(scene.tiles_loaded, 1);
        assert_eq!(scene.items.len(), 7);
        assert!(scene.items.iter().any(|o| o.kind == "Static"));
        assert!(scene.items.iter().any(|o| o.kind == "Forest"));
        assert!(scene.items.iter().any(|o| o.kind == "HWater"));
        assert!(scene.items.iter().any(|o| o.kind == "Transfer"));
        assert!(scene.items.iter().any(|o| o.kind == "Dyntrack"));
    }

    #[test]
    fn platform_siding_are_labels_not_shape_or_placeholder() {
        assert!(is_tr_item_label_only("Platform"));
        assert!(is_tr_item_label_only("Siding"));
        assert!(!is_tr_item_label_only("Static"));
        assert!(suppress_world_placeholder("Platform"));
        assert!(suppress_world_placeholder("Other"));
        let platform = WorldObject {
            kind: "Platform",
            uid: Some(1),
            label: String::new(),
            shape_file: Some("Platforms1and2.s".into()),
            section_idx: None,
            dyntrack_sections: Vec::new(),
            position: Vec3::ZERO,
            position_precision_offset: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            linear: None,
            tile_x: 0,
            tile_z: 0,
            forest: None,
            water: None,
            transfer: None,
            car_spawner: None,
            signal: None,
            tr_item_ids: Vec::new(),
            static_detail_level: 0,
        };
        assert!(
            !shape_eligible(&platform),
            "OR draws Platform as TrItemLabel only, never as scenery mesh"
        );
    }

    /// Chiltern Birmingham-like focus: WORLD/TDB Y ~35–37 m, terrain RAW ~28.5 m.
    fn chiltern_like_focus() -> RouteFocus {
        RouteFocus {
            center: Vec3::new(-12_450_948.0, 35.7818, -30_566_982.0),
            height_origin: 28.5,
        }
    }

    #[test]
    fn default_visible_radius_matches_viewing_distance() {
        assert_eq!(VISIBLE_RADIUS_M, crate::launch::VIEWING_DISTANCE_M);
        assert_eq!(shape_mesh_radius_m(), visible_radius_m());
        assert!(crate::launch::scenery_content_radius_m() > visible_radius_m());
        assert!(crate::launch::view_unload_radius_m() > visible_radius_m());
    }

    #[test]
    fn route_focus_scenery_preserves_absolute_y_offset() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(focus.center.x + 50.0, 55.0, focus.center.z + 60.0);
        let local = focus.to_render(obj);
        assert!((local.y - (55.0 - focus.center.y)).abs() < 1e-3);
        assert!(
            local.x.abs() < 500.0 && local.z.abs() < 500.0,
            "horizontal rebasing failed: {:?}",
            local
        );
    }

    #[test]
    fn scenery_y_to_msl_is_absolute_datum() {
        let focus = chiltern_like_focus();
        // #64: no (y - center.y) remap onto height_origin.
        assert!((focus.scenery_y_to_msl(35.7818) - 35.7818).abs() < 1e-4);
        assert!((focus.scenery_y_to_msl(55.0) - 55.0).abs() < 1e-4);
    }

    #[test]
    fn route_focus_surface_uses_height_origin() {
        let focus = chiltern_like_focus();
        let rail = Vec3::new(focus.center.x, 35.7818, focus.center.z);
        let local = focus.to_render_surface(rail);
        assert!(
            (local.y - (35.7818 - 28.5)).abs() < 0.05,
            "rail should sit ~7.3 m above terrain plane, got {}",
            local.y
        );
    }

    #[test]
    fn culling_uses_horizontal_distance_not_msl_y() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(focus.center.x + 80.0, 55.0, focus.center.z + 60.0);
        assert!(
            !should_cull_world_object(&focus, obj),
            "object ~100 m away horizontally must not be culled (default view radius 120 m)"
        );
        let high = Vec3::new(focus.center.x, 200.0, focus.center.z);
        assert!(
            !should_cull_world_object(&focus, high),
            "same xz as centre must not be culled despite high y"
        );
    }

    #[test]
    fn scenery_rail_height_not_flattened_to_terrain() {
        let focus = chiltern_like_focus();
        // Object authored at OR rail height must NOT collapse to render Y≈0.
        let rail_obj = Vec3::new(focus.center.x, 35.7818, focus.center.z);
        let render = focus.scenery_to_render(rail_obj);
        assert!(
            (render.y - (35.7818 - focus.height_origin)).abs() < 0.05,
            "WORLD Y=35.78 must keep embankment offset, got {}",
            render.y
        );
        assert!(
            render.y > 5.0,
            "rail-height WORLD must sit above terrain plane, got {}",
            render.y
        );
    }

    #[test]
    fn scenery_y_delta_vs_center_is_preserved_in_render() {
        let focus = chiltern_like_focus();
        let low = focus.scenery_to_render(Vec3::new(focus.center.x, 28.5, focus.center.z));
        let high = focus.scenery_to_render(Vec3::new(focus.center.x, 35.7818, focus.center.z));
        assert!(
            (high.y - low.y - (35.7818 - 28.5)).abs() < 0.05,
            "vertical authored delta must survive scenery_to_render"
        );
    }

    #[test]
    fn from_scene_world_and_elevation_prefers_terrain_msl() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("TILES").is_dir() {
            return;
        }
        let world = load_world_from_route_dir(&route_dir);
        let scene = TrackScene::from_graph(openrailsrs_track::TrackGraph::new());
        let elev = TerrainElevation::load_from_route_dir_near(&route_dir, None, f32::MAX);
        let focus = RouteFocus::from_scene_world_and_elevation(&scene, &world, Some(&elev));

        // With elevation loaded, height_origin must be a finite terrain MSL sample.
        assert!(focus.height_origin.is_finite() && focus.height_origin >= 0.0);
        assert!(!elev.is_empty(), "expected Chiltern elevation tiles");
    }

    #[test]
    fn chiltern_world_loads_near_focus_not_all_tiles() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("WORLD").is_dir() {
            return;
        }
        let all = load_world_from_route_dir(&route_dir);
        let center = world_tile_center_hint(&route_dir).expect("tile hint");
        let near = load_world_from_route_dir_near(&route_dir, Some(center), VISIBLE_RADIUS_M);
        assert!(
            near.tiles_loaded < all.tiles_loaded,
            "near load should parse fewer tiles than full route ({} vs {})",
            near.tiles_loaded,
            all.tiles_loaded
        );
        assert!(
            near.tiles_loaded >= 1,
            "expected at least 1 tile near centre, got {}",
            near.tiles_loaded
        );
        assert!(!near.items.is_empty(), "expected scenery items near centre");
    }

    #[test]
    fn early_item_window_matches_post_retain_set() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("WORLD").is_dir() {
            return;
        }
        let center = world_tile_center_hint(&route_dir).expect("tile hint");
        let tile_radius = 300.0_f32;
        let keep = world_item_keep_radius_m(tile_radius);
        let filtered =
            load_world_from_route_dir_near_filtered(&route_dir, Some(center), tile_radius, true);
        let mut unfiltered =
            load_world_from_route_dir_near_filtered(&route_dir, Some(center), tile_radius, false);
        assert!(
            filtered.items_skipped_out_of_window > 0,
            "expected early skips on Chiltern overlay tiles"
        );
        assert!(
            unfiltered.items.len() > filtered.items.len(),
            "unfiltered materialization should keep more objects before retain"
        );
        let focus = RouteFocus::at_world_center(center, None);
        unfiltered.retain_within_visible_radius(&focus, keep);
        let key = |o: &WorldObject| {
            (
                o.tile_x,
                o.tile_z,
                o.uid,
                o.kind,
                o.position.x.to_bits(),
                o.position.z.to_bits(),
            )
        };
        let mut a: Vec<_> = filtered.items.iter().map(key).collect();
        let mut b: Vec<_> = unfiltered.items.iter().map(key).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(
            a,
            b,
            "early filter must match retain set ({} vs {} items)",
            a.len(),
            b.len()
        );
        let window = WorldItemWindow {
            center,
            radius_m: keep,
        };
        assert!(
            filtered
                .items
                .iter()
                .all(|o| window.contains_xz(o.position)),
            "no out-of-window objects may remain in WorldScene"
        );
    }

    #[test]
    fn world_item_window_rejects_far_points() {
        let w = WorldItemWindow {
            center: Vec3::new(0.0, 0.0, 0.0),
            radius_m: 100.0,
        };
        assert!(w.contains_xz(Vec3::new(50.0, 999.0, 0.0)));
        assert!(!w.contains_xz(Vec3::new(101.0, 0.0, 0.0)));
    }

    #[test]
    fn discover_world_tile_entries_matches_chiltern_anchor() {
        let route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route_dir.join("WORLD").is_dir() {
            return;
        }
        // Render-space anchor: signed tile X, whole-world Z negation.
        let anchor = Vec3::new(
            -6080.0 * MSTS_TILE_SIZE_M as f32 + 891.8,
            35.8,
            -(14925.0 * MSTS_TILE_SIZE_M as f32 + 582.8),
        );
        let entries = discover_world_tile_entries(&route_dir, Some(anchor), VISIBLE_RADIUS_M);
        assert!(
            !entries.is_empty() && entries.len() <= 16,
            "expected 1–16 tiles at {:.0} m, got {}",
            VISIBLE_RADIUS_M,
            entries.len()
        );
        // The anchor sits in tile (-6080, 14925); its own tile must be discovered.
        assert!(
            entries.iter().any(|(x, z, _)| *x == -6080 && *z == 14925),
            "expected tile w-006080+014925 (anchor tile) in discovered entries"
        );
    }

    #[test]
    fn world_tile_path_resolves_mixed_case_fixture() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("World");
        std::fs::create_dir_all(&world).expect("mkdir");
        let canonical = openrailsrs_formats::world_w_filename_from_tile_xz(-1, 2);
        let on_disk = world.join(canonical.replace(".w", ".W"));
        std::fs::write(&on_disk, b"x").expect("write");
        let resolved = world_tile_path_for_coords(dir.path(), -1, 2).expect("resolve");
        assert!(resolved.is_file());
        let catalog = openrailsrs_formats::build_world_tile_catalog(dir.path());
        assert!(catalog.contains_key(&(-1, 2)));
    }

    fn dummy_shape_asset() -> ShapeRenderAsset {
        ShapeRenderAsset {
            combined_mesh: Handle::default(),
            parts: Vec::new(),
            has_texture: false,
            has_night_subobj: false,
            texture_flags: openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(
                openrailsrs_bevy_scenery::textures::TextureFlags::NONE,
            ),
        }
    }

    fn dummy_shape_part(sub_object_idx: u32, prim_state_idx: i32) -> ShapePartAsset {
        ShapePartAsset {
            prim_state_idx,
            sub_object_idx,
            sort_index: 0,
            cab_matrix_idx: None,
            mesh: Handle::default(),
            material: Handle::default(),
            or_cab_material: None,
            has_texture: false,
            is_transparent: false,
            texture_name: None,
            shader_name: None,
            light_mat_idx: None,
            solid_color: None,
            lever_pivot_at_mesh_center: false,
            lever_local_axis: None,
            bounds_center: None,
        }
    }

    #[test]
    fn lod_part_lookup_survives_omitted_and_reordered_groups() {
        // Real track shapes do this: a coarser band removes prim-state 1, so
        // prim-state 2 shifts from vector index 2 to index 1.
        let mut asset = dummy_shape_asset();
        asset.parts = vec![
            dummy_shape_part(u32::MAX, 0),
            dummy_shape_part(u32::MAX, 2),
            dummy_shape_part(u32::MAX, 4),
        ];

        let (index, part) =
            shape_lod_part_by_identity(&asset, u32::MAX, 2).expect("stable part identity");
        assert_eq!(index, 1);
        assert_eq!(part.prim_state_idx, 2);
        assert!(shape_lod_part_by_identity(&asset, u32::MAX, 1).is_none());
    }

    #[test]
    fn evict_unreferenced_world_shapes_keeps_shared_and_frees_unused() {
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();

        let keep_path = PathBuf::from("/shapes/keep.s");
        let drop_path = PathBuf::from("/shapes/drop.s");
        let shared_tex = images.add(Image::default());
        let drop_tex = images.add(Image::default());

        let keep_mat = materials.add(StandardMaterial {
            base_color_texture: Some(shared_tex.clone()),
            ..default()
        });
        let drop_mat = materials.add(StandardMaterial {
            base_color_texture: Some(drop_tex.clone()),
            ..default()
        });
        let keep_mesh = meshes.add(Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        ));
        let drop_mesh = meshes.add(Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        ));

        let make_part = |mesh: Handle<Mesh>, material: Handle<StandardMaterial>| ShapePartAsset {
            prim_state_idx: 0,
            sub_object_idx: 0,
            sort_index: 0,
            cab_matrix_idx: None,
            mesh,
            material,
            or_cab_material: None,
            has_texture: true,
            is_transparent: false,
            texture_name: None,
            shader_name: None,
            light_mat_idx: None,
            solid_color: None,
            lever_pivot_at_mesh_center: false,
            lever_local_axis: None,
            bounds_center: None,
        };

        let mut session = WorldShapeLodCache::default();
        session.shape_assets.insert(
            keep_path.clone(),
            ShapeRenderAsset {
                combined_mesh: keep_mesh.clone(),
                parts: vec![make_part(keep_mesh.clone(), keep_mat)],
                has_texture: true,
                has_night_subobj: false,
                texture_flags: openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(
                    openrailsrs_bevy_scenery::textures::TextureFlags::NONE,
                ),
            },
        );
        session.shape_assets.insert(
            drop_path.clone(),
            ShapeRenderAsset {
                combined_mesh: drop_mesh.clone(),
                parts: vec![make_part(drop_mesh.clone(), drop_mat)],
                has_texture: true,
                has_night_subobj: false,
                texture_flags: openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(
                    openrailsrs_bevy_scenery::textures::TextureFlags::NONE,
                ),
            },
        );
        session
            .texture_images
            .insert((PathBuf::from("shared.ace"), 1), shared_tex.clone());
        session
            .texture_images
            .insert((PathBuf::from("drop.ace"), 1), drop_tex.clone());
        session
            .shapes
            .insert(keep_path.clone(), ShapeFile::default());
        session
            .shapes
            .insert(drop_path.clone(), ShapeFile::default());

        let live = HashSet::from([keep_path.clone()]);
        let (shapes, textures) = evict_unreferenced_world_shapes(
            &mut session,
            &live,
            &mut meshes,
            &mut images,
            &mut materials,
        );
        assert_eq!(shapes, 1);
        assert_eq!(textures, 1);
        assert!(session.shape_assets.contains_key(&keep_path));
        assert!(!session.shape_assets.contains_key(&drop_path));
        assert!(
            session
                .texture_images
                .contains_key(&(PathBuf::from("shared.ace"), 1))
        );
        assert!(
            !session
                .texture_images
                .contains_key(&(PathBuf::from("drop.ace"), 1))
        );
        assert!(meshes.get(keep_mesh.id()).is_some());
        assert!(meshes.get(drop_mesh.id()).is_none());
        assert!(images.get(shared_tex.id()).is_some());
        assert!(images.get(drop_tex.id()).is_none());
    }

    #[test]
    fn hydrate_spawn_from_session_skips_cached_paths() {
        let hit = PathBuf::from("/shapes/shared.s");
        let miss = PathBuf::from("/shapes/new.s");
        let mut session = WorldShapeLodCache::default();
        session
            .shape_assets
            .insert(hit.clone(), dummy_shape_asset());
        session.shapes.insert(hit.clone(), ShapeFile::default());

        let mut progress = WorldSpawnProgress::new(1.0);
        progress.shape_load_paths = vec![hit.clone(), miss.clone()];
        hydrate_spawn_from_session(&mut progress, &mut session);

        assert_eq!(progress.cache_hits, 1);
        assert_eq!(progress.cache_misses, 1);
        assert_eq!(progress.shape_load_paths, vec![miss]);
        assert!(progress.shape_cache.contains_key(&hit));
        assert!(progress.parsed_shape_files.contains_key(&hit));
        assert_eq!(session.session_hits(), 1);
        assert_eq!(session.session_misses(), 1);

        // Second hydrate is a no-op for the same progress cycle.
        hydrate_spawn_from_session(&mut progress, &mut session);
        assert_eq!(session.session_hits(), 1);
    }

    #[test]
    fn commit_spawn_to_session_merges_without_dropping_priors() {
        let prior = PathBuf::from("/shapes/prior.s");
        let fresh = PathBuf::from("/shapes/fresh.s");
        let mut session = WorldShapeLodCache::default();
        session
            .shape_assets
            .insert(prior.clone(), dummy_shape_asset());
        session.shapes.insert(prior.clone(), ShapeFile::default());

        let mut progress = WorldSpawnProgress::new(1.0);
        progress.cache_hits = 1;
        progress
            .shape_cache
            .insert(fresh.clone(), dummy_shape_asset());
        progress
            .parsed_shape_files
            .insert(fresh.clone(), ShapeFile::default());

        commit_spawn_to_session(&mut session, &mut progress);
        assert!(session.shape_assets.contains_key(&prior));
        assert!(session.shape_assets.contains_key(&fresh));
        assert_eq!(session.shape_assets.len(), 2);
        assert!(progress.shape_cache.is_empty());
    }

    #[test]
    fn session_cache_second_spawn_does_not_reparse_shared_shape() {
        let route = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        let Some(path) = std::fs::read_dir(route.join("DYNATRAX"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("s"))
            })
        else {
            eprintln!("skip: Chiltern DYNATRAX shapes not available");
            return;
        };

        reset_shape_file_parse_count();
        let (shape, _loaded) =
            load_shape_file_and_loaded(&path, None).expect("parse Chiltern shape once");
        assert_eq!(shape_file_parse_count(), 1);

        let mut session = WorldShapeLodCache::default();
        session.shapes.insert(path.clone(), shape);
        session
            .shape_assets
            .insert(path.clone(), dummy_shape_asset());

        // Second stream cycle: same path must be a hit (zero additional ShapeFile parses).
        let mut progress = WorldSpawnProgress::new(1.0);
        progress.shape_instances.insert(
            path.clone(),
            vec![ShapeInstancePlacement {
                transform: Transform::default(),
                linear: None,
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
                signal_sub_obj: None,
            }],
        );
        prepare_shape_load_paths(&mut progress);
        hydrate_spawn_from_session(&mut progress, &mut session);
        assert_eq!(progress.cache_hits, 1);
        assert!(progress.shape_load_paths.is_empty());

        let before = shape_file_parse_count();
        assert!(!parse_next_shape_batch(&mut progress, Path::new(".")));
        assert_eq!(
            shape_file_parse_count(),
            before,
            "shared shape must not call ShapeFile::from_path again"
        );

        // Simulate the next Update: prepare must not refill all-hit miss list.
        prepare_shape_load_paths(&mut progress);
        assert!(
            progress.shape_load_paths.is_empty(),
            "all-hit cycle must not re-queue shapes for parse"
        );
        assert!(!parse_next_shape_batch(&mut progress, Path::new(".")));
        assert_eq!(shape_file_parse_count(), before);
    }

    #[test]
    fn unload_uses_tile_bound_not_distance_when_tagged() {
        let mut unloaded = HashSet::new();
        unloaded.insert((10, 20));
        let a = WorldTileBound {
            tile_x: 10,
            tile_z: 20,
        };
        let b = WorldTileBound {
            tile_x: 11,
            tile_z: 20,
        };
        // Far from center but wrong tile → keep.
        assert!(!scenery_entity_should_unload(
            Some(b),
            &unloaded,
            9_999.0,
            100.0
        ));
        // Matching tile → unload even if close.
        assert!(scenery_entity_should_unload(Some(a), &unloaded, 1.0, 100.0));
        // Legacy without bound → distance.
        assert!(scenery_entity_should_unload(None, &unloaded, 200.0, 100.0));
        assert!(!scenery_entity_should_unload(None, &unloaded, 50.0, 100.0));
    }

    #[test]
    fn lod_early_out_when_camera_and_focus_still() {
        let mut state = WorldLodCameraState {
            last_cam: Some(Vec3::ZERO),
            last_focus: Some(Vec3::new(10.0, 0.0, 0.0)),
        };
        assert!(!lod_camera_needs_update(
            &state,
            Vec3::new(0.1, 0.0, 0.0),
            Vec3::new(10.2, 0.0, 0.0),
            WORLD_LOD_EPS_M
        ));
        assert!(lod_camera_needs_update(
            &state,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(10.2, 0.0, 0.0),
            WORLD_LOD_EPS_M
        ));
        // First frame (no prior sample) always updates.
        state.last_cam = None;
        assert!(lod_camera_needs_update(
            &state,
            Vec3::ZERO,
            Vec3::ZERO,
            WORLD_LOD_EPS_M
        ));
    }

    #[test]
    fn world_lod_distance_is_camera_to_entity_not_sum_via_focus() {
        // Camera and entity on the same side of a distant focus (#74).
        let cam = Vec3::new(100.0, 0.0, 0.0);
        let entity = Vec3::new(105.0, 0.0, 0.0);
        let focus = Vec3::ZERO;
        let correct = world_lod_distance_m(cam, entity);
        let legacy_sum = cam.distance(focus) + entity.distance(focus);
        assert!((correct - 5.0).abs() < 1e-4);
        assert!(legacy_sum > 190.0, "legacy sum overestimates: {legacy_sum}");
        assert!(correct < legacy_sum);

        // Opposite sides of focus: sum ≈ correct (degenerate), still equal to cam→entity.
        let cam2 = Vec3::new(-10.0, 0.0, 0.0);
        let entity2 = Vec3::new(10.0, 0.0, 0.0);
        let focus2 = Vec3::ZERO;
        let correct2 = world_lod_distance_m(cam2, entity2);
        let legacy2 = cam2.distance(focus2) + entity2.distance(focus2);
        assert!((correct2 - 20.0).abs() < 1e-4);
        assert!((legacy2 - correct2).abs() < 1e-4);
    }
}
