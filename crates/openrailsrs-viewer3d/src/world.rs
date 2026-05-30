//! MSTS world tiles (`.w`) as coloured placeholder boxes (order 5 / issue #8).

use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use openrailsrs_formats::{WorldFile, WorldItem};

use crate::shapes::{
    RouteAssets, ShapeRenderAsset, collect_loaded_shape_texture_paths, load_shape_from_path,
    prefetch_ace_textures, shape_render_asset_from_loaded_with_ace_cache,
    texture_search_dirs_for_shape,
};
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::{log_step, viewer_log};
use rayon::prelude::*;

/// MSTS / Open Rails world tile size (metres).
pub const MSTS_TILE_SIZE_M: f64 = 2048.0;

/// Maximum distance (m) from the route centre at which world objects are spawned.
/// Objects beyond this radius are skipped to keep draw call count manageable on
/// large imported routes.
pub const VISIBLE_RADIUS_M: f32 = 8000.0;

/// Shapes closer than this use the highest LOD; farther shapes use coarser LOD.
pub const SHAPE_LOD_DISTANCE_M: f32 = 2000.0;

/// Within this radius, spawn real `.s` meshes when the file resolves; beyond it, placeholders only.
pub const SHAPE_MESH_RADIUS_M: f32 = 4_000.0;

/// Merge repeated `.s` instances into one baked mesh (small meshes only; disabled — breaks visuals on Chiltern).
const ENABLE_SHAPE_INSTANCE_MERGE: bool = false;

/// Only bake merged instance meshes when the source part has at most this many vertices.
const SHAPE_INSTANCE_MERGE_MAX_VERTS: usize = 256;

/// Minimum instances before merge is considered (ignored when [`ENABLE_SHAPE_INSTANCE_MERGE`] is false).
const SHAPE_INSTANCE_MERGE_MIN: usize = 12;

/// Set to `false` to skip TrackObj placeholder cuboids (tiny rail clutter, very numerous).
const SPAWN_TRACKOBJ_PLACEHOLDERS: bool = false;

/// World items classified per frame during progressive spawn.
const CLASSIFY_ITEMS_PER_FRAME: usize = 12_000;

/// Shape assets converted to Bevy handles per frame.
const SHAPE_ASSETS_PER_FRAME: usize = 64;

/// Shape groups turned into spawn bundles per frame.
const BUILD_QUEUE_SHAPES_PER_FRAME: usize = 32;

/// Mesh entities spawned per frame.
const SPAWN_ENTITIES_PER_FRAME: usize = 600;

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
    pub label: String,
    /// Shape filename from the world item (`FileName`), if any.
    pub shape_file: Option<String>,
    pub position: Vec3,
    pub rotation: Quat,
    pub tile_x: i32,
    pub tile_z: i32,
    pub forest: Option<ForestPatch>,
    pub water: Option<WaterPatch>,
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

    /// Horizontal distance from route centre in render space (for culling).
    pub fn horizontal_distance(&self, world: Vec3) -> f32 {
        let local = self.to_render(world);
        Vec2::new(local.x, local.z).length()
    }
}

/// Whether a world object should be culled for being outside [`VISIBLE_RADIUS_M`].
#[inline]
pub fn should_cull_world_object(focus: &RouteFocus, world: Vec3) -> bool {
    focus.horizontal_distance(world) > VISIBLE_RADIUS_M
}

/// Translates abstract graph coordinates into MSTS world space when the two diverge.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RouteWorldOffset {
    pub delta: Vec3,
}

impl RouteWorldOffset {
    pub fn from_scene_and_world(scene: &TrackScene, world: &WorldScene) -> Self {
        let graph_center = scene.bounds.center;
        let Some(world_center) = world.position_center() else {
            return Self::default();
        };
        if (world_center - graph_center).length() <= 2_000.0 {
            return Self::default();
        }
        let delta = world_center - graph_center;
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
/// Global X/Z follow the usual MSTS rule: `tile * 2048 + local`.
/// MSTS `y` maps to Bevy `Y` (height).
pub fn msts_to_bevy(tile_x: i32, tile_z: i32, local: openrailsrs_formats::Vec3) -> Vec3 {
    Vec3::new(
        (tile_x as f64 * MSTS_TILE_SIZE_M + local.x) as f32,
        local.y as f32,
        (tile_z as f64 * MSTS_TILE_SIZE_M + local.z) as f32,
    )
}

/// Open Rails XNA quaternion from a `.w` `QDirection` (`Scenery.cs`).
fn qdir_to_xna_quat(qdir: &[f64; 4]) -> Quat {
    Quat::from_xyzw(
        qdir[0] as f32,
        qdir[1] as f32,
        -(qdir[2] as f32),
        qdir[3] as f32,
    )
}

/// Open Rails XNA 3×3 rotation from a `.w` `Matrix3x3`.
fn matrix3x3_to_xna_mat3(m: &[f64; 9]) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(m[0] as f32, m[3] as f32, -m[6] as f32),
        Vec3::new(m[1] as f32, m[4] as f32, -m[7] as f32),
        Vec3::new(-m[2] as f32, -m[5] as f32, m[8] as f32),
    )
}

const MSTS_Z_REFLECT: Mat3 = Mat3::from_diagonal(Vec3::new(1.0, 1.0, -1.0));

/// MSTS `QDirection` → Bevy rotation (terrain uses native +Z, not XNA −Z).
pub fn qdir_to_quat(qdir: &[f64; 4]) -> Quat {
    xna_rotation_to_bevy(qdir_to_xna_quat(qdir))
}

/// MSTS `Matrix3x3` → Bevy rotation.
pub fn matrix3x3_to_quat(m: &[f64; 9]) -> Quat {
    xna_rotation_to_bevy(Quat::from_mat3(&matrix3x3_to_xna_mat3(m)))
}

/// Map an Open Rails / XNA rotation into Bevy's native MSTS +Z world axes.
fn xna_rotation_to_bevy(rot_xna: Quat) -> Quat {
    let r = Mat3::from_quat(rot_xna);
    Quat::from_mat3(&(MSTS_Z_REFLECT * r * MSTS_Z_REFLECT))
}

fn world_item_rotation(item: &WorldItem) -> Quat {
    if let Some(m) = item.matrix3x3() {
        return matrix3x3_to_quat(&m);
    }
    item.qdirection()
        .map(|q| qdir_to_quat(&q))
        .unwrap_or(Quat::IDENTITY)
}

fn object_label(item: &WorldItem) -> String {
    item.file_name()
        .map(str::to_string)
        .unwrap_or_else(|| item.kind().to_string())
}

fn object_from_item(tile_x: i32, tile_z: i32, item: &WorldItem) -> Option<WorldObject> {
    let position = msts_to_bevy(tile_x, tile_z, item.position()?);
    let rotation = world_item_rotation(item);
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
        label: object_label(item),
        shape_file: item.file_name().map(str::to_string),
        position,
        rotation,
        tile_x,
        tile_z,
        forest,
        water,
    })
}

/// Scan `route_dir/WORLD` and `route_dir/world` for `.w` files and parse them.
pub fn load_world_from_route_dir(route_dir: &Path) -> WorldScene {
    let mut paths = discover_world_files(route_dir);
    paths.sort();

    let mut scene = WorldScene::default();
    let mut skip_count = 0usize;
    let mut skip_sample: Option<String> = None;
    for path in paths {
        match WorldFile::from_path(&path) {
            Ok(world) => {
                scene.tiles_loaded += 1;
                for item in &world.items {
                    if let Some(obj) = object_from_item(world.tile_x, world.tile_z, item) {
                        scene.items.push(obj);
                    }
                }
            }
            Err(err) => {
                skip_count += 1;
                if skip_sample.is_none() {
                    skip_sample = Some(format!("{} ({err})", path.display()));
                }
            }
        }
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
    obj.shape_file
        .as_ref()
        .is_some_and(|f| f.to_ascii_lowercase().ends_with(".s"))
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

type ShapeSpawnBundle = (Transform, Mesh3d, MeshMaterial3d<StandardMaterial>, Name);

#[derive(Resource)]
pub struct WorldSpawnProgress {
    phase: WorldSpawnPhase,
    started: Instant,
    item_index: usize,
    shape_path_cache: std::collections::HashMap<String, Option<PathBuf>>,
    shape_instances: std::collections::HashMap<PathBuf, Vec<Transform>>,
    merged_boxes: std::collections::HashMap<String, MergedBoxGroup>,
    culled_count: usize,
    skipped_trackobj_placeholders: usize,
    placeholder_base: f32,
    shape_fallback_color: Color,
    shape_fallback_material: Option<Handle<StandardMaterial>>,
    parsed_shapes: Vec<(PathBuf, Option<crate::shapes::LoadedShape>)>,
    ace_cache: std::collections::HashMap<PathBuf, openrailsrs_ace::AceFile>,
    shape_cache: std::collections::HashMap<PathBuf, ShapeRenderAsset>,
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
        Self {
            phase: WorldSpawnPhase::Classifying,
            started: Instant::now(),
            item_index: 0,
            shape_path_cache: std::collections::HashMap::new(),
            shape_instances: std::collections::HashMap::new(),
            merged_boxes: std::collections::HashMap::new(),
            culled_count: 0,
            skipped_trackobj_placeholders: 0,
            placeholder_base,
            shape_fallback_color: Color::srgb(0.95, 0.25, 0.85),
            shape_fallback_material: None,
            parsed_shapes: Vec::new(),
            ace_cache: std::collections::HashMap::new(),
            shape_cache: std::collections::HashMap::new(),
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
    mut commands: Commands,
) {
    if world.is_empty() {
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

    if shape_eligible(obj) && dist <= SHAPE_MESH_RADIUS_M {
        let file_name = obj.shape_file.as_deref().unwrap_or("");
        let shape_path = progress
            .shape_path_cache
            .entry(file_name.to_string())
            .or_insert_with(|| assets.resolve_shape(file_name))
            .clone();
        if let Some(shape_path) = shape_path {
            let render_pos =
                focus.to_render_surface(Vec3::new(obj.position.x, obj.position.y, obj.position.z));
            progress
                .shape_instances
                .entry(shape_path)
                .or_default()
                .push(Transform {
                    translation: render_pos,
                    rotation: obj.rotation,
                    scale: Vec3::ONE,
                });
            return;
        }
    }

    if obj.kind == "TrackObj" && !SPAWN_TRACKOBJ_PLACEHOLDERS {
        progress.skipped_trackobj_placeholders += 1;
        return;
    }
    if dist > SHAPE_MESH_RADIUS_M {
        return;
    }

    let size = box_size_for_kind(obj.kind, progress.placeholder_base);
    let translation = focus.to_render_surface(Vec3::new(
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

fn append_shape_spawn_entries(
    progress: &mut WorldSpawnProgress,
    shape_path: &Path,
    asset: &ShapeRenderAsset,
    meshes: &mut Assets<Mesh>,
) {
    let Some(transforms) = progress.shape_instances.get(shape_path) else {
        return;
    };
    if asset.has_texture {
        progress.shape_texture_count += transforms.len();
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
        progress.merged_shape_groups += 1;
        progress.shape_mesh_count += asset.parts.len();
        for part in &asset.parts {
            if let Some(merged) = build_merged_instance_mesh(meshes, &part.mesh, transforms) {
                progress.spawn_queue.push((
                    Transform::IDENTITY,
                    Mesh3d(meshes.add(merged)),
                    MeshMaterial3d(part.material.clone()),
                    Name::new("world:merged"),
                ));
            }
        }
    } else {
        progress.shape_mesh_count += asset.parts.len() * transforms.len();
        for tf in transforms {
            for part in &asset.parts {
                progress.spawn_queue.push((
                    *tf,
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(part.material.clone()),
                    Name::new("world:mesh"),
                ));
            }
        }
    }
}

fn log_world_spawn_summary(progress: &WorldSpawnProgress) {
    if progress.culled_count > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {culled} world object(s) culled (>{VISIBLE_RADIUS_M:.0}m from centre)",
            culled = progress.culled_count
        );
    }
    if progress.skipped_trackobj_placeholders > 0 {
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
    log_step("spawned world objects (progressive)", progress.started);
}

/// Continue progressive world spawn across frames so the window stays responsive.
#[allow(clippy::too_many_arguments)]
pub fn progressive_world_spawn_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    focus: Res<RouteFocus>,
    assets: Res<RouteAssets>,
    progress: Option<ResMut<WorldSpawnProgress>>,
) {
    let Some(mut progress) = progress else {
        return;
    };

    match progress.phase {
        WorldSpawnPhase::Classifying => {
            let end = (progress.item_index + CLASSIFY_ITEMS_PER_FRAME).min(world.items.len());
            for obj in &world.items[progress.item_index..end] {
                classify_one_object(obj, &focus, &assets, &mut progress);
            }
            progress.item_index = end;
            if progress.item_index >= world.items.len() {
                viewer_log!(
                    "openrailsrs-viewer3d: classified {} visible world item(s)",
                    world.items.len().saturating_sub(progress.culled_count)
                );
                progress.phase = WorldSpawnPhase::LoadingShapes;
                progress.loading_shapes_started = Some(Instant::now());
            }
        }
        WorldSpawnPhase::LoadingShapes => {
            if progress.parsed_shapes.is_empty() {
                let unique_shape_paths: Vec<PathBuf> =
                    progress.shape_instances.keys().cloned().collect();
                progress.parsed_shapes = unique_shape_paths
                    .par_iter()
                    .map(|path| (path.clone(), load_shape_from_path(path, None)))
                    .collect();
                let mut texture_paths = Vec::new();
                for (shape_path, loaded) in &progress.parsed_shapes {
                    if let Some(loaded) = loaded {
                        let tex_dirs = texture_search_dirs_for_shape(shape_path, &assets.route_dir);
                        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
                        texture_paths.extend(collect_loaded_shape_texture_paths(loaded, &tex_refs));
                    }
                }
                texture_paths.sort_unstable();
                texture_paths.dedup();
                progress.ace_cache = prefetch_ace_textures(&texture_paths);
                if progress.shape_fallback_material.is_none() {
                    let color = progress.shape_fallback_color;
                    progress.shape_fallback_material = Some(materials.add(StandardMaterial {
                        base_color: color,
                        emissive: LinearRgba::from(color) * 0.35,
                        perceptual_roughness: 0.75,
                        metallic: 0.1,
                        double_sided: true,
                        ..default()
                    }));
                }
                viewer_log!(
                    "openrailsrs-viewer3d: parsed {} shape(s), prefetched {} texture(s)",
                    progress.parsed_shapes.len(),
                    progress.ace_cache.len()
                );
            }

            let fallback_material = progress.shape_fallback_material.clone().unwrap();
            let fallback_color = progress.shape_fallback_color;
            let ace_cache = progress.ace_cache.clone();
            let end = (progress.asset_build_index + SHAPE_ASSETS_PER_FRAME)
                .min(progress.parsed_shapes.len());
            let batch: Vec<(PathBuf, Option<crate::shapes::LoadedShape>)> =
                progress.parsed_shapes[progress.asset_build_index..end].to_vec();
            for (shape_path, loaded) in batch {
                let tex_dirs = texture_search_dirs_for_shape(&shape_path, &assets.route_dir);
                let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
                let asset = match loaded {
                    Some(loaded) => shape_render_asset_from_loaded_with_ace_cache(
                        loaded,
                        &tex_refs,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                        &mut progress.texture_image_cache,
                        &ace_cache,
                        fallback_color,
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
                                mesh: unit,
                                material: fallback_material.clone(),
                                has_texture: false,
                                is_transparent: false,
                            }],
                            has_texture: false,
                        }
                    }
                };
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
            let end = (progress.build_queue_index + BUILD_QUEUE_SHAPES_PER_FRAME)
                .min(progress.instance_paths.len());
            let paths: Vec<PathBuf> =
                progress.instance_paths[progress.build_queue_index..end].to_vec();
            for shape_path in paths {
                let Some(asset) = progress.shape_cache.get(&shape_path).cloned() else {
                    continue;
                };
                append_shape_spawn_entries(&mut progress, &shape_path, &asset, &mut meshes);
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
            let end =
                (progress.spawn_index + SPAWN_ENTITIES_PER_FRAME).min(progress.spawn_queue.len());
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
                    Transform::IDENTITY,
                    Name::new(format!("world-boxes:{kind}")),
                ));
                viewer_log!("openrailsrs-viewer3d: merged {cuboid_count} {kind} placeholder(s)");
            }
            log_world_spawn_summary(&progress);
            commands.remove_resource::<WorldSpawnProgress>();
        }
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
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
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
    let mut shape_path_cache: std::collections::HashMap<String, Option<PathBuf>> =
        std::collections::HashMap::new();
    let mut texture_image_cache: std::collections::HashMap<PathBuf, Handle<Image>> =
        std::collections::HashMap::new();
    let mut shape_instances: std::collections::HashMap<PathBuf, Vec<Transform>> =
        std::collections::HashMap::new();

    let mut merged_boxes: std::collections::HashMap<&str, MergedBoxGroup> =
        std::collections::HashMap::new();

    let shape_fallback_color = Color::srgb(0.95, 0.25, 0.85);
    let shape_fallback_material = materials.add(StandardMaterial {
        base_color: shape_fallback_color,
        emissive: LinearRgba::from(shape_fallback_color) * 0.35,
        perceptual_roughness: 0.75,
        metallic: 0.1,
        double_sided: true,
        ..default()
    });

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

        if shape_eligible(obj) && dist <= SHAPE_MESH_RADIUS_M {
            let file_name = obj.shape_file.as_deref().unwrap_or("");
            let shape_path = shape_path_cache
                .entry(file_name.to_string())
                .or_insert_with(|| assets.resolve_shape(file_name))
                .clone();
            if let Some(shape_path) = shape_path {
                let render_pos = focus.to_render_surface(Vec3::new(
                    obj.position.x,
                    obj.position.y,
                    obj.position.z,
                ));
                shape_instances
                    .entry(shape_path)
                    .or_default()
                    .push(Transform {
                        translation: render_pos,
                        rotation: obj.rotation,
                        scale: Vec3::ONE,
                    });
                continue;
            }
        }

        if obj.kind == "TrackObj" && !SPAWN_TRACKOBJ_PLACEHOLDERS {
            skipped_trackobj_placeholders += 1;
            continue;
        }

        // Placeholders only near the camera; 4–8 km clutter dominated spawn time on large routes.
        if dist > SHAPE_MESH_RADIUS_M {
            continue;
        }

        let size = box_size_for_kind(obj.kind, base);
        let translation = focus.to_render_surface(Vec3::new(
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
        let tex_dirs = texture_search_dirs_for_shape(&shape_path, &assets.route_dir);
        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let asset = match loaded {
            Some(loaded) => shape_render_asset_from_loaded_with_ace_cache(
                loaded,
                &tex_refs,
                &mut meshes,
                &mut images,
                &mut materials,
                &mut texture_image_cache,
                &ace_cache,
                shape_fallback_color,
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
                        mesh: unit,
                        material: shape_fallback_material.clone(),
                        has_texture: false,
                        is_transparent: false,
                    }],
                    has_texture: false,
                }
            }
        };
        shape_cache.insert(shape_path, asset);
    }
    log_step("built world shape Bevy assets", asset_start);

    for (shape_path, transforms) in shape_instances {
        let Some(asset) = shape_cache.get(&shape_path) else {
            continue;
        };
        if asset.has_texture {
            shape_texture_count += transforms.len();
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
            merged_shape_groups += 1;
            shape_mesh_count += asset.parts.len();
            for part in &asset.parts {
                if let Some(merged) = build_merged_instance_mesh(&meshes, &part.mesh, &transforms) {
                    shape_spawn_batches.push((
                        Transform::IDENTITY,
                        Mesh3d(meshes.add(merged)),
                        MeshMaterial3d(part.material.clone()),
                        Name::new("world:merged"),
                    ));
                }
            }
        } else {
            shape_mesh_count += asset.parts.len() * transforms.len();
            for tf in transforms {
                for part in &asset.parts {
                    shape_spawn_batches.push((
                        tf,
                        Mesh3d(part.mesh.clone()),
                        MeshMaterial3d(part.material.clone()),
                        Name::new("world:mesh"),
                    ));
                }
            }
        }
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
            Transform::IDENTITY,
            Name::new(format!("world-boxes:{}", kind)),
        ));
        viewer_log!(
            "openrailsrs-viewer3d: merged {cuboid_count} {} placeholder(s)",
            kind
        );
    }

    if culled_count > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: {culled_count} world object(s) culled (>{VISIBLE_RADIUS_M:.0}m from centre)"
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
    fn qdir_y_90_matches_bevy_native_y_rotation() {
        // File stores (0, sin45, 0, cos45) — same numeric values OR uses in XNA.
        let q = qdir_to_quat(&[
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        ]);
        let expected = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        assert!((q.dot(expected).abs() - 1.0) < 1e-3 || (q.dot(-expected).abs() - 1.0) < 1e-3);
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
        assert_eq!(p, Vec3::new(100.0, 5.0, -3.0));
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
        assert_eq!(p, Vec3::new(4106.0, 0.0, 2068.0));
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
        let obj = Vec3::new(focus.center.x + 100.0, 55.0, focus.center.z + 80.0);
        assert!(
            !should_cull_world_object(&focus, obj),
            "object 130 m away horizontally must not be culled"
        );
        let wrongly_vertical = Vec3::new(focus.center.x, 13_190.0, focus.center.z);
        assert!(
            !should_cull_world_object(&focus, wrongly_vertical),
            "same xz as centre must not be culled despite MSL y"
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
        let focus_no_elev = RouteFocus::from_scene_world_and_elevation(&scene, &world, None);
        let focus = RouteFocus::from_scene_world_and_elevation(&scene, &world, Some(&elev));

        // With terrain, height_origin should come from the terrain sample, not scenery bbox Y.
        assert!(
            focus.height_origin.is_finite()
                && focus.height_origin >= 0.0
                && focus.height_origin < 500.0,
            "height_origin should be a terrain MSL value, got {}",
            focus.height_origin
        );
        // The terrain sample should differ from the bare scenery bbox fallback.
        assert!(
            (focus.height_origin - focus_no_elev.height_origin).abs() > 0.1,
            "with elevation, height_origin ({}) should differ from fallback ({})",
            focus.height_origin,
            focus_no_elev.height_origin
        );
    }
}
