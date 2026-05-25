//! MSTS ASCII `.s` shapes → Bevy meshes (order 6) + `.ace` textures (order 7).

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_ace::{AceFile, read_ace};
use openrailsrs_formats::{DistanceLevel, ShapeFile, Vec3 as ShapeVec3};

/// Route directory for resolving `SHAPES/` and `TEXTURES/` assets.
#[derive(Resource, Clone)]
pub struct RouteAssets {
    pub route_dir: PathBuf,
}

impl RouteAssets {
    pub fn new(route_dir: impl Into<PathBuf>) -> Self {
        Self {
            route_dir: route_dir.into(),
        }
    }
}

/// Parsed shape geometry plus optional primary texture filename from the shape.
#[derive(Clone, Debug)]
pub struct LoadedShape {
    pub mesh: Mesh,
    pub texture_file: Option<String>,
}

/// Map a shape point from MSTS local space to Bevy (Y up).
pub fn shape_point_to_bevy(v: ShapeVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
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

/// Resolve the first texture referenced by the closest LOD (prim_state → `texture_filenames`).
pub fn primary_texture_filename(shape: &ShapeFile) -> Option<String> {
    let level = closest_lod_level(shape)?;
    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            let idx = prim.prim_state_idx;
            if idx < 0 {
                continue;
            }
            let ps = shape.prim_states.get(idx as usize)?;
            if ps.texture_idx < 0 {
                continue;
            }
            return shape
                .texture_filenames
                .get(ps.texture_idx as usize)
                .cloned();
        }
    }
    shape.texture_filenames.first().cloned()
}

/// Build a Bevy mesh from the closest LOD of a parsed ASCII shape.
pub fn build_mesh_from_shape(shape: &ShapeFile) -> Option<Mesh> {
    let level = closest_lod_level(shape)?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });

    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            for tri in prim.vertex_indices.chunks(3) {
                if tri.len() < 3 {
                    continue;
                }
                for &idx in tri {
                    let i = idx as usize;
                    let Some(point) = shape.points.get(i) else {
                        continue;
                    };
                    positions.push(shape_point_to_bevy(*point));
                    let normal = shape.normals.get(i).copied().unwrap_or(default_normal);
                    normals.push(shape_point_to_bevy(normal));
                    let uv = shape.uvs.get(i).copied().unwrap_or_default();
                    // MSTS UV origin differs from Bevy; flip V for textured quads.
                    uvs.push(Vec2::new(uv.u as f32, 1.0 - uv.v as f32));
                }
            }
        }
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
    Some(mesh)
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

/// Resolve `SHAPES/foo.s` under the route directory.
pub fn resolve_shape_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    for subdir in ["SHAPES", "shapes"] {
        let path = route_dir.join(subdir).join(file_name);
        if path.is_file() {
            return Some(path);
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

/// Resolve `TEXTURES/foo.ace` under the route directory.
pub fn resolve_texture_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    for subdir in ["TEXTURES", "textures"] {
        let path = route_dir.join(subdir).join(file_name);
        if path.is_file() {
            return Some(path);
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

/// Load shape mesh and discover its primary texture filename, if any.
pub fn load_shape_from_path(path: &Path) -> Option<LoadedShape> {
    let shape = ShapeFile::from_path(path).ok()?;
    let mesh = build_mesh_from_shape(&shape)?;
    let texture_file = primary_texture_filename(&shape);
    Some(LoadedShape { mesh, texture_file })
}

/// Load and convert a shape file from disk (mesh only).
pub fn load_shape_mesh(path: &Path) -> Option<Mesh> {
    load_shape_from_path(path).map(|loaded| loaded.mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn minimal_shape_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-formats/tests/fixtures/minimal.s")
    }

    fn ace_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-ace/tests/fixtures/rgba8_4x4.ace")
    }

    #[test]
    fn build_mesh_from_minimal_shape_has_two_triangles() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse minimal.s");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        assert_eq!(mesh.count_vertices(), 6);
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
    fn resolve_smoke_route_assets() {
        let route =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        assert!(resolve_shape_path(&route, "yard_shed.s").is_some());
        assert!(resolve_texture_path(&route, "yard.ace").is_some());
        let loaded = load_shape_from_path(&resolve_shape_path(&route, "yard_shed.s").unwrap())
            .expect("shape");
        assert_eq!(loaded.texture_file.as_deref(), Some("yard.ace"));
    }
}
