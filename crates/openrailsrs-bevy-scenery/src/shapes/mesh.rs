//! MSTS `.s` shape → Bevy mesh builders (shared; cab-specific paths stay in viewer3d).

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use openrailsrs_formats::{DistanceLevel, Matrix43, ShapeFile, Vec3 as ShapeVec3};

use openrailsrs_or_shader::coordinates::{
    matrix43_transform_point_xna, matrix43_transform_vector_xna, shape_point_to_bevy,
};

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
    /// Baked mesh AABB (cab CVF proximity filter).
    pub bounds_center: Option<Vec3>,
    pub bounds_half_extent: Option<Vec3>,
    /// Cab lever rotates about mesh center instead of matrix pivot (far 3D wheel).
    pub lever_pivot_at_mesh_center: bool,
    /// Override local rotation axis for fallback lever animation.
    pub lever_local_axis: Option<Vec3>,
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
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    let mut parts: BTreeMap<i32, MeshBuffers> = BTreeMap::new();

    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            let buffers = parts.entry(prim.prim_state_idx).or_default();
            append_primitive_mesh_buffers(
                shape,
                level,
                sub,
                prim,
                default_normal,
                buffers,
                None,
                false,
            );
        }
    }

    parts
        .into_iter()
        .filter_map(|(prim_state_idx, buffers)| {
            let (mesh, solid_color) = buffers.into_mesh_with_color()?;
            let (alpha_test_mode, z_bias, z_buf_mode) = shape
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
            Some(LoadedShapePart {
                prim_state_idx,
                sub_object_idx: u32::MAX,
                cab_matrix_idx: None,
                mesh,
                texture_file: texture_for_prim_state(shape, prim_state_idx),
                shader_name: shader_name_for_prim_state(shape, prim_state_idx),
                solid_color,
                alpha_test_mode,
                z_bias,
                z_buf_mode,
                light_mat_idx: light_mat_idx_for_prim_state(shape, prim_state_idx),
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
    let start = chain_start.unwrap_or_else(|| {
        shape
            .prim_states
            .get(prim.prim_state_idx.max(0) as usize)
            .and_then(|ps| shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize))
            .map(|vs| vs.matrix_idx)
            .unwrap_or(0)
    });
    let matrix_chain = primitive_matrix_chain_bake(shape, level, start, omit_leaf_matrix);
    for tri in prim.vertex_indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        for &vertex_idx in tri {
            let Some((point_idx, normal_idx, uv_idx, vertex_color)) =
                resolve_shape_vertex(shape, sub, vertex_idx)
            else {
                continue;
            };
            let Some(point) = shape.points.get(point_idx) else {
                continue;
            };
            buffers.positions.push(transform_shape_point(
                shape_point_to_bevy(*point),
                &matrix_chain,
            ));
            let normal = normal_idx
                .and_then(|idx| shape.normals.get(idx).copied())
                .unwrap_or(default_normal);
            buffers.normals.push(transform_shape_normal(
                shape_point_to_bevy(normal),
                &matrix_chain,
            ));
            let uv = uv_idx
                .and_then(|idx| shape.uvs.get(idx).copied())
                .unwrap_or_default();
            // MSTS UV origin differs from Bevy; flip V for textured quads.
            buffers.uvs.push(Vec2::new(uv.u as f32, 1.0 - uv.v as f32));
            buffers.colors.push(vertex_color);
        }
    }
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
    let mut out = Vec::new();
    let mut matrix_idx = chain_start;
    let mut guard = 0usize;
    while matrix_idx >= 0 && guard < shape.matrices.len() {
        let idx = matrix_idx as usize;
        let Some(matrix) = shape.matrices.get(idx) else {
            break;
        };
        out.push(ShapeMatrixRef {
            matrix: &matrix.matrix,
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
        .map(|vs| vs.lighting_model)
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

/// Index of the distance level chosen for `distance_m` (0 = finest declared LOD).
pub fn lod_level_index_for_distance(shape: &ShapeFile, distance_m: f32) -> usize {
    let Some(control) = shape.lod_controls.first() else {
        return 0;
    };
    let levels = &control.distance_levels;
    if levels.is_empty() {
        return 0;
    }
    let mut best_idx = 0usize;
    for (i, lvl) in levels.iter().enumerate() {
        if (lvl.selection_m as f32) <= distance_m && lvl.selection_m >= levels[best_idx].selection_m
        {
            best_idx = i;
        }
    }
    best_idx
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
