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

/// Build a Bevy mesh from a specific distance level.
pub fn build_mesh_from_shape_lod(shape: &ShapeFile, level: &DistanceLevel) -> Option<Mesh> {
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

/// Build a Bevy mesh from the closest LOD of a parsed shape.
pub fn build_mesh_from_shape(shape: &ShapeFile) -> Option<Mesh> {
    let level = closest_lod_level(shape)?;
    build_mesh_from_shape_lod(shape, level)
}

/// Build mesh choosing LOD from camera distance (m) to the shape origin.
pub fn build_mesh_from_shape_at_distance(shape: &ShapeFile, distance_m: f32) -> Option<Mesh> {
    let level = lod_level_for_distance(shape, distance_m).or_else(|| closest_lod_level(shape))?;
    build_mesh_from_shape_lod(shape, level)
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
///
/// When `camera_distance_m` is set, picks a coarser LOD farther from the camera.
pub fn load_shape_from_path(path: &Path, camera_distance_m: Option<f32>) -> Option<LoadedShape> {
    let shape = ShapeFile::from_path(path).ok()?;
    let mesh = match camera_distance_m {
        Some(d) => build_mesh_from_shape_at_distance(&shape, d)?,
        None => build_mesh_from_shape(&shape)?,
    };
    let texture_file = primary_texture_filename(&shape);
    Some(LoadedShape { mesh, texture_file })
}

/// Load and convert a shape file from disk (mesh only).
pub fn load_shape_mesh(path: &Path) -> Option<Mesh> {
    load_shape_from_path(path, None).map(|loaded| loaded.mesh)
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
}
