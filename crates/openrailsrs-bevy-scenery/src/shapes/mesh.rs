//! MSTS `.s` shape → Bevy mesh builders (shared; cab-specific paths stay in viewer3d).

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use openrailsrs_formats::{DistanceLevel, Matrix43, ShapeFile, Vec3 as ShapeVec3};

use openrailsrs_or_shader::coordinates::{
    matrix43_transform_point_xna, matrix43_transform_vector_xna, shape_point_to_bevy,
};

use super::anim::animation_pose_matrices;
use super::debug::{
    clamp_msts_z_bias_for_bevy, set_train_shape_debug_scope, shape_uv_to_bevy,
    train_debug_flip_winding_active,
};
use super::descriptor::ShapeDescriptor;

/// Options for [`build_mesh_parts_from_shape_lod_with_options`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshPartBuildOptions {
    /// Keep one part per `(sub_object_idx, prim_state)` (night sub-objects, cab).
    /// Default merges all sub-objects that share a `prim_state_idx`.
    pub keep_sub_objects: bool,
    /// When set, bake `animation_pose_matrices(shape, key)` into vertex positions
    /// (render3d WORLD rest / water-column key `0`). Viewer WORLD prefers rest + runtime anim.
    pub bake_animation_key: Option<f32>,
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
    /// Source `sub_object` index (cab CVF binding); `u32::MAX` when merged across sub-objects.
    pub sub_object_idx: u32,
    /// Open Rails `SortIndex` of the first file-order primitive merged into this part (#102).
    pub sort_index: u32,
    /// Animated MSTS matrix for cab levers (`sub_object_idx == matrix_idx` convention).
    pub cab_matrix_idx: Option<usize>,
    pub mesh: Mesh,
    pub texture_file: Option<String>,
    pub shader_name: Option<String>,
    /// Uniform vertex colour for TexDiff/Tex (MSTS colour × texture).
    pub solid_color: Option<[f32; 3]>,
    /// From `prim_state.alpha_test_mode`: -1 = unknown, 0 = opaque, 1 = test, 2 = blend.
    pub alpha_test_mode: i32,
    pub z_bias: Option<f32>,
    pub z_buf_mode: i32,
    /// OR `vtx_state` light material index (`12 + idx` → HalfBright / FullBright).
    pub light_mat_idx: Option<i32>,
    /// OR first `uv_op.TexAddrMode` (1=Wrap, 2=Mirror, 3=Clamp, 4=Border).
    pub tex_addr_mode: Option<i32>,
    /// Baked mesh AABB (cab CVF proximity filter).
    pub bounds_center: Option<Vec3>,
    pub bounds_half_extent: Option<Vec3>,
    /// Cab lever rotates about mesh center instead of matrix pivot (far 3D wheel).
    pub lever_pivot_at_mesh_center: bool,
    /// Override local rotation axis for fallback lever animation.
    pub lever_local_axis: Option<Vec3>,
}

/// Tiny monotonic depth nudge so coplanar blend parts follow OR `SortIndex` order (#102).
pub fn sort_index_depth_nudge(sort_index: u32) -> f32 {
    sort_index as f32 * 1e-5
}

/// WORLD mesh options from the shape `.sd` (keep night sub-objects when `ESD_SubObj`).
pub fn world_mesh_options_for_shape(shape_path: &Path) -> MeshPartBuildOptions {
    let desc = ShapeDescriptor::load_for_shape(shape_path);
    MeshPartBuildOptions {
        keep_sub_objects: desc.has_night_subobj,
        bake_animation_key: None,
    }
}
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

/// UV bounds for a mesh (`ATTRIBUTE_UV_0`).
pub fn mesh_uv_aabb(mesh: &Mesh) -> Option<(Vec2, Vec2)> {
    let uvs = mesh.attribute(Mesh::ATTRIBUTE_UV_0)?;
    let slice = match uvs {
        VertexAttributeValues::Float32x2(uvs) => uvs.as_slice(),
        _ => return None,
    };
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for uv in slice {
        let p = Vec2::from(*uv);
        min = min.min(p);
        max = max.max(p);
    }
    if min.x.is_finite() {
        Some((min, max))
    } else {
        None
    }
}

pub fn mesh_has_uvs(mesh: &Mesh) -> bool {
    mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some()
}

/// True when all UVs collapse to a tiny range (typical broken / missing UV symptom).
pub fn mesh_uv_degenerate(min: Vec2, max: Vec2) -> bool {
    (max - min).length() < 1e-4
}

/// Per-vertex or uniform colour stats for cab diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshVertexColorMode {
    #[default]
    None,
    Uniform,
    Varying,
}

#[derive(Debug, Clone, Copy)]
pub struct MeshVertexColorStats {
    pub mode: MeshVertexColorMode,
    pub min: Vec3,
    pub max: Vec3,
    pub count: usize,
}

impl Default for MeshVertexColorStats {
    fn default() -> Self {
        Self {
            mode: MeshVertexColorMode::None,
            min: Vec3::ZERO,
            max: Vec3::ZERO,
            count: 0,
        }
    }
}

pub fn mesh_vertex_color_stats(mesh: &Mesh) -> MeshVertexColorStats {
    let Some(colors) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else {
        return MeshVertexColorStats::default();
    };
    let slice = match colors {
        VertexAttributeValues::Float32x4(colors) => colors.as_slice(),
        _ => return MeshVertexColorStats::default(),
    };
    if slice.is_empty() {
        return MeshVertexColorStats::default();
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for c in slice {
        let p = Vec3::new(c[0], c[1], c[2]);
        min = min.min(p);
        max = max.max(p);
    }
    let first = Vec3::new(slice[0][0], slice[0][1], slice[0][2]);
    let uniform = slice.iter().all(|c| {
        colors_close(
            &[c[0], c[1], c[2], c[3]],
            &[first.x, first.y, first.z, slice[0][3]],
        )
    });
    MeshVertexColorStats {
        mode: if uniform {
            MeshVertexColorMode::Uniform
        } else {
            MeshVertexColorMode::Varying
        },
        min,
        max,
        count: slice.len(),
    }
}
/// Open Rails LOD selection policy (`UserSettings.LODBias` / `LODViewingExtension`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodPolicy {
    /// −100…100; `100` forces highest detail (LOD0) with coarsest viewing distance.
    pub bias: i32,
    /// When true, the coarsest LOD keeps drawing past the shape's declared viewing distance.
    pub viewing_extension: bool,
    /// Global camera viewing distance cap (metres), mirrors OR `ViewingDistance`.
    pub viewing_distance_m: f32,
}

impl Default for LodPolicy {
    fn default() -> Self {
        Self {
            bias: std::env::var("OPENRAILSRS_LOD_BIAS")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0)
                .clamp(-100, 100),
            viewing_extension: std::env::var("OPENRAILSRS_LOD_VIEWING_EXTENSION")
                .ok()
                .map(|v| {
                    let t = v.trim();
                    !(t == "0" || t.eq_ignore_ascii_case("false") || t.eq_ignore_ascii_case("off"))
                })
                .unwrap_or(true),
            viewing_distance_m: std::env::var("OPENRAILSRS_VIEWING_DISTANCE")
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
                .unwrap_or(2000.0)
                .clamp(50.0, 20_000.0),
        }
    }
}

/// Indices of `control.distance_levels` sorted finest→coarsest (`dlevel_selection` ascending).
fn sorted_level_indices(control: &openrailsrs_formats::LodControl) -> Vec<usize> {
    let mut idxs: Vec<usize> = (0..control.distance_levels.len()).collect();
    idxs.sort_by(|&a, &b| {
        control.distance_levels[a]
            .selection_m
            .partial_cmp(&control.distance_levels[b].selection_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idxs
}

/// OR `Camera.InRange` (XZ approximated by camera–object distance).
fn lod_in_range(
    distance_m: f32,
    view_sphere_radius: f32,
    viewing_distance_m: f32,
    global_viewing_distance_m: f32,
) -> bool {
    let vd = viewing_distance_m.min(global_viewing_distance_m);
    distance_m < view_sphere_radius + vd
}

/// Per-control LOD index (into `distance_levels`) using Open Rails bias / sphere / extension (#96).
pub fn lod_level_index_for_control(
    shape: &ShapeFile,
    control: &openrailsrs_formats::LodControl,
    distance_m: f32,
    policy: LodPolicy,
) -> usize {
    let levels = &control.distance_levels;
    if levels.is_empty() {
        return 0;
    }
    let sorted = sorted_level_indices(control);
    let coarsest_pos = sorted.len() - 1;
    let sphere = shape.view_sphere_radius_or_default();
    let lod_bias = (policy.bias as f32 / 100.0) + 1.0;

    let mut display_pos = coarsest_pos;
    if policy.bias == 100 {
        // Maximum detail; viewing distance still uses the coarsest level (OR special case).
        display_pos = 0;
    } else if policy.bias > -100 {
        while display_pos > 0 {
            let candidate = sorted[display_pos - 1];
            let viewing = levels[candidate].selection_m as f32 * lod_bias;
            if lod_in_range(distance_m, sphere, viewing, policy.viewing_distance_m) {
                display_pos -= 1;
            } else {
                break;
            }
        }
    }
    // `viewing_extension` is applied by callers when culling the coarsest band
    // (OR sets that level's ViewingDistance to MaxValue after selection).
    let _ = policy.viewing_extension;
    sorted[display_pos]
}

/// Pick the highest-detail distance level (lowest `dlevel_selection` metres) of the first control.
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

/// Finest distance level of every `lod_control` (#97).
pub fn closest_lod_levels(shape: &ShapeFile) -> Vec<&DistanceLevel> {
    shape
        .lod_controls
        .iter()
        .filter_map(|control| {
            control.distance_levels.iter().min_by(|a, b| {
                a.selection_m
                    .partial_cmp(&b.selection_m)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .collect()
}

/// Selected distance level per `lod_control` at `distance_m` (#96 / #97).
pub fn lod_levels_for_distance(
    shape: &ShapeFile,
    distance_m: f32,
    policy: LodPolicy,
) -> Vec<&DistanceLevel> {
    shape
        .lod_controls
        .iter()
        .filter_map(|control| {
            let idx = lod_level_index_for_control(shape, control, distance_m, policy);
            control.distance_levels.get(idx)
        })
        .collect()
}

/// LOD level for a camera distance (m) — first control only (compat).
pub fn lod_level_for_distance(shape: &ShapeFile, distance_m: f32) -> Option<&DistanceLevel> {
    lod_levels_for_distance(shape, distance_m, LodPolicy::default())
        .into_iter()
        .next()
        .or_else(|| closest_lod_level(shape))
}

/// Resolve the first texture referenced by the closest LOD (prim_state → `texture_filenames`).
pub fn primary_texture_filename(shape: &ShapeFile) -> Option<String> {
    let level = closest_lod_level(shape)?;
    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            if let Some(texture) = texture_for_prim_state(shape, prim.prim_state_idx) {
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
            append_primitive_mesh_buffers(
                shape,
                level,
                sub,
                prim,
                default_normal,
                &mut buffers,
                None,
                false,
            );
        }
    }

    buffers.into_mesh()
}

/// Build one Bevy mesh per `prim_state_idx` for a specific distance level.
pub fn build_mesh_parts_from_shape_lod(
    shape: &ShapeFile,
    level: &DistanceLevel,
) -> Vec<LoadedShapePart> {
    build_mesh_parts_from_shape_lod_with_options(shape, level, MeshPartBuildOptions::default())
}

/// Like [`build_mesh_parts_from_shape_lod`] with sub-object split and optional anim bake.
pub fn build_mesh_parts_from_shape_lod_with_options(
    shape: &ShapeFile,
    level: &DistanceLevel,
    options: MeshPartBuildOptions,
) -> Vec<LoadedShapePart> {
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    let pose = options
        .bake_animation_key
        .map(|key| animation_pose_matrices(shape, key));
    let pose_matrices = pose.as_deref();

    // Preserve Open Rails `SortIndex` / file order (#102). Do not use BTreeMap key order.
    struct PartAccum {
        sort_index: u32,
        buffers: MeshBuffers,
    }
    let mut order: Vec<(u32, i32)> = Vec::new();
    let mut parts: HashMap<(u32, i32), PartAccum> = HashMap::new();
    let mut next_sort = 0u32;
    for (sub_idx, sub) in level.sub_objects.iter().enumerate() {
        for prim in &sub.primitives {
            let key = if options.keep_sub_objects {
                (sub_idx as u32, prim.prim_state_idx)
            } else {
                (u32::MAX, prim.prim_state_idx)
            };
            let accum = parts.entry(key).or_insert_with(|| {
                let sort_index = next_sort;
                order.push(key);
                PartAccum {
                    sort_index,
                    buffers: MeshBuffers::default(),
                }
            });
            append_primitive_mesh_buffers_ex(
                shape,
                level,
                sub,
                prim,
                default_normal,
                &mut accum.buffers,
                None,
                false,
                pose_matrices,
            );
            // Advance like OR `SortIndex = ++totalPrimitiveIndex` so later distinct
            // groups get indices matching their first file-order primitive.
            next_sort += 1;
        }
    }

    order
        .into_iter()
        .filter_map(|key| {
            let PartAccum {
                sort_index,
                buffers,
            } = parts.remove(&key)?;
            let (sub_object_idx, prim_state_idx) = key;
            let (mesh, solid_color) = buffers.into_mesh_with_color()?;
            let (alpha_test_mode, z_bias_raw, z_buf_mode) = shape
                .prim_states
                .get(prim_state_idx.max(0) as usize)
                .map(|ps| {
                    (
                        ps.alpha_test_mode,
                        ps.z_bias.map(|z| z as f32),
                        ps.z_buf_mode,
                    )
                })
                .unwrap_or((-1, None, -1));
            let z_bias = Some(clamp_msts_z_bias_for_bevy(z_bias_raw, None));
            Some(LoadedShapePart {
                prim_state_idx,
                sub_object_idx,
                sort_index,
                cab_matrix_idx: None,
                mesh,
                texture_file: texture_for_prim_state(shape, prim_state_idx),
                shader_name: shader_name_for_prim_state(shape, prim_state_idx),
                solid_color,
                alpha_test_mode,
                z_bias,
                z_buf_mode,
                light_mat_idx: light_mat_idx_for_prim_state(shape, prim_state_idx),
                tex_addr_mode: shape.tex_addr_mode_for_prim_state(prim_state_idx),
                bounds_center: None,
                bounds_half_extent: None,
                lever_pivot_at_mesh_center: false,
                lever_local_axis: None,
            })
        })
        .collect()
}
pub fn mesh_buffers_bounds(buffers: &MeshBuffers) -> (Vec3, Vec3) {
    if buffers.positions.is_empty() {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in &buffers.positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    ((min + max) * 0.5, (max - min) * 0.5)
}

#[derive(Default)]
pub struct MeshBuffers {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub colors: Vec<[f32; 4]>,
}

impl MeshBuffers {
    pub fn into_mesh(self) -> Option<Mesh> {
        self.into_mesh_with_color().map(|(m, _)| m)
    }

    pub fn into_mesh_with_color(self) -> Option<(Mesh, Option<[f32; 3]>)> {
        if self.positions.is_empty() {
            return None;
        }

        let (vertex_colors, solid_color) = part_vertex_colors(&self.colors);
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        if let Some(colors) = vertex_colors {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
        Some((mesh, solid_color))
    }
}

/// Generate MikkTSpace tangents for PBR normal mapping (#44).
///
/// Call only when a normal map will be assigned. Requires final POSITION/NORMAL/UV_0
/// (post winding, Z-flip, and V-flip). Returns `false` on failure (degenerate UVs).
pub fn ensure_tangents_for_normal_mapping(mesh: &mut Mesh) -> bool {
    if mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some() {
        return true;
    }
    match mesh.generate_tangents() {
        Ok(()) => true,
        Err(e) => {
            bevy::log::debug!("generate_tangents failed ({e:?}); skipping normal map");
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn append_primitive_mesh_buffers(
    shape: &ShapeFile,
    level: &DistanceLevel,
    sub: &openrailsrs_formats::SubObject,
    prim: &openrailsrs_formats::Primitive,
    default_normal: ShapeVec3,
    buffers: &mut MeshBuffers,
    chain_start: Option<i32>,
    omit_leaf_matrix: bool,
) {
    append_primitive_mesh_buffers_ex(
        shape,
        level,
        sub,
        prim,
        default_normal,
        buffers,
        chain_start,
        omit_leaf_matrix,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn append_primitive_mesh_buffers_ex(
    shape: &ShapeFile,
    level: &DistanceLevel,
    sub: &openrailsrs_formats::SubObject,
    prim: &openrailsrs_formats::Primitive,
    default_normal: ShapeVec3,
    buffers: &mut MeshBuffers,
    chain_start: Option<i32>,
    omit_leaf_matrix: bool,
    pose_matrices: Option<&[Matrix43]>,
) {
    let start = chain_start.unwrap_or_else(|| {
        shape
            .prim_states
            .get(prim.prim_state_idx.max(0) as usize)
            .and_then(|ps| shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize))
            .map(|vs| vs.matrix_idx)
            .unwrap_or(0)
    });
    let matrix_chain =
        primitive_matrix_chain_bake_ex(shape, level, start, omit_leaf_matrix, pose_matrices);
    for tri in prim.vertex_indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let mut resolved = Vec::with_capacity(3);
        let mut skip_tri = false;
        for &vertex_idx in tri {
            let Some(resolved_vertex) = resolve_shape_vertex(shape, sub, vertex_idx) else {
                skip_tri = true;
                break;
            };
            resolved.push(resolved_vertex);
        }
        if skip_tri {
            continue;
        }
        // Open Rails reverses winding when loading shapes into XNA (`Coordinates.cs`).
        // MSTS→Bevy uses the same Z-handedness flip via [`shape_point_to_bevy`].
        resolved.swap(1, 2);
        if train_debug_flip_winding_active() {
            resolved.swap(1, 2);
        }
        if !resolved
            .iter()
            .all(|(point_idx, ..)| shape.points.get(*point_idx).is_some())
        {
            continue;
        }
        // Face normal from post-winding Bevy positions (matches OR Z-flip + XNA bake).
        let bevy_pts: [Vec3; 3] = std::array::from_fn(|i| {
            let (point_idx, ..) = resolved[i];
            let point = shape.points.get(point_idx).expect("checked");
            transform_shape_point(shape_point_to_bevy(*point), &matrix_chain)
        });
        let face_normal = face_normal_from_triangle(bevy_pts[0], bevy_pts[1], bevy_pts[2]);
        let fallback_normal = if shape_normal_is_usable(default_normal) {
            transform_shape_normal(shape_point_to_bevy(default_normal), &matrix_chain)
        } else {
            face_normal
        };

        for ((_point_idx, normal_idx, uv_idx, vertex_color), position) in
            resolved.into_iter().zip(bevy_pts)
        {
            buffers.positions.push(position);
            let authored = normal_idx
                .and_then(|idx| shape.normals.get(idx).copied())
                .filter(|n| shape_normal_is_usable(*n));
            let normal = if let Some(n) = authored {
                transform_shape_normal(shape_point_to_bevy(n), &matrix_chain)
            } else {
                // Prefer geometric face normal over a single shape-wide default.
                if face_normal.length_squared() > 0.0 {
                    face_normal
                } else {
                    fallback_normal
                }
            };
            buffers.normals.push(normal);
            let uv = uv_idx
                .and_then(|idx| shape.uvs.get(idx).copied())
                .unwrap_or_default();
            buffers.uvs.push(shape_uv_to_bevy(uv.u as f32, uv.v as f32));
            buffers.colors.push(vertex_color);
        }
    }
}

/// True when an authored MSTS normal can be used for lighting (finite, non-zero).
pub fn shape_normal_is_usable(n: ShapeVec3) -> bool {
    let len2 = n.x * n.x + n.y * n.y + n.z * n.z;
    n.x.is_finite() && n.y.is_finite() && n.z.is_finite() && len2 > 1e-12
}

fn face_normal_from_triangle(p0: Vec3, p1: Vec3, p2: Vec3) -> Vec3 {
    (p1 - p0)
        .cross(p2 - p0)
        .try_normalize()
        .unwrap_or(Vec3::ZERO)
}

#[derive(Clone, Copy)]
pub struct ShapeMatrixRef<'a> {
    matrix: &'a Matrix43,
    zero_translation: bool,
}

pub fn primitive_matrix_chain_bake<'a>(
    shape: &'a ShapeFile,
    level: &DistanceLevel,
    chain_start: i32,
    omit_leaf_matrix: bool,
) -> Vec<ShapeMatrixRef<'a>> {
    primitive_matrix_chain_bake_ex(shape, level, chain_start, omit_leaf_matrix, None)
}

/// Like [`primitive_matrix_chain_bake`], optionally substituting animated pose matrices.
pub fn primitive_matrix_chain_bake_ex<'a>(
    shape: &'a ShapeFile,
    level: &DistanceLevel,
    chain_start: i32,
    omit_leaf_matrix: bool,
    pose_matrices: Option<&'a [Matrix43]>,
) -> Vec<ShapeMatrixRef<'a>> {
    let mut out = Vec::new();
    let mut matrix_idx = chain_start;
    let mut guard = 0usize;
    let n = pose_matrices
        .map(|p| p.len())
        .unwrap_or(shape.matrices.len());
    while matrix_idx >= 0 && guard < n {
        let idx = matrix_idx as usize;
        let Some(matrix) = pose_matrices
            .and_then(|p| p.get(idx))
            .or_else(|| shape.matrices.get(idx).map(|m| &m.matrix))
        else {
            break;
        };
        out.push(ShapeMatrixRef {
            matrix,
            zero_translation: idx == 0 && level.hierarchy.first().copied() == Some(-1),
        });
        matrix_idx = level.hierarchy.get(idx).copied().unwrap_or(-1);
        guard += 1;
    }
    if omit_leaf_matrix && !out.is_empty() {
        out.remove(0);
    }
    out
}

// matrix43_transform_point_xna and matrix43_transform_vector_xna are imported from
// crate::coordinates at the top of this file.

pub fn transform_shape_point(mut point: Vec3, matrices: &[ShapeMatrixRef<'_>]) -> Vec3 {
    for matrix in matrices {
        point = matrix43_transform_point_xna(matrix.matrix, point, matrix.zero_translation);
    }
    point
}

pub fn transform_shape_normal(mut normal: Vec3, matrices: &[ShapeMatrixRef<'_>]) -> Vec3 {
    for matrix in matrices {
        normal = matrix43_transform_vector_xna(matrix.matrix, normal);
    }
    normal.try_normalize().unwrap_or(Vec3::Y)
}

pub fn texture_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
    shape
        .texture_for_prim_state_idx(prim_state_idx)
        .or_else(|| fallback_shape_texture(shape, prim_state_idx))
}

/// Heurísticas cuando el `prim_state` no declara `tex_idxs` (paridad OR + render3d).
pub(crate) fn fallback_shape_texture(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
    if shape.texture_filenames.is_empty() {
        return None;
    }
    if shape.texture_filenames.len() == 1 {
        return shape.texture_filenames.first().cloned();
    }
    for (i, other) in shape.prim_states.iter().enumerate() {
        if i as i32 == prim_state_idx {
            continue;
        }
        let tex_slot = other
            .tex_indices
            .first()
            .copied()
            .unwrap_or(other.texture_idx);
        if let Some(name) = shape.resolve_texture_for_tex_slot(tex_slot) {
            return Some(name);
        }
    }
    let ps = shape.prim_states.get(prim_state_idx.max(0) as usize);
    if ps.is_some_and(|ps| shader_requests_texture(shape, ps)) {
        return shape
            .primary_texture_filename()
            .or_else(|| shape.texture_filenames.first().cloned());
    }
    None
}

pub(crate) fn shader_requests_texture(
    shape: &ShapeFile,
    ps: &openrailsrs_formats::PrimState,
) -> bool {
    shape
        .shader_names
        .get(ps.shader_idx.max(0) as usize)
        .is_some_and(|name| {
            let n = name.to_ascii_lowercase();
            matches!(
                n.as_str(),
                "tex" | "texdiff" | "blendatex" | "blendatexdiff" | "addatex" | "addatexdiff"
            ) || n.contains("tex")
                || n.contains("blend")
        })
}

pub fn light_mat_idx_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<i32> {
    let ps = shape.prim_states.get(prim_state_idx.max(0) as usize)?;
    if ps.vertex_state_idx < 0 {
        return None;
    }
    shape
        .vtx_states
        .get(ps.vertex_state_idx as usize)
        .map(|vs| vs.light_mat_idx)
}

pub fn shader_name_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
    if prim_state_idx < 0 {
        return None;
    }
    let ps = shape.prim_states.get(prim_state_idx as usize)?;
    if ps.shader_idx < 0 {
        return None;
    }
    shape.shader_names.get(ps.shader_idx as usize).cloned()
}

#[allow(clippy::type_complexity)]
pub fn resolve_shape_vertex(
    shape: &ShapeFile,
    sub: &openrailsrs_formats::SubObject,
    vertex_idx: u32,
) -> Option<(usize, Option<usize>, Option<usize>, [f32; 4])> {
    if let Some(vertex) = sub.vertices.get(vertex_idx as usize) {
        return Some((
            index_to_usize(vertex.point_idx)?,
            index_to_usize(vertex.normal_idx),
            vertex
                .uv_indices
                .first()
                .and_then(|idx| index_to_usize(*idx)),
            vertex
                .color1
                .map(rgba_u8_to_f32)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]),
        ));
    }

    // Older ASCII fixtures can use `vertex_idxs` directly against points.
    let idx = vertex_idx as usize;
    if idx < shape.points.len() {
        return Some((idx, Some(idx), Some(idx), [1.0, 1.0, 1.0, 1.0]));
    }

    None
}

pub fn rgba_u8_to_f32([r, g, b, a]: [u8; 4]) -> [f32; 4] {
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}

pub fn part_vertex_colors(colors: &[[f32; 4]]) -> (Option<Vec<[f32; 4]>>, Option<[f32; 3]>) {
    if colors.is_empty() || !colors.iter().any(color_is_meaningful) {
        return (None, None);
    }
    let first = colors[0];
    let uniform = colors.iter().all(|c| colors_close(c, &first));
    if uniform {
        return (None, Some([first[0], first[1], first[2]]));
    }
    (Some(colors.to_vec()), None)
}

pub fn color_is_meaningful(c: &[f32; 4]) -> bool {
    (c[0] - 1.0).abs() > 0.02 || (c[1] - 1.0).abs() > 0.02 || (c[2] - 1.0).abs() > 0.02
}

pub fn colors_close(a: &[f32; 4], b: &[f32; 4]) -> bool {
    (a[0] - b[0]).abs() < 0.02
        && (a[1] - b[1]).abs() < 0.02
        && (a[2] - b[2]).abs() < 0.02
        && (a[3] - b[3]).abs() < 0.05
}

pub fn index_to_usize(idx: i32) -> Option<usize> {
    (idx >= 0).then_some(idx as usize)
}

/// Build a Bevy mesh from the closest LOD of every `lod_control` (#97).
pub fn build_mesh_from_shape(shape: &ShapeFile) -> Option<Mesh> {
    let levels = closest_lod_levels(shape);
    if levels.is_empty() {
        return None;
    }
    if levels.len() == 1 {
        return build_mesh_from_shape_lod(shape, levels[0]);
    }
    let mut buffers = MeshBuffers::default();
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    for level in levels {
        for sub in &level.sub_objects {
            for prim in &sub.primitives {
                append_primitive_mesh_buffers(
                    shape,
                    level,
                    sub,
                    prim,
                    default_normal,
                    &mut buffers,
                    None,
                    false,
                );
            }
        }
    }
    buffers.into_mesh()
}

/// Build one Bevy mesh per `prim_state_idx` from every control's closest LOD (#97).
pub fn build_mesh_parts_from_shape(shape: &ShapeFile) -> Vec<LoadedShapePart> {
    let mut parts = Vec::new();
    for level in closest_lod_levels(shape) {
        parts.extend(build_mesh_parts_from_shape_lod(shape, level));
    }
    parts
}

/// Index of the distance level chosen for `distance_m` on the first `lod_control`.
///
/// Uses Open Rails LODBias / ViewSphereRadius / viewing-distance policy (#96).
pub fn lod_level_index_for_distance(shape: &ShapeFile, distance_m: f32) -> usize {
    lod_level_index_for_distance_with_policy(shape, distance_m, LodPolicy::default())
}

/// Like [`lod_level_index_for_distance`] with an explicit [`LodPolicy`].
pub fn lod_level_index_for_distance_with_policy(
    shape: &ShapeFile,
    distance_m: f32,
    policy: LodPolicy,
) -> usize {
    let Some(control) = shape.lod_controls.first() else {
        return 0;
    };
    lod_level_index_for_control(shape, control, distance_m, policy)
}

/// Build mesh choosing LOD from camera distance (m) to the shape origin.
pub fn build_mesh_from_shape_at_distance(shape: &ShapeFile, distance_m: f32) -> Option<Mesh> {
    let levels = lod_levels_for_distance(shape, distance_m, LodPolicy::default());
    if levels.is_empty() {
        return build_mesh_from_shape(shape);
    }
    if levels.len() == 1 {
        return build_mesh_from_shape_lod(shape, levels[0]);
    }
    let mut buffers = MeshBuffers::default();
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    for level in levels {
        for sub in &level.sub_objects {
            for prim in &sub.primitives {
                append_primitive_mesh_buffers(
                    shape,
                    level,
                    sub,
                    prim,
                    default_normal,
                    &mut buffers,
                    None,
                    false,
                );
            }
        }
    }
    buffers.into_mesh()
}

/// Build mesh parts choosing LOD from camera distance (m) to the shape origin.
pub fn build_mesh_parts_from_shape_at_distance(
    shape: &ShapeFile,
    distance_m: f32,
) -> Vec<LoadedShapePart> {
    build_mesh_parts_from_shape_at_distance_with_options(
        shape,
        distance_m,
        MeshPartBuildOptions::default(),
    )
}

/// LOD-aware parts with [`MeshPartBuildOptions`] (sub-objects / anim bake).
pub fn build_mesh_parts_from_shape_at_distance_with_options(
    shape: &ShapeFile,
    distance_m: f32,
    options: MeshPartBuildOptions,
) -> Vec<LoadedShapePart> {
    let levels = lod_levels_for_distance(shape, distance_m, LodPolicy::default());
    if levels.is_empty() {
        return build_mesh_parts_from_shape(shape);
    }
    let mut parts = Vec::new();
    for level in levels {
        parts.extend(build_mesh_parts_from_shape_lod_with_options(
            shape, level, options,
        ));
    }
    parts
}

/// Merge geometry from every `lod_control` at sorted level band `band_idx` (#97).
///
/// `band_idx` 0 = finest declared band across controls; higher = coarser.
pub fn build_mesh_parts_for_lod_band(
    shape: &ShapeFile,
    band_idx: usize,
    options: MeshPartBuildOptions,
) -> Vec<LoadedShapePart> {
    let mut parts = Vec::new();
    for control in &shape.lod_controls {
        let sorted = sorted_level_indices(control);
        if sorted.is_empty() {
            continue;
        }
        let pos = band_idx.min(sorted.len() - 1);
        let level = &control.distance_levels[sorted[pos]];
        parts.extend(build_mesh_parts_from_shape_lod_with_options(
            shape, level, options,
        ));
    }
    parts
}

/// Number of LOD bands for WORLD asset caching (max levels across controls).
pub fn lod_band_count(shape: &ShapeFile) -> usize {
    shape
        .lod_controls
        .iter()
        .map(|c| c.distance_levels.len())
        .max()
        .unwrap_or(0)
}

/// Options used by render3d WORLD spawn: keep night sub-objects + bake anim key 0.
pub fn render3d_world_mesh_options() -> MeshPartBuildOptions {
    MeshPartBuildOptions {
        keep_sub_objects: true,
        bake_animation_key: Some(0.0),
    }
}

/// Write a baked Bevy mesh as Wavefront OBJ (positions, UVs, normals, triangle list).
pub fn write_mesh_wavefront(
    mesh: &Mesh,
    w: &mut dyn std::io::Write,
    obj_name: &str,
) -> std::io::Result<()> {
    use bevy::mesh::VertexAttributeValues;

    writeln!(w, "# openrailsrs shape-obj-dump")?;
    writeln!(w, "o {obj_name}")?;

    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|a| match a {
            VertexAttributeValues::Float32x3(v) => Some(v.as_slice()),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing positions"))?;
    for p in positions {
        writeln!(w, "v {} {} {}", p[0], p[1], p[2])?;
    }

    let uvs = mesh.attribute(Mesh::ATTRIBUTE_UV_0).and_then(|a| match a {
        VertexAttributeValues::Float32x2(v) => Some(v.as_slice()),
        _ => None,
    });
    if let Some(uvs) = uvs {
        for uv in uvs {
            writeln!(w, "vt {} {}", uv[0], uv[1])?;
        }
    }

    let normals = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(|a| match a {
            VertexAttributeValues::Float32x3(v) => Some(v.as_slice()),
            _ => None,
        });
    if let Some(normals) = normals {
        for n in normals {
            writeln!(w, "vn {} {} {}", n[0], n[1], n[2])?;
        }
    }

    let has_uv = uvs.is_some();
    let has_n = normals.is_some();
    let n = positions.len();
    for i in (0..n).step_by(3) {
        let a = i + 1;
        let b = i + 2;
        let c = i + 3;
        if has_uv && has_n {
            writeln!(w, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
        } else if has_uv {
            writeln!(w, "f {a}/{a} {b}/{b} {c}/{c}")?;
        } else {
            writeln!(w, "f {a} {b} {c}")?;
        }
    }
    Ok(())
}

/// Bake an MSTS `.s` shape (Bevy path: coord flip + UV conversion) and write OBJ.
///
/// Enables [`set_train_shape_debug_scope`] so `OPENRAILSRS_DEBUG_*` UV/winding flags apply.
pub fn write_shape_wavefront_from_path(
    shape_path: &std::path::Path,
    out_path: &std::path::Path,
    distance_m: Option<f32>,
) -> Result<(), String> {
    use std::io::Write;

    set_train_shape_debug_scope(true);
    let result = (|| {
        let shape = openrailsrs_formats::ShapeFile::from_path(shape_path)
            .map_err(|e| format!("parse {}: {e}", shape_path.display()))?;
        let mesh = match distance_m {
            Some(d) => build_mesh_from_shape_at_distance(&shape, d),
            None => build_mesh_from_shape(&shape),
        }
        .ok_or_else(|| format!("no mesh geometry in {}", shape_path.display()))?;

        let mut file = std::fs::File::create(out_path)
            .map_err(|e| format!("create {}: {e}", out_path.display()))?;
        write_mesh_wavefront(
            &mesh,
            &mut file,
            shape_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("shape"),
        )
        .map_err(|e| format!("write {}: {e}", out_path.display()))?;
        file.flush()
            .map_err(|e| format!("flush {}: {e}", out_path.display()))?;
        Ok(())
    })();
    set_train_shape_debug_scope(false);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{
        DistanceLevel, LodControl, NamedMatrix, PrimState, Primitive, SubObject, Vertex, VtxState,
    };

    fn identity_matrix() -> Matrix43 {
        Matrix43 {
            rows: [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
            ],
        }
    }

    fn unit_quad_shape(with_normals: bool) -> ShapeFile {
        unit_triangle_shape(with_normals, false)
    }

    /// Unit right triangle in XY (MSTS Z+ face); optional authored normals and UVs.
    fn unit_triangle_shape(with_normals: bool, with_uvs: bool) -> ShapeFile {
        let normals = if with_normals {
            vec![ShapeVec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }]
        } else {
            Vec::new()
        };
        let normal_idx = if with_normals { 0 } else { -1 };
        let uvs = if with_uvs {
            vec![
                openrailsrs_formats::Vec2 { u: 0.0, v: 0.0 },
                openrailsrs_formats::Vec2 { u: 1.0, v: 0.0 },
                openrailsrs_formats::Vec2 { u: 0.0, v: 1.0 },
            ]
        } else {
            Vec::new()
        };
        let uv_for = |i: i32| {
            if with_uvs { vec![i] } else { Vec::new() }
        };
        ShapeFile {
            points: vec![
                ShapeVec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                ShapeVec3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                ShapeVec3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            ],
            normals,
            uvs,
            prim_states: vec![PrimState {
                shader_idx: 0,
                vertex_state_idx: 0,
                ..Default::default()
            }],
            vtx_states: vec![VtxState {
                matrix_idx: 0,
                ..Default::default()
            }],
            matrices: vec![NamedMatrix {
                name: "MAIN".into(),
                matrix: identity_matrix(),
            }],
            lod_controls: vec![LodControl {
                distance_levels: vec![DistanceLevel {
                    selection_m: 200.0,
                    hierarchy: vec![-1],
                    sub_objects: vec![SubObject {
                        vertex_count: 3,
                        vertices: vec![
                            Vertex {
                                point_idx: 0,
                                normal_idx,
                                uv_indices: uv_for(0),
                                ..Default::default()
                            },
                            Vertex {
                                point_idx: 1,
                                normal_idx,
                                uv_indices: uv_for(1),
                                ..Default::default()
                            },
                            Vertex {
                                point_idx: 2,
                                normal_idx,
                                uv_indices: uv_for(2),
                                ..Default::default()
                            },
                        ],
                        primitives: vec![Primitive {
                            prim_state_idx: 0,
                            vertex_indices: vec![0, 1, 2],
                        }],
                    }],
                }],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn parts_preserve_file_order_sort_index_not_btree_key_order() {
        // Prim states 1 then 0 in file order — BTreeMap would emit 0 before 1.
        let mut shape = unit_quad_shape(true);
        shape.prim_states = vec![
            PrimState {
                shader_idx: 0,
                vertex_state_idx: 0,
                ..Default::default()
            },
            PrimState {
                shader_idx: 0,
                vertex_state_idx: 0,
                ..Default::default()
            },
        ];
        shape.lod_controls[0].distance_levels[0].sub_objects[0].primitives = vec![
            Primitive {
                prim_state_idx: 1,
                vertex_indices: vec![0, 1, 2],
            },
            Primitive {
                prim_state_idx: 0,
                vertex_indices: vec![0, 1, 2],
            },
        ];
        let parts = build_mesh_parts_from_shape(&shape);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].prim_state_idx, 1);
        assert_eq!(parts[0].sort_index, 0);
        assert_eq!(parts[1].prim_state_idx, 0);
        assert_eq!(parts[1].sort_index, 1);
        assert!(parts[0].sort_index < parts[1].sort_index);
    }

    #[test]
    fn missing_normals_get_face_normal_after_z_flip() {
        let shape = unit_quad_shape(false);
        let parts = build_mesh_parts_from_shape(&shape);
        assert_eq!(parts.len(), 1);
        let normals = parts[0]
            .mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|a| match a {
                VertexAttributeValues::Float32x3(v) => Some(v.as_slice()),
                _ => None,
            })
            .expect("normals");
        assert_eq!(normals.len(), 3);
        for n in normals {
            // MSTS (0,0,1) → Bevy (0,0,-1); face normal after winding swap matches.
            assert!(
                (n[2] + 1.0).abs() < 1e-4,
                "expected -Z face normal, got {n:?}"
            );
            assert!(n[0].abs() < 1e-4 && n[1].abs() < 1e-4);
        }
    }

    #[test]
    fn authored_normals_are_preserved() {
        let shape = unit_quad_shape(true);
        let parts = build_mesh_parts_from_shape(&shape);
        let normals = parts[0]
            .mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|a| match a {
                VertexAttributeValues::Float32x3(v) => Some(v.as_slice()),
                _ => None,
            })
            .expect("normals");
        for n in normals {
            assert!(
                (n[2] + 1.0).abs() < 1e-4,
                "authored Z+ became Bevy -Z: {n:?}"
            );
        }
    }

    #[test]
    fn degenerate_normal_falls_back_to_face() {
        let mut shape = unit_quad_shape(true);
        shape.normals[0] = ShapeVec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let parts = build_mesh_parts_from_shape(&shape);
        let normals = parts[0]
            .mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|a| match a {
                VertexAttributeValues::Float32x3(v) => Some(v.as_slice()),
                _ => None,
            })
            .expect("normals");
        for n in normals {
            assert!((n[2] + 1.0).abs() < 1e-4, "zero normal → face: {n:?}");
        }
    }

    #[test]
    fn classic_shape_mesh_has_no_tangents() {
        let shape = unit_triangle_shape(true, true);
        let parts = build_mesh_parts_from_shape(&shape);
        assert!(
            parts[0].mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_none(),
            "MSTS path must not generate tangents by default"
        );
    }

    #[test]
    fn ensure_tangents_on_uv_triangle_are_finite() {
        let shape = unit_triangle_shape(true, true);
        let parts = build_mesh_parts_from_shape(&shape);
        let mut mesh = parts[0].mesh.clone();
        assert!(ensure_tangents_for_normal_mapping(&mut mesh));
        let tangents = mesh
            .attribute(Mesh::ATTRIBUTE_TANGENT)
            .and_then(|a| match a {
                VertexAttributeValues::Float32x4(v) => Some(v.as_slice()),
                _ => None,
            })
            .expect("ATTRIBUTE_TANGENT");
        assert_eq!(tangents.len(), 3);
        for t in tangents {
            assert!(t.iter().all(|c| c.is_finite()), "tangent {t:?}");
            let xyz = Vec3::new(t[0], t[1], t[2]);
            assert!((xyz.length() - 1.0).abs() < 1e-2, "unit tangent {t:?}");
            // Handedness / bitangent sign stored in W must be ±1.
            assert!(
                (t[3].abs() - 1.0).abs() < 1e-3,
                "expected |w|≈1, got {}",
                t[3]
            );
        }
        // Idempotent.
        assert!(ensure_tangents_for_normal_mapping(&mut mesh));
    }

    fn two_level_shape() -> ShapeFile {
        let mut shape = unit_quad_shape(true);
        shape.view_sphere_radius = 50.0;
        let fine = shape.lod_controls[0].distance_levels[0].clone();
        let mut coarse = fine.clone();
        coarse.selection_m = 2000.0;
        shape.lod_controls[0].distance_levels = vec![fine, coarse];
        shape
    }

    #[test]
    fn lod_bias_100_selects_finest_level() {
        let shape = two_level_shape();
        let policy = LodPolicy {
            bias: 100,
            viewing_extension: true,
            viewing_distance_m: 2000.0,
        };
        let idx = lod_level_index_for_distance_with_policy(&shape, 1500.0, policy);
        assert_eq!(idx, 0, "LODBias=100 must force highest detail");
    }

    #[test]
    fn lod_policy_picks_coarser_level_at_long_range() {
        let shape = two_level_shape();
        let policy = LodPolicy {
            bias: 0,
            viewing_extension: true,
            viewing_distance_m: 4000.0,
        };
        let near = lod_level_index_for_distance_with_policy(&shape, 100.0, policy);
        let far = lod_level_index_for_distance_with_policy(&shape, 2500.0, policy);
        assert_eq!(near, 0);
        assert_eq!(far, 1);
    }

    #[test]
    fn multi_lod_control_builds_parts_from_all_controls() {
        let mut shape = unit_quad_shape(true);
        let second = shape.lod_controls[0].clone();
        shape.lod_controls.push(second);
        assert_eq!(closest_lod_levels(&shape).len(), 2);
        let parts = build_mesh_parts_from_shape(&shape);
        // Two controls × one prim_state each → two parts (same prim idx, both kept).
        assert!(
            parts.len() >= 2,
            "expected geometry from both lod_controls, got {}",
            parts.len()
        );
    }
}
