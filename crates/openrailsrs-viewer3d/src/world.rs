//! MSTS world tiles (`.w`) as coloured placeholder boxes (order 5 / issue #8).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    build_mesh_from_shape_lod, build_mesh_parts_from_shape_lod, lod_level_index_for_distance,
    primary_texture_filename,
};
use openrailsrs_formats::{
    ShapeFile, WorldFile, WorldItem, msts_tile_world_origin, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord, parse_world_w_tile_xz, world_w_filename_from_tile_xz,
};

use crate::camera::CameraFollowMode;
use crate::coordinates::{
    matrix3x3_to_rotation_scale, msts_local_offset_to_bevy, msts_tile_local_to_bevy, qdir_to_quat,
};
use crate::floating_origin::{FloatingOrigin, view_transform, view_translation};
use crate::launch::ViewerSceneryMode;
use crate::shapes::{
    RouteAssets, ShapeRenderAsset, collect_loaded_shape_texture_paths, load_shape_from_path,
    prefetch_ace_textures, shape_render_asset_from_loaded_with_ace_cache,
    texture_search_dirs_for_shape,
};

/// Tracks which LOD level a spawned world shape part is using (runtime swap).
#[derive(Component, Clone, Debug)]
pub struct WorldSceneryLod {
    pub enabled: bool,
    pub shape_path: PathBuf,
    pub prim_state_idx: i32,
    pub part_index: usize,
    pub lod_idx: usize,
}

/// Cached parsed shapes + pre-built assets per LOD level for runtime swaps.
#[derive(Resource, Default)]
pub struct WorldShapeLodCache {
    pub shapes: HashMap<PathBuf, ShapeFile>,
    pub assets_by_lod: HashMap<PathBuf, Vec<ShapeRenderAsset>>,
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

/// Within this radius, spawn real `.s` meshes when the file resolves; matches [`visible_radius_m`].
pub fn shape_mesh_radius_m() -> f32 {
    visible_radius_m()
}

/// Merge repeated `.s` instances into one baked mesh (small meshes only; disabled — breaks visuals on Chiltern).
const ENABLE_SHAPE_INSTANCE_MERGE: bool = false;

/// Only bake merged instance meshes when the source part has at most this many vertices.
const SHAPE_INSTANCE_MERGE_MAX_VERTS: usize = 256;

/// Minimum instances before merge is considered (ignored when [`ENABLE_SHAPE_INSTANCE_MERGE`] is false).
const SHAPE_INSTANCE_MERGE_MIN: usize = 12;

/// Set to `false` to skip TrackObj placeholder cuboids (tiny rail clutter, very numerous).
const SPAWN_TRACKOBJ_PLACEHOLDERS: bool = false;
/// Procedural sleepers/rails for TrackObj without a resolvable `.s` mesh (Open Rails TSection fallback).
const SPAWN_TRACKOBJ_PROCEDURAL: bool = true;

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
    pub position: Vec3,
    pub rotation: Quat,
    /// Non-uniform scale from `.w` `Matrix3x3` when present.
    pub scale: Vec3,
    pub tile_x: i32,
    pub tile_z: i32,
    pub forest: Option<ForestPatch>,
    pub water: Option<WaterPatch>,
    /// TDB `TrItemId`s when this object references track items (Signal, Speedpost, …).
    pub tr_item_ids: Vec<u32>,
}

/// All world objects discovered under a route's `WORLD/` (or `world/`) folder.
#[derive(Resource, Clone, Default)]
pub struct WorldScene {
    pub tiles_loaded: usize,
    pub items: Vec<WorldObject>,
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

    /// Convert `.w` tile-local Y to terrain MSL for [`Self::to_render_surface`].
    pub fn scenery_y_to_msl(&self, scenery_y: f32) -> f32 {
        self.height_origin + (scenery_y - self.center.y)
    }

    /// MSTS world position from a `.w` item (scenery-local Y) → Bevy render space.
    pub fn scenery_to_render(&self, scenery_world: Vec3) -> Vec3 {
        self.to_render_surface(Vec3::new(
            scenery_world.x,
            self.scenery_y_to_msl(scenery_world.y),
            scenery_world.z,
        ))
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
        let local = self.to_render(world);
        Vec2::new(local.x, local.z).length()
    }
}

/// Whether a world object should be culled for being outside [`VISIBLE_RADIUS_M`].
#[inline]
pub fn should_cull_world_object(focus: &RouteFocus, world: Vec3) -> bool {
    focus.horizontal_distance(world) > visible_radius_m()
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

/// MSTS `Matrix3x3` → Bevy rotation.
pub fn matrix3x3_to_quat(m: &[f64; 9]) -> Quat {
    matrix3x3_to_rotation_scale(m).0
}

// `qdir_to_quat` and `matrix3x3_to_rotation_scale` are imported from `crate::coordinates`.

fn world_item_transform(item: &WorldItem) -> (Quat, Vec3) {
    if let Some(m) = item.matrix3x3() {
        return matrix3x3_to_rotation_scale(&m);
    }
    let rot = item
        .qdirection()
        .map(|q| qdir_to_quat(&q))
        .unwrap_or(Quat::IDENTITY);
    (rot, Vec3::ONE)
}

fn object_label(item: &WorldItem) -> String {
    item.file_name()
        .map(str::to_string)
        .unwrap_or_else(|| item.kind().to_string())
}

fn object_from_item(tile_x: i32, tile_z: i32, item: &WorldItem) -> Option<WorldObject> {
    let position = msts_to_bevy(tile_x, tile_z, item.position()?);
    let (rotation, scale) = world_item_transform(item);
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
    Some(WorldObject {
        kind: item.kind(),
        uid: item.uid(),
        label: object_label(item),
        shape_file: item.file_name().map(str::to_string),
        section_idx: item.section_idx(),
        position,
        rotation,
        scale,
        tile_x,
        tile_z,
        forest,
        water,
        tr_item_ids: item.tr_item_ids(),
    })
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
    let name = world_w_filename_from_tile_xz(tile_x, tile_z);
    for subdir in ["WORLD", "world"] {
        let path = route_dir.join(subdir).join(&name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
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
pub fn load_world_from_route_dir_near(
    route_dir: &Path,
    center: Option<Vec3>,
    radius_m: f32,
) -> WorldScene {
    let mut entries = discover_world_tile_entries(route_dir, center, radius_m);
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut scene = WorldScene::default();
    let mut skip_count = 0usize;
    let mut skip_sample: Option<String> = None;
    for (_display_x, _display_z, path) in entries {
        if append_world_tile_file(&mut scene, &path).is_err() {
            skip_count += 1;
            if skip_sample.is_none() {
                skip_sample = Some(path.display().to_string());
            }
        }
    }
    if let Some(c) = center.filter(|_| radius_m.is_finite() && radius_m < f32::MAX / 2.0) {
        viewer_log!(
            "openrailsrs-viewer3d: loaded {} world tile(s) ({} item(s)) within {:.0}m of ({:.0},{:.0})",
            scene.tiles_loaded,
            scene.items.len(),
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

fn append_world_tile_file(scene: &mut WorldScene, path: &Path) -> Result<(), String> {
    let world = WorldFile::from_path(path).map_err(|e| e.to_string())?;
    scene.tiles_loaded += 1;
    for item in &world.items {
        if let Some(obj) = object_from_item(world.tile_x, world.tile_z, item) {
            scene.items.push(obj);
        }
    }
    Ok(())
}

fn discover_world_files(route_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for subdir in ["WORLD", "world"] {
        let dir = route_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("w"))
            {
                out.push(path);
            }
        }
    }
    out
}

fn kind_color(kind: &str) -> Color {
    match kind {
        "Static" => Color::srgb(0.6, 0.65, 0.75),
        "Forest" => Color::srgb(0.22, 0.72, 0.28),
        "TrackObj" => Color::srgb(0.78, 0.48, 0.18),
        "Signal" => Color::srgb(1.0, 0.85, 0.2),
        "Dyntrack" => Color::srgb(0.58, 0.32, 0.82),
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

fn shape_eligible(obj: &WorldObject) -> bool {
    trackobj_effective_shape_file(obj).is_some_and(|f| f.to_ascii_lowercase().ends_with(".s"))
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
);

#[derive(Resource)]
pub struct WorldSpawnProgress {
    phase: WorldSpawnPhase,
    started: Instant,
    item_index: usize,
    shape_path_cache: std::collections::HashMap<String, Option<PathBuf>>,
    shape_instances: std::collections::HashMap<PathBuf, Vec<Transform>>,
    shape_instance_min_dist: std::collections::HashMap<PathBuf, f32>,
    merged_boxes: std::collections::HashMap<String, MergedBoxGroup>,
    culled_count: usize,
    skipped_trackobj_placeholders: usize,
    trackobj_seen: usize,
    trackobj_mesh_queued: usize,
    trackobj_no_filename: usize,
    trackobj_unresolved: usize,
    trackobj_procedural: Vec<crate::dyntrack::ProceduralTrackSegment>,
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
    texture_image_cache: std::collections::HashMap<PathBuf, Handle<Image>>,
    asset_build_index: usize,
    instance_paths: Vec<PathBuf>,
    build_queue_index: usize,
    spawn_queue: Vec<ShapeSpawnBundle>,
    spawn_index: usize,
    shape_mesh_count: usize,
    shape_texture_count: usize,
    merged_shape_groups: usize,
    loading_shapes_started: Option<Instant>,
    build_queue_started: Option<Instant>,
    scenery_audit: Option<crate::scenery_audit::ShapeAuditSummary>,
    paused_for_driver_log: bool,
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
            shape_instance_min_dist: std::collections::HashMap::new(),
            merged_boxes: std::collections::HashMap::new(),
            culled_count: 0,
            skipped_trackobj_placeholders: 0,
            trackobj_seen: 0,
            trackobj_mesh_queued: 0,
            trackobj_no_filename: 0,
            trackobj_unresolved: 0,
            trackobj_procedural: Vec::new(),
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
            spawn_index: 0,
            shape_mesh_count: 0,
            shape_texture_count: 0,
            merged_shape_groups: 0,
            loading_shapes_started: None,
            build_queue_started: None,
            scenery_audit: None,
            paused_for_driver_log: false,
        }
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
    commands.insert_resource(WorldSpawnProgress::new(placeholder_base));
}

fn classify_one_object(
    obj: &WorldObject,
    focus: &RouteFocus,
    assets: &RouteAssets,
    mode: ViewerSceneryMode,
    progress: &mut WorldSpawnProgress,
) {
    if obj.kind == "Dyntrack" || obj.kind == "Forest" || obj.kind == "HWater" {
        return;
    }
    if should_cull_world_object(focus, obj.position) {
        progress.culled_count += 1;
        return;
    }

    let dist = focus.horizontal_distance(obj.position);

    if !mode.loads_msts_scenery() {
        return;
    }

    if obj.kind == "TrackObj" && dist <= shape_mesh_radius_m() {
        progress.trackobj_seen += 1;
    }

    if shape_eligible(obj) && dist <= shape_mesh_radius_m() {
        let cache_key = obj
            .shape_file
            .clone()
            .or_else(|| {
                obj.section_idx
                    .and_then(|idx| assets.tsection().shape_file_name(idx).map(str::to_string))
            })
            .unwrap_or_default();
        let shape_path = progress
            .shape_path_cache
            .entry(cache_key)
            .or_insert_with(|| resolve_object_shape_path(obj, assets))
            .clone();
        if let Some(shape_path) = shape_path {
            if obj.kind == "TrackObj" {
                progress.trackobj_mesh_queued += 1;
            }
            let render_pos = focus.scenery_to_render(obj.position);
            let tf = Transform {
                translation: render_pos,
                rotation: obj.rotation,
                scale: obj.scale,
            };
            progress
                .shape_instances
                .entry(shape_path.clone())
                .or_default()
                .push(tf);
            progress
                .shape_instance_min_dist
                .entry(shape_path)
                .and_modify(|d| *d = d.min(dist))
                .or_insert(dist);
            return;
        }
        if obj.kind == "TrackObj" {
            progress.trackobj_unresolved += 1;
            if progress.trackobj_unresolved <= 8 {
                let name = obj
                    .shape_file
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        obj.section_idx
                            .and_then(|idx| assets.tsection().shape_file_name(idx))
                    })
                    .unwrap_or("<sin nombre>");
                viewer_log!(
                    "openrailsrs-viewer3d: TrackObj sin .s resuelto: {name} (section={:?})",
                    obj.section_idx
                );
            }
        }
    } else if obj.kind == "TrackObj" && dist <= shape_mesh_radius_m() {
        progress.trackobj_no_filename += 1;
    }

    if obj.kind == "TrackObj" && !SPAWN_TRACKOBJ_PLACEHOLDERS {
        if SPAWN_TRACKOBJ_PROCEDURAL && dist <= shape_mesh_radius_m() {
            let render_pos = focus.scenery_to_render(obj.position);
            progress
                .trackobj_procedural
                .extend(trackobj_procedural_segments(obj, render_pos, assets, mode));
        } else {
            progress.skipped_trackobj_placeholders += 1;
        }
        return;
    }
    if dist > shape_mesh_radius_m() {
        return;
    }

    let size = box_size_for_kind(obj.kind, progress.placeholder_base);
    let translation = focus.scenery_to_render(Vec3::new(
        obj.position.x,
        obj.position.y + size.y * 0.5,
        obj.position.z,
    ));
    let tf = Transform {
        translation,
        rotation: obj.rotation,
        scale: size,
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

#[allow(clippy::too_many_arguments)]
fn append_shape_spawn_entries_for_transforms(
    shape_path: &Path,
    asset: &ShapeRenderAsset,
    meshes: &mut Assets<Mesh>,
    transforms: &[Transform],
    spawn_queue: &mut Vec<ShapeSpawnBundle>,
    initial_lod_idx: usize,
    shape_mesh_count: &mut usize,
    shape_texture_count: &mut usize,
    merged_shape_groups: &mut usize,
    origin: &FloatingOrigin,
) {
    if asset.has_texture {
        *shape_texture_count += transforms.len();
    }
    let mergeable = ENABLE_SHAPE_INSTANCE_MERGE
        && transforms.len() >= SHAPE_INSTANCE_MERGE_MIN
        && asset.parts.iter().all(|part| {
            !part.is_transparent
                && meshes
                    .get(&part.mesh)
                    .map(|mesh| mesh.count_vertices() <= SHAPE_INSTANCE_MERGE_MAX_VERTS)
                    .unwrap_or(false)
        });

    if mergeable {
        *merged_shape_groups += 1;
        *shape_mesh_count += asset.parts.len();
        let view_transforms: Vec<Transform> = transforms
            .iter()
            .map(|tf| view_transform(*tf, origin))
            .collect();
        for part in &asset.parts {
            if let Some(merged) = build_merged_instance_mesh(meshes, &part.mesh, &view_transforms) {
                spawn_queue.push((
                    Transform::IDENTITY,
                    Mesh3d(meshes.add(merged)),
                    MeshMaterial3d(part.material.clone()),
                    Name::new("world:merged"),
                    WorldSceneryLod {
                        enabled: false,
                        shape_path: PathBuf::new(),
                        prim_state_idx: -1,
                        part_index: 0,
                        lod_idx: 0,
                    },
                ));
            }
        }
    } else {
        *shape_mesh_count += asset.parts.len() * transforms.len();
        for tf in transforms {
            let tf = view_transform(*tf, origin);
            for (part_index, part) in asset.parts.iter().enumerate() {
                spawn_queue.push((
                    tf,
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(part.material.clone()),
                    Name::new("world:mesh"),
                    WorldSceneryLod {
                        enabled: true,
                        shape_path: shape_path.to_path_buf(),
                        prim_state_idx: part.prim_state_idx,
                        part_index,
                        lod_idx: initial_lod_idx,
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
    origin: &FloatingOrigin,
) {
    let Some(transforms) = progress.shape_instances.get(shape_path).cloned() else {
        return;
    };
    let initial_lod_idx = progress
        .parsed_shape_files
        .get(shape_path)
        .and_then(|shape| {
            progress
                .shape_instance_min_dist
                .get(shape_path)
                .copied()
                .map(|d| lod_level_index_for_distance(shape, d))
        })
        .unwrap_or(0);
    append_shape_spawn_entries_for_transforms(
        shape_path,
        asset,
        meshes,
        &transforms,
        &mut progress.spawn_queue,
        initial_lod_idx,
        &mut progress.shape_mesh_count,
        &mut progress.shape_texture_count,
        &mut progress.merged_shape_groups,
        origin,
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
    texture_image_cache: &mut std::collections::HashMap<PathBuf, Handle<Image>>,
    ace_cache: &std::collections::HashMap<PathBuf, openrailsrs_ace::AceFile>,
    fallback_color: Color,
    fallback_material: &Handle<StandardMaterial>,
) -> (PathBuf, ShapeRenderAsset) {
    let tex_dirs = texture_search_dirs_for_shape(&shape_path, route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
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
            }
        }
    };
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
    texture_image_cache: &mut std::collections::HashMap<PathBuf, Handle<Image>>,
    ace_cache: &std::collections::HashMap<PathBuf, openrailsrs_ace::AceFile>,
    fallback_color: Color,
    _fallback_material: &Handle<StandardMaterial>,
) -> Vec<ShapeRenderAsset> {
    let Some(control) = shape.lod_controls.first() else {
        return Vec::new();
    };
    if control.distance_levels.len() <= 1 {
        return Vec::new();
    }
    let tex_dirs = texture_search_dirs_for_shape(shape_path, route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    control
        .distance_levels
        .iter()
        .filter_map(|level| {
            let mesh = build_mesh_from_shape_lod(shape, level)?;
            let parts = build_mesh_parts_from_shape_lod(shape, level);
            let loaded = openrailsrs_bevy_scenery::shapes::LoadedShape {
                mesh,
                texture_file: primary_texture_filename(shape),
                parts,
            };
            Some(shape_render_asset_from_loaded_with_ace_cache(
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
            ))
        })
        .collect()
}

fn prepare_shape_load_paths(progress: &mut WorldSpawnProgress) {
    if !progress.shape_load_paths.is_empty() {
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
    let end =
        (progress.shape_parse_index + SHAPE_PARSE_PER_FRAME).min(progress.shape_load_paths.len());
    let batch: Vec<PathBuf> = progress.shape_load_paths[progress.shape_parse_index..end].to_vec();
    let parsed: Vec<(PathBuf, Option<crate::shapes::LoadedShape>)> = batch
        .par_iter()
        .map(|path| {
            let lod_dist = progress.shape_instance_min_dist.get(path).copied();
            (path.clone(), load_shape_from_path(path, lod_dist))
        })
        .collect();
    for path in &batch {
        if let Ok(shape) = ShapeFile::from_path(path) {
            progress.parsed_shape_files.insert(path.clone(), shape);
        }
    }
    for (shape_path, loaded) in &parsed {
        if let Some(loaded) = loaded {
            let tex_dirs = texture_search_dirs_for_shape(shape_path, route_dir);
            let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
            progress
                .texture_paths
                .extend(collect_loaded_shape_texture_paths(loaded, &tex_refs));
        }
    }
    progress.parsed_shapes.extend(parsed);
    progress.shape_parse_index = end;
    if progress.shape_parse_index >= progress.shape_load_paths.len() {
        progress.texture_paths.sort_unstable();
        progress.texture_paths.dedup();
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
    progress.ace_cache.extend(prefetch_ace_textures(&batch));
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
    viewer_log!(
        "openrailsrs-viewer3d: parsed {} shape(s), prefetched {} texture(s)",
        progress.parsed_shapes.len(),
        progress.ace_cache.len()
    );
    if progress.scenery_audit.is_none() && crate::scenery_audit::scenery_audit_enabled() {
        progress.scenery_audit = Some(crate::scenery_audit::audit_parsed_shapes(
            &progress.parsed_shapes,
            route_dir,
        ));
    }
}

fn log_world_spawn_summary(progress: &WorldSpawnProgress) {
    if progress.culled_count > 0 {
        let radius_m = visible_radius_m();
        viewer_log!(
            "openrailsrs-viewer3d: {culled} world object(s) culled (>{radius_m:.0}m from centre)",
            culled = progress.culled_count
        );
    }
    if progress.skipped_trackobj_placeholders > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: TrackObj sin mesh visible: {} omitido(s) (sin placeholder)",
            progress.skipped_trackobj_placeholders
        );
    }
    if progress.trackobj_seen > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: TrackObj <={}m: {} con mesh, {} sin FileName, {} .s no resuelto",
            shape_mesh_radius_m() as u32,
            progress.trackobj_mesh_queued,
            progress.trackobj_no_filename,
            progress.trackobj_unresolved
        );
    }
    if !progress.trackobj_procedural.is_empty() {
        viewer_log!(
            "openrailsrs-viewer3d: {} TrackObj procedural segment(s) (tsection/tdb)",
            progress.trackobj_procedural.len()
        );
    }
    if progress.skipped_trackobj_placeholders > 0 && progress.trackobj_seen == 0 {
        viewer_log!(
            "openrailsrs-viewer3d: skipped {} TrackObj placeholder(s) (no .s mesh)",
            progress.skipped_trackobj_placeholders
        );
    }
    if progress.merged_shape_groups > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: merged {} repeated shape(s) (≥{SHAPE_INSTANCE_MERGE_MIN} instances)",
            progress.merged_shape_groups
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
}

/// Index of every `.w` tile on disk; loads additional tiles as the camera moves (OR `SceneryDrawer`).
#[derive(Resource, Default)]
pub struct WorldTileStream {
    catalog: std::collections::HashMap<(i32, i32), PathBuf>,
    loaded: std::collections::HashSet<(i32, i32)>,
    route_dir: PathBuf,
    radius_m: f32,
    last_camera_tile: Option<(i32, i32)>,
}

impl WorldTileStream {
    pub fn new(route_dir: &Path, world: &WorldScene, radius_m: f32) -> Self {
        let catalog = discover_world_files(route_dir)
            .into_iter()
            .filter_map(|path| parse_world_w_tile_xz(&path).map(|xz| (xz, path)))
            .collect();
        let mut loaded = std::collections::HashSet::new();
        for obj in &world.items {
            loaded.insert((obj.tile_x, obj.tile_z));
        }
        Self {
            catalog,
            loaded,
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

/// Tracks how many world items already have forest / water / dyntrack GPU spawns.
#[derive(Resource, Default)]
pub struct WorldSceneryStreamState {
    pub processed_items: usize,
}

/// After startup (or tile stream), spawn forest / water / dyntrack for newly loaded items.
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
    track: Res<TrackScene>,
    terrain: Option<Res<TerrainElevation>>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    offset: Res<RouteWorldOffset>,
) {
    if progress.is_some() {
        return;
    }
    if world.items.len() <= state.processed_items {
        return;
    }
    let new_items = &world.items[state.processed_items..];
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
    let dyntracks = new_items
        .iter()
        .filter(|obj| obj.kind == "Dyntrack")
        .count();
    if forests > 0 {
        crate::forest::spawn_forest_objects(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut materials,
            new_items,
            &track,
            terrain.as_deref(),
            &assets,
            &focus,
            &offset,
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
    if dyntracks > 0 {
        crate::dyntrack::spawn_dyntrack_objects(
            &mut commands,
            &mut meshes,
            &mut materials,
            new_items,
            &track,
            &focus,
        );
    }
    if forests + waters + dyntracks > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: streamed scenery — {forests} forest, {waters} water, {dyntracks} dyntrack"
        );
    }
    state.processed_items = world.items.len();
}

/// Load `.w` tiles around the view window (train in live, camera in replay).
#[allow(clippy::too_many_arguments)]
pub fn world_tile_stream_system(
    mut world: ResMut<WorldScene>,
    mut stream: ResMut<WorldTileStream>,
    focus: Res<RouteFocus>,
    window: Res<crate::view_window::ViewWindow>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    scene: Res<TrackScene>,
    camera: Query<&Transform, With<Camera3d>>,
    progress: Option<Res<WorldSpawnProgress>>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    mut commands: Commands,
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
    let tile = MSTS_TILE_SIZE_M as f32;
    let cam_tile_x = msts_tile_x_index_for_coord(center.x);
    let cam_tile_z = msts_tile_z_index_for_coord(center.z);
    if stream.last_camera_tile == Some((cam_tile_x, cam_tile_z)) && !opts.live {
        return;
    }
    stream.last_camera_tile = Some((cam_tile_x, cam_tile_z));

    let radius_tiles = (stream.radius_m / tile).ceil() as i32 + 1;
    let item_base = world.items.len();
    let mut tiles_loaded = 0usize;

    for dtx in -radius_tiles..=radius_tiles {
        for dtz in -radius_tiles..=radius_tiles {
            let tile_x = cam_tile_x + dtx;
            let tile_z = cam_tile_z + dtz;
            if tile_center_distance_m(tile_x, tile_z, center) > stream.radius_m + tile {
                continue;
            }
            let key = (tile_x, tile_z);
            if stream.loaded.contains(&key) {
                continue;
            }
            let Some(path) = stream
                .catalog
                .get(&key)
                .cloned()
                .or_else(|| world_tile_path_for_coords(&stream.route_dir, tile_x, tile_z))
            else {
                continue;
            };
            if append_world_tile_file(&mut world, &path).is_ok() {
                stream.loaded.insert(key);
                tiles_loaded += 1;
            }
        }
    }

    let new_items = world.items.len().saturating_sub(item_base);
    if tiles_loaded > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: streamed {tiles_loaded} world tile(s) ({new_items} item(s)) near tile ({cam_tile_x},{cam_tile_z})"
        );
        let placeholder_base = scene.bounds.edge_radius().max(2.0) * 1.5;
        commands.insert_resource(WorldSpawnProgress::new_from_item_index(
            placeholder_base,
            item_base,
        ));
    }
}

/// Unload distant world tiles and despawn scenery meshes in live mode.
#[allow(clippy::too_many_arguments)]
pub fn world_tile_unload_system(
    mut world: ResMut<WorldScene>,
    mut stream: ResMut<WorldTileStream>,
    window: Res<crate::view_window::ViewWindow>,
    opts: Res<crate::launch::ViewerLaunchOpts>,
    focus: Res<RouteFocus>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mode: Res<ViewerSceneryMode>,
    mut commands: Commands,
    scenery: Query<(Entity, &Transform), With<WorldSceneryLod>>,
    mut stream_state: ResMut<WorldSceneryStreamState>,
) {
    if !opts.live || !mode.loads_msts_scenery() || mode.is_tile_lab() {
        return;
    }
    let tile = MSTS_TILE_SIZE_M as f32;
    let unload_radius = window.radius_m + tile + 64.0;
    let center = window.center_world;

    let mut unloaded_tiles = HashSet::new();
    stream.loaded.retain(|key| {
        let keep = tile_center_distance_m(key.0, key.1, center) <= unload_radius;
        if !keep {
            unloaded_tiles.insert(*key);
        }
        keep
    });
    if unloaded_tiles.is_empty() {
        return;
    }
    let before = world.items.len();
    world
        .items
        .retain(|obj| !unloaded_tiles.contains(&(obj.tile_x, obj.tile_z)));
    stream_state.processed_items = stream_state.processed_items.min(world.items.len());

    for (entity, tf) in scenery.iter() {
        let msts_x = tf.translation.x + focus.center.x + origin.shift.x;
        let msts_z = tf.translation.z + focus.center.z + origin.shift.z;
        let dist = Vec2::new(msts_x - center.x, msts_z - center.z).length();
        if dist > unload_radius {
            commands.entity(entity).despawn();
        }
    }
    viewer_log!(
        "openrailsrs-viewer3d: unloaded {} world tile(s) ({} → {} items)",
        unloaded_tiles.len(),
        before,
        world.items.len()
    );
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
    _scene: Res<TrackScene>,
    focus: Res<RouteFocus>,
    origin: Res<FloatingOrigin>,
    assets: Res<RouteAssets>,
    mode: Res<ViewerSceneryMode>,
    progress: Option<ResMut<WorldSpawnProgress>>,
) {
    let Some(mut progress) = progress else {
        return;
    };

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
            let end = (progress.item_index + classify_batch).min(world.items.len());
            for obj in &world.items[progress.item_index..end] {
                classify_one_object(obj, &focus, &assets, *mode, &mut progress);
            }
            progress.item_index = end;
            if progress.item_index >= world.items.len() {
                viewer_log!(
                    "openrailsrs-viewer3d: classified {} visible world item(s)",
                    world.items.len().saturating_sub(progress.culled_count)
                );
                if progress.trackobj_seen > 0 {
                    viewer_log!(
                        "openrailsrs-viewer3d: TrackObj <={}m: {} con mesh, {} sin FileName, {} .s no resuelto",
                        shape_mesh_radius_m() as u32,
                        progress.trackobj_mesh_queued,
                        progress.trackobj_no_filename,
                        progress.trackobj_unresolved
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
                    &origin,
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
            if progress.spawn_index == 0 && !progress.spawn_queue.is_empty() {
                viewer_log!(
                    "openrailsrs-viewer3d: spawning {} world mesh entit(ies) progressively",
                    progress.spawn_queue.len()
                );
            }
            let end = (progress.spawn_index + spawn_batch).min(progress.spawn_queue.len());
            let batch: Vec<ShapeSpawnBundle> =
                progress.spawn_queue[progress.spawn_index..end].to_vec();
            commands.spawn_batch(batch);
            progress.spawn_index = end;
            if progress.spawn_index >= progress.spawn_queue.len() {
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
            log_world_spawn_summary(&progress);
            commands.insert_resource(WorldShapeLodCache {
                shapes: std::mem::take(&mut progress.parsed_shape_files),
                assets_by_lod: std::mem::take(&mut progress.shape_lod_assets),
            });
            commands.remove_resource::<WorldSpawnProgress>();
        }
    }
}

/// Swap world shape meshes when the camera crosses MSTS `dlevel_selection` thresholds.
pub fn update_world_scenery_lod(
    cache: Option<Res<WorldShapeLodCache>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    focus: Option<Res<RouteFocus>>,
    mut parts: Query<(&GlobalTransform, &mut WorldSceneryLod, &mut Mesh3d)>,
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
    let cam_dist = cam_pos.distance(focus_pos);

    for (gt, mut lod, mut mesh3d) in &mut parts {
        if !lod.enabled {
            continue;
        }
        let Some(shape) = cache.shapes.get(&lod.shape_path) else {
            continue;
        };
        let Some(lod_assets) = cache.assets_by_lod.get(&lod.shape_path) else {
            continue;
        };
        if lod_assets.is_empty() {
            continue;
        }
        let instance_dist = cam_dist + gt.translation().distance(focus_pos);
        let new_lod = lod_level_index_for_distance(shape, instance_dist).min(lod_assets.len() - 1);
        if new_lod == lod.lod_idx {
            continue;
        }
        let Some(asset) = lod_assets.get(new_lod) else {
            continue;
        };
        let Some(part) = asset.parts.get(lod.part_index) else {
            continue;
        };
        mesh3d.0 = part.mesh.clone();
        lod.lod_idx = new_lod;
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
    let mut texture_image_cache: std::collections::HashMap<PathBuf, Handle<Image>> =
        std::collections::HashMap::new();
    let mut shape_instances: std::collections::HashMap<PathBuf, Vec<Transform>> =
        std::collections::HashMap::new();
    let mut trackobj_procedural: Vec<crate::dyntrack::ProceduralTrackSegment> = Vec::new();

    let mut merged_boxes: std::collections::HashMap<&str, MergedBoxGroup> =
        std::collections::HashMap::new();

    let shape_fallback_color = Color::srgb(0.72, 0.55, 0.42);
    let shape_fallback_material = add_shape_fallback_material(&mut materials, shape_fallback_color);

    let mut shape_mesh_count = 0usize;
    let mut shape_texture_count = 0usize;
    let mut culled_count = 0usize;
    let mut skipped_trackobj_placeholders = 0usize;
    let mut shape_spawn_batches: Vec<ShapeSpawnBundle> = Vec::new();
    let mut merged_shape_groups = 0usize;

    for obj in &world.items {
        if obj.kind == "Dyntrack" || obj.kind == "Forest" || obj.kind == "HWater" {
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

        if shape_eligible(obj) && dist <= shape_mesh_radius_m() {
            let shape_path = resolve_object_shape_path(obj, &assets);
            if let Some(shape_path) = shape_path {
                let render_pos = focus.scenery_to_render(obj.position);
                shape_instances
                    .entry(shape_path)
                    .or_default()
                    .push(Transform {
                        translation: render_pos,
                        rotation: obj.rotation,
                        scale: obj.scale,
                    });
                continue;
            }
        }

        if obj.kind == "TrackObj" && !SPAWN_TRACKOBJ_PLACEHOLDERS {
            if SPAWN_TRACKOBJ_PROCEDURAL && dist <= shape_mesh_radius_m() {
                trackobj_procedural.extend(trackobj_procedural_segments(
                    obj,
                    focus.scenery_to_render(obj.position),
                    &assets,
                    *mode,
                ));
            } else {
                skipped_trackobj_placeholders += 1;
            }
            continue;
        }

        // Placeholders only near the camera; 4–8 km clutter dominated spawn time on large routes.
        if dist > shape_mesh_radius_m() {
            continue;
        }

        let size = box_size_for_kind(obj.kind, base);
        let translation = focus.scenery_to_render(Vec3::new(
            obj.position.x,
            obj.position.y + size.y * 0.5,
            obj.position.z,
        ));
        let tf = Transform {
            translation,
            rotation: obj.rotation,
            scale: size,
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
    let parsed_shapes: Vec<(PathBuf, Option<crate::shapes::LoadedShape>)> = unique_shape_paths
        .par_iter()
        .map(|path| (path.clone(), load_shape_from_path(path, None)))
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
        if let Some(loaded) = loaded {
            let tex_dirs = texture_search_dirs_for_shape(shape_path, &assets.route_dir);
            let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
            texture_paths.extend(collect_loaded_shape_texture_paths(loaded, &tex_refs));
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
    for (shape_path, loaded) in parsed_shapes {
        let (shape_path, asset) = build_world_shape_asset(
            shape_path,
            loaded,
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

    for (shape_path, transforms) in shape_instances {
        let Some(asset) = shape_cache.get(&shape_path) else {
            continue;
        };
        append_shape_spawn_entries_for_transforms(
            &shape_path,
            asset,
            &mut meshes,
            &transforms,
            &mut shape_spawn_batches,
            0,
            &mut shape_mesh_count,
            &mut shape_texture_count,
            &mut merged_shape_groups,
            &origin,
        );
    }

    if !shape_spawn_batches.is_empty() {
        commands.spawn_batch(shape_spawn_batches);
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
    if skipped_trackobj_placeholders > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: skipped {skipped_trackobj_placeholders} TrackObj placeholder(s) (no .s mesh)"
        );
    }
    if merged_shape_groups > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: merged {merged_shape_groups} repeated shape(s) (≥{SHAPE_INSTANCE_MERGE_MIN} instances)"
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
    fn load_fixture_world_from_route_dir() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        assert_eq!(scene.tiles_loaded, 1);
        assert_eq!(scene.items.len(), 6);
        assert!(scene.items.iter().any(|o| o.kind == "Static"));
        assert!(scene.items.iter().any(|o| o.kind == "Forest"));
        assert!(scene.items.iter().any(|o| o.kind == "HWater"));
    }

    /// Chiltern-like focus: MSTS bbox `y` is tile-local (~80 m) but terrain MSL is ~13 km.
    fn chiltern_like_focus() -> RouteFocus {
        RouteFocus {
            center: Vec3::new(12_494_846.0, 82.0, 30_600_240.0),
            height_origin: 13_184.0,
        }
    }

    #[test]
    fn default_visible_radius_is_400m() {
        assert_eq!(VISIBLE_RADIUS_M, 120.0);
        assert_eq!(shape_mesh_radius_m(), visible_radius_m());
    }

    #[test]
    fn route_focus_scenery_uses_bbox_y_not_msl() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(12_494_900.0, 55.0, 30_600_300.0);
        let local = focus.to_render(obj);
        assert!(
            local.y.abs() < 200.0,
            "scenery local y should be O(100 m), got {}",
            local.y
        );
        assert!((local.y - (55.0 - 82.0)).abs() < 1.0);
        assert!(
            local.x.abs() < 500.0 && local.z.abs() < 500.0,
            "horizontal rebasing failed: {:?}",
            local
        );
    }

    #[test]
    fn scenery_y_to_msl_maps_tile_local_to_height_origin() {
        let focus = chiltern_like_focus();
        assert!((focus.scenery_y_to_msl(55.0) - 13_157.0).abs() < 1.0);
        assert!((focus.to_render_surface(Vec3::new(0.0, 13_157.0, 0.0)).y - (-27.0)).abs() < 1.0);
    }

    #[test]
    fn route_focus_surface_uses_height_origin() {
        let focus = chiltern_like_focus();
        let rail = Vec3::new(focus.center.x, 13_190.0, focus.center.z);
        let local = focus.to_render_surface(rail);
        assert!(
            (local.y - 6.0).abs() < 1.0,
            "MSL rail height should be ~0 local, got {}",
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
        let wrongly_vertical = Vec3::new(focus.center.x, 13_190.0, focus.center.z);
        assert!(
            !should_cull_world_object(&focus, wrongly_vertical),
            "same xz as centre must not be culled despite MSL y"
        );
    }

    #[test]
    fn scenery_at_bbox_center_renders_on_terrain_plane() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(focus.center.x, focus.center.y, focus.center.z);
        let render = focus.scenery_to_render(obj);
        assert!(
            render.y.abs() < 1.0,
            "object at bbox centre Y should sit on render Y≈0, got {}",
            render.y
        );
    }

    #[test]
    fn using_height_origin_for_scenery_y_would_cull_everything() {
        let focus = chiltern_like_focus();
        let obj = Vec3::new(focus.center.x + 50.0, 55.0, focus.center.z);
        let buggy_y = obj.y - focus.height_origin;
        assert!(
            buggy_y.abs() > 10_000.0,
            "sanity: old bug shifted scenery y by ~-13 km"
        );
        assert!(!should_cull_world_object(&focus, obj));
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
}
