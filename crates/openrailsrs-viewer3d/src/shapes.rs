//! MSTS ASCII `.s` shapes → Bevy meshes (order 6) + `.ace` textures (order 7).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_ace::{AceFile, read_ace};
use openrailsrs_formats::{DistanceLevel, ShapeFile, Vec3 as ShapeVec3};

use crate::viewer_log;

/// Route directory for resolving `SHAPES/` and `TEXTURES/` assets.
#[derive(Resource, Clone)]
pub struct RouteAssets {
    pub route_dir: PathBuf,
    shape_path_index: HashMap<String, PathBuf>,
}

impl RouteAssets {
    pub fn new(route_dir: impl Into<PathBuf>) -> Self {
        let route_dir = route_dir.into();
        let shape_path_index = build_shape_path_index(&shape_search_dirs(&route_dir));
        Self {
            route_dir,
            shape_path_index,
        }
    }

    /// Resolve a shape filename using a pre-built index (case-insensitive).
    pub fn resolve_shape(&self, file_name: &str) -> Option<PathBuf> {
        if file_name.is_empty() {
            return None;
        }
        self.shape_path_index
            .get(&file_name.to_ascii_lowercase())
            .cloned()
    }
}

/// Parsed shape geometry plus optional primary texture filename from the shape.
#[derive(Clone, Debug)]
pub struct LoadedShape {
    pub mesh: Mesh,
    pub texture_file: Option<String>,
    pub parts: Vec<LoadedShapePart>,
}

/// One mesh/material slice of a shape, grouped by `prim_state_idx`.
#[derive(Clone, Debug)]
pub struct LoadedShapePart {
    pub prim_state_idx: i32,
    pub mesh: Mesh,
    pub texture_file: Option<String>,
    pub shader_name: Option<String>,
}

/// Bevy asset handles for one renderable shape part.
#[derive(Clone, Debug)]
pub struct ShapePartAsset {
    pub prim_state_idx: i32,
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub has_texture: bool,
    pub is_transparent: bool,
}

/// Bevy asset handles for a shape, including a combined mesh for fitting/bounds.
#[derive(Clone, Debug)]
pub struct ShapeRenderAsset {
    pub combined_mesh: Handle<Mesh>,
    pub parts: Vec<ShapePartAsset>,
    pub has_texture: bool,
}

/// Map a shape point from MSTS local space to Bevy (Y up).
pub fn shape_point_to_bevy(v: ShapeVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// MSTS shape space: +X lateral, +Y up, +Z forward. Train consist local: +X forward.
pub fn msts_shape_to_train_rotation() -> Quat {
    Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
}

/// Axis-aligned bounds of mesh positions (metres, shape local space).
pub fn mesh_aabb(mesh: &Mesh) -> Option<(Vec3, Vec3)> {
    let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?;
    let slice = positions.as_float3()?;
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for pos in slice {
        let p = Vec3::from(*pos);
        min = min.min(p);
        max = max.max(p);
    }
    if min.x.is_finite() {
        Some((min, max))
    } else {
        None
    }
}

fn aabb_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}

/// Uniform scale so the shape's MSTS forward extent (or best fallback) matches `length_m`.
pub fn vehicle_shape_fit_scale(extent: Vec3, length_m: f32) -> f32 {
    let target = length_m.max(1.0);
    let forward = extent.z;
    if forward >= 0.1 {
        return target / forward;
    }
    // Paper-thin along +Z (profile facing forward): scale from the largest visible axis.
    let reference = extent.x.max(extent.y).max(0.01);
    target / reference
}

/// Local transform for a vehicle `.s` mesh: MSTS→train rotation, bbox scale, front at `offset_m`.
pub fn vehicle_shape_local_transform(mesh: &Mesh, offset_m: f32, length_m: f32) -> Transform {
    let rotation = msts_shape_to_train_rotation();
    let (min, max) = mesh_aabb(mesh).unwrap_or((Vec3::ZERO, Vec3::splat(0.01)));
    let extent = max - min;
    let center = (min + max) * 0.5;
    let scale_factor = vehicle_shape_fit_scale(extent, length_m);
    let scale = Vec3::splat(scale_factor);

    let front = Vec3::new(center.x, center.y, max.z);
    let front_local_x = (rotation * (scale * front)).x;

    let min_y = aabb_corners(min, max)
        .iter()
        .map(|p| (rotation * (scale * *p)).y)
        .fold(f32::INFINITY, f32::min);

    Transform {
        translation: Vec3::new(offset_m - front_local_x, -min_y, 0.0),
        rotation,
        scale,
    }
}

/// Pick the highest-detail distance level (lowest `dlevel_selection` metres).
pub fn closest_lod_level(shape: &ShapeFile) -> Option<&DistanceLevel> {
    shape
        .lod_controls
        .first()?
        .distance_levels
        .iter()
        .min_by(|a, b| {
            a.selection_m
                .partial_cmp(&b.selection_m)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// LOD level for a camera distance (m): finest level whose `dlevel_selection` ≤ `distance_m`.
pub fn lod_level_for_distance(shape: &ShapeFile, distance_m: f32) -> Option<&DistanceLevel> {
    let control = shape.lod_controls.first()?;
    let levels = &control.distance_levels;
    if levels.is_empty() {
        return None;
    }
    let mut best = levels.iter().min_by(|a, b| {
        a.selection_m
            .partial_cmp(&b.selection_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    for lvl in levels {
        if (lvl.selection_m as f32) <= distance_m && lvl.selection_m >= best.selection_m {
            best = lvl;
        }
    }
    Some(best)
}

/// Resolve the first texture referenced by the closest LOD (prim_state → `texture_filenames`).
pub fn primary_texture_filename(shape: &ShapeFile) -> Option<String> {
    let level = closest_lod_level(shape)?;
    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            if let Some(texture) = texture_filename_for_prim_state(shape, prim.prim_state_idx) {
                return Some(texture);
            }
        }
    }
    shape.texture_filenames.first().cloned()
}

/// Build a Bevy mesh from a specific distance level.
pub fn build_mesh_from_shape_lod(shape: &ShapeFile, level: &DistanceLevel) -> Option<Mesh> {
    let mut buffers = MeshBuffers::default();

    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });

    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            append_primitive_mesh_buffers(shape, sub, prim, default_normal, &mut buffers);
        }
    }

    buffers.into_mesh()
}

/// Build one Bevy mesh per `prim_state_idx` for a specific distance level.
pub fn build_mesh_parts_from_shape_lod(
    shape: &ShapeFile,
    level: &DistanceLevel,
) -> Vec<LoadedShapePart> {
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    let mut parts: BTreeMap<i32, MeshBuffers> = BTreeMap::new();

    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            let buffers = parts.entry(prim.prim_state_idx).or_default();
            append_primitive_mesh_buffers(shape, sub, prim, default_normal, buffers);
        }
    }

    parts
        .into_iter()
        .filter_map(|(prim_state_idx, buffers)| {
            let mesh = buffers.into_mesh()?;
            Some(LoadedShapePart {
                prim_state_idx,
                mesh,
                texture_file: texture_filename_for_prim_state(shape, prim_state_idx),
                shader_name: shader_name_for_prim_state(shape, prim_state_idx),
            })
        })
        .collect()
}

#[derive(Default)]
struct MeshBuffers {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
}

impl MeshBuffers {
    fn into_mesh(self) -> Option<Mesh> {
        if self.positions.is_empty() {
            return None;
        }

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        Some(mesh)
    }
}

fn append_primitive_mesh_buffers(
    shape: &ShapeFile,
    sub: &openrailsrs_formats::SubObject,
    prim: &openrailsrs_formats::Primitive,
    default_normal: ShapeVec3,
    buffers: &mut MeshBuffers,
) {
    for tri in prim.vertex_indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        for &vertex_idx in tri {
            let Some((point_idx, normal_idx, uv_idx)) =
                resolve_shape_vertex_indices(shape, sub, vertex_idx)
            else {
                continue;
            };
            let Some(point) = shape.points.get(point_idx) else {
                continue;
            };
            buffers.positions.push(shape_point_to_bevy(*point));
            let normal = normal_idx
                .and_then(|idx| shape.normals.get(idx).copied())
                .unwrap_or(default_normal);
            buffers.normals.push(shape_point_to_bevy(normal));
            let uv = uv_idx
                .and_then(|idx| shape.uvs.get(idx).copied())
                .unwrap_or_default();
            // MSTS UV origin differs from Bevy; flip V for textured quads.
            buffers.uvs.push(Vec2::new(uv.u as f32, 1.0 - uv.v as f32));
        }
    }
}

fn texture_filename_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
    if prim_state_idx < 0 {
        return None;
    }
    let ps = shape.prim_states.get(prim_state_idx as usize)?;
    let texture_idx = ps.tex_indices.first().copied().unwrap_or(ps.texture_idx);
    if texture_idx < 0 {
        return None;
    }
    shape.texture_filenames.get(texture_idx as usize).cloned()
}

fn shader_name_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
    if prim_state_idx < 0 {
        return None;
    }
    let ps = shape.prim_states.get(prim_state_idx as usize)?;
    if ps.shader_idx < 0 {
        return None;
    }
    shape.shader_names.get(ps.shader_idx as usize).cloned()
}

fn resolve_shape_vertex_indices(
    shape: &ShapeFile,
    sub: &openrailsrs_formats::SubObject,
    vertex_idx: u32,
) -> Option<(usize, Option<usize>, Option<usize>)> {
    if let Some(vertex) = sub.vertices.get(vertex_idx as usize) {
        return Some((
            index_to_usize(vertex.point_idx)?,
            index_to_usize(vertex.normal_idx),
            vertex
                .uv_indices
                .first()
                .and_then(|idx| index_to_usize(*idx)),
        ));
    }

    // Older ASCII fixtures can use `vertex_idxs` directly against points.
    let idx = vertex_idx as usize;
    if idx < shape.points.len() {
        return Some((idx, Some(idx), Some(idx)));
    }

    None
}

fn index_to_usize(idx: i32) -> Option<usize> {
    (idx >= 0).then_some(idx as usize)
}

/// Build a Bevy mesh from the closest LOD of a parsed shape.
pub fn build_mesh_from_shape(shape: &ShapeFile) -> Option<Mesh> {
    let level = closest_lod_level(shape)?;
    build_mesh_from_shape_lod(shape, level)
}

/// Build one Bevy mesh per `prim_state_idx` from the closest LOD.
pub fn build_mesh_parts_from_shape(shape: &ShapeFile) -> Vec<LoadedShapePart> {
    let Some(level) = closest_lod_level(shape) else {
        return Vec::new();
    };
    build_mesh_parts_from_shape_lod(shape, level)
}

/// Build mesh choosing LOD from camera distance (m) to the shape origin.
pub fn build_mesh_from_shape_at_distance(shape: &ShapeFile, distance_m: f32) -> Option<Mesh> {
    let level = lod_level_for_distance(shape, distance_m).or_else(|| closest_lod_level(shape))?;
    build_mesh_from_shape_lod(shape, level)
}

/// Build mesh parts choosing LOD from camera distance (m) to the shape origin.
pub fn build_mesh_parts_from_shape_at_distance(
    shape: &ShapeFile,
    distance_m: f32,
) -> Vec<LoadedShapePart> {
    let Some(level) =
        lod_level_for_distance(shape, distance_m).or_else(|| closest_lod_level(shape))
    else {
        return Vec::new();
    };
    build_mesh_parts_from_shape_lod(shape, level)
}

/// Convert decoded ACE mip 0 (RGBA8) into a Bevy GPU image.
pub fn ace_to_image(ace: &AceFile) -> Image {
    Image::new(
        Extent3d {
            width: ace.width,
            height: ace.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        ace.mip0.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Optional MSTS install root (`Content/`) for `GLOBAL/SHAPES` lookup.
pub fn msts_content_root() -> Option<PathBuf> {
    std::env::var("OPENRAILSRS_MSTS_CONTENT")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// Route directory plus optional `GLOBAL` from [`msts_content_root`].
pub fn shape_search_dirs(route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(global) = global_assets_dir() {
        dirs.push(global);
    }
    dirs
}

/// `GLOBAL/` under [`msts_content_root`], when configured.
pub fn global_assets_dir() -> Option<PathBuf> {
    msts_content_root()
        .map(|root| root.join("GLOBAL"))
        .filter(|p| p.is_dir())
}

/// Directories to search for `.ace` textures given a resolved shape path.
pub fn texture_search_dirs_for_shape(shape_path: &Path, route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(shapes_dir) = shape_path.parent().filter(|p| {
        p.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("shapes"))
    }) {
        if let Some(asset_root) = shapes_dir.parent() {
            if asset_root != route_dir {
                dirs.push(asset_root.to_path_buf());
            }
        }
    }
    if let Some(global) = global_assets_dir() {
        dirs.push(global);
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Resolve `SHAPES/foo.s` under the route directory (case-insensitive on Linux).
pub fn resolve_shape_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    for subdir in ["SHAPES", "shapes"] {
        let path = route_dir.join(subdir).join(file_name);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    None
}

/// Search several asset roots (route dir, scenario dir, …) for a shape file.
pub fn resolve_shape_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(path) = resolve_shape_path(dir, file_name) {
            return Some(path);
        }
    }
    None
}

/// Scan `SHAPES/` under each asset root once and map lowercase filename → path.
pub fn build_shape_path_index(dirs: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut index = HashMap::new();
    for dir in dirs {
        for subdir in ["SHAPES", "shapes"] {
            let shapes_dir = dir.join(subdir);
            if !shapes_dir.is_dir() {
                continue;
            }
            let Ok(read_dir) = std::fs::read_dir(&shapes_dir) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                if !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("s"))
                {
                    continue;
                }
                if let Some(name) = path.file_name() {
                    index
                        .entry(name.to_string_lossy().to_ascii_lowercase())
                        .or_insert(path);
                }
            }
        }
    }
    index
}

/// Resolve `TEXTURES/foo.ace` under one asset root directory.
pub fn resolve_texture_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    for subdir in ["TEXTURES", "textures"] {
        let textures_root = route_dir.join(subdir);
        let direct = textures_root.join(file_name);
        if direct.is_file() {
            return Some(direct);
        }
        if let Some(p) = openrailsrs_formats::resolve_path_case_insensitive(&direct) {
            return Some(p);
        }
        // MSTS routes often store seasonal variants in TEXTURES/SPRING/, etc.
        if let Ok(entries) = std::fs::read_dir(&textures_root) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let candidate = entry.path().join(file_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
                if let Some(p) = openrailsrs_formats::resolve_path_case_insensitive(&candidate) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Search several asset roots for `TEXTURES/foo.ace`, returning the first match.
///
/// Use this instead of `resolve_texture_path` when a shape may live in a
/// directory other than the route root (e.g. a trainset folder).
pub fn resolve_texture_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(p) = resolve_texture_path(dir, file_name) {
            return Some(p);
        }
    }
    None
}

/// Load and decode an `.ace` file into a Bevy image (mip 0 only).
pub fn load_ace_image(route_dir: &Path, file_name: &str) -> Option<Image> {
    let path = resolve_texture_path(route_dir, file_name)?;
    let ace = read_ace(&path).ok()?;
    Some(ace_to_image(&ace))
}

/// Load a shape and prepare Bevy mesh/material handles for each `prim_state` part.
///
/// `texture_dirs` is searched in order for `TEXTURES/<name>.ace`.  Pass at
/// least `&[route_dir]`; for trainset shapes also include the trainset root so
/// that textures stored alongside the rolling stock are found.
#[allow(clippy::too_many_arguments)]
pub fn load_shape_render_asset_from_path(
    shape_path: &Path,
    texture_dirs: &[&Path],
    camera_distance_m: Option<f32>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_cache: &mut HashMap<PathBuf, Handle<Image>>,
    fallback_color: Color,
) -> Option<ShapeRenderAsset> {
    let loaded = load_shape_from_path(shape_path, camera_distance_m)?;
    Some(shape_render_asset_from_loaded(
        loaded,
        texture_dirs,
        meshes,
        images,
        materials,
        texture_cache,
        fallback_color,
    ))
}

/// Turn parsed shape geometry into Bevy asset handles (main thread — touches `Assets`).
#[allow(clippy::too_many_arguments)]
pub fn shape_render_asset_from_loaded(
    loaded: LoadedShape,
    texture_dirs: &[&Path],
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_cache: &mut HashMap<PathBuf, Handle<Image>>,
    fallback_color: Color,
) -> ShapeRenderAsset {
    shape_render_asset_from_loaded_with_ace_cache(
        loaded,
        texture_dirs,
        meshes,
        images,
        materials,
        texture_cache,
        &HashMap::new(),
        fallback_color,
    )
}

/// Like [`shape_render_asset_from_loaded`] but uses a pre-decoded ACE cache (from parallel prefetch).
#[allow(clippy::too_many_arguments)]
pub fn shape_render_asset_from_loaded_with_ace_cache(
    loaded: LoadedShape,
    texture_dirs: &[&Path],
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_cache: &mut HashMap<PathBuf, Handle<Image>>,
    ace_cache: &HashMap<PathBuf, AceFile>,
    fallback_color: Color,
) -> ShapeRenderAsset {
    let combined_mesh = meshes.add(loaded.mesh);

    let mut has_any_texture = false;
    let mut parts = Vec::with_capacity(loaded.parts.len().max(1));
    if loaded.parts.is_empty() {
        let (material, has_texture, is_transparent) = material_for_shape_texture(
            texture_dirs,
            loaded.texture_file.as_deref(),
            None,
            images,
            materials,
            texture_cache,
            ace_cache,
            fallback_color,
        );
        has_any_texture |= has_texture;
        parts.push(ShapePartAsset {
            prim_state_idx: -1,
            mesh: combined_mesh.clone(),
            material,
            has_texture,
            is_transparent,
        });
    }
    for part in loaded.parts {
        let (material, has_texture, is_transparent) = material_for_shape_texture(
            texture_dirs,
            part.texture_file.as_deref(),
            part.shader_name.as_deref(),
            images,
            materials,
            texture_cache,
            ace_cache,
            fallback_color,
        );
        has_any_texture |= has_texture;
        parts.push(ShapePartAsset {
            prim_state_idx: part.prim_state_idx,
            mesh: meshes.add(part.mesh),
            material,
            has_texture,
            is_transparent,
        });
    }

    ShapeRenderAsset {
        combined_mesh,
        parts,
        has_texture: has_any_texture,
    }
}

/// Resolve on-disk paths for every ACE referenced by a parsed shape.
pub fn collect_loaded_shape_texture_paths(
    loaded: &LoadedShape,
    texture_dirs: &[&Path],
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut names = Vec::new();
    if let Some(name) = loaded.texture_file.as_deref() {
        names.push(name);
    }
    for part in &loaded.parts {
        if let Some(name) = part.texture_file.as_deref() {
            names.push(name);
        }
    }
    for name in names {
        if let Some(path) = resolve_texture_path_in_dirs(texture_dirs, name) {
            paths.push(path);
        }
    }
    paths
}

/// Decode `.ace` files in parallel (safe before inserting into Bevy `Assets`).
pub fn prefetch_ace_textures(paths: &[PathBuf]) -> HashMap<PathBuf, AceFile> {
    use rayon::prelude::*;
    paths
        .par_iter()
        .filter_map(|path| read_ace(path).ok().map(|ace| (path.clone(), ace)))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn material_for_shape_texture(
    texture_dirs: &[&Path],
    texture_file: Option<&str>,
    shader_name: Option<&str>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_cache: &mut HashMap<PathBuf, Handle<Image>>,
    ace_cache: &HashMap<PathBuf, AceFile>,
    fallback_color: Color,
) -> (Handle<StandardMaterial>, bool, bool) {
    if let Some(tex_name) = texture_file {
        match resolve_texture_path_in_dirs(texture_dirs, tex_name) {
            None => {}
            Some(tex_path) => {
                let ace = if let Some(ace) = ace_cache.get(&tex_path) {
                    Some(ace.clone())
                } else {
                    match read_ace(&tex_path) {
                        Ok(ace) => Some(ace),
                        Err(e) => {
                            viewer_log!(
                                "openrailsrs-viewer3d: ACE decode error for {}: {e}",
                                tex_path.display()
                            );
                            None
                        }
                    }
                };
                if let Some(ace) = ace {
                    let alpha_mode = shape_alpha_mode(&ace, tex_name, shader_name);
                    let is_transparent = !matches!(alpha_mode, AlphaMode::Opaque);
                    let image = ace_to_image(&ace);
                    let handle = texture_cache
                        .entry(tex_path)
                        .or_insert_with(|| images.add(image))
                        .clone();
                    let material = materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        base_color_texture: Some(handle),
                        perceptual_roughness: 0.85,
                        metallic: 0.05,
                        double_sided: true,
                        alpha_mode,
                        ..default()
                    });
                    return (material, true, is_transparent);
                }
            }
        }
    }

    let material = materials.add(StandardMaterial {
        base_color: fallback_color,
        emissive: LinearRgba::from(fallback_color) * 0.35,
        perceptual_roughness: 0.75,
        metallic: 0.1,
        double_sided: true,
        ..default()
    });
    (material, false, false)
}

fn shape_alpha_mode(ace: &AceFile, texture_file: &str, shader_name: Option<&str>) -> AlphaMode {
    let alpha = shape_alpha_stats(ace);
    if !alpha.has_any {
        return AlphaMode::Opaque;
    }

    if alpha.has_semitransparent
        && shader_name
            .map(shape_shader_requests_blending)
            .unwrap_or_else(|| texture_name_suggests_transparency(texture_file))
    {
        AlphaMode::Blend
    } else {
        AlphaMode::Mask(0.5)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShapeAlphaStats {
    has_any: bool,
    has_semitransparent: bool,
}

fn shape_alpha_stats(ace: &AceFile) -> ShapeAlphaStats {
    let mut stats = ShapeAlphaStats {
        has_any: ace.has_mask_channel,
        has_semitransparent: false,
    };
    for rgba in ace.mip0.chunks_exact(4) {
        let a = rgba[3];
        if a < 250 {
            stats.has_any = true;
        }
        if (9..248).contains(&a) {
            stats.has_semitransparent = true;
        }
    }
    stats
}

fn texture_name_suggests_transparency(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    ["glass", "window", "alpha", "trans", "transp"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn shape_shader_requests_blending(shader_name: &str) -> bool {
    matches!(
        shader_name,
        "BlendATex" | "BlendATexDiff" | "AddATex" | "AddATexDiff"
    )
}

/// Load shape mesh and discover its primary texture filename, if any.
///
/// When `camera_distance_m` is set, picks a coarser LOD farther from the camera.
pub fn load_shape_from_path(path: &Path, camera_distance_m: Option<f32>) -> Option<LoadedShape> {
    let shape = ShapeFile::from_path(path).ok()?;
    let parts = match camera_distance_m {
        Some(d) => build_mesh_parts_from_shape_at_distance(&shape, d),
        None => build_mesh_parts_from_shape(&shape),
    };
    let mesh = match camera_distance_m {
        Some(d) => build_mesh_from_shape_at_distance(&shape, d)?,
        None => build_mesh_from_shape(&shape)?,
    };
    let texture_file = primary_texture_filename(&shape);
    Some(LoadedShape {
        mesh,
        texture_file,
        parts,
    })
}

/// Load a shape as one mesh per `prim_state_idx`.
pub fn load_shape_parts_from_path(
    path: &Path,
    camera_distance_m: Option<f32>,
) -> Option<Vec<LoadedShapePart>> {
    let shape = ShapeFile::from_path(path).ok()?;
    let parts = match camera_distance_m {
        Some(d) => build_mesh_parts_from_shape_at_distance(&shape, d),
        None => build_mesh_parts_from_shape(&shape),
    };
    (!parts.is_empty()).then_some(parts)
}

/// Load and convert a shape file from disk (mesh only).
pub fn load_shape_mesh(path: &Path) -> Option<Mesh> {
    load_shape_from_path(path, None).map(|loaded| loaded.mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn minimal_shape_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-formats/tests/fixtures/minimal.s")
    }

    fn ace_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-ace/tests/fixtures/rgba8_4x4.ace")
    }

    fn write_synthetic_ace(path: &std::path::Path, rgba: &[u8]) {
        let pixel_count = rgba.len() / 4;
        let mut bytes = b"@ACE".to_vec();
        bytes.extend_from_slice(&(pixel_count as u32).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(1);
        bytes.push(4);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(rgba);
        std::fs::write(path, bytes).unwrap();
    }

    fn chiltern_shape_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES")
            .join(name)
    }

    fn chiltern_route_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman")
    }

    #[test]
    fn build_mesh_from_minimal_shape_has_two_triangles() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse minimal.s");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        assert_eq!(mesh.count_vertices(), 6);
    }

    #[test]
    fn build_mesh_from_binary_shape_resolves_vertex_table() {
        let shape = ShapeFile::from_path(chiltern_shape_fixture("RF_WP_DMBSA.s"))
            .expect("parse binary shape");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        assert_eq!(mesh.count_vertices(), 14610);
    }

    #[test]
    fn build_mesh_parts_from_binary_shape_groups_by_prim_state() {
        let shape = ShapeFile::from_path(chiltern_shape_fixture("RF_WP_DMBSA.s"))
            .expect("parse binary shape");
        let parts = build_mesh_parts_from_shape(&shape);
        assert!(parts.len() > 1);

        let total_vertices: usize = parts.iter().map(|part| part.mesh.count_vertices()).sum();
        assert_eq!(total_vertices, 14610);

        for part in &parts {
            assert!(part.mesh.count_vertices() > 0);
            assert!(part.prim_state_idx >= 0);
            if let Some(texture_file) = &part.texture_file {
                assert!(shape.texture_filenames.contains(texture_file));
            }
        }
    }

    #[test]
    fn load_shape_parts_from_path_preserves_part_textures() {
        let parts = load_shape_parts_from_path(&chiltern_shape_fixture("RF_WP_DMBSA.s"), None)
            .expect("parts");
        assert!(parts.len() > 1);
        assert!(parts.iter().any(|part| part.texture_file.is_some()));
    }

    #[test]
    fn load_shape_render_asset_builds_part_handles() {
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();
        let trainset_dir = chiltern_route_dir();

        let asset = load_shape_render_asset_from_path(
            &chiltern_shape_fixture("RF_WP_DMBSA.s"),
            &[trainset_dir.as_path()],
            None,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            Color::srgb(0.95, 0.25, 0.85),
        )
        .expect("render asset");

        assert!(asset.parts.len() > 1);
        assert_eq!(materials.len(), asset.parts.len());
        assert!(
            asset
                .parts
                .iter()
                .all(|part| meshes.get(&part.mesh).is_some())
        );
    }

    /// Verify that `resolve_texture_path_in_dirs` finds a texture in the
    /// trainset directory even when it is absent from the route dir.
    ///
    /// This mirrors the real Chiltern layout where
    /// `examples/chiltern/TEXTURES/` and `trains/RF_Blue_Pullman/TEXTURES/`
    /// are distinct directories.
    #[test]
    fn resolve_texture_path_in_dirs_finds_trainset_texture() {
        let fake_route_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // no TEXTURES here
        let trainset_dir = chiltern_route_dir(); // .../RF_Blue_Pullman – has TEXTURES/

        if !trainset_dir.join("TEXTURES/bp01.ace").exists() {
            return; // Skip when Chiltern data is absent (CI)
        }

        // Route-only search should NOT find trainset textures.
        assert!(
            resolve_texture_path(&fake_route_dir, "bp01.ace").is_none(),
            "fake route_dir should have no TEXTURES/bp01.ace"
        );

        // Multi-dir search that includes trainset_dir SHOULD find it.
        let found = resolve_texture_path_in_dirs(
            &[fake_route_dir.as_path(), trainset_dir.as_path()],
            "bp01.ace",
        );
        assert!(
            found.is_some(),
            "resolve_texture_path_in_dirs should find bp01.ace in trainset TEXTURES/"
        );
        assert!(
            found.unwrap().ends_with("TEXTURES/bp01.ace"),
            "resolved path should end with TEXTURES/bp01.ace"
        );
    }

    #[test]
    fn closest_lod_picks_nearest_distance_level() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let level = closest_lod_level(&shape).expect("lod");
        assert!((level.selection_m - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn primary_texture_from_minimal_shape() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        assert_eq!(
            primary_texture_filename(&shape).as_deref(),
            Some("wagon.ace")
        );
    }

    #[test]
    fn ace_to_image_preserves_dimensions() {
        let ace = read_ace(ace_fixture()).expect("ace");
        let image = ace_to_image(&ace);
        assert_eq!(image.size().x, 4);
        assert_eq!(image.size().y, 4);
    }

    #[test]
    fn material_for_shape_texture_uses_alpha_blend() {
        let route = std::env::temp_dir().join("openrailsrs_alpha_shape_material");
        let textures = route.join("TEXTURES");
        std::fs::create_dir_all(&textures).unwrap();
        let texture = textures.join("alpha_test.ace");
        write_synthetic_ace(&texture, &[0xFF, 0xFF, 0xFF, 0x80]);

        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();

        let (handle, has_texture, is_transparent) = material_for_shape_texture(
            &[route.as_path()],
            Some("alpha_test.ace"),
            Some("BlendATexDiff"),
            &mut images,
            &mut materials,
            &mut texture_cache,
            &HashMap::new(),
            Color::srgb(0.95, 0.25, 0.85),
        );

        let material = materials.get(&handle).expect("material");
        assert!(has_texture);
        assert!(is_transparent);
        assert!(matches!(material.alpha_mode, AlphaMode::Blend));

        let _ = std::fs::remove_file(texture);
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn material_for_shape_texture_uses_alpha_mask_for_binary_cutout() {
        let route = std::env::temp_dir().join("openrailsrs_mask_shape_material");
        let textures = route.join("TEXTURES");
        std::fs::create_dir_all(&textures).unwrap();
        let texture = textures.join("body.ace");
        write_synthetic_ace(&texture, &[0xFF, 0xFF, 0xFF, 0x00]);

        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();

        let (handle, has_texture, is_transparent) = material_for_shape_texture(
            &[route.as_path()],
            Some("body.ace"),
            Some("TexDiff"),
            &mut images,
            &mut materials,
            &mut texture_cache,
            &HashMap::new(),
            Color::srgb(0.95, 0.25, 0.85),
        );

        let material = materials.get(&handle).expect("material");
        assert!(has_texture);
        assert!(is_transparent);
        assert!(matches!(material.alpha_mode, AlphaMode::Mask(_)));

        let _ = std::fs::remove_file(texture);
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn shape_alpha_mode_does_not_blend_opaque_window_named_texture() {
        let route = std::env::temp_dir().join("openrailsrs_opaque_window_shape_material");
        std::fs::create_dir_all(&route).unwrap();
        let texture = route.join("window_body.ace");
        write_synthetic_ace(&texture, &[0xFF, 0xFF, 0xFF, 0xFF]);
        let ace = read_ace(&texture).expect("ace");

        assert!(matches!(
            shape_alpha_mode(&ace, "window_body.ace", Some("TexDiff")),
            AlphaMode::Opaque
        ));

        let _ = std::fs::remove_file(texture);
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn resolve_texture_path_finds_seasonal_subdir_on_chiltern() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route.join("TEXTURES/SPRING").is_dir() {
            return;
        }
        // poplar15_1.ace exists under TEXTURES/ and TEXTURES/SPRING/
        assert!(resolve_texture_path(&route, "poplar15_1.ace").is_some());
    }

    #[test]
    fn resolve_smoke_route_assets() {
        let route =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        assert!(resolve_shape_path(&route, "yard_shed.s").is_some());
        assert!(resolve_texture_path(&route, "yard.ace").is_some());
        let loaded =
            load_shape_from_path(&resolve_shape_path(&route, "yard_shed.s").unwrap(), None)
                .expect("shape");
        assert_eq!(loaded.texture_file.as_deref(), Some("yard.ace"));
    }

    #[test]
    fn msts_forward_maps_to_train_plus_x() {
        let forward = msts_shape_to_train_rotation() * Vec3::Z;
        assert!((forward.x - 1.0).abs() < 1e-4);
        assert!(forward.z.abs() < 1e-4);
    }

    #[test]
    fn vehicle_shape_scales_flat_profile_to_length() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let transform = vehicle_shape_local_transform(&mesh, 0.0, 18.0);
        assert!((transform.scale.x - 18.0).abs() < 1e-3);
        let rotated = transform.rotation * Vec3::Z;
        assert!((rotated.x - 1.0).abs() < 1e-3);
    }

    #[test]
    fn vehicle_shape_front_stays_at_offset() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let t0 = vehicle_shape_local_transform(&mesh, 0.0, 18.0);
        let t1 = vehicle_shape_local_transform(&mesh, -18.0, 14.0);
        assert!(t0.translation.x.abs() < 1e-3);
        assert!((t1.translation.x + 18.0).abs() < 1e-3);
    }

    #[test]
    fn chiltern_tree_shapes_build_meshes() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route.join("SHAPES/POPLAR15.S").is_file() {
            return;
        }
        for name in [
            "SHAPES/POPLAR15.S",
            "SHAPES/ASH12.S",
            "SHAPES/by_poplar2_treeline.s",
        ] {
            let path = route.join(name);
            let shape = ShapeFile::from_path(&path).expect("parse tree shape");
            let mesh = build_mesh_from_shape(&shape).expect("mesh");
            let parts = build_mesh_parts_from_shape(&shape);
            assert!(mesh.count_vertices() > 0, "{name}: empty combined mesh");
            assert!(
                parts.iter().all(|p| p.mesh.count_vertices() > 0),
                "{name}: empty part"
            );
            assert!(
                load_shape_from_path(&path, None).is_some(),
                "{name}: load failed"
            );
        }
    }

    #[test]
    fn texture_search_dirs_include_route_root() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let shape = route.join("SHAPES/POPLAR15.S");
        if !shape.is_file() {
            return;
        }
        let dirs = texture_search_dirs_for_shape(&shape, &route);
        assert!(
            dirs.iter().any(|d| d.ends_with("chiltern")),
            "expected route root in texture dirs: {dirs:?}"
        );
    }
}
