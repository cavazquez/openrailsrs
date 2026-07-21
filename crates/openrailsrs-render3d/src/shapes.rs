//! Capa 4b: mallas reales de los shapes `.s`.
//!
//! Reutiliza el parser de datos `openrailsrs_formats::ShapeFile` y construye
//! aquí la geometría Bevy (LOD de mayor detalle), aplicando la jerarquía de
//! matrices y la conversión XNA. Una parte de malla por `prim_state` (textura).
//!
//! Los shapes animados (p. ej. columnas de agua `Pickup`) se evalúan en el
//! fotograma 0 de su primera animación, igual que Open Rails en reposo.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::*;
use openrailsrs_formats::{AnimController, DistanceLevel, Matrix43, ShapeFile, Vec3 as ShapeVec3};

use openrailsrs_bevy_scenery::shapes::{light_mat_idx_for_prim_state, shape_normal_is_usable};
use openrailsrs_or_shader::coordinates::{
    matrix43_transform_point_xna, matrix43_transform_vector_xna, shape_point_to_bevy,
};

/// Una parte de malla de un shape (todos los triángulos que comparten textura).
#[derive(Clone, Debug)]
pub struct ShapePart {
    /// Índice del sub-objeto MSTS dentro del LOD (0 = principal).
    pub sub_object_idx: u32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Nombre del archivo de textura (`TEXTURES/*.ace`), si la parte tiene.
    pub texture: Option<String>,
    /// Modo alpha MSTS (`prim_state.alpha_test_mode`); -1 = derivado de flags.
    pub alpha_test_mode: i32,
    /// Nombre del shader MSTS de este prim_state (ej. "AddATex", "BlendATex", "TexDiff").
    pub shader_name: Option<String>,
    /// OR `vtx_state.LightMatIdx` (Specular25/750, HalfBright, …).
    pub light_mat_idx: Option<i32>,
    /// OR first `uv_op.TexAddrMode` (1=Wrap, 2=Mirror, 3=Clamp, 4=Border).
    pub tex_addr_mode: Option<i32>,
    /// Color por vértice (RGBA lineal) cuando el shape no tiene textura.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Color uniforme si todos los vértices comparten el mismo tono.
    pub solid_color: Option<[f32; 3]>,
}

/// Bucket de LOD para la clave de caché de mallas (evita recomputar por metro).
pub fn lod_cache_key(distance_m: f32) -> u32 {
    if distance_m <= 200.0 {
        0
    } else if distance_m <= 800.0 {
        1
    } else {
        2
    }
}

/// Carga un `.s` con el LOD apropiado para la distancia a cámara (metros).
pub fn load_shape_parts_at_distance(path: &Path, distance_m: f32) -> Option<Vec<ShapePart>> {
    let shape = ShapeFile::from_path(path).ok()?;
    let level = lod_level_for_distance(&shape, distance_m).or_else(|| closest_lod(&shape))?;
    Some(build_parts_for_level(&shape, level))
}

/// Carga un `.s` y construye sus partes de malla (LOD de mayor detalle).
#[allow(dead_code)]
pub fn load_shape_parts(path: &Path) -> Option<Vec<ShapePart>> {
    load_shape_parts_at_distance(path, 0.0)
}

#[cfg(test)]
fn shape_local_bounds(parts: &[ShapePart]) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for part in parts {
        for v in &part.positions {
            let p = Vec3::from_array(*v);
            min = min.min(p);
            max = max.max(p);
        }
    }
    min.x.is_finite().then_some((min, max))
}

fn closest_lod(shape: &ShapeFile) -> Option<&DistanceLevel> {
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

/// LOD para una distancia de cámara: el nivel más fino cuyo `selection_m` ≤ `distance_m`.
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

#[derive(Default)]
struct PartBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
}

/// Construye una parte de malla por `prim_state` del LOD de mayor detalle.
#[allow(dead_code)]
pub fn build_parts(shape: &ShapeFile) -> Vec<ShapePart> {
    let Some(level) = closest_lod(shape) else {
        return Vec::new();
    };
    build_parts_for_level(shape, level)
}

fn build_parts_for_level(shape: &ShapeFile, level: &DistanceLevel) -> Vec<ShapePart> {
    let pose = animation_pose_matrices(shape, 0.0);
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });

    let mut by_state: BTreeMap<(u32, i32), PartBuffers> = BTreeMap::new();
    for (sub_idx, sub) in level.sub_objects.iter().enumerate() {
        for prim in &sub.primitives {
            let chain = matrix_chain(shape, level, prim.prim_state_idx, &pose);
            let buf = by_state
                .entry((sub_idx as u32, prim.prim_state_idx))
                .or_default();
            for tri in prim.vertex_indices.chunks(3) {
                if tri.len() < 3 {
                    continue;
                }
                let mut resolved = Vec::with_capacity(3);
                let mut skip = false;
                for &vidx in tri {
                    let Some(v) = resolve_vertex(shape, sub, vidx) else {
                        skip = true;
                        break;
                    };
                    if shape.points.get(v.0).is_none() {
                        skip = true;
                        break;
                    }
                    resolved.push(v);
                }
                if skip {
                    continue;
                }
                // Face normals follow this builder's current winding (#56 will unify with bevy-scenery).
                let positions: [Vec3; 3] = std::array::from_fn(|i| {
                    let (pi, ..) = resolved[i];
                    let point = shape.points.get(pi).expect("checked");
                    transform_point(shape_point_to_bevy(*point), &chain)
                });
                let face_n = (positions[1] - positions[0])
                    .cross(positions[2] - positions[0])
                    .try_normalize()
                    .unwrap_or(Vec3::ZERO);
                let fallback_n = if shape_normal_is_usable(default_normal) {
                    transform_normal(shape_point_to_bevy(default_normal), &chain)
                } else {
                    face_n
                };
                for ((pi, ni, ui, vertex_color), pos) in resolved.into_iter().zip(positions) {
                    let _ = pi;
                    let authored = ni
                        .and_then(|i| shape.normals.get(i).copied())
                        .filter(|n| shape_normal_is_usable(*n));
                    let nrm = if let Some(n) = authored {
                        transform_normal(shape_point_to_bevy(n), &chain)
                    } else if face_n.length_squared() > 0.0 {
                        face_n
                    } else {
                        fallback_n
                    };
                    let uv = ui
                        .and_then(|i| shape.uvs.get(i))
                        .copied()
                        .unwrap_or_default();
                    buf.positions.push(pos.to_array());
                    buf.normals.push(nrm.to_array());
                    buf.uvs.push([uv.u as f32, 1.0 - uv.v as f32]);
                    buf.colors.push(vertex_color);
                }
            }
        }
    }

    by_state
        .into_iter()
        .filter(|(_, b)| !b.positions.is_empty())
        .map(|((sub_object_idx, prim_state_idx), b)| {
            let (colors, solid_color) = part_vertex_colors(&b.colors);
            ShapePart {
                sub_object_idx,
                positions: b.positions,
                normals: b.normals,
                uvs: b.uvs,
                texture: texture_for_prim_state(shape, prim_state_idx),
                alpha_test_mode: shape
                    .prim_states
                    .get(prim_state_idx.max(0) as usize)
                    .map(|ps| ps.alpha_test_mode)
                    .unwrap_or(-1),
                shader_name: shape
                    .prim_states
                    .get(prim_state_idx.max(0) as usize)
                    .and_then(|ps| shape.shader_names.get(ps.shader_idx.max(0) as usize))
                    .cloned(),
                light_mat_idx: light_mat_idx_for_prim_state(shape, prim_state_idx),
                tex_addr_mode: shape.tex_addr_mode_for_prim_state(prim_state_idx),
                colors,
                solid_color,
            }
        })
        .collect()
}

/// Sub-objeto nocturno (índice 1) oculto de día cuando el `.sd` declara `ESD_SubObj`.
pub fn part_visible(
    descriptor: &crate::shape_descriptor::ShapeDescriptor,
    part: &ShapePart,
    env: &crate::textures::TextureEnvironment,
) -> bool {
    !(descriptor.has_night_subobj && part.sub_object_idx == 1 && env.is_day())
}

/// Matrices de pose del shape en un fotograma de animación (Open Rails `AnimateMatrix`).
fn animation_pose_matrices(shape: &ShapeFile, key: f32) -> Vec<Matrix43> {
    let mut pose: Vec<Matrix43> = shape.matrices.iter().map(|m| m.matrix).collect();
    let Some(anim) = shape.animations.first() else {
        return pose;
    };

    for (i, node) in anim.nodes.iter().enumerate() {
        if node.controllers.is_empty() || i >= pose.len() {
            continue;
        }
        pose[i] = animate_matrix(pose[i], &node.controllers, key);
    }
    pose
}

fn animate_matrix(base: Matrix43, controllers: &[AnimController], key: f32) -> Matrix43 {
    let mut m = base;
    for controller in controllers {
        m = apply_controller(m, controller, key);
    }
    m
}

fn apply_controller(mut m: Matrix43, controller: &AnimController, key: f32) -> Matrix43 {
    match controller {
        AnimController::LinearPos { keys } => {
            let Some((frame1, p1, frame2, p2)) = bracket_keys(keys, key) else {
                return m;
            };
            let t = if (frame2 - frame1).abs() > 1e-6 {
                ((key - frame1) / (frame2 - frame1)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let pos = lerp3(p1, p2, t);
            set_matrix_translation(&mut m, pos);
            m
        }
        AnimController::SlerpRot { keys } | AnimController::TcbRot { keys } => {
            let Some((frame1, q1, frame2, q2)) = bracket_quat_keys(keys, key) else {
                return m;
            };
            let t = if (frame2 - frame1).abs() > 1e-6 {
                ((key - frame1) / (frame2 - frame1)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let translation = matrix_translation(m);
            let q = msts_quat_to_bevy(q1).slerp(msts_quat_to_bevy(q2), t);
            set_matrix_rotation(&mut m, q);
            set_matrix_translation(&mut m, translation);
            m
        }
    }
}

fn bracket_keys(keys: &[(f32, [f32; 3])], key: f32) -> Option<(f32, [f32; 3], f32, [f32; 3])> {
    if keys.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for (i, (frame, _)) in keys.iter().enumerate() {
        if *frame <= key {
            index = i;
        } else {
            break;
        }
    }
    let (frame1, p1) = keys[index];
    let (frame2, p2) = keys.get(index + 1).copied().unwrap_or(keys[index]);
    Some((frame1, p1, frame2, p2))
}

fn bracket_quat_keys(keys: &[(f32, [f32; 4])], key: f32) -> Option<(f32, [f32; 4], f32, [f32; 4])> {
    if keys.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for (i, (frame, _)) in keys.iter().enumerate() {
        if *frame <= key {
            index = i;
        } else {
            break;
        }
    }
    let (frame1, q1) = keys[index];
    let (frame2, q2) = keys.get(index + 1).copied().unwrap_or(keys[index]);
    Some((frame1, q1, frame2, q2))
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn msts_quat_to_bevy(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[1], -q[2], q[3])
}

fn matrix_translation(m: Matrix43) -> [f32; 3] {
    let d = m.rows[3];
    [d[0] as f32, d[1] as f32, d[2] as f32]
}

fn set_matrix_translation(m: &mut Matrix43, pos: [f32; 3]) {
    m.rows[3] = [pos[0] as f64, pos[1] as f64, pos[2] as f64];
}

fn set_matrix_rotation(m: &mut Matrix43, q: Quat) {
    let m3 = Mat3::from_quat(q);
    m.rows[0] = [m3.x_axis.x as f64, m3.x_axis.y as f64, m3.x_axis.z as f64];
    m.rows[1] = [m3.y_axis.x as f64, m3.y_axis.y as f64, m3.y_axis.z as f64];
    m.rows[2] = [m3.z_axis.x as f64, m3.z_axis.y as f64, m3.z_axis.z as f64];
}

#[derive(Clone, Copy)]
struct MatrixRef {
    matrix: Matrix43,
    zero_translation: bool,
}

fn matrix_chain(
    shape: &ShapeFile,
    level: &DistanceLevel,
    prim_state_idx: i32,
    pose: &[Matrix43],
) -> Vec<MatrixRef> {
    let Some(prim_state) = shape.prim_states.get(prim_state_idx.max(0) as usize) else {
        return Vec::new();
    };
    let Some(vtx_state) = shape
        .vtx_states
        .get(prim_state.vertex_state_idx.max(0) as usize)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut matrix_idx = vtx_state.matrix_idx;
    let mut guard = 0usize;
    while matrix_idx >= 0 && guard < shape.matrices.len() {
        let idx = matrix_idx as usize;
        let Some(matrix) = pose
            .get(idx)
            .copied()
            .or_else(|| shape.matrices.get(idx).map(|m| m.matrix))
        else {
            break;
        };
        out.push(MatrixRef {
            matrix,
            zero_translation: idx == 0 && level.hierarchy.first().copied() == Some(-1),
        });
        matrix_idx = level.hierarchy.get(idx).copied().unwrap_or(-1);
        guard += 1;
    }
    out
}

fn transform_point(mut p: Vec3, chain: &[MatrixRef]) -> Vec3 {
    for m in chain {
        p = matrix43_transform_point_xna(&m.matrix, p, m.zero_translation);
    }
    p
}

fn transform_normal(mut n: Vec3, chain: &[MatrixRef]) -> Vec3 {
    for m in chain {
        n = matrix43_transform_vector_xna(&m.matrix, n);
    }
    n.try_normalize().unwrap_or(Vec3::Y)
}

#[allow(clippy::type_complexity)]
fn resolve_vertex(
    shape: &ShapeFile,
    sub: &openrailsrs_formats::SubObject,
    vertex_idx: u32,
) -> Option<(usize, Option<usize>, Option<usize>, [f32; 4])> {
    if let Some(v) = sub.vertices.get(vertex_idx as usize) {
        return Some((
            idx_to_usize(v.point_idx)?,
            idx_to_usize(v.normal_idx),
            v.uv_indices.first().copied().and_then(idx_to_usize),
            v.color1.map(rgba_u8_to_f32).unwrap_or([1.0, 1.0, 1.0, 1.0]),
        ));
    }
    let idx = vertex_idx as usize;
    if idx < shape.points.len() {
        return Some((idx, Some(idx), Some(idx), [1.0, 1.0, 1.0, 1.0]));
    }
    None
}

fn rgba_u8_to_f32([r, g, b, a]: [u8; 4]) -> [f32; 4] {
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}

fn part_vertex_colors(colors: &[[f32; 4]]) -> (Option<Vec<[f32; 4]>>, Option<[f32; 3]>) {
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

fn color_is_meaningful(c: &[f32; 4]) -> bool {
    (c[0] - 1.0).abs() > 0.02 || (c[1] - 1.0).abs() > 0.02 || (c[2] - 1.0).abs() > 0.02
}

fn colors_close(a: &[f32; 4], b: &[f32; 4]) -> bool {
    (a[0] - b[0]).abs() < 0.02
        && (a[1] - b[1]).abs() < 0.02
        && (a[2] - b[2]).abs() < 0.02
        && (a[3] - b[3]).abs() < 0.05
}

fn idx_to_usize(idx: i32) -> Option<usize> {
    (idx >= 0).then_some(idx as usize)
}

fn texture_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
    shape
        .texture_for_prim_state_idx(prim_state_idx)
        .or_else(|| fallback_shape_texture(shape, prim_state_idx))
}

/// Heurísticas cuando el `prim_state` no declara `tex_idxs` (paridad OR + extensiones).
fn fallback_shape_texture(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
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

fn shader_requests_texture(shape: &ShapeFile, ps: &openrailsrs_formats::PrimState) -> bool {
    shape
        .shader_names
        .get(ps.shader_idx.max(0) as usize)
        .is_some_and(|name| {
            let n = name.to_ascii_lowercase();
            // Open Rails SceneryShader names that expect a texture stage.
            matches!(
                n.as_str(),
                "tex" | "texdiff" | "blendatex" | "blendatexdiff" | "addatex" | "addatexdiff"
            ) || n.contains("tex")
                || n.contains("blend")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    /// Resumen de partes con/sin textura (diagnóstico).
    #[derive(Debug, Default, Clone)]
    struct ShapeTextureAudit {
        total_parts: u32,
        textured_parts: u32,
        untextured_parts: u32,
        untextured_with_shape_textures: u32,
        untextured_no_shape_textures: u32,
    }

    fn audit_shape_textures(shape: &ShapeFile) -> ShapeTextureAudit {
        let parts = build_parts(shape);
        let mut audit = ShapeTextureAudit {
            total_parts: parts.len() as u32,
            ..Default::default()
        };
        let has_shape_textures = !shape.texture_filenames.is_empty();
        for part in parts {
            if part.texture.is_some() {
                audit.textured_parts += 1;
                continue;
            }
            audit.untextured_parts += 1;
            if has_shape_textures {
                audit.untextured_with_shape_textures += 1;
            } else {
                audit.untextured_no_shape_textures += 1;
            }
        }
        audit
    }

    fn chiltern_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    fn water_column_path() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from(
                "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/SHAPES/RF_GW_WaterColumn.s",
            ),
            chiltern_dir().join("SHAPES/RF_GW_WaterColumn.s"),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    #[test]
    fn lod_cache_key_buckets_distance() {
        assert_eq!(lod_cache_key(50.0), 0);
        assert_eq!(lod_cache_key(200.0), 0);
        assert_eq!(lod_cache_key(201.0), 1);
        assert_eq!(lod_cache_key(800.0), 1);
        assert_eq!(lod_cache_key(801.0), 2);
    }

    #[test]
    fn chiltern_shape_builds_nonempty_mesh() {
        let dir = chiltern_dir();
        let shapes_dir = dir.join("SHAPES");
        let Ok(rd) = std::fs::read_dir(&shapes_dir) else {
            eprintln!("skip: SHAPES de Chiltern no disponible");
            return;
        };
        let Some(shape_path) = rd
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("s")))
        else {
            eprintln!("skip: no hay archivos .s");
            return;
        };

        let parts = load_shape_parts(&shape_path).expect("parsear shape");
        let total: usize = parts.iter().map(|p| p.positions.len()).sum();
        assert!(total > 0, "el shape {shape_path:?} no produjo vértices");
        for p in &parts {
            assert_eq!(p.normals.len(), p.positions.len());
            assert_eq!(p.uvs.len(), p.positions.len());
            assert!(p.positions.len() % 3 == 0, "triángulos completos");
            assert!(p.positions.iter().all(|v| v.iter().all(|c| c.is_finite())));
        }
    }

    /// Las columnas de agua GWR deben apoyarse en Y≈0 local tras el fotograma 0.
    #[test]
    fn water_column_rest_pose_grounded() {
        let Some(path) = water_column_path() else {
            eprintln!("skip: RF_GW_WaterColumn.s no disponible");
            return;
        };
        let parts = load_shape_parts(&path).expect("load water column");
        let (min, max) = shape_local_bounds(&parts).expect("bounds");
        eprintln!("water column bounds min={min:?} max={max:?}");
        assert!(
            min.y < 0.5,
            "la base debería estar cerca del suelo (min_y={})",
            min.y
        );
        assert!(max.y < 8.0, "altura razonable (max_y={})", max.y);
    }

    #[test]
    fn water_column_resolves_shape_and_texture() {
        let dir = chiltern_dir();
        let tex = dir.join("TEXTURES/RFwatercolumn.ace");
        assert!(tex.is_file(), "RFwatercolumn.ace en ruta local");
        let Some(path) = water_column_path() else {
            return;
        };
        let parts = load_shape_parts(&path).expect("shape");
        assert!(parts.iter().all(|p| p.texture.is_some()));
        let ace = openrailsrs_ace::read_ace(&tex).expect("ace");
        assert!(ace.width > 0);
    }

    #[test]
    fn chiltern_untextured_parts_audit() {
        use crate::objects::{load_objects, object_wants_shape_mesh};
        use crate::terrain::load_tile_geometry;
        use crate::textures::{resolve_shape_path_in_dirs, shape_search_dirs};

        let dir = chiltern_dir();
        let (tx, tz) = (-6082, 14925);
        let loaded = load_tile_geometry(&dir, tx, tz).expect("tile");
        let base = loaded.height.base_y();
        let markers = load_objects(&dir, tx, tz, base);
        let shape_dirs = shape_search_dirs(&dir, &dir);

        let mut seen = HashSet::new();
        let mut total = ShapeTextureAudit::default();
        let mut top_shapes: HashMap<String, u32> = HashMap::new();

        for m in markers.iter().filter(|m| object_wants_shape_mesh(m)) {
            let Some(file) = &m.file_name else { continue };
            let key = file.to_ascii_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let refs: Vec<&Path> = shape_dirs.iter().map(|p| p.as_path()).collect();
            let Some(path) = resolve_shape_path_in_dirs(&refs, file) else {
                continue;
            };
            let Ok(shape) = ShapeFile::from_path(&path) else {
                continue;
            };
            let audit = audit_shape_textures(&shape);
            total.total_parts += audit.total_parts;
            total.textured_parts += audit.textured_parts;
            total.untextured_parts += audit.untextured_parts;
            total.untextured_with_shape_textures += audit.untextured_with_shape_textures;
            total.untextured_no_shape_textures += audit.untextured_no_shape_textures;
            if audit.untextured_parts > 0 {
                *top_shapes.entry(key).or_default() += audit.untextured_parts;
            }
        }

        let mut ranked: Vec<_> = top_shapes.into_iter().collect();
        ranked.sort_by_key(|b| std::cmp::Reverse(b.1));
        eprintln!("audit tile ({tx},{tz}) shapes únicos={}", seen.len());
        eprintln!("  total_parts={}", total.total_parts);
        eprintln!("  textured={}", total.textured_parts);
        eprintln!("  untextured={}", total.untextured_parts);
        eprintln!(
            "    con texturas en el shape pero prim_state sin tex_idxs={}",
            total.untextured_with_shape_textures
        );
        eprintln!(
            "    shape sin texture_filenames={}",
            total.untextured_no_shape_textures
        );
        eprintln!("  top sin textura:");
        for (name, n) in ranked.iter().take(12) {
            eprintln!("    {n:4}  {name}");
        }
    }

    #[test]
    fn ukfs_curve_module_bounds_are_short_not_km() {
        let path = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/GLOBAL/SHAPES/ukfs_c_1x1200m_5d.s"))
            .filter(|p| p.is_file());
        let Some(path) = path else {
            eprintln!("skip: ukfs_c_1x1200m_5d.s no disponible");
            return;
        };
        let parts = load_shape_parts(&path).expect("ukfs curve");
        let (min, max) = shape_local_bounds(&parts).expect("bounds");
        eprintln!("ukfs_c_1x1200m_5d bounds min={min:?} max={max:?}");
        let extent = max - min;
        let longest = extent.x.max(extent.y).max(extent.z);
        assert!(
            longest < 200.0,
            "módulo UKFS debería ser un tramo corto, no km (longest={longest:.1}m)"
        );
    }

    #[test]
    fn ukfs_track_shape_extends_along_z_near_ground() {
        let path = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/GLOBAL/SHAPES/ukfs_s_1x25m.s"))
            .filter(|p| p.is_file());
        let Some(path) = path else {
            eprintln!("skip: ukfs_s_1x25m.s no disponible");
            return;
        };
        let parts = load_shape_parts(&path).expect("ukfs shape");
        let (min, max) = shape_local_bounds(&parts).expect("bounds");
        eprintln!("ukfs_s_1x25m bounds min={min:?} max={max:?}");
        assert!(
            min.y.abs() < 1.5,
            "base del tramo UKFS cerca de Y=0 (min_y={})",
            min.y
        );
        let extent_x = max.x - min.x;
        let extent_y = max.y - min.y;
        let extent_z = max.z - min.z;
        let longest = extent_x.max(extent_y).max(extent_z);
        assert!(
            longest > 10.0 && longest < 35.0,
            "tramo ~25 m en algún eje (extents x={extent_x:.1} y={extent_y:.1} z={extent_z:.1})"
        );
        assert!(
            extent_y < longest * 0.5,
            "eje principal no debería ser vertical (y={extent_y:.1})"
        );
    }

    fn new_forest_route() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/Routes/Watersnake"))
            .unwrap_or_default()
    }

    #[test]
    fn new_forest_untextured_parts_audit() {
        use crate::objects::{load_objects, object_wants_shape_mesh};
        use crate::terrain::load_tile_geometry;
        use crate::textures::{resolve_shape_path_in_dirs, shape_search_dirs};

        let dir = new_forest_route();
        if !dir.is_dir() {
            return;
        }
        let msts = dir.ancestors().nth(2).unwrap_or(&dir);
        let (tx, tz) = (-6096, 14916);
        let loaded = match load_tile_geometry(&dir, tx, tz) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("NF audit skip: {e}");
                return;
            }
        };
        let base = loaded.height.base_y();
        let markers = load_objects(&dir, tx, tz, base);
        let shape_dirs = shape_search_dirs(&dir, msts);

        let mut seen = HashSet::new();
        let mut total = ShapeTextureAudit::default();
        let mut top_shapes: HashMap<String, u32> = HashMap::new();

        for m in markers.iter().filter(|m| object_wants_shape_mesh(m)) {
            let Some(file) = &m.file_name else { continue };
            let key = file.to_ascii_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let refs: Vec<&Path> = shape_dirs.iter().map(|p| p.as_path()).collect();
            let Some(path) = resolve_shape_path_in_dirs(&refs, file) else {
                continue;
            };
            let Ok(shape) = ShapeFile::from_path(&path) else {
                continue;
            };
            let audit = audit_shape_textures(&shape);
            total.total_parts += audit.total_parts;
            total.textured_parts += audit.textured_parts;
            total.untextured_parts += audit.untextured_parts;
            total.untextured_with_shape_textures += audit.untextured_with_shape_textures;
            total.untextured_no_shape_textures += audit.untextured_no_shape_textures;
            if audit.untextured_parts > 0 {
                *top_shapes.entry(key).or_default() += audit.untextured_parts;
            }
        }

        let mut ranked: Vec<_> = top_shapes.into_iter().collect();
        ranked.sort_by_key(|b| std::cmp::Reverse(b.1));
        eprintln!("NF audit tile ({tx},{tz}) shapes únicos={}", seen.len());
        eprintln!("  total_parts={}", total.total_parts);
        eprintln!("  textured={}", total.textured_parts);
        eprintln!("  untextured={}", total.untextured_parts);
        eprintln!(
            "    con texturas en el shape pero prim_state sin tex_idxs={}",
            total.untextured_with_shape_textures
        );
        eprintln!(
            "    shape sin texture_filenames={}",
            total.untextured_no_shape_textures
        );
        eprintln!("  top sin textura:");
        for (name, n) in ranked.iter().take(12) {
            eprintln!("    {n:4}  {name}");
        }
    }

    #[test]
    fn chalk_cliff80m_world_bounds_vs_terrain() {
        use crate::objects::load_objects;
        use crate::terrain::load_tile_geometry;
        use bevy::math::Vec3;

        let dir = new_forest_route();
        if !dir.is_dir() {
            return;
        }
        let path = dir.join("SHAPES/ChalkCliff80m.s");
        if !path.is_file() {
            return;
        }
        let parts = load_shape_parts(&path).expect("parts");
        let (tx, tz) = (-6144, 14900);
        let tile = load_tile_geometry(&dir, tx, tz).expect("tile");
        let base = tile.height.base_y();
        let objs = load_objects(&dir, tx, tz, base);
        for obj in objs
            .iter()
            .filter(|o| o.file_name.as_deref() == Some("ChalkCliff80m.s"))
        {
            let mut min_wy = f32::INFINITY;
            let mut max_wy = f32::NEG_INFINITY;
            for part in &parts {
                for p in &part.positions {
                    let local = Vec3::from_array(*p);
                    let rotated = obj.rotation * (local * obj.scale);
                    min_wy = min_wy.min(rotated.y);
                    max_wy = max_wy.max(rotated.y);
                }
            }
            let foot_y = obj.position.y + min_wy;
            let top_y = obj.position.y + max_wy;
            let terrain = tile.height.local_y(obj.position.x, obj.position.z);
            assert!(
                terrain >= foot_y - 0.5 && terrain <= top_y + 0.5,
                "terrain {terrain:.1} should fall within cliff mesh Y [{foot_y:.1}, {top_y:.1}] at ({:.1},{:.1})",
                obj.position.x,
                obj.position.z,
            );
        }
    }

    #[test]
    fn new_forest_jinx_tunnel_and_chalk_parts_use_distinct_textures() {
        let dir = new_forest_route();
        if !dir.is_dir() {
            return;
        }
        for file in ["IJ_tunnel_1bore.s", "ChalkCliff80m.s"] {
            let path = dir.join("SHAPES").join(file);
            if !path.is_file() {
                continue;
            }
            let shape = ShapeFile::from_path(&path).expect(file);
            let parts = build_parts(&shape);
            let textures: HashSet<_> = parts.iter().filter_map(|p| p.texture.as_ref()).collect();
            assert_eq!(
                textures.len(),
                2,
                "{file} expected two distinct part textures, got {textures:?}"
            );
        }
    }
}
