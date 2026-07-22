//! MSTS ASCII `.s` shapes → Bevy meshes (order 6) + `.ace` textures (order 7).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::coordinates::{
    matrix43_to_transform, rebase_points_to_bone_local, rebase_vectors_to_bone_local,
};
use crate::viewer_log;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_ace::{AceFile, read_ace};
pub use openrailsrs_bevy_scenery::shapes::mesh::{
    MeshBuffers, ShapeMatrixRef, append_primitive_mesh_buffers, mesh_buffers_bounds,
};
pub use openrailsrs_bevy_scenery::shapes::{
    DARK_TEXTURE_LUMA_THRESHOLD, LoadedShape, LoadedShapePart, MSTS_Z_BIAS_CLAMP,
    MeshVertexColorMode, MeshVertexColorStats, SCENERY_TEXTURE_ALBEDO_BOOST,
    SCENERY_TEXTURE_TARGET_LUMA, ShapeMaterialDebugCtx, ShapePbrSidecar, ace_mean_luma,
    alpha_mode_from_prim_state, apply_msts_vertex_tint, apply_shape_debug_material_overrides,
    apply_standard_normal_map, apply_train_debug_material_overrides, apply_train_exterior_culling,
    apply_z_buf_mode, blend_alpha_passes_from_prim_state, brighten_cab_ace_rgba,
    brighten_dark_ace_rgba, build_mesh_from_shape, build_mesh_from_shape_at_distance,
    build_mesh_from_shape_lod, build_mesh_parts_from_shape,
    build_mesh_parts_from_shape_at_distance, build_mesh_parts_from_shape_at_distance_with_options,
    build_mesh_parts_from_shape_lod, cab_ace_brighten_enabled, cab_albedo_tint,
    cab_interior_albedo_boost, cab_or_scenery_material_with_texture_ex, clamp_msts_z_bias_for_bevy,
    closest_lod_level, debug_materials_enabled, debug_shape_stats_enabled,
    ensure_tangents_for_normal_mapping, finalize_scenery_material, legacy_standard_scenery_enabled,
    light_mat_idx_for_prim_state, load_shape_pbr_sidecar, lod_level_for_distance,
    log_shape_material_debug, mesh_aabb, mesh_has_uvs, mesh_position_count,
    mesh_triangle_list_valid, mesh_uv_aabb, mesh_uv_degenerate, mesh_vertex_color_stats,
    msts_shape_to_train_rotation, or_lighting_enabled, primary_texture_filename,
    resolve_or_lighting, scenery_albedo_tint, scenery_base_tint, scenery_material_tint_for_ace,
    scenery_materials_lit, scenery_uses_or_wgsl_shaders, set_train_shape_debug_scope,
    shader_name_for_prim_state, shader_uses_vertex_color_multiply, shape_alpha_mode,
    shape_point_to_bevy, shape_shader_requests_blending, sort_index_depth_nudge,
    texture_for_prim_state, texture_name_suggests_transparency,
    train_exterior_material_with_texture_ex, train_shape_debug_scope, world_mesh_options_for_shape,
};
use openrailsrs_bevy_scenery::shapes::{ShapeDescriptor, night_subobj_part_visible};
use openrailsrs_formats::{DistanceLevel, ShapeFile, Vec3 as ShapeVec3};
use openrailsrs_or_shader::OR_MSTS_ALPHA_TEST_CUTOFF;

pub use openrailsrs_bevy_scenery::textures::{DdsAlpha, ace_to_image, dds_alpha_type};

/// MSTS `ROUTES/<name>/` when the repo only ships a slim `examples/<name>/` overlay.
pub fn resolve_msts_route_dir(route_dir: &Path) -> Option<PathBuf> {
    let stem = route_dir.file_name()?.to_str()?;
    let content = msts_content_root()?;
    let mut candidates = vec![
        content.join(stem).join("ROUTES").join(stem),
        content.join("ROUTES").join(stem),
    ];
    if let Ok(entries) = std::fs::read_dir(&content) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(stem)
            {
                let pack = entry.path();
                candidates.push(pack.join("ROUTES").join(stem));
                if let Ok(route_entries) = std::fs::read_dir(pack.join("ROUTES")) {
                    for route in route_entries.flatten() {
                        if route
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(stem)
                        {
                            candidates.push(route.path());
                        }
                    }
                }
            }
        }
    }
    for candidate in candidates {
        if !candidate.is_dir() {
            continue;
        }
        let has_tsection = [
            "OpenRails/tsection.dat",
            "openrails/tsection.dat",
            "tsection.dat",
        ]
        .iter()
        .any(|rel| candidate.join(rel).is_file());
        if has_tsection || candidate.join("WORLD").is_dir() || candidate.join("world").is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn track_db_search_dirs(route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(msts_route) = resolve_msts_route_dir(route_dir) {
        if msts_route != route_dir {
            dirs.push(msts_route);
        }
    }
    dirs
}

fn load_track_db(route_dir: &Path) -> Option<openrailsrs_formats::TrackDbFile> {
    for dir in track_db_search_dirs(route_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tdb"))
            {
                continue;
            }
            if let Ok(mut tdb) = openrailsrs_formats::TrackDbFile::from_path(&path) {
                let tit = path.with_extension("tit");
                if tit.is_file() {
                    let _ = tdb.merge_tit_speed_posts(&tit);
                }
                return Some(tdb);
            }
        }
    }
    None
}

fn load_road_db(route_dir: &Path) -> Option<openrailsrs_formats::TrackDbFile> {
    for dir in track_db_search_dirs(route_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("rdb"))
            {
                continue;
            }
            if let Ok(rdb) = openrailsrs_formats::TrackDbFile::from_path(&path) {
                return Some(rdb);
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug)]
struct TdbSectionAnchor {
    bevy_x: f32,
    bevy_z: f32,
    heading_deg: f64,
}

fn heading_from_vector_geometry(geometry: openrailsrs_formats::TrackVectorGeometry) -> Option<f64> {
    let (x0, _, z0) = geometry.start.bevy_position();
    let (x1, _, z1) = geometry.end.bevy_position();
    let dx = x1 - x0;
    let dz = z1 - z0;
    if dx * dx + dz * dz < 0.01 {
        return None;
    }
    Some((dx as f64).atan2(dz as f64).to_degrees())
}

fn build_tdb_section_index(
    tdb: &openrailsrs_formats::TrackDbFile,
) -> HashMap<u32, Vec<TdbSectionAnchor>> {
    let mut geometry_by_node: HashMap<u32, openrailsrs_formats::TrackVectorGeometry> =
        HashMap::new();
    for node in &tdb.nodes {
        if let openrailsrs_formats::TrackNodeKind::Vector {
            geometry: Some(geom),
            ..
        } = &node.kind
        {
            geometry_by_node.insert(node.id, *geom);
        }
    }

    let mut out: HashMap<u32, Vec<TdbSectionAnchor>> = HashMap::new();
    for (shape_idx, entries) in tdb.index_vector_sections_by_shape() {
        let mut anchors = Vec::new();
        for entry in entries {
            let heading = entry.record.heading_deg().or_else(|| {
                geometry_by_node
                    .get(&entry.node_id)
                    .copied()
                    .and_then(heading_from_vector_geometry)
            });
            let Some(heading_deg) = heading else {
                continue;
            };
            let (bevy_x, _, bevy_z) = entry.record.start.bevy_position();
            anchors.push(TdbSectionAnchor {
                bevy_x,
                bevy_z,
                heading_deg,
            });
        }
        if !anchors.is_empty() {
            out.insert(shape_idx, anchors);
        }
    }
    out
}

/// Route directory for resolving `SHAPES/` and `TEXTURES/` assets.
#[derive(Resource, Clone)]
pub struct RouteAssets {
    pub route_dir: PathBuf,
    /// Catálogo compartido (#49): shapes/texturas/tsection con precedencia ruta > pack > GLOBAL.
    catalog: openrailsrs_bevy_scenery::MstsRouteCatalog,
    track_db: Option<openrailsrs_formats::TrackDbFile>,
    /// Road database (`.rdb`) — same schema as TDB; used for CarSpawner endpoints (#32).
    road_db: Option<openrailsrs_formats::TrackDbFile>,
    carspawn: openrailsrs_formats::CarSpawnerCatalog,
    sigcfg: openrailsrs_formats::SigCfgFile,
    tdb_sections_by_shape: HashMap<u32, Vec<TdbSectionAnchor>>,
}

impl RouteAssets {
    pub fn new(route_dir: impl Into<PathBuf>) -> Self {
        let route_dir = route_dir.into();
        let msts_root = msts_content_root().unwrap_or_else(|| route_dir.clone());
        let catalog = openrailsrs_bevy_scenery::MstsRouteCatalog::build(&route_dir, &msts_root);
        if !catalog.tsection.shapes.is_empty() {
            let junction_clearance = catalog
                .tsection
                .shapes
                .iter()
                .filter(|(id, shape)| {
                    shape.is_junction() && catalog.tsection.clearance_dist_m(**id).is_some()
                })
                .count();
            crate::viewer_log!(
                "openrailsrs-viewer3d: tsection — {} shape(s), {} section(s), {} junction(s) with ClearanceDist",
                catalog.tsection.shapes.len(),
                catalog.tsection.sections.len(),
                junction_clearance
            );
        }
        crate::viewer_log!(
            "openrailsrs-viewer3d: route catalog — {} shape(s), {} texture(s)",
            catalog.shape_count(),
            catalog.texture_count()
        );
        let (track_db, tdb_sections_by_shape) = load_track_db(&route_dir)
            .map(|tdb| {
                let indexed_shapes = tdb.index_vector_sections_by_shape().len();
                let junctions = tdb.junction_nodes().count();
                let anchors = build_tdb_section_index(&tdb);
                let anchor_count: usize = anchors.values().map(|v| v.len()).sum();
                crate::viewer_log!(
                    "openrailsrs-viewer3d: tdb — {} node(s), {} junction(s), {} vector section(s) with heading ({indexed_shapes} shape(s))",
                    tdb.nodes.len(),
                    junctions,
                    anchor_count,
                );
                (Some(tdb), anchors)
            })
            .unwrap_or((None, HashMap::new()));
        let road_db = load_road_db(&route_dir).inspect(|rdb| {
            let posed = rdb.items.iter().filter(|i| i.world.is_some()).count();
            crate::viewer_log!(
                "openrailsrs-viewer3d: rdb — {} node(s), {} item(s) ({} with TrItemRData)",
                rdb.nodes.len(),
                rdb.items.len(),
                posed
            );
        });
        let carspawn =
            openrailsrs_formats::CarSpawnerCatalog::load_for_route(&route_dir).unwrap_or_default();
        if !carspawn.lists.is_empty() {
            let models: usize = carspawn.lists.iter().map(|l| l.items.len()).sum();
            crate::viewer_log!(
                "openrailsrs-viewer3d: carspawn — {} list(s), {} model(s)",
                carspawn.lists.len(),
                models
            );
        }
        let sigcfg =
            openrailsrs_formats::SigCfgFile::load_for_route(&route_dir).unwrap_or_default();
        if !sigcfg.signal_shapes.is_empty() {
            crate::viewer_log!(
                "openrailsrs-viewer3d: sigcfg — {} shape(s), {} type(s), {} light colour(s)",
                sigcfg.signal_shapes.len(),
                sigcfg.signal_types.len(),
                sigcfg.lights_tab.len()
            );
        }
        Self {
            route_dir,
            catalog,
            track_db,
            road_db,
            carspawn,
            sigcfg,
            tdb_sections_by_shape,
        }
    }

    pub fn catalog(&self) -> &openrailsrs_bevy_scenery::MstsRouteCatalog {
        &self.catalog
    }

    pub fn track_db(&self) -> Option<&openrailsrs_formats::TrackDbFile> {
        self.track_db.as_ref()
    }

    pub fn road_db(&self) -> Option<&openrailsrs_formats::TrackDbFile> {
        self.road_db.as_ref()
    }

    pub fn carspawn(&self) -> &openrailsrs_formats::CarSpawnerCatalog {
        &self.carspawn
    }

    pub fn sigcfg(&self) -> &openrailsrs_formats::SigCfgFile {
        &self.sigcfg
    }

    pub fn tsection(&self) -> &openrailsrs_formats::TSectionCatalog {
        &self.catalog.tsection
    }

    /// Refine TrackObj yaw from `.tdb` `TrVectorSection` when a nearby anchor matches `SectionIdx`.
    pub fn refine_trackobj_rotation(
        &self,
        section_idx: Option<u32>,
        position: Vec3,
        rotation: Quat,
    ) -> Quat {
        let Some(shape_idx) = section_idx else {
            return rotation;
        };
        let Some(entries) = self.tdb_sections_by_shape.get(&shape_idx) else {
            return rotation;
        };
        const MAX_DIST_M: f32 = 25.0;
        let max_dist_sq = MAX_DIST_M * MAX_DIST_M;
        let mut best: Option<(f32, f64)> = None;
        for entry in entries {
            let dx = entry.bevy_x - position.x;
            let dz = entry.bevy_z - position.z;
            let dist_sq = dx * dx + dz * dz;
            if dist_sq <= max_dist_sq
                && best
                    .map(|(best_dist, _)| dist_sq < best_dist)
                    .unwrap_or(true)
            {
                best = Some((dist_sq, entry.heading_deg));
            }
        }
        let Some((_, heading_deg)) = best else {
            return rotation;
        };
        let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
        let tdb_yaw = heading_deg.to_radians() as f32;
        if (yaw - tdb_yaw).abs() < 0.01 {
            return rotation;
        }
        Quat::from_euler(EulerRot::YXZ, tdb_yaw, pitch, roll)
    }

    /// Resolve a shape filename using a pre-built index (case-insensitive).
    pub fn resolve_shape(&self, file_name: &str) -> Option<PathBuf> {
        self.catalog.resolve_shape(file_name)
    }

    /// Resolve scenery shapes; `TrackObj` / `Hazard` prefer route-pack `GLOBAL/SHAPES/`.
    pub fn resolve_world_shape(&self, kind: &str, file_name: &str) -> Option<PathBuf> {
        self.catalog.resolve_world_shape(kind, file_name)
    }

    /// Resolve `TrackObj` mesh path using `.w` `FileName` and/or `SectionIdx` → `tsection.dat`.
    pub fn resolve_trackobj_shape(
        &self,
        file_name: Option<&str>,
        section_idx: Option<u32>,
    ) -> Option<PathBuf> {
        self.catalog.resolve_trackobj_shape(file_name, section_idx)
    }

    /// Resolve a texture via the shared catalog index (summer/day, no seasonal flags).
    pub fn resolve_texture(&self, dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
        self.catalog.resolve_texture(
            dirs,
            file_name,
            &openrailsrs_bevy_scenery::textures::TextureEnvironment::summer_day(),
            openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(
                openrailsrs_bevy_scenery::textures::TextureFlags::NONE,
            ),
        )
    }
}

/// Bevy asset handles for one renderable shape part.
#[derive(Clone, Debug)]
pub struct ShapePartAsset {
    pub prim_state_idx: i32,
    pub sub_object_idx: u32,
    /// Open Rails `SortIndex` (first file-order primitive in this part) (#102).
    pub sort_index: u32,
    pub cab_matrix_idx: Option<usize>,
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    /// Open Rails cab shader (`or_cab.wgsl`); preferred in cab spawn when set.
    pub or_cab_material: Option<Handle<crate::or_cab_material::OrCabMaterial>>,
    pub has_texture: bool,
    pub is_transparent: bool,
    /// Diagnostic metadata (cab / shape load).
    pub texture_name: Option<String>,
    pub shader_name: Option<String>,
    pub light_mat_idx: Option<i32>,
    /// MSTS uniform vertex tint when all verts share one colour (TexDiff).
    pub solid_color: Option<[f32; 3]>,
    pub lever_pivot_at_mesh_center: bool,
    pub lever_local_axis: Option<Vec3>,
    pub bounds_center: Option<Vec3>,
}

/// Bevy asset handles for a shape, including a combined mesh for fitting/bounds.
#[derive(Clone, Debug)]
pub struct ShapeRenderAsset {
    pub combined_mesh: Handle<Mesh>,
    pub parts: Vec<ShapePartAsset>,
    pub has_texture: bool,
    /// `.sd` `ESD_SubObj`: sub-object 1 is night-only geometry (#95).
    pub has_night_subobj: bool,
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

/// Local transform for a vehicle `.s` mesh: authored shape origin at `offset_m`, unit scale.
///
/// Open Rails does **not** scale vehicle meshes to match `Size` / `length_m`, and does **not**
/// re-anchor meshes by AABB `min.z` / `min.y`. Those Size values control coupler spacing /
/// consist offsets only; mesh proportions and the shape origin stay as authored (plus any
/// Matrix3x3 baked into the shape hierarchy). Exterior and 3D cab share this frame so
/// `ORTS3DCabHeadPos` stays in the same local space as the body mesh.
pub fn vehicle_shape_local_transform(mesh: &Mesh, offset_m: f32, length_m: f32) -> Transform {
    cab_shape_placement_transform(mesh, offset_m, length_m)
}

/// Shared exterior / 3D-cab vehicle frame: MSTS→train rotation, authored origin at `offset_m`.
///
/// The `mesh` argument is accepted for call-site symmetry but does **not** shift the frame
/// (no AABB front/ground compensation). `length_m` likewise does not affect mesh scale.
pub fn cab_shape_placement_transform(mesh: &Mesh, offset_m: f32, _length_m: f32) -> Transform {
    let _ = mesh;
    vehicle_authored_frame_transform(offset_m)
}

/// Train-local vehicle frame with authored shape origin at `offset_m` (Open Rails parity).
pub fn vehicle_authored_frame_transform(offset_m: f32) -> Transform {
    Transform {
        translation: Vec3::new(offset_m, 0.0, 0.0),
        rotation: msts_shape_to_train_rotation(),
        scale: Vec3::ONE,
    }
}

/// Cab frame transform plus exterior child scale (always `1.0`; no length-fit).
///
/// Kept for call-site symmetry with the former length-fit path; exterior parts spawn at
/// unit scale under the frame.
pub fn vehicle_cab_frame_and_exterior_scale(
    mesh: &Mesh,
    offset_m: f32,
    length_m: f32,
) -> (Transform, f32) {
    (cab_shape_placement_transform(mesh, offset_m, length_m), 1.0)
}

/// Union AABB of several meshes in their local space.
pub fn union_meshes_aabb(meshes: &[&Mesh]) -> Option<(Vec3, Vec3)> {
    let mut min_all = Vec3::splat(f32::INFINITY);
    let mut max_all = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for mesh in meshes {
        let Some((mn, mx)) = mesh_aabb(mesh) else {
            continue;
        };
        min_all = min_all.min(mn);
        max_all = max_all.max(mx);
        any = true;
    }
    any.then_some((min_all, max_all))
}

/// True when `point` lies inside an axis-aligned box (inclusive).
pub fn point_in_aabb(point: Vec3, min: Vec3, max: Vec3) -> bool {
    point.x >= min.x
        && point.x <= max.x
        && point.y >= min.y
        && point.y <= max.y
        && point.z >= min.z
        && point.z <= max.z
}

/// MSTS shape-file coordinates (`.s` points, `.eng` ORTS3DCabHeadPos) → Bevy mesh space.
/// Delegates to [`crate::coordinates::msts_shape_vec3_to_bevy`].
pub use crate::coordinates::msts_shape_vec3_to_bevy;

/// `ORTS3DCabHeadPos` inside the cab shape AABB (MSTS shape metres, unit scale).
pub fn orts_head_inside_cab_aabb(head_msts: Vec3, cab_meshes: &[&Mesh]) -> bool {
    let Some((min, max)) = union_meshes_aabb(cab_meshes) else {
        return false;
    };
    point_in_aabb(msts_shape_vec3_to_bevy(head_msts), min, max)
}

/// Same check after applying the lead-vehicle cab frame (train-local metres).
pub fn orts_head_inside_cab_train_space(
    head_msts: Vec3,
    exterior_mesh: &Mesh,
    cab_meshes: &[&Mesh],
    offset_m: f32,
    length_m: f32,
) -> bool {
    let frame = cab_shape_placement_transform(exterior_mesh, offset_m, length_m);
    let head_shape = msts_shape_vec3_to_bevy(head_msts);
    let head_train = frame.transform_point(head_shape);
    let Some((min, max)) = union_meshes_aabb(cab_meshes) else {
        return false;
    };
    let mut mn = Vec3::splat(f32::INFINITY);
    let mut mx = Vec3::splat(f32::NEG_INFINITY);
    for corner in aabb_corners(min, max) {
        let p = frame.transform_point(corner);
        mn = mn.min(p);
        mx = mx.max(p);
    }
    point_in_aabb(head_train, mn, mx)
}

fn push_unique_path(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !candidates.iter().any(|p| p == &path) {
        candidates.push(path);
    }
}

/// Open Rails `Content/` trainset folders for a vehicle (e.g. `RF_Blue_Pullman`).
pub fn or_content_trainset_roots(route_dir: &Path, trainset_name: &str) -> Vec<PathBuf> {
    let Some(content) = msts_content_root() else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    let route_names = route_dir
        .file_name()
        .into_iter()
        .map(|n| n.to_string_lossy().into_owned())
        .chain([String::from("Chiltern")]);
    for route_name in route_names {
        for trains_sub in [
            "TRAINS/TRAINSET",
            "trains/trainset",
            "trains/TRAINSET",
            "Trains/Trainset",
        ] {
            push_unique_path(
                &mut roots,
                content
                    .join(&route_name)
                    .join(trains_sub)
                    .join(trainset_name),
            );
        }
    }
    for trains_sub in ["TRAINS/TRAINSET", "trains/trainset", "trains/TRAINSET"] {
        push_unique_path(&mut roots, content.join(trains_sub).join(trainset_name));
    }
    roots
}

/// Trainset folder name (`RF_Blue_Pullman`) from the first shape search hit.
pub fn trainset_name_from_shape_search(shape_dirs: &[&Path], shape_file: &str) -> Option<String> {
    for dir in shape_dirs {
        if resolve_shape_path(dir, shape_file).is_some() {
            return dir.file_name().map(|n| n.to_string_lossy().into_owned());
        }
    }
    None
}

/// Resolve a vehicle `.s`, preferring Open Rails Content over scenario stubs.
pub fn resolve_vehicle_shape_path(
    shape_dirs: &[&Path],
    shape_file: &str,
    route_dir: &Path,
) -> Option<PathBuf> {
    if let Some(name) = trainset_name_from_shape_search(shape_dirs, shape_file) {
        for root in or_content_trainset_roots(route_dir, &name) {
            if let Some(path) = resolve_shape_path(&root, shape_file) {
                return Some(path);
            }
        }
    }
    resolve_shape_path_in_dirs(shape_dirs, shape_file)
}

/// Cab interior: one part per (`sub_object`, `prim_state`); lever matrix bones omit leaf from bake.
pub fn build_mesh_parts_from_shape_lod_cab(
    shape: &ShapeFile,
    level: &DistanceLevel,
    lever_matrices: &HashSet<usize>,
) -> Vec<LoadedShapePart> {
    let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    });
    let mut parts = Vec::new();

    for (sub_idx, sub) in level.sub_objects.iter().enumerate() {
        for prim in sub.primitives.iter() {
            let mut buffers = MeshBuffers::default();
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
            let (bounds_center, bounds_half_extent) = mesh_buffers_bounds(&buffers);
            let cab_matrix_idx =
                cab_matrix_for_prim(shape, sub_idx, sub, prim.prim_state_idx, lever_matrices);
            let matrix_needs_rebase =
                cab_matrix_idx.is_some_and(|idx| lever_matrices.contains(&idx));
            let lever_bone = cab_matrix_idx.and_then(|idx| {
                if !matrix_needs_rebase {
                    return None;
                }
                shape
                    .matrices
                    .get(idx)
                    .map(|m| matrix43_to_transform(&m.matrix))
            });

            let mut buffers = MeshBuffers::default();
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
            if let Some(bone) = lever_bone.as_ref() {
                rebase_points_to_bone_local(&mut buffers.positions, *bone);
                rebase_vectors_to_bone_local(&mut buffers.normals, *bone);
            }
            let (mesh, solid_color) = match buffers.into_mesh_with_color() {
                Some(v) => v,
                None => continue,
            };
            let prim_state_idx = prim.prim_state_idx;
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
            parts.push(LoadedShapePart {
                prim_state_idx,
                sub_object_idx: sub_idx as u32,
                sort_index: parts.len() as u32,
                cab_matrix_idx,
                mesh,
                texture_file: texture_for_prim_state(shape, prim_state_idx),
                shader_name: shader_name_for_prim_state(shape, prim_state_idx),
                solid_color,
                alpha_test_mode,
                z_bias,
                z_buf_mode,
                light_mat_idx: light_mat_idx_for_prim_state(shape, prim_state_idx),
                tex_addr_mode: shape.tex_addr_mode_for_prim_state(prim_state_idx),
                mip_map_lod_bias: Some(shape.mip_map_lod_bias_for_prim_state(prim_state_idx)),
                bounds_center: Some(bounds_center),
                bounds_half_extent: Some(bounds_half_extent),
                lever_pivot_at_mesh_center: false,
                lever_local_axis: None,
            });
        }
    }

    parts
}

/// CVF matrix for one cab primitive — **authored** hierarchy only (#146).
///
/// Bind only when `vtx_state.matrix_idx` names a lever bone (≠ MAIN/0).
/// Never promote MAIN geometry by texture, proximity, or sub_object index
/// coincidence (Pullman has small 1-prim subobjects 4/8 that are still MAIN).
pub fn cab_matrix_for_prim(
    shape: &ShapeFile,
    _sub_idx: usize,
    _sub: &openrailsrs_formats::SubObject,
    prim_state_idx: i32,
    lever_matrices: &HashSet<usize>,
) -> Option<usize> {
    let idx = crate::cab_cvf::matrix_idx_for_prim_state(shape, prim_state_idx)?;
    if idx != 0 && lever_matrices.contains(&idx) {
        Some(idx)
    } else {
        None
    }
}

pub fn matrix_pivot_bevy(shape: &ShapeFile, matrix_idx: usize) -> Option<Vec3> {
    shape.matrices.get(matrix_idx).map(|m| {
        let r = &m.matrix.rows[3];
        openrailsrs_bevy_scenery::shapes::shape_point_to_bevy(openrailsrs_formats::Vec3 {
            x: r[0],
            y: r[1],
            z: r[2],
        })
    })
}

/// ACE → GPU image with dark-atlas normalization for world / train scenery.
pub fn ace_to_scenery_image(ace: &AceFile) -> (Image, bool) {
    let (rgba, brightened) = brighten_dark_ace_rgba(&ace.mip0);
    (ace_rgba_to_image(ace.width, ace.height, &rgba), brightened)
}

fn ace_rgba_to_image(width: u32, height: u32, rgba: &[u8]) -> Image {
    ace_rgba_to_image_with_addr(width, height, rgba, None)
}

fn ace_rgba_to_image_with_addr(
    width: u32,
    height: u32,
    rgba: &[u8],
    tex_addr_mode: Option<i32>,
) -> Image {
    ace_rgba_to_image_with_sampler(width, height, rgba, tex_addr_mode, None)
}

fn ace_rgba_to_image_with_sampler(
    width: u32,
    height: u32,
    rgba: &[u8],
    tex_addr_mode: Option<i32>,
    mip_map_lod_bias: Option<f32>,
) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    openrailsrs_bevy_scenery::textures::apply_msts_texture_sampler(
        &mut image,
        tex_addr_mode,
        mip_map_lod_bias,
    );
    image
}

fn texture_cache_addr_key(tex_addr_mode: Option<i32>) -> i32 {
    texture_cache_sampler_key(tex_addr_mode, None)
}

/// Pack address mode + quantized mip bias into the image cache key (#108).
fn texture_cache_sampler_key(tex_addr_mode: Option<i32>, mip_map_lod_bias: Option<f32>) -> i32 {
    let addr = tex_addr_mode.unwrap_or(1).clamp(0, 15);
    let bias_cents = (mip_map_lod_bias.unwrap_or(-3.0) * 100.0).round() as i32;
    addr + bias_cents.saturating_mul(16)
}

/// Decode a DDS file from raw bytes into a Bevy GPU image (keeps block compression).
pub fn decode_dds_to_image(bytes: &[u8]) -> Result<Image, String> {
    decode_dds_to_image_with_addr(bytes, None)
}

pub fn decode_dds_to_image_with_addr(
    bytes: &[u8],
    tex_addr_mode: Option<i32>,
) -> Result<Image, String> {
    decode_dds_to_image_with_sampler(bytes, tex_addr_mode, None)
}

pub fn decode_dds_to_image_with_sampler(
    bytes: &[u8],
    tex_addr_mode: Option<i32>,
    mip_map_lod_bias: Option<f32>,
) -> Result<Image, String> {
    openrailsrs_bevy_scenery::textures::decode_dds_to_image_with_sampler(
        bytes,
        tex_addr_mode,
        mip_map_lod_bias,
    )
}

/// Decode DDS mip0 to uncompressed RGBA8 (reliable alpha blend in custom shaders).
pub fn decode_dds_to_rgba_image(bytes: &[u8]) -> Result<Image, String> {
    decode_dds_to_rgba_image_with_addr(bytes, None)
}

pub fn decode_dds_to_rgba_image_with_addr(
    bytes: &[u8],
    tex_addr_mode: Option<i32>,
) -> Result<Image, String> {
    decode_dds_to_rgba_image_with_sampler(bytes, tex_addr_mode, None)
}

pub fn decode_dds_to_rgba_image_with_sampler(
    bytes: &[u8],
    tex_addr_mode: Option<i32>,
    mip_map_lod_bias: Option<f32>,
) -> Result<Image, String> {
    use image::ImageFormat;
    let dyn_img =
        image::load_from_memory_with_format(bytes, ImageFormat::Dds).map_err(|e| e.to_string())?;
    let rgba = dyn_img.to_rgba8();
    let mut image = Image::new(
        Extent3d {
            width: rgba.width(),
            height: rgba.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    openrailsrs_bevy_scenery::textures::apply_msts_texture_sampler(
        &mut image,
        tex_addr_mode,
        mip_map_lod_bias,
    );
    Ok(image)
}

/// Optional MSTS install root (`Content/`) for `GLOBAL/SHAPES` lookup.
pub fn msts_content_root() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("OPENRAILSRS_MSTS_CONTENT") {
        let path = PathBuf::from(env);
        if path.is_dir() {
            return Some(path);
        }
    }
    let home = std::env::var_os("HOME")?;
    for rel in [
        "Documentos/Open Rails/Content",
        "Documents/Open Rails/Content",
    ] {
        let path = PathBuf::from(&home).join(rel);
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

/// All candidate MSTS/OR `Content/` roots (`OPENRAILSRS_MSTS_CONTENT` first, then default installs).
pub fn msts_content_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut push = |path: PathBuf| {
        if path.is_dir() && !roots.iter().any(|r| r == &path) {
            roots.push(path);
        }
    };
    if let Ok(env) = std::env::var("OPENRAILSRS_MSTS_CONTENT") {
        push(PathBuf::from(env));
    }
    if let Some(home) = std::env::var_os("HOME") {
        for rel in [
            "Documentos/Open Rails/Content",
            "Documents/Open Rails/Content",
        ] {
            push(PathBuf::from(&home).join(rel));
        }
    }
    roots
}

fn msts_root_for_route(route_dir: &Path) -> PathBuf {
    msts_content_root().unwrap_or_else(|| route_dir.to_path_buf())
}

/// All `GLOBAL/` asset roots under MSTS Content (OR uses per-route-pack trees like `Chiltern/GLOBAL/`).
pub fn global_assets_dirs(route_dir: &Path) -> Vec<PathBuf> {
    let Some(content) = msts_content_root() else {
        return Vec::new();
    };
    openrailsrs_bevy_scenery::textures::global_assets_dirs(route_dir, &content)
}

/// Route directory plus route-pack and root `GLOBAL` trees from [`msts_content_root`].
///
/// Orden: **ruta → pack → GLOBAL** (sin `sort`, para preservar precedencia).
pub fn shape_search_dirs(route_dir: &Path) -> Vec<PathBuf> {
    openrailsrs_bevy_scenery::textures::shape_search_dirs(
        route_dir,
        &msts_root_for_route(route_dir),
    )
}

/// First `GLOBAL/` root, if any (legacy helper).
pub fn global_assets_dir() -> Option<PathBuf> {
    msts_content_root()
        .map(|root| root.join("GLOBAL"))
        .filter(|p| p.is_dir())
}

/// Directories to search for `.ace` textures given a resolved shape path.
pub fn texture_search_dirs_for_shape(shape_path: &Path, route_dir: &Path) -> Vec<PathBuf> {
    openrailsrs_bevy_scenery::textures::texture_search_dirs_for_shape(
        shape_path,
        route_dir,
        &msts_root_for_route(route_dir),
    )
}

/// Texture search dirs for CVF sprites (includes sibling `CabView/` on the trainset).
pub fn cvf_texture_search_dirs(cvf_or_shape_path: &Path, route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = texture_search_dirs_for_shape(cvf_or_shape_path, route_dir);
    if let Some(cab_dir) = cvf_or_shape_path.parent() {
        if let Some(trainset) = cab_dir.parent() {
            for name in ["CabView", "Cabview", "CABVIEW", "Cabview3d"] {
                let sibling = trainset.join(name);
                if sibling.is_dir() {
                    dirs.push(sibling);
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Resolve a CVF `Graphic` path (`hornlever.ace`, `../../KIHA31/CabView/foo.ace`, …).
pub fn resolve_cvf_graphic_path(
    search_dirs: &[&Path],
    cab_dir: &Path,
    graphic: &str,
) -> Option<PathBuf> {
    let g = graphic.trim().trim_matches('"');
    if g.is_empty() {
        return None;
    }
    let normalized = g.replace('\\', "/");
    if normalized.contains("..") {
        let mut path = cab_dir.to_path_buf();
        for part in normalized.split('/') {
            match part {
                ".." => {
                    path.pop();
                }
                "." | "" => {}
                other => path.push(other),
            }
        }
        if path.is_file() {
            return Some(path);
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(found) = resolve_texture_path_in_dirs(search_dirs, name) {
                return Some(found);
            }
        }
    }
    resolve_texture_path_in_dirs(search_dirs, g)
}

/// Basename of a `.w` `FileName` (strips `SHAPES\\foo.s` / `foo.s`).
pub fn shape_file_basename(file_name: &str) -> &str {
    openrailsrs_bevy_scenery::textures::shape_file_basename(file_name)
}

/// Resolve `SHAPES/foo.s` under the route directory (case-insensitive on Linux).
pub fn resolve_shape_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    openrailsrs_bevy_scenery::textures::resolve_shape_path(route_dir, file_name)
}

/// Search several asset roots (route dir, scenario dir, …) for a shape file.
pub fn resolve_shape_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    openrailsrs_bevy_scenery::textures::resolve_shape_path_in_dirs(dirs, file_name)
}

/// Resolve using a pre-built index first, then fall back to per-directory search.
pub fn resolve_shape_path_with_index(
    index: &HashMap<String, PathBuf>,
    dirs: &[&Path],
    file_name: &str,
) -> Option<PathBuf> {
    openrailsrs_bevy_scenery::textures::resolve_shape_path_with_index(index, dirs, file_name)
}

/// Resolve `TEXTURES/foo.ace` under one asset root directory.
pub fn resolve_texture_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    openrailsrs_bevy_scenery::textures::resolve_texture_path_legacy(route_dir, file_name)
}

/// Search several asset roots for `TEXTURES/foo.ace`, returning the first match.
///
/// Use this instead of `resolve_texture_path` when a shape may live in a
/// directory other than the route root (e.g. a trainset folder).
pub fn resolve_texture_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    openrailsrs_bevy_scenery::textures::resolve_texture_path_legacy_in_dirs(dirs, file_name)
}

/// Root folder for a vehicle / cab shape's textures (viewer train/cab layout).
///
/// MSTS/Open Rails trainsets appear in both layouts:
/// - `<trainset>/SHAPES/car.s` with textures in `<trainset>/TEXTURES/`
/// - `<trainset>/car.s` with textures directly in `<trainset>/`
/// - `<trainset>/Cabview3d/cab.s` → trainset root (cab-specific; not in bevy-scenery)
///
/// Open Rails passes this as `ReferencePath` on `SharedShape`; exterior textures
/// resolve as `{ReferencePath}\{imageName}`, **not** from route `TEXTURES/`.
pub fn vehicle_texture_root_for_shape_path(shape_path: &Path) -> Option<&Path> {
    let parent = shape_path.parent()?;
    if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("SHAPES")
                || name.eq_ignore_ascii_case("CABVIEW3D")
                || name.eq_ignore_ascii_case("CABVIEW")
                || name.eq_ignore_ascii_case("CabView")
        })
    {
        parent.parent()
    } else {
        Some(parent)
    }
}

/// Texture search order for rolling-stock shapes (live train, replay markers).
///
/// Matches Open Rails `SharedShape.ReferencePath` + `GLOBAL` fallback — wagon folder
/// first, route `TEXTURES/` only as last resort (OR never uses route for ReferencePath).
pub fn vehicle_texture_search_dirs(shape_path: &Path, route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = vehicle_texture_root_for_shape_path(shape_path) {
        dirs.push(root.to_path_buf());
    }
    for global in global_assets_dirs(route_dir) {
        if !dirs.iter().any(|d| d.as_path() == global.as_path()) {
            dirs.push(global);
        }
    }
    if !dirs.iter().any(|d| d.as_path() == route_dir) {
        dirs.push(route_dir.to_path_buf());
    }
    dirs
}

/// Load and decode an `.ace` file into a Bevy image (mip 0 only).
pub fn load_ace_image(route_dir: &Path, file_name: &str) -> Option<Image> {
    let path = resolve_texture_path(route_dir, file_name)?;
    let ace = read_ace(&path).ok()?;
    Some(ace_to_scenery_image(&ace).0)
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
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    fallback_color: Color,
    train_exterior: bool,
) -> Option<ShapeRenderAsset> {
    load_shape_render_asset_and_file_from_path(
        shape_path,
        texture_dirs,
        camera_distance_m,
        meshes,
        images,
        materials,
        texture_cache,
        fallback_color,
        train_exterior,
    )
    .map(|(asset, _)| asset)
}

/// Like [`load_shape_render_asset_from_path`] but also returns the parsed [`ShapeFile`]
/// for rolling-stock exterior animation (#40) without a second disk parse.
#[allow(clippy::too_many_arguments)]
pub fn load_shape_render_asset_and_file_from_path(
    shape_path: &Path,
    texture_dirs: &[&Path],
    camera_distance_m: Option<f32>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    fallback_color: Color,
    train_exterior: bool,
) -> Option<(ShapeRenderAsset, ShapeFile)> {
    if train_exterior {
        set_train_shape_debug_scope(true);
    }
    let (shape, loaded) = load_shape_file_and_loaded(shape_path, camera_distance_m)?;
    let pbr = load_shape_pbr_sidecar(shape_path);
    let result = shape_render_asset_from_loaded(
        loaded,
        texture_dirs,
        meshes,
        images,
        materials,
        None,
        texture_cache,
        fallback_color,
        None,
        false,
        train_exterior,
        pbr.as_ref(),
    );
    if train_exterior {
        set_train_shape_debug_scope(false);
    }
    Some((result, shape))
}

/// Cab `CABVIEW3D` shapes: lit PBR + cab alpha/brighten (Bevy forward path; not unlit).
#[allow(clippy::too_many_arguments)]
pub fn load_cab_interior_render_asset_from_path(
    shape_path: &Path,
    texture_dirs: &[&Path],
    camera_distance_m: Option<f32>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<crate::or_cab_material::OrCabMaterial>,
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    fallback_color: Color,
    lever_matrices: &HashSet<usize>,
) -> Option<ShapeRenderAsset> {
    let loaded = load_cab_shape_from_path(shape_path, camera_distance_m, lever_matrices)?;
    // Cab interior uses OrCabMaterial / no PBR normal-map path (#44).
    Some(shape_render_asset_from_loaded(
        loaded,
        texture_dirs,
        meshes,
        images,
        materials,
        Some(or_materials),
        texture_cache,
        fallback_color,
        Some(true),
        true,
        false,
        None,
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
    or_materials: Option<&mut Assets<crate::or_cab_material::OrCabMaterial>>,
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    fallback_color: Color,
    lit_override: Option<bool>,
    cab_interior: bool,
    train_exterior: bool,
    pbr: Option<&ShapePbrSidecar>,
) -> ShapeRenderAsset {
    shape_render_asset_from_loaded_with_ace_cache(
        loaded,
        texture_dirs,
        meshes,
        images,
        materials,
        or_materials,
        texture_cache,
        &HashMap::new(),
        fallback_color,
        lit_override,
        cab_interior,
        train_exterior,
        pbr,
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
    mut or_materials: Option<&mut Assets<crate::or_cab_material::OrCabMaterial>>,
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    ace_cache: &HashMap<PathBuf, AceFile>,
    fallback_color: Color,
    lit_override: Option<bool>,
    cab_interior: bool,
    train_exterior: bool,
    pbr: Option<&ShapePbrSidecar>,
) -> ShapeRenderAsset {
    let triangle_count_total: usize = loaded
        .parts
        .iter()
        .map(|p| mesh_position_count(&p.mesh) / 3)
        .sum();
    let combined_mesh = meshes.add(loaded.mesh);

    let mut has_any_texture = false;
    let mut parts = Vec::with_capacity(loaded.parts.len().max(1));
    if loaded.parts.is_empty() {
        let (material, or_cab_material, has_texture, is_transparent, _) =
            material_for_shape_texture(
                texture_dirs,
                loaded.texture_file.as_deref(),
                None,
                -1, // no prim_state for combined fallback
                None,
                -1,
                images,
                materials,
                or_materials.as_deref_mut(),
                texture_cache,
                ace_cache,
                fallback_color,
                lit_override,
                None,
                cab_interior,
                train_exterior,
                None,
                None,
                None,
            );
        has_any_texture |= has_texture;
        parts.push(ShapePartAsset {
            prim_state_idx: -1,
            sub_object_idx: u32::MAX,
            sort_index: 0,
            cab_matrix_idx: None,
            mesh: combined_mesh.clone(),
            material,
            or_cab_material,
            has_texture,
            is_transparent,
            texture_name: loaded.texture_file.clone(),
            shader_name: None,
            light_mat_idx: None,
            solid_color: None,
            lever_pivot_at_mesh_center: false,
            lever_local_axis: None,
            bounds_center: None,
        });
    }
    for part in &loaded.parts {
        let tri_count = mesh_position_count(&part.mesh) / 3;
        let z_bias = part
            .z_bias
            .map(|z| z + sort_index_depth_nudge(part.sort_index));
        let (material, or_cab_material, has_texture, is_transparent, dual_blend) =
            material_for_shape_texture(
                texture_dirs,
                part.texture_file.as_deref(),
                part.shader_name.as_deref(),
                part.alpha_test_mode,
                z_bias,
                part.z_buf_mode,
                images,
                materials,
                or_materials.as_deref_mut(),
                texture_cache,
                ace_cache,
                fallback_color,
                lit_override,
                part.solid_color,
                cab_interior,
                train_exterior,
                part.light_mat_idx,
                part.tex_addr_mode,
                part.mip_map_lod_bias,
            );
        let mut mesh = part.mesh.clone();
        // StandardMaterial + sidecar only (skip OrCab / no albedo) — #44.
        if or_cab_material.is_none() && !cab_interior {
            if let (Some(sidecar), Some(albedo)) = (pbr, part.texture_file.as_deref()) {
                if let Some(nm_name) = sidecar.normal_map_for_albedo(albedo) {
                    if ensure_tangents_for_normal_mapping(&mut mesh) {
                        if let Some(nm_handle) = load_normal_map_image_handle(
                            texture_dirs,
                            nm_name,
                            images,
                            texture_cache,
                            ace_cache,
                            part.tex_addr_mode,
                        ) {
                            if let Some(mut mat) = materials.get_mut(&material) {
                                apply_standard_normal_map(
                                    &mut mat,
                                    nm_handle,
                                    sidecar.flip_normal_map_y,
                                );
                            }
                        }
                    }
                }
            }
        }
        if debug_materials_enabled() {
            if let Some(mat) = materials.get(&material) {
                log_shape_material_debug(
                    &ShapeMaterialDebugCtx {
                        shape_name: None,
                        prim_state_idx: part.prim_state_idx,
                        prim_state_name: None,
                        shader_name: part.shader_name.clone(),
                        texture_name: part.texture_file.clone(),
                    },
                    mat.alpha_mode,
                    part.z_bias,
                    mat.depth_bias,
                    part.z_buf_mode,
                    part.alpha_test_mode,
                    tri_count,
                );
            }
        }
        has_any_texture |= has_texture;
        let mesh_handle = meshes.add(mesh);
        parts.push(ShapePartAsset {
            prim_state_idx: part.prim_state_idx,
            sub_object_idx: part.sub_object_idx,
            sort_index: part.sort_index,
            cab_matrix_idx: part.cab_matrix_idx,
            mesh: mesh_handle.clone(),
            material: material.clone(),
            or_cab_material,
            has_texture,
            is_transparent,
            texture_name: part.texture_file.clone(),
            shader_name: part.shader_name.clone(),
            light_mat_idx: part.light_mat_idx,
            solid_color: part.solid_color,
            lever_pivot_at_mesh_center: part.lever_pivot_at_mesh_center,
            lever_local_axis: part.lever_local_axis,
            bounds_center: part.bounds_center,
        });
        // OR BlendATexDiff second pass: soft alpha with depth read (#101).
        if dual_blend {
            if let Some(base) = materials.get(&material) {
                let mut blend_mat = base.clone();
                blend_mat.alpha_mode = AlphaMode::Blend;
                blend_mat.depth_bias += 0.0002;
                let blend_handle = materials.add(blend_mat);
                parts.push(ShapePartAsset {
                    prim_state_idx: part.prim_state_idx,
                    sub_object_idx: part.sub_object_idx,
                    sort_index: part.sort_index,
                    cab_matrix_idx: part.cab_matrix_idx,
                    mesh: mesh_handle,
                    material: blend_handle,
                    or_cab_material: None,
                    has_texture,
                    is_transparent: true,
                    texture_name: part.texture_file.clone(),
                    shader_name: part.shader_name.clone(),
                    light_mat_idx: part.light_mat_idx,
                    solid_color: part.solid_color,
                    lever_pivot_at_mesh_center: part.lever_pivot_at_mesh_center,
                    lever_local_axis: part.lever_local_axis,
                    bounds_center: part.bounds_center,
                });
            }
        }
    }

    let asset = ShapeRenderAsset {
        combined_mesh,
        parts,
        has_texture: has_any_texture,
        has_night_subobj: false,
    };
    if debug_shape_stats_enabled() {
        log_shape_render_stats(&loaded.parts, triangle_count_total, &asset, materials);
    }
    asset
}

fn log_shape_render_stats(
    source_parts: &[LoadedShapePart],
    triangle_count: usize,
    asset: &ShapeRenderAsset,
    materials: &Assets<StandardMaterial>,
) {
    let mut opaque = 0usize;
    let mut blend = 0usize;
    let mut mask = 0usize;
    let mut suspicious_z = 0usize;
    for part in &asset.parts {
        if let Some(mat) = materials.get(&part.material) {
            match mat.alpha_mode {
                AlphaMode::Opaque => opaque += 1,
                AlphaMode::Blend
                | AlphaMode::Add
                | AlphaMode::Premultiplied
                | AlphaMode::Multiply
                | AlphaMode::AlphaToCoverage => blend += 1,
                AlphaMode::Mask(_) => mask += 1,
            }
            if mat.depth_bias.abs() > MSTS_Z_BIAS_CLAMP {
                suspicious_z += 1;
            }
        }
    }
    viewer_log!(
        "openrailsrs-viewer3d: shape-stats parts={} triangles={} textures={} \
         alpha opaque={opaque} blend={blend} mask={mask} suspicious_z_bias={suspicious_z} \
         mesh_valid={}",
        asset.parts.len(),
        triangle_count,
        asset.parts.iter().filter(|p| p.has_texture).count(),
        source_parts
            .iter()
            .all(|p| mesh_triangle_list_valid(&p.mesh))
    );
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

/// Append normal-map ACE/DDS paths from an optional PBR sidecar (#44).
pub fn collect_pbr_normal_map_texture_paths(
    pbr: Option<&ShapePbrSidecar>,
    texture_dirs: &[&Path],
) -> Vec<PathBuf> {
    let Some(sidecar) = pbr else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for name in sidecar.normal_map_filenames() {
        if let Some(path) = resolve_texture_path_in_dirs(texture_dirs, name) {
            paths.push(path);
        }
    }
    paths
}

/// Load a normal-map texture as a linear (`Rgba8Unorm`) image handle for PBR (#44).
fn load_normal_map_image_handle(
    texture_dirs: &[&Path],
    file_name: &str,
    images: &mut Assets<Image>,
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    ace_cache: &HashMap<PathBuf, AceFile>,
    tex_addr_mode: Option<i32>,
) -> Option<Handle<Image>> {
    let tex_path = resolve_texture_path_in_dirs(texture_dirs, file_name)?;
    // Distinct cache key from albedo sRGB entries (negative addr key).
    let addr_key = -(texture_cache_addr_key(tex_addr_mode)
        .saturating_abs()
        .max(1));
    if let Some(h) = texture_cache.get(&(tex_path.clone(), addr_key)) {
        return Some(h.clone());
    }
    let is_dds = tex_path.extension().map(|e| e.to_ascii_lowercase())
        == Some(std::ffi::OsString::from("dds"));
    let image = if is_dds {
        let bytes = std::fs::read(&tex_path).ok()?;
        let mut img = decode_dds_to_image_with_addr(&bytes, tex_addr_mode).ok()?;
        // Prefer linear sampling for normal maps when decoded as uncompressed RGBA.
        if img.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb {
            img.texture_descriptor.format = TextureFormat::Rgba8Unorm;
        }
        img
    } else {
        let ace = if let Some(ace) = ace_cache.get(&tex_path) {
            ace.clone()
        } else {
            read_ace(&tex_path).ok()?
        };
        // No albedo brighten — raw mip0 as linear.
        let mut image = Image::new(
            Extent3d {
                width: ace.width,
                height: ace.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            ace.mip0.clone(),
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::default(),
        );
        openrailsrs_bevy_scenery::textures::apply_tex_addr_mode(&mut image, tex_addr_mode);
        image
    };
    let handle = images.add(image);
    texture_cache.insert((tex_path, addr_key), handle.clone());
    Some(handle)
}

#[allow(clippy::too_many_arguments)]
fn finish_shape_textured_part(
    handle: Handle<Image>,
    rgba_for_luma: &[u8],
    tint: Color,
    alpha_mode: AlphaMode,
    is_transparent: bool,
    z_bias: f32,
    z_buf_mode: i32,
    lit: bool,
    shader_name: Option<&str>,
    texture_name: &str,
    solid_color: Option<[f32; 3]>,
    cab_interior: bool,
    train_exterior: bool,
    or_materials: Option<&mut Assets<crate::or_cab_material::OrCabMaterial>>,
    materials: &mut Assets<StandardMaterial>,
    light_mat_idx: Option<i32>,
) -> (
    Handle<StandardMaterial>,
    Option<Handle<crate::or_cab_material::OrCabMaterial>>,
    bool,
    bool,
) {
    // Note: dual-pass follow-up is decided by the caller (#101).
    let z_bias = clamp_msts_z_bias_for_bevy(Some(z_bias), None);
    let use_or_cab =
        cab_interior && crate::or_cab_material::or_cab_shaders_enabled() && or_materials.is_some();
    if use_or_cab {
        let or_materials = or_materials.expect("checked is_some");
        let or_mat = crate::or_cab_material::create_or_cab_material(
            or_materials,
            handle.clone(),
            tint,
            alpha_mode,
            shader_name,
            light_mat_idx,
        );
        let mut placeholder = StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            alpha_mode,
            depth_bias: z_bias,
            fog_enabled: false,
            ..default()
        };
        apply_z_buf_mode(&mut placeholder, z_buf_mode);
        apply_train_debug_material_overrides(&mut placeholder);
        let placeholder = materials.add(placeholder);
        return (placeholder, Some(or_mat), true, is_transparent);
    }
    let mut mat = if train_exterior && !cab_interior {
        train_exterior_material_with_texture_ex(
            tint,
            handle,
            rgba_for_luma,
            alpha_mode,
            z_bias,
            lit,
            shader_name,
            light_mat_idx,
            texture_name,
            solid_color,
        )
    } else {
        cab_or_scenery_material_with_texture_ex(
            tint,
            handle,
            rgba_for_luma,
            alpha_mode,
            z_bias,
            lit,
            shader_name,
            light_mat_idx,
            texture_name,
            solid_color,
            cab_interior,
        )
    };
    apply_z_buf_mode(&mut mat, z_buf_mode);
    apply_train_debug_material_overrides(&mut mat);
    let material = materials.add(mat);
    (material, None, true, is_transparent)
}

fn scenery_dds_alpha_mode(
    dds_path: &Path,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> AlphaMode {
    match alpha_test_mode {
        0 => return AlphaMode::Opaque,
        1 => return AlphaMode::Mask(OR_MSTS_ALPHA_TEST_CUTOFF),
        2 => return AlphaMode::Blend,
        _ => {}
    }
    let has_alpha = matches!(dds_alpha_type(dds_path), Some(DdsAlpha::Full) | None);
    if !has_alpha {
        return AlphaMode::Opaque;
    }
    if shader_name
        .map(shape_shader_requests_blending)
        .unwrap_or(false)
        && texture_name_suggests_transparency(texture_file)
    {
        AlphaMode::Blend
    } else {
        AlphaMode::Opaque
    }
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
    alpha_test_mode: i32,
    z_bias: Option<f32>,
    z_buf_mode: i32,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: Option<&mut Assets<crate::or_cab_material::OrCabMaterial>>,
    texture_cache: &mut HashMap<(PathBuf, i32), Handle<Image>>,
    ace_cache: &HashMap<PathBuf, AceFile>,
    fallback_color: Color,
    lit_override: Option<bool>,
    solid_color: Option<[f32; 3]>,
    cab_interior: bool,
    train_exterior: bool,
    light_mat_idx: Option<i32>,
    tex_addr_mode: Option<i32>,
    mip_map_lod_bias: Option<f32>,
) -> (
    Handle<StandardMaterial>,
    Option<Handle<crate::or_cab_material::OrCabMaterial>>,
    bool,
    bool,
    // dual_blend: spawn a second Blend draw (OR BlendATexDiff dual-pass #101).
    bool,
) {
    let lit = lit_override.unwrap_or_else(scenery_materials_lit);
    let addr_key = texture_cache_sampler_key(tex_addr_mode, mip_map_lod_bias);
    if let Some(tex_name) = texture_file {
        match resolve_texture_path_in_dirs(texture_dirs, tex_name) {
            None => {}
            Some(tex_path) => {
                let is_dds = tex_path.extension().map(|e| e.to_ascii_lowercase())
                    == Some(std::ffi::OsString::from("dds"));
                if is_dds {
                    if let Ok(bytes) = std::fs::read(&tex_path) {
                        let alpha_mode = if cab_interior {
                            cab_dds_alpha_mode(&tex_path, tex_name, shader_name, alpha_test_mode)
                        } else {
                            scenery_dds_alpha_mode(
                                &tex_path,
                                tex_name,
                                shader_name,
                                alpha_test_mode,
                            )
                        };
                        let use_rgba =
                            cab_interior && matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add);
                        let image = if use_rgba {
                            decode_dds_to_rgba_image_with_sampler(
                                &bytes,
                                tex_addr_mode,
                                mip_map_lod_bias,
                            )
                        } else {
                            decode_dds_to_image_with_sampler(
                                &bytes,
                                tex_addr_mode,
                                mip_map_lod_bias,
                            )
                        };
                        if let Ok(image) = image {
                            let handle = texture_cache
                                .entry((tex_path.clone(), addr_key))
                                .or_insert_with(|| images.add(image))
                                .clone();
                            let is_transparent =
                                !matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_));
                            let tint = apply_msts_vertex_tint(
                                if cab_interior {
                                    Color::WHITE
                                } else {
                                    scenery_base_tint(lit)
                                },
                                solid_color,
                                shader_name,
                            );
                            let (m, o, ht, it) = finish_shape_textured_part(
                                handle,
                                &[],
                                tint,
                                alpha_mode,
                                is_transparent,
                                z_bias.unwrap_or(0.0),
                                z_buf_mode,
                                lit,
                                shader_name,
                                tex_name,
                                solid_color,
                                cab_interior,
                                train_exterior,
                                or_materials,
                                materials,
                                light_mat_idx,
                            );
                            return (m, o, ht, it, false);
                        }
                    }
                }

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
                    let (alpha_mode, dual_blend) = if cab_interior {
                        (
                            cab_shape_alpha_mode(&ace, tex_name, shader_name, alpha_test_mode),
                            false,
                        )
                    } else {
                        let passes = blend_alpha_passes_from_prim_state(
                            &ace,
                            tex_name,
                            shader_name,
                            alpha_test_mode,
                        );
                        (passes[0].alpha_mode, passes.len() > 1)
                    };
                    let is_transparent =
                        dual_blend || !matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_));
                    let (rgba, pixel_brightened) = if cab_interior {
                        brighten_cab_ace_rgba(&ace.mip0)
                    } else {
                        brighten_dark_ace_rgba(&ace.mip0)
                    };
                    let tint = if cab_interior {
                        apply_msts_vertex_tint(
                            cab_albedo_tint(pixel_brightened),
                            solid_color,
                            shader_name,
                        )
                    } else {
                        scenery_albedo_tint(pixel_brightened, lit)
                    };
                    let image = ace_rgba_to_image_with_sampler(
                        ace.width,
                        ace.height,
                        &rgba,
                        tex_addr_mode,
                        mip_map_lod_bias,
                    );
                    let handle = texture_cache
                        .entry((tex_path, addr_key))
                        .or_insert_with(|| images.add(image))
                        .clone();
                    let (m, o, ht, it) = finish_shape_textured_part(
                        handle,
                        &rgba,
                        tint,
                        alpha_mode,
                        is_transparent,
                        z_bias.unwrap_or(0.0),
                        z_buf_mode,
                        lit,
                        shader_name,
                        tex_name,
                        solid_color,
                        cab_interior,
                        train_exterior,
                        or_materials,
                        materials,
                        light_mat_idx,
                    );
                    return (m, o, ht, it, dual_blend);
                }
            }
        }
    }

    // Untextured fallback: the emissive lift fakes brightness for the unlit path only.
    let fallback_emissive = if lit {
        LinearRgba::BLACK
    } else {
        LinearRgba::from(fallback_color) * 0.35
    };
    let mut fallback_mat = StandardMaterial {
        base_color: fallback_color,
        emissive: fallback_emissive,
        perceptual_roughness: 0.75,
        metallic: 0.1,
        double_sided: true,
        depth_bias: z_bias.unwrap_or(0.0),
        ..default()
    };
    apply_z_buf_mode(&mut fallback_mat, z_buf_mode);
    if train_exterior && !cab_interior {
        apply_train_exterior_culling(&mut fallback_mat);
    }
    apply_train_debug_material_overrides(&mut fallback_mat);
    let material = materials.add(finalize_scenery_material(fallback_mat, lit));
    (material, None, false, false, false)
}

/// Alpha mode for CABVIEW3D interiors (paridad `openrailsrs-render3d` / OR `TexDiff`).
///
/// MSTS cab `.ace` often carry an alpha channel without meaning cutout; defaulting to
/// `Mask` leaves only edge pixels visible (black silhouettes on a flat background).
fn cab_shape_alpha_mode(
    ace: &AceFile,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> AlphaMode {
    cab_shape_alpha_mode_with_stats(
        shape_alpha_stats(ace),
        texture_file,
        shader_name,
        alpha_test_mode,
    )
}

fn cab_dds_alpha_mode(
    dds_path: &Path,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> AlphaMode {
    let has_alpha = matches!(dds_alpha_type(dds_path), Some(DdsAlpha::Full) | None);
    cab_shape_alpha_mode_with_stats(
        ShapeAlphaStats {
            has_any: has_alpha,
            has_semitransparent: has_alpha,
        },
        texture_file,
        shader_name,
        alpha_test_mode,
    )
}

fn cab_shape_alpha_mode_with_stats(
    alpha: ShapeAlphaStats,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> AlphaMode {
    match alpha_test_mode {
        1 => return AlphaMode::Mask(OR_MSTS_ALPHA_TEST_CUTOFF),
        2 => return AlphaMode::Blend,
        _ => {}
    }

    if let Some(shader) = shader_name {
        if shader.eq_ignore_ascii_case("AddATex") || shader.eq_ignore_ascii_case("AddATexDiff") {
            return AlphaMode::Add;
        }
        let blend_shader = shader.eq_ignore_ascii_case("BlendATex")
            || shader.eq_ignore_ascii_case("BlendATexDiff");
        if blend_shader {
            if alpha.has_semitransparent && texture_name_suggests_transparency(texture_file) {
                return AlphaMode::Blend;
            }
            if alpha.has_any {
                return AlphaMode::Mask(0.5);
            }
            return AlphaMode::Opaque;
        }
        // TexDiff / Tex / HalfBright / FullBright: draw opaque unless explicitly alpha-tested.
        if alpha_test_mode != 1 {
            if !alpha.has_any {
                return AlphaMode::Opaque;
            }
            if alpha.has_semitransparent && texture_name_suggests_transparency(texture_file) {
                return AlphaMode::Blend;
            }
            return AlphaMode::Opaque;
        }
    }

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

/// Process-wide count of successful `ShapeFile::from_path` calls via this module's loaders.
/// Used to verify WORLD spawn parses each unique `.s` once (#57).
static SHAPE_FILE_PARSE_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn shape_file_parse_count() -> usize {
    SHAPE_FILE_PARSE_COUNT.load(Ordering::Relaxed)
}

pub fn reset_shape_file_parse_count() {
    SHAPE_FILE_PARSE_COUNT.store(0, Ordering::Relaxed);
}

fn parse_shape_file(path: &Path) -> Option<ShapeFile> {
    let shape = ShapeFile::from_path(path).ok()?;
    SHAPE_FILE_PARSE_COUNT.fetch_add(1, Ordering::Relaxed);
    Some(shape)
}

/// Build render geometry from an already-parsed `ShapeFile` (no disk I/O).
pub fn loaded_shape_from_shape_file(
    shape: &ShapeFile,
    camera_distance_m: Option<f32>,
) -> Option<LoadedShape> {
    loaded_shape_from_shape_file_with_options(
        shape,
        camera_distance_m,
        openrailsrs_bevy_scenery::shapes::MeshPartBuildOptions::default(),
    )
}

/// Like [`loaded_shape_from_shape_file`] with mesh bake options (night sub-objects, …).
pub fn loaded_shape_from_shape_file_with_options(
    shape: &ShapeFile,
    camera_distance_m: Option<f32>,
    options: openrailsrs_bevy_scenery::shapes::MeshPartBuildOptions,
) -> Option<LoadedShape> {
    let parts = match camera_distance_m {
        Some(d) => build_mesh_parts_from_shape_at_distance_with_options(shape, d, options),
        None => {
            let mut parts = Vec::new();
            for level in openrailsrs_bevy_scenery::shapes::closest_lod_levels(shape) {
                parts.extend(
                    openrailsrs_bevy_scenery::shapes::build_mesh_parts_from_shape_lod_with_options(
                        shape, level, options,
                    ),
                );
            }
            parts
        }
    };
    let mesh = match camera_distance_m {
        Some(d) => build_mesh_from_shape_at_distance(shape, d)?,
        None => build_mesh_from_shape(shape)?,
    };
    let texture_file = primary_texture_filename(shape);
    Some(LoadedShape {
        mesh,
        texture_file,
        parts,
    })
}

/// Single parse → typed file (LOD/anim) + render mesh. Prefer this over separate loads.
///
/// When the sibling `.sd` declares `ESD_SubObj`, keeps per-sub-object parts so night
/// geometry (index 1) can be filtered of day (#95).
pub fn load_shape_file_and_loaded(
    path: &Path,
    camera_distance_m: Option<f32>,
) -> Option<(ShapeFile, LoadedShape)> {
    let shape = parse_shape_file(path)?;
    let options = world_mesh_options_for_shape(path);
    let loaded = loaded_shape_from_shape_file_with_options(&shape, camera_distance_m, options)?;
    Some((shape, loaded))
}

/// Attach `.sd` night-subobj flag after building GPU assets (#95).
pub fn apply_shape_descriptor_to_asset(shape_path: &Path, asset: &mut ShapeRenderAsset) {
    asset.has_night_subobj = ShapeDescriptor::load_for_shape(shape_path).has_night_subobj;
}

/// Whether a shape part should spawn for the current day/night (#95).
pub fn shape_part_visible_for_day_night(
    asset: &ShapeRenderAsset,
    part: &ShapePartAsset,
    is_day: bool,
) -> bool {
    night_subobj_part_visible(asset.has_night_subobj, part.sub_object_idx, is_day)
}

/// Load cab interior shape; lever matrix indices omit leaf bone from vertex bake (CVF anim).
pub fn load_cab_shape_from_path(
    path: &Path,
    camera_distance_m: Option<f32>,
    lever_matrices: &HashSet<usize>,
) -> Option<LoadedShape> {
    let shape = parse_shape_file(path)?;
    let level = match camera_distance_m {
        Some(d) => lod_level_for_distance(&shape, d).or_else(|| closest_lod_level(&shape))?,
        None => closest_lod_level(&shape)?,
    };
    let parts = build_mesh_parts_from_shape_lod_cab(&shape, level, lever_matrices);
    let mesh = build_mesh_from_shape_lod(&shape, level)?;
    let texture_file = primary_texture_filename(&shape);
    Some(LoadedShape {
        mesh,
        texture_file,
        parts,
    })
}

/// Load shape mesh and discover its primary texture filename, if any.
///
/// When `camera_distance_m` is set, picks a coarser LOD farther from the camera.
pub fn load_shape_from_path(path: &Path, camera_distance_m: Option<f32>) -> Option<LoadedShape> {
    load_shape_file_and_loaded(path, camera_distance_m).map(|(_, loaded)| loaded)
}

/// Load a shape as one mesh per `prim_state_idx`.
pub fn load_shape_parts_from_path(
    path: &Path,
    camera_distance_m: Option<f32>,
) -> Option<Vec<LoadedShapePart>> {
    let shape = parse_shape_file(path)?;
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

    #[test]
    fn night_subobj_part_hidden_of_day_on_asset() {
        let asset = ShapeRenderAsset {
            combined_mesh: Handle::default(),
            parts: vec![
                ShapePartAsset {
                    prim_state_idx: 0,
                    sub_object_idx: 0,
                    sort_index: 0,
                    cab_matrix_idx: None,
                    mesh: Handle::default(),
                    material: Handle::default(),
                    or_cab_material: None,
                    has_texture: false,
                    is_transparent: false,
                    texture_name: None,
                    shader_name: None,
                    light_mat_idx: None,
                    solid_color: None,
                    lever_pivot_at_mesh_center: false,
                    lever_local_axis: None,
                    bounds_center: None,
                },
                ShapePartAsset {
                    prim_state_idx: 1,
                    sub_object_idx: 1,
                    sort_index: 1,
                    cab_matrix_idx: None,
                    mesh: Handle::default(),
                    material: Handle::default(),
                    or_cab_material: None,
                    has_texture: false,
                    is_transparent: false,
                    texture_name: None,
                    shader_name: None,
                    light_mat_idx: None,
                    solid_color: None,
                    lever_pivot_at_mesh_center: false,
                    lever_local_axis: None,
                    bounds_center: None,
                },
            ],
            has_texture: false,
            has_night_subobj: true,
        };
        assert!(shape_part_visible_for_day_night(
            &asset,
            &asset.parts[0],
            true
        ));
        assert!(!shape_part_visible_for_day_night(
            &asset,
            &asset.parts[1],
            true
        ));
        assert!(shape_part_visible_for_day_night(
            &asset,
            &asset.parts[1],
            false
        ));
    }

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
    fn load_shape_file_and_loaded_parses_once_per_path() {
        let path = minimal_shape_fixture();
        reset_shape_file_parse_count();
        let (shape, loaded) =
            load_shape_file_and_loaded(&path, None).expect("parse+load minimal.s");
        assert_eq!(shape_file_parse_count(), 1);
        assert!(!loaded.parts.is_empty() || loaded.mesh.count_vertices() > 0);
        // LOD/anim consumers reuse the same ShapeFile without a second disk parse.
        let again = loaded_shape_from_shape_file(&shape, Some(50.0)).expect("lod from file");
        assert_eq!(shape_file_parse_count(), 1);
        assert!(again.mesh.count_vertices() > 0);
    }

    #[test]
    fn world_style_batch_does_not_double_parse_unique_paths() {
        let path = minimal_shape_fixture();
        let paths = [path.clone(), path.clone(), path];
        reset_shape_file_parse_count();
        let mut files = HashMap::new();
        let mut loaded_n = 0usize;
        for p in paths.iter().collect::<HashSet<_>>().into_iter() {
            if let Some((shape, _loaded)) = load_shape_file_and_loaded(p, None) {
                files.insert(p.clone(), shape);
                loaded_n += 1;
            }
        }
        assert_eq!(loaded_n, 1);
        assert_eq!(files.len(), 1);
        assert_eq!(shape_file_parse_count(), 1);
    }

    #[test]
    fn shape_hierarchy_matrix_translates_primitive_vertices_like_openrails() {
        let ascii = r#"
        ( shape
            ( shader_names 1 "TexDiff" )
            ( points 3
                ( point 0 0 0 )
                ( point 1 0 0 )
                ( point 0 1 0 )
            )
            ( normals 1 ( vector 0 1 0 ) )
            ( prim_states 1
                ( prim_state "body" 0 0 ( tex_idxs 0 ) 0 0 0 0 )
            )
            ( vtx_states 1
                ( vtx_state 0 1 0 )
            )
            ( lod_controls 1
                ( lod_control
                    ( distance_levels_header )
                    ( distance_levels 1
                        ( distance_level
                            ( distance_level_header
                                ( dlevel_selection 100 )
                                ( hierarchy 2 -1 0 )
                            )
                            ( sub_objects 1
                                ( sub_object
                                    ( vertices 3 )
                                    ( primitives 1
                                        ( prim_state_idx 0 )
                                        ( indexed_trilist
                                            ( vertex_idxs 3 0 1 2 )
                                        )
                                    )
                                )
                            )
                        )
                    )
                )
            )
            ( matrices 2
                ( matrix "ROOT"
                    1 0 0
                    0 1 0
                    0 0 1
                    0 0 0
                )
                ( matrix "CHILD"
                    1 0 0
                    0 1 0
                    0 0 1
                    10 0 0
                )
            )
        )
        "#;
        let ast = openrailsrs_formats::parse_first_from_first_paren(ascii).expect("parse AST");
        let shape = ShapeFile::from_ast(&ast).expect("shape");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let (min, max) = mesh_aabb(&mesh).expect("AABB");
        assert!((min.x - 10.0).abs() < 1e-4, "min={min:?} max={max:?}");
        assert!((max.x - 11.0).abs() < 1e-4, "min={min:?} max={max:?}");
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
    fn pullman_cab_mesh_uvs_are_not_degenerate() {
        let cab = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !cab.is_file() {
            return;
        }
        let loaded = load_shape_from_path(&cab, Some(2.0)).expect("cab shape");
        let mut degenerate = 0usize;
        for part in &loaded.parts {
            if let Some((mn, mx)) = mesh_uv_aabb(&part.mesh) {
                if mesh_uv_degenerate(mn, mx) {
                    degenerate += 1;
                }
            }
        }
        assert_eq!(
            degenerate,
            0,
            "cab parts should have varying UVs, {degenerate}/{} degenerate",
            loaded.parts.len()
        );
    }

    #[test]
    fn pullman_cab_texture_slots_match_or_resolution() {
        let cab = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !cab.is_file() {
            return;
        }
        let shape = openrailsrs_formats::ShapeFile::from_path(&cab).expect("parse cab");
        let loaded = load_shape_from_path(&cab, Some(2.0)).expect("cab shape");
        let mut mismatches = 0_u32;
        for part in &loaded.parts {
            let resolved = texture_for_prim_state(&shape, part.prim_state_idx);
            if part.texture_file != resolved {
                mismatches += 1;
            }
        }
        assert!(
            loaded
                .parts
                .iter()
                .filter(|p| p.texture_file.is_some())
                .count()
                >= 30,
            "expected textured cab parts"
        );
        assert_eq!(
            mismatches, 0,
            "loaded texture_file must match texture_for_prim_state"
        );
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
            false,
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

    #[test]
    fn pbr_sidecar_adds_tangents_and_normal_map() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shape_src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-bevy-scenery/assets/msts/minimal.s");
        let shape_path = dir.path().join("minimal.s");
        std::fs::copy(&shape_src, &shape_path).expect("copy shape");
        // Flat OpenGL-style normal (0.5, 0.5, 1.0) × 255.
        let nm_pixel = [128u8, 128, 255, 255];
        let mut nm_rgba = Vec::new();
        for _ in 0..4 {
            nm_rgba.extend_from_slice(&nm_pixel);
        }
        let albedo = [
            200u8, 180, 160, 255, 200, 180, 160, 255, 200, 180, 160, 255, 200, 180, 160, 255,
        ];
        write_synthetic_ace(&dir.path().join("wagon.ace"), &albedo);
        write_synthetic_ace(&dir.path().join("wagon_n.ace"), &nm_rgba);
        std::fs::write(
            dir.path().join("minimal.s.pbr.json"),
            r#"{"normal_maps":{"wagon.ace":"wagon_n.ace"},"flip_normal_map_y":false}"#,
        )
        .expect("sidecar");

        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();
        let asset = load_shape_render_asset_from_path(
            &shape_path,
            &[dir.path()],
            None,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            Color::srgb(0.5, 0.5, 0.5),
            false,
        )
        .expect("pbr render asset");

        let part = asset.parts.first().expect("part");
        let mesh = meshes.get(&part.mesh).expect("mesh");
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(),
            "sidecar should generate tangents"
        );
        let mat = materials.get(&part.material).expect("material");
        assert!(
            mat.normal_map_texture.is_some(),
            "sidecar should assign normal_map_texture"
        );
        assert!(!mat.flip_normal_map_y);
    }

    #[test]
    fn without_pbr_sidecar_mesh_has_no_tangents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shape_src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-bevy-scenery/assets/msts/minimal.s");
        let shape_path = dir.path().join("minimal.s");
        std::fs::copy(&shape_src, &shape_path).expect("copy shape");
        let albedo = [
            200u8, 180, 160, 255, 200, 180, 160, 255, 200, 180, 160, 255, 200, 180, 160, 255,
        ];
        write_synthetic_ace(&dir.path().join("wagon.ace"), &albedo);

        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();
        let asset = load_shape_render_asset_from_path(
            &shape_path,
            &[dir.path()],
            None,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            Color::srgb(0.5, 0.5, 0.5),
            false,
        )
        .expect("classic render asset");

        let part = asset.parts.first().expect("part");
        let mesh = meshes.get(&part.mesh).expect("mesh");
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_none(),
            "classic MSTS path must not add tangents"
        );
        let mat = materials.get(&part.material).expect("material");
        assert!(mat.normal_map_texture.is_none());
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
    fn brighten_dark_ace_rgba_lifts_near_black_atlas() {
        let rgba = vec![1u8, 2, 1, 255, 3, 1, 2, 255];
        let (out, brightened) = brighten_dark_ace_rgba(&rgba);
        assert!(brightened);
        let mean = ace_mean_luma(&out);
        assert!(
            mean >= SCENERY_TEXTURE_TARGET_LUMA * 0.85,
            "expected ~{SCENERY_TEXTURE_TARGET_LUMA}, got {mean}"
        );
    }

    #[test]
    fn brighten_dark_ace_rgba_leaves_bright_atlas_unchanged() {
        let rgba = vec![200u8, 180, 160, 255, 190, 170, 150, 255];
        let (out, brightened) = brighten_dark_ace_rgba(&rgba);
        assert!(!brightened);
        assert_eq!(out, rgba);
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

        let (handle, has_texture, is_transparent, dual_blend) = {
            let (handle, _or, has_texture, is_transparent, dual_blend) = material_for_shape_texture(
                &[route.as_path()],
                Some("alpha_test.ace"),
                Some("BlendATexDiff"),
                -1, // no explicit alpha_test_mode → heuristic path
                None,
                -1,
                &mut images,
                &mut materials,
                None,
                &mut texture_cache,
                &HashMap::new(),
                Color::srgb(0.95, 0.25, 0.85),
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            );
            (handle, has_texture, is_transparent, dual_blend)
        };

        let material = materials.get(&handle).expect("material");
        assert!(has_texture);
        assert!(is_transparent);
        assert!(
            dual_blend,
            "BlendATexDiff with mid-alpha must dual-pass (#101)"
        );
        // First pass is OR ReferenceAlpha=250 (Mask); Blend follow-up is spawned by caller.
        assert!(matches!(material.alpha_mode, AlphaMode::Mask(_)));

        let _ = std::fs::remove_file(texture);
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn cab_shape_alpha_mode_texdiff_stays_opaque_with_alpha_channel() {
        let route = std::env::temp_dir().join(format!(
            "openrailsrs_cab_alpha_opaque_{}",
            std::process::id()
        ));
        let textures = route.join("TEXTURES");
        std::fs::create_dir_all(&textures).unwrap();
        let texture = textures.join("cab_panel.ace");
        let rgba: Vec<u8> = (0..16 * 16)
            .flat_map(|i| {
                let a = if i % 17 == 0 { 0 } else { 255 };
                [200_u8, 180, 160, a]
            })
            .collect();
        write_synthetic_ace(&texture, &rgba);
        let ace = read_ace(&texture).expect("ace");
        let mode = cab_shape_alpha_mode(&ace, "cab_panel.ace", Some("TexDiff"), -1);
        assert!(matches!(mode, AlphaMode::Opaque));
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn cab_shape_alpha_mode_addatex_uses_additive() {
        let route =
            std::env::temp_dir().join(format!("openrailsrs_cab_alpha_add_{}", std::process::id()));
        let textures = route.join("TEXTURES");
        std::fs::create_dir_all(&textures).unwrap();
        let texture = textures.join("glow.ace");
        write_synthetic_ace(&texture, &[255, 255, 255, 255]);
        let ace = read_ace(&texture).expect("ace");
        let mode = cab_shape_alpha_mode(&ace, "glow.ace", Some("AddATex"), -1);
        assert!(matches!(mode, AlphaMode::Add));
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn apply_msts_vertex_tint_multiplies_texdiff_albedo() {
        let base = Color::linear_rgb(2.0, 2.0, 2.0);
        let tinted = apply_msts_vertex_tint(base, Some([0.5, 0.8, 1.0]), Some("TexDiff"));
        let c = tinted.to_linear();
        assert!((c.red - 1.0).abs() < 0.02);
        assert!((c.green - 1.6).abs() < 0.02);
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

        // OR: TexDiff ignores ACE alpha unless prim_state requests alpha test.
        let (opaque_handle, _) = {
            let (handle, _or, _has_texture, is_transparent, _) = material_for_shape_texture(
                &[route.as_path()],
                Some("body.ace"),
                Some("TexDiff"),
                -1,
                None,
                -1,
                &mut images,
                &mut materials,
                None,
                &mut texture_cache,
                &HashMap::new(),
                Color::srgb(0.95, 0.25, 0.85),
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            );
            (handle, is_transparent)
        };
        assert!(matches!(
            materials.get(&opaque_handle).expect("mat").alpha_mode,
            AlphaMode::Opaque
        ));

        let (mask_handle, _) = {
            let (handle, _or, _has_texture, _is_transparent, _) = material_for_shape_texture(
                &[route.as_path()],
                Some("body.ace"),
                Some("TexDiff"),
                1, // explicit alpha test
                None,
                -1,
                &mut images,
                &mut materials,
                None,
                &mut texture_cache,
                &HashMap::new(),
                Color::srgb(0.95, 0.25, 0.85),
                None,
                None,
                false,
                false,
                None,
                None,
                None,
            );
            (handle, ())
        };
        assert!(matches!(
            materials.get(&mask_handle).expect("mat").alpha_mode,
            AlphaMode::Mask(_)
        ));

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
    fn resolve_texture_path_strips_msts_path_prefix() {
        let route = std::env::temp_dir().join("openrailsrs_tex_basename");
        let textures = route.join("TEXTURES");
        std::fs::create_dir_all(&textures).unwrap();
        let ace_file = textures.join("ballast.ace");
        std::fs::write(&ace_file, b"ACE").unwrap();

        let found = resolve_texture_path(&route, r"TEXTURES\ballast.ace");
        assert_eq!(found.as_ref(), Some(&ace_file));

        let _ = std::fs::remove_file(ace_file);
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn resolve_texture_path_dds_fallback() {
        let route = std::env::temp_dir().join("openrailsrs_dds_fallback");
        let textures = route.join("TEXTURES");
        std::fs::create_dir_all(&textures).unwrap();
        let dds_file = textures.join("glass.dds");
        std::fs::write(&dds_file, b"DDS_RAW_BYTES").unwrap();

        let found = resolve_texture_path(&route, "glass.ace");
        assert_eq!(found, Some(dds_file));

        let _ = std::fs::remove_file(textures.join("glass.dds"));
        let _ = std::fs::remove_dir_all(route);
    }

    #[test]
    fn window_dds_decodes_with_semitransparent_alpha() {
        let path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/Window_front.dds",
        );
        if !path.is_file() {
            return;
        }
        let bytes = std::fs::read(&path).expect("read dds");
        let img = decode_dds_to_rgba_image(&bytes).expect("decode rgba");
        assert_eq!(img.texture_descriptor.size.width, 1024);
        let data = img.data.as_ref().expect("pixel data");
        let semi = data
            .chunks_exact(4)
            .filter(|px| (9..250).contains(&px[3]))
            .count();
        assert!(
            semi > 100_000,
            "window glass should be mostly semi-transparent, got {semi} px"
        );
    }

    #[test]
    fn cab_dds_window_textures_use_blend_alpha() {
        let dds = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/Window_front.dds",
        );
        if !dds.is_file() {
            return;
        }
        assert_eq!(
            cab_dds_alpha_mode(&dds, "Window_front.ace", Some("BlendATexDiff"), -1),
            AlphaMode::Blend
        );
    }

    #[test]
    fn resolve_texture_path_dds_fallback_in_cabview3d() {
        let cab_dir = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d",
        );
        if !cab_dir.is_dir() {
            return;
        }
        for ace_name in ["Window_front.ace", "Window_front4.ace"] {
            let found = resolve_texture_path(&cab_dir, ace_name);
            assert!(
                found.is_some(),
                "{ace_name} should resolve to .dds in Cabview3d"
            );
            assert!(
                found
                    .as_ref()
                    .and_then(|p| p.extension())
                    .is_some_and(|e| e.eq_ignore_ascii_case("dds")),
                "expected .dds fallback for {ace_name}, got {:?}",
                found
            );
        }
    }

    #[test]
    fn pullman_cab_window_parts_use_or_shader_on_dds() {
        let cab = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !cab.is_file() {
            return;
        }
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let tex_dirs: Vec<PathBuf> = texture_search_dirs_for_shape(&cab, &route);
        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut or_materials = Assets::<crate::or_cab_material::OrCabMaterial>::default();
        let mut texture_cache = HashMap::new();
        let asset = load_cab_interior_render_asset_from_path(
            &cab,
            &tex_refs,
            Some(2.0),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut or_materials,
            &mut texture_cache,
            Color::srgb(0.35, 0.38, 0.42),
            &HashSet::new(),
        )
        .expect("cab asset");
        let windows: Vec<_> = asset
            .parts
            .iter()
            .filter(|p| {
                p.texture_name
                    .as_deref()
                    .is_some_and(|t| t.to_ascii_lowercase().contains("window"))
            })
            .collect();
        assert!(
            windows.len() >= 2,
            "expected at least two window parts, got {}",
            windows.len()
        );
        for part in windows {
            assert!(
                part.or_cab_material.is_some(),
                "window part prim={} should use OrCabMaterial",
                part.prim_state_idx
            );
            assert!(
                part.is_transparent,
                "window prim={} should be blend-transparent",
                part.prim_state_idx,
            );
            assert!(part.has_texture);
        }
    }

    #[test]
    fn resolve_texture_path_finds_ace_in_cabview3d_folder() {
        let cab_dir = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d",
        );
        if !cab_dir.is_dir() {
            return;
        }
        let found = resolve_texture_path(&cab_dir, "Cab1.ace");
        assert!(
            found.is_some(),
            "CABVIEW3D stores .ace next to .s, not only in TEXTURES/"
        );
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
    fn vehicle_shape_keeps_unit_scale() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let transform = vehicle_shape_local_transform(&mesh, 0.0, 18.0);
        assert!((transform.scale - Vec3::ONE).length() < 1e-4);
        let rotated = transform.rotation * Vec3::Z;
        assert!((rotated.x - 1.0).abs() < 1e-3);
    }

    #[test]
    fn vehicle_shape_ignores_length_for_mesh_scale() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let t_short = vehicle_shape_local_transform(&mesh, 0.0, 10.0);
        let t_long = vehicle_shape_local_transform(&mesh, 0.0, 30.0);
        assert!((t_short.scale - Vec3::ONE).length() < 1e-4);
        assert!((t_long.scale - Vec3::ONE).length() < 1e-4);
        // Same mesh front → same local origin; length only affects consist offsets.
        assert!((t_short.translation.x - t_long.translation.x).abs() < 1e-3);
        assert!((t_short.translation.y - t_long.translation.y).abs() < 1e-3);
    }

    #[test]
    fn vehicle_shape_authored_origin_stays_at_offset() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let t0 = vehicle_shape_local_transform(&mesh, 0.0, 18.0);
        let t1 = vehicle_shape_local_transform(&mesh, -18.0, 14.0);
        assert!(t0.translation.x.abs() < 1e-3);
        assert!(t0.translation.y.abs() < 1e-3);
        assert!((t1.translation.x + 18.0).abs() < 1e-3);
        // Spacing uses Size/length offsets, not mesh stretch or AABB anchors.
        assert!((t1.translation.x - t0.translation.x + 18.0).abs() < 1e-3);
    }

    #[test]
    fn vehicle_and_cab_frame_share_authored_origin() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let exterior = vehicle_shape_local_transform(&mesh, -5.0, 18.0);
        let cab = cab_shape_placement_transform(&mesh, -5.0, 18.0);
        let authored = vehicle_authored_frame_transform(-5.0);
        assert!((exterior.translation - cab.translation).length() < 1e-5);
        assert!((exterior.translation - authored.translation).length() < 1e-5);
        assert!((exterior.rotation.xyz() - cab.rotation.xyz()).length() < 1e-5);
    }

    #[test]
    fn vehicle_frame_ignores_mesh_aabb() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let with_mesh = cab_shape_placement_transform(&mesh, 3.0, 20.0);
        let authored = vehicle_authored_frame_transform(3.0);
        assert!((with_mesh.translation - authored.translation).length() < 1e-5);
        assert!((with_mesh.translation.x - 3.0).abs() < 1e-5);
        assert!(with_mesh.translation.y.abs() < 1e-5);
    }

    #[test]
    fn chiltern_pullman_shape_bounds_are_vehicle_sized() {
        let path = chiltern_shape_fixture("RF_WP_DMBSA.s");
        if !path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(path).expect("parse Pullman shape");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let (min, max) = mesh_aabb(&mesh).expect("mesh AABB");
        let extent = max - min;
        assert!(extent.x < 5.0, "width extent too large: {extent:?}");
        assert!(extent.y < 6.0, "height extent too large: {extent:?}");
        assert!(extent.z < 30.0, "length extent too large: {extent:?}");
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
    fn vehicle_cab_frame_keeps_unit_scale_on_lead_car() {
        let shape = ShapeFile::from_path(minimal_shape_fixture()).expect("parse");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let (frame, exterior_scale) = vehicle_cab_frame_and_exterior_scale(&mesh, 0.0, 18.0);
        assert!((frame.scale - Vec3::ONE).length() < 1e-4);
        assert!((exterior_scale - 1.0).abs() < 1e-4);
        let full = vehicle_shape_local_transform(&mesh, 0.0, 18.0);
        assert!((full.scale - Vec3::ONE).length() < 1e-4);
        assert!((full.translation - frame.translation).length() < 1e-4);
    }

    #[test]
    fn or_lighting_defaults_to_lit() {
        assert!(resolve_or_lighting(None, None), "lit is the default");
        assert!(!resolve_or_lighting(Some("1"), Some("0")));
    }

    #[test]
    fn or_lighting_unlit_opt_out_wins() {
        assert!(!resolve_or_lighting(Some("1"), Some("1")));
        assert!(!resolve_or_lighting(Some("true"), None));
        assert!(resolve_or_lighting(Some("0"), None), "0 is not opt-out");
    }

    #[test]
    fn or_lighting_explicit_disable() {
        assert!(!resolve_or_lighting(None, Some("0")));
        assert!(!resolve_or_lighting(None, Some("")));
        assert!(resolve_or_lighting(None, Some("1")));
    }

    #[test]
    fn finalize_scenery_material_unlit_path_disables_lighting() {
        let mat = finalize_scenery_material(StandardMaterial::default(), false);
        assert!(
            mat.unlit,
            "legacy opt-out path must stay unlit (fixed-function look)"
        );
        assert!(!mat.fog_enabled);
    }

    #[test]
    fn finalize_scenery_material_lit_path_enables_lighting() {
        let mat = finalize_scenery_material(StandardMaterial::default(), true);
        assert!(
            !mat.unlit,
            "OR-style path must be lit to receive sun + shadows"
        );
        assert!(mat.fog_enabled);
    }

    #[test]
    fn scenery_albedo_tint_neutralizes_boost_when_lit() {
        // Unlit: keep the ×boost / post-brighten tint to claw brightness back.
        let unlit_tint = scenery_albedo_tint(false, false).to_linear();
        assert!(unlit_tint.red > 1.0, "unlit tint should boost albedo");
        // Lit: lighting provides brightness, so the tint collapses to white.
        let lit_tint = scenery_albedo_tint(false, true).to_linear();
        assert!((lit_tint.red - 1.0).abs() < 1e-4);
        assert!((lit_tint.green - 1.0).abs() < 1e-4);
        assert!((lit_tint.blue - 1.0).abs() < 1e-4);
    }

    #[test]
    fn scenery_base_tint_neutralizes_boost_when_lit() {
        assert!(scenery_base_tint(false).to_linear().red > 1.0);
        assert!((scenery_base_tint(true).to_linear().red - 1.0).abs() < 1e-4);
    }

    #[test]
    fn shape_file_basename_strips_path() {
        assert_eq!(
            super::shape_file_basename(r"SHAPES\ukfs_s_1x10m.s"),
            "ukfs_s_1x10m.s"
        );
        assert_eq!(super::shape_file_basename("yard_shed.s"), "yard_shed.s");
    }

    #[test]
    fn resolve_trackobj_relative_dynatrax_on_chiltern() {
        let Some(route) = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"))
        else {
            return;
        };
        if !route.is_dir() {
            eprintln!("skip: Chiltern route not available");
            return;
        }
        let assets = RouteAssets::new(&route);
        let relative = "../../ROUTES/Chiltern/DYNATRAX/DynaTrax-42142.s";
        let path = assets
            .resolve_trackobj_shape(Some(relative), None)
            .expect("relative DYNATRAX TrackObj shape");
        assert!(path.is_file(), "{}", path.display());
        assert!(
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("DynaTrax-42142.s"))
        );
    }

    #[test]
    fn birmingham_trackobj_resolution_accounts_all_1659() {
        let Some(route) = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"))
        else {
            return;
        };
        let world_path = route.join("WORLD/w-006080+014925.w");
        if !world_path.is_file() {
            eprintln!("skip: Birmingham .w not available");
            return;
        }
        let assets = RouteAssets::new(&route);
        let world = openrailsrs_formats::WorldFile::from_path(&world_path).expect("parse .w");
        let mut trackobj = 0usize;
        let mut resolved = 0usize;
        let mut unresolved = 0usize;
        for item in &world.items {
            let openrailsrs_formats::WorldItem::Track {
                file_name,
                section_idx,
                ..
            } = item
            else {
                continue;
            };
            trackobj += 1;
            if assets
                .resolve_trackobj_shape(file_name.as_deref(), *section_idx)
                .is_some()
            {
                resolved += 1;
            } else {
                unresolved += 1;
            }
        }
        assert_eq!(trackobj, 1659, "Birmingham TrackObj count");
        // Relative DYNATRAX + indexed SHAPES should resolve the vast majority.
        assert!(
            resolved >= 1600,
            "expected ≥1600 resolved TrackObj shapes, got {resolved} resolved / {unresolved} unresolved"
        );
        eprintln!("Birmingham TrackObj resolve: {resolved}/{trackobj} (unresolved {unresolved})");
    }

    #[test]
    fn resolve_chiltern_pack_global_trackobj() {
        let content = PathBuf::from(env!("HOME")).join("Documentos/Open Rails/Content");
        if !content
            .join("Chiltern/GLOBAL/SHAPES/ukfs_s_1x10m.s")
            .is_file()
        {
            return;
        }
        unsafe {
            std::env::set_var("OPENRAILSRS_MSTS_CONTENT", &content);
        }
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let globals = global_assets_dirs(&route);
        assert!(
            globals.iter().any(|p| p.ends_with("Chiltern/GLOBAL")),
            "expected Chiltern/GLOBAL in {:?}",
            globals
        );
        let assets = RouteAssets::new(&route);
        assert!(
            assets
                .resolve_world_shape("TrackObj", "ukfs_s_1x10m.s")
                .is_some(),
            "ukfs TrackObj should resolve from route-pack GLOBAL"
        );
        assert_eq!(
            assets.tsection().shape_file_name(38508),
            Some("ukfs_s_1x25m.s"),
            "tsection catalog should load from MSTS ROUTES/Chiltern"
        );
    }

    #[test]
    fn resolve_vehicle_shape_prefers_or_content_when_present() {
        let content = PathBuf::from("/home/cristian/Documentos/Open Rails/Content");
        if !content.is_dir() {
            return;
        }
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let stub = route.join("trains/RF_Blue_Pullman");
        let shape_dirs: Vec<&Path> = vec![stub.as_path()];
        let path = resolve_vehicle_shape_path(shape_dirs.as_slice(), "RF_WP_DMBSA.s", &route)
            .expect("shape");
        assert!(
            path.starts_with(&content),
            "expected OR content shape, got {}",
            path.display()
        );
        let shape = ShapeFile::from_path(&path).expect("parse OR content Pullman shape");
        let mesh = build_mesh_from_shape(&shape).expect("mesh");
        let (min, max) = mesh_aabb(&mesh).expect("mesh AABB");
        let extent = max - min;
        assert!(
            extent.x < 5.0 && extent.y < 6.0 && extent.z < 30.0,
            "OR content shape extent too large: {extent:?}"
        );
        let texture_root =
            vehicle_texture_root_for_shape_path(&path).expect("vehicle texture root");
        assert!(
            resolve_texture_path(texture_root, "bp01.ace").is_some(),
            "flat OR trainset texture should resolve from {}",
            texture_root.display()
        );

        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut texture_cache = HashMap::new();
        let tex_dirs_owned = vehicle_texture_search_dirs(&path, &route);
        let tex_dirs: Vec<&Path> = tex_dirs_owned.iter().map(|p| p.as_path()).collect();
        let asset = load_shape_render_asset_from_path(
            &path,
            &tex_dirs,
            Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            Color::srgb(0.55, 0.58, 0.62),
            true,
        )
        .expect("render OR content Pullman shape");
        let textured_parts = asset.parts.iter().filter(|part| part.has_texture).count();
        assert!(
            textured_parts > 20,
            "expected most OR content Pullman parts to resolve textures, got {textured_parts}/{}",
            asset.parts.len()
        );
    }

    #[test]
    fn pullman_lod_levels_audit() {
        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&shape_path).expect("parse DMBSA");
        for (i, lvl) in shape
            .lod_controls
            .first()
            .map(|c| c.distance_levels.iter().enumerate())
            .into_iter()
            .flatten()
        {
            let parts = build_mesh_parts_from_shape_lod(&shape, lvl);
            assert!(
                !parts.is_empty(),
                "LOD {i} selection_m={} should have parts",
                lvl.selection_m
            );
        }
        let at25 = build_mesh_parts_from_shape_at_distance(&shape, 25.0);
        let finest = build_mesh_parts_from_shape(&shape);
        assert_eq!(
            at25.len(),
            finest.len(),
            "live LOD distance should not drop exterior parts on DMBSA"
        );
    }

    #[test]
    fn pullman_consist_shapes_alpha_audit() {
        let trainset = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman",
        );
        if !trainset.is_dir() {
            return;
        }
        let shapes = [
            "RF_WP_DMBSA.s",
            "RF_WP_PSB.s",
            "RF_WP_KFC.s",
            "RF_BP_PCFfwd.s",
            "RF_WP_PSG.s",
        ];
        let tex_dirs: Vec<PathBuf> =
            vehicle_texture_search_dirs(&trainset.join(shapes[0]), &trainset);
        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        for name in shapes {
            let shape_path = trainset.join(name);
            if !shape_path.is_file() {
                continue;
            }
            let _shape = ShapeFile::from_path(&shape_path).expect("parse");
            let mut meshes = Assets::<Mesh>::default();
            let mut images = Assets::<Image>::default();
            let mut materials = Assets::<StandardMaterial>::default();
            let mut cache = HashMap::new();
            let Some(asset) = load_shape_render_asset_from_path(
                &shape_path,
                &tex_refs,
                Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
                &mut meshes,
                &mut images,
                &mut materials,
                &mut cache,
                Color::WHITE,
                true,
            ) else {
                continue;
            };
            let mut opaque = 0usize;
            let mut blend = 0usize;
            let mut mask = 0usize;
            let mut dual_mask = 0usize;
            for part in &asset.parts {
                let mat = materials.get(&part.material).expect("mat");
                match mat.alpha_mode {
                    AlphaMode::Opaque => opaque += 1,
                    AlphaMode::Blend | AlphaMode::Add => blend += 1,
                    AlphaMode::Mask(c)
                        if (c - openrailsrs_bevy_scenery::shapes::OR_BLEND_PASS_OPAQUE_CUTOFF)
                            .abs()
                            < 1e-4 =>
                    {
                        // OR BlendATexDiff first pass (ReferenceAlpha=250), not cutout holes.
                        dual_mask += 1;
                        mask += 1;
                    }
                    AlphaMode::Mask(_) => mask += 1,
                    _ => {}
                }
            }
            assert!(
                mask == 0 || (dual_mask == mask && blend >= dual_mask),
                "{name}: unexpected Mask parts (holes) mask={mask} dual={dual_mask} opaque={opaque} blend={blend}"
            );
        }
    }

    #[test]
    fn pullman_exterior_alpha_modes_audit() {
        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        let trainset = shape_path.parent().expect("trainset root");
        if !shape_path.is_file() {
            return;
        }
        let tex_dirs = vehicle_texture_search_dirs(&shape_path, trainset);
        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut cache = HashMap::new();
        let asset = load_shape_render_asset_from_path(
            &shape_path,
            &tex_refs,
            Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut cache,
            Color::srgb(0.55, 0.58, 0.62),
            true,
        )
        .expect("render asset");
        let mut blend = 0usize;
        let mut opaque = 0usize;
        for part in &asset.parts {
            let mat = materials.get(&part.material).expect("material");
            match mat.alpha_mode {
                AlphaMode::Opaque => opaque += 1,
                AlphaMode::Blend | AlphaMode::Add | AlphaMode::AlphaToCoverage => blend += 1,
                AlphaMode::Mask(_) => {}
                AlphaMode::Premultiplied | AlphaMode::Multiply => blend += 1,
            }
        }
        assert_eq!(opaque, 28, "shell + interior panels should be opaque");
        assert_eq!(blend, 2, "only glass.ace parts should blend");
    }

    #[test]
    fn pullman_exterior_texture_audit() {
        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&shape_path).expect("parse DMBSA");
        let parts = build_mesh_parts_from_shape_at_distance(&shape, 25.0);
        assert!(!parts.is_empty(), "DMBSA should have mesh parts at LOD 25m");
        for part in &parts {
            let strict = shape.texture_for_prim_state_idx(part.prim_state_idx);
            let with_fallback = texture_for_prim_state(&shape, part.prim_state_idx);
            assert_eq!(
                strict, with_fallback,
                "prim_state {} should not use fallback texture heuristics",
                part.prim_state_idx
            );
            if let Some((min, max)) = mesh_uv_aabb(&part.mesh) {
                assert!(
                    (max - min).length_squared() >= 1e-6,
                    "prim_state {} has degenerate UVs",
                    part.prim_state_idx
                );
            }
        }
    }

    #[test]
    fn pullman_prim_state_z_bias_sane() {
        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&shape_path).expect("parse DMBSA");
        for (i, ps) in shape.prim_states.iter().enumerate() {
            let z = ps.z_bias.unwrap_or(0.0);
            assert!(
                z.is_finite() && z.abs() < 100.0,
                "prim_state {i} name={:?} shader={:?} tex={:?} z_bias={z} z_buf={}",
                ps.name,
                shape.shader_names.get(ps.shader_idx.max(0) as usize),
                ps.tex_indices
                    .first()
                    .and_then(|ti| shape.texture_filenames.get(*ti as usize)),
                ps.z_buf_mode
            );
        }
    }

    #[test]
    fn no_huge_depth_bias_in_bevy_materials() {
        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let trainset = shape_path.parent().expect("trainset");
        let tex_dirs = vehicle_texture_search_dirs(&shape_path, trainset);
        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut cache = HashMap::new();
        let asset = load_shape_render_asset_from_path(
            &shape_path,
            &tex_refs,
            Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut cache,
            Color::srgb(0.55, 0.58, 0.62),
            true,
        )
        .expect("render asset");
        for part in &asset.parts {
            let mat = materials.get(&part.material).expect("material");
            assert!(
                mat.depth_bias.is_finite() && mat.depth_bias.abs() <= MSTS_Z_BIAS_CLAMP,
                "prim_state {} depth_bias={}",
                part.prim_state_idx,
                mat.depth_bias
            );
        }
    }

    #[test]
    fn pullman_train_exterior_single_sided_back_cull() {
        use bevy::render::render_resource::Face;

        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        if !shape_path.is_file() {
            let fixture = chiltern_shape_fixture("RF_WP_DMBSA.s");
            if !fixture.is_file() {
                return;
            }
        }
        let path = if shape_path.is_file() {
            shape_path
        } else {
            chiltern_shape_fixture("RF_WP_DMBSA.s")
        };
        let trainset = path.parent().expect("trainset");
        let tex_dirs = vehicle_texture_search_dirs(&path, trainset);
        let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut cache = HashMap::new();
        let asset = load_shape_render_asset_from_path(
            &path,
            &tex_refs,
            Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut cache,
            Color::srgb(0.55, 0.58, 0.62),
            true,
        )
        .expect("render asset");
        for part in &asset.parts {
            let mat = materials.get(&part.material).expect("material");
            assert!(
                !mat.double_sided,
                "prim_state {} should be single-sided train exterior",
                part.prim_state_idx
            );
            assert_eq!(
                mat.cull_mode,
                Some(Face::Back),
                "prim_state {} should cull back faces (OR CullCounterClockwise)",
                part.prim_state_idx
            );
        }
    }

    #[test]
    fn mesh_triangle_vertex_count_multiple_of_3() {
        let shape_path = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.s",
        );
        if !shape_path.is_file() {
            let fixture = chiltern_shape_fixture("RF_WP_DMBSA.s");
            if !fixture.is_file() {
                return;
            }
            let shape = ShapeFile::from_path(&fixture).expect("parse");
            let parts = build_mesh_parts_from_shape(&shape);
            for part in &parts {
                assert!(
                    mesh_triangle_list_valid(&part.mesh),
                    "prim_state {} vertex count not multiple of 3",
                    part.prim_state_idx
                );
            }
            return;
        }
        let shape = ShapeFile::from_path(&shape_path).expect("parse DMBSA");
        let parts = build_mesh_parts_from_shape_at_distance(&shape, 25.0);
        assert!(!parts.is_empty());
        for part in &parts {
            assert!(
                mesh_triangle_list_valid(&part.mesh),
                "prim_state {} invalid triangle list",
                part.prim_state_idx
            );
        }
        assert!(mesh_triangle_list_valid(
            &build_mesh_from_shape_at_distance(&shape, 25.0).expect("combined mesh")
        ));
    }

    #[test]
    fn vehicle_texture_search_dirs_prefers_trainset_before_route() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let trainset = chiltern_route_dir();
        if !trainset.join("TEXTURES/bp01.ace").exists() {
            return;
        }
        let shape = trainset.join("SHAPES/RF_WP_DMBSA.s");
        if !shape.is_file() {
            return;
        }
        let dirs = vehicle_texture_search_dirs(&shape, &route);
        assert!(
            dirs.first()
                .is_some_and(|d| d.as_path() == trainset.as_path()),
            "trainset root must be first (OR ReferencePath), got {:?}",
            dirs
        );
        let found = resolve_texture_path_in_dirs(
            &dirs.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
            "bp01.ace",
        );
        assert!(found.is_some());
        assert!(
            found.unwrap().starts_with(&trainset),
            "bp01.ace should resolve from trainset, not route TEXTURES/"
        );
    }

    #[test]
    fn vehicle_texture_root_supports_shapes_subdir_and_flat_trainset() {
        let shapes_layout = Path::new("/tmp/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s");
        assert_eq!(
            vehicle_texture_root_for_shape_path(shapes_layout),
            Some(Path::new("/tmp/RF_Blue_Pullman"))
        );

        let flat_layout = Path::new("/tmp/RF_Blue_Pullman/RF_WP_DMBSA.s");
        assert_eq!(
            vehicle_texture_root_for_shape_path(flat_layout),
            Some(Path::new("/tmp/RF_Blue_Pullman"))
        );
    }
}
