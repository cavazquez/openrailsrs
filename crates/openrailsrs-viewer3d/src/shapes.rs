//! MSTS ASCII `.s` shapes → Bevy meshes (order 6 / issue #8).

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use openrailsrs_formats::{DistanceLevel, ShapeFile, Vec3 as ShapeVec3};

/// Route directory for resolving `SHAPES/` assets referenced by world tiles.
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

/// Build a Bevy mesh from the closest LOD of a parsed ASCII shape.
///
/// Uses flat per-triangle vertices (no index sharing) for simplicity.
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
                    uvs.push(Vec2::new(uv.u as f32, uv.v as f32));
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

/// Load and convert a shape file from disk.
pub fn load_shape_mesh(path: &Path) -> Option<Mesh> {
    let shape = ShapeFile::from_path(path).ok()?;
    build_mesh_from_shape(&shape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn minimal_shape_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-formats/tests/fixtures/minimal.s")
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
    fn resolve_shape_path_finds_smoke_route_asset() {
        let route =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let path = resolve_shape_path(&route, "yard_shed.s").expect("yard_shed.s");
        assert!(path.is_file());
    }
}
