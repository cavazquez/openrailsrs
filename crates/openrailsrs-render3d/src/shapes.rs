//! Capa 4b: mallas de shapes `.s` — adaptador sobre `openrailsrs-bevy-scenery` (#56).
//!
//! El builder geométrico canónico vive en `bevy-scenery`; este módulo solo carga
//! archivos, aplica la política WORLD de render3d (sub-objetos + anim key 0) y
//! expone [`ShapePart`] con buffers crudos para `world_spawn`.

use std::path::Path;

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    LoadedShapePart, build_mesh_parts_from_shape_at_distance_with_options,
    build_mesh_parts_from_shape_lod_with_options, lod_level_for_distance as scenery_lod_level,
    render3d_world_mesh_options,
};
use openrailsrs_formats::{DistanceLevel, ShapeFile};

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
    /// MSTS `texture.MipMapLODBias` (#108).
    pub mip_map_lod_bias: Option<f32>,
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
    let loaded = build_mesh_parts_from_shape_at_distance_with_options(
        &shape,
        distance_m,
        render3d_world_mesh_options(),
    );
    Some(
        loaded
            .into_iter()
            .filter_map(loaded_part_to_shape_part)
            .collect(),
    )
}

/// Carga un `.s` y construye sus partes de malla (LOD de mayor detalle).
#[allow(dead_code)]
pub fn load_shape_parts(path: &Path) -> Option<Vec<ShapePart>> {
    load_shape_parts_at_distance(path, 0.0)
}

/// LOD para una distancia de cámara (delegado al builder canónico).
pub fn lod_level_for_distance(shape: &ShapeFile, distance_m: f32) -> Option<&DistanceLevel> {
    scenery_lod_level(shape, distance_m)
}

/// Construye partes de malla con la política WORLD de render3d.
#[allow(dead_code)]
pub fn build_parts(shape: &ShapeFile) -> Vec<ShapePart> {
    let Some(level) = scenery_lod_level(shape, 0.0) else {
        return Vec::new();
    };
    build_mesh_parts_from_shape_lod_with_options(shape, level, render3d_world_mesh_options())
        .into_iter()
        .filter_map(loaded_part_to_shape_part)
        .collect()
}

/// Sub-objeto nocturno (índice 1) oculto de día cuando el `.sd` declara `ESD_SubObj`.
pub fn part_visible(
    descriptor: &crate::shape_descriptor::ShapeDescriptor,
    part: &ShapePart,
    env: &crate::textures::TextureEnvironment,
) -> bool {
    crate::shape_descriptor::night_subobj_part_visible(
        descriptor.has_night_subobj,
        part.sub_object_idx,
        env.is_day(),
    )
}

fn loaded_part_to_shape_part(part: LoadedShapePart) -> Option<ShapePart> {
    let positions = mesh_float3(&part.mesh, Mesh::ATTRIBUTE_POSITION)?;
    let normals = mesh_float3(&part.mesh, Mesh::ATTRIBUTE_NORMAL)
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
    let uvs = mesh_float2(&part.mesh, Mesh::ATTRIBUTE_UV_0)
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
    let colors = mesh_float4(&part.mesh, Mesh::ATTRIBUTE_COLOR);
    Some(ShapePart {
        sub_object_idx: part.sub_object_idx,
        positions,
        normals,
        uvs,
        texture: part.texture_file,
        alpha_test_mode: part.alpha_test_mode,
        shader_name: part.shader_name,
        light_mat_idx: part.light_mat_idx,
        tex_addr_mode: part.tex_addr_mode,
        mip_map_lod_bias: part.mip_map_lod_bias,
        colors,
        solid_color: part.solid_color,
    })
}

fn mesh_float3(
    mesh: &Mesh,
    attr: impl Into<bevy::mesh::MeshVertexAttributeId>,
) -> Option<Vec<[f32; 3]>> {
    match mesh.attribute(attr)? {
        VertexAttributeValues::Float32x3(v) => Some(v.clone()),
        _ => None,
    }
}

fn mesh_float2(
    mesh: &Mesh,
    attr: impl Into<bevy::mesh::MeshVertexAttributeId>,
) -> Option<Vec<[f32; 2]>> {
    match mesh.attribute(attr)? {
        VertexAttributeValues::Float32x2(v) => Some(v.clone()),
        _ => None,
    }
}

fn mesh_float4(
    mesh: &Mesh,
    attr: impl Into<bevy::mesh::MeshVertexAttributeId>,
) -> Option<Vec<[f32; 4]>> {
    match mesh.attribute(attr)? {
        VertexAttributeValues::Float32x4(v) => Some(v.clone()),
        _ => None,
    }
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
        // TEXTURES/ is gitignored; CI has no synced Chiltern assets (#73).
        if !tex.is_file() {
            eprintln!("skip: RFwatercolumn.ace no disponible (sync_chiltern_assets)");
            return;
        }
        let Some(path) = water_column_path() else {
            eprintln!("skip: RF_GW_WaterColumn.s no disponible");
            return;
        };
        let parts = load_shape_parts(&path).expect("shape");
        assert!(parts.iter().all(|p| p.texture.is_some()));
        let ace = openrailsrs_ace::read_ace(&tex).expect("ace");
        assert!(ace.width > 0);
    }

    #[test]
    fn canonical_builder_matches_render3d_adapter_vertex_counts() {
        let dir = chiltern_dir();
        let path = dir.join("SHAPES");
        let Ok(rd) = std::fs::read_dir(&path) else {
            return;
        };
        let Some(shape_path) = rd
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("s")))
        else {
            return;
        };
        let shape = ShapeFile::from_path(&shape_path).expect("parse");
        let via_api = load_shape_parts(&shape_path).expect("api");
        let via_build = build_parts(&shape);
        assert_eq!(via_api.len(), via_build.len());
        let api_verts: usize = via_api.iter().map(|p| p.positions.len()).sum();
        let build_verts: usize = via_build.iter().map(|p| p.positions.len()).sum();
        assert_eq!(api_verts, build_verts);
        assert!(api_verts > 0);
    }

    #[test]
    fn chiltern_untextured_parts_audit() {
        use crate::objects::{load_objects, object_wants_shape_mesh};
        use crate::terrain::load_tile_geometry;
        use crate::textures::{resolve_shape_path_in_dirs, shape_search_dirs};

        let dir = chiltern_dir();
        let (tx, tz) = (-6082, 14925);
        // TILES/ is gitignored; skip unless local sync (#73).
        let Ok(loaded) = load_tile_geometry(&dir, tx, tz) else {
            eprintln!("skip: tile Chiltern ({tx},{tz}) no disponible");
            return;
        };
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
