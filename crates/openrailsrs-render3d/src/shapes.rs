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

use crate::coords::{
    matrix43_transform_point_xna, matrix43_transform_vector_xna, shape_point_to_bevy,
};

/// Una parte de malla de un shape (todos los triángulos que comparten textura).
#[derive(Clone, Debug)]
pub struct ShapePart {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Nombre del archivo de textura (`TEXTURES/*.ace`), si la parte tiene.
    pub texture: Option<String>,
    /// Modo alpha MSTS (`prim_state.alpha_test_mode`); -1 = derivado de flags.
    pub alpha_test_mode: i32,
    /// Nombre del shader MSTS de este prim_state (ej. "AddATex", "BlendATex", "TexDiff").
    pub shader_name: Option<String>,
}

/// Carga un `.s` y construye sus partes de malla (LOD de mayor detalle).
pub fn load_shape_parts(path: &Path) -> Option<Vec<ShapePart>> {
    let shape = ShapeFile::from_path(path).ok()?;
    Some(build_parts(&shape))
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

#[derive(Default)]
struct PartBuffers {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
}

/// Construye una parte de malla por `prim_state` del LOD de mayor detalle.
pub fn build_parts(shape: &ShapeFile) -> Vec<ShapePart> {
    let Some(level) = closest_lod(shape) else {
        return Vec::new();
    };
    let pose = animation_pose_matrices(shape, 0.0);
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });

    let mut by_state: BTreeMap<i32, PartBuffers> = BTreeMap::new();
    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            let chain = matrix_chain(shape, level, prim.prim_state_idx, &pose);
            let buf = by_state.entry(prim.prim_state_idx).or_default();
            for tri in prim.vertex_indices.chunks(3) {
                if tri.len() < 3 {
                    continue;
                }
                for &vidx in tri {
                    let Some((pi, ni, ui)) = resolve_vertex(shape, sub, vidx) else {
                        continue;
                    };
                    let Some(point) = shape.points.get(pi) else {
                        continue;
                    };
                    let pos = transform_point(shape_point_to_bevy(*point), &chain);
                    let normal = ni
                        .and_then(|i| shape.normals.get(i).copied())
                        .unwrap_or(default_normal);
                    let nrm = transform_normal(shape_point_to_bevy(normal), &chain);
                    let uv = ui
                        .and_then(|i| shape.uvs.get(i))
                        .copied()
                        .unwrap_or_default();
                    buf.positions.push(pos.to_array());
                    buf.normals.push(nrm.to_array());
                    buf.uvs.push([uv.u as f32, 1.0 - uv.v as f32]);
                }
            }
        }
    }

    by_state
        .into_iter()
        .filter(|(_, b)| !b.positions.is_empty())
        .map(|(prim_state_idx, b)| ShapePart {
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
        })
        .collect()
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

fn resolve_vertex(
    shape: &ShapeFile,
    sub: &openrailsrs_formats::SubObject,
    vertex_idx: u32,
) -> Option<(usize, Option<usize>, Option<usize>)> {
    if let Some(v) = sub.vertices.get(vertex_idx as usize) {
        return Some((
            idx_to_usize(v.point_idx)?,
            idx_to_usize(v.normal_idx),
            v.uv_indices.first().and_then(|i| idx_to_usize(*i)),
        ));
    }
    let idx = vertex_idx as usize;
    if idx < shape.points.len() {
        return Some((idx, Some(idx), Some(idx)));
    }
    None
}

fn idx_to_usize(idx: i32) -> Option<usize> {
    (idx >= 0).then_some(idx as usize)
}

fn texture_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<String> {
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
        use crate::objects::{load_objects, wants_shape_mesh};
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

        for m in markers
            .iter()
            .filter(|m| wants_shape_mesh(m.kind, m.file_name.as_deref()))
        {
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
}
