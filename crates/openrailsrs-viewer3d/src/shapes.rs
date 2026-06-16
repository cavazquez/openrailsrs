//! MSTS ASCII `.s` shapes → Bevy meshes (order 6) + `.ace` textures (order 7).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_ace::{AceFile, read_ace};
use openrailsrs_formats::{DistanceLevel, Matrix43, ShapeFile, Vec3 as ShapeVec3};

use crate::coordinates::{matrix43_transform_point_xna, matrix43_transform_vector_xna};
use crate::viewer_log;

/// HDR multiplier on textured scenery whose `.ace` mip0 is already reasonably bright.
pub const SCENERY_TEXTURE_ALBEDO_BOOST: f32 = 4.0;

/// Mean sRGB luma below this → MSTS atlas looks black even with unlit + albedo boost.
pub const DARK_TEXTURE_LUMA_THRESHOLD: f32 = 32.0;

/// Target mean luma after normalizing dark MSTS `.ace` mip0 (Open Rails draws these unlit).
const SCENERY_TEXTURE_TARGET_LUMA: f32 = 112.0;

/// Max per-pixel scale when lifting near-black atlases (signals, tunnels, night textures).
const SCENERY_TEXTURE_MAX_PIXEL_SCALE: f32 = 128.0;

/// Small extra tint after pixel normalization (avoid double-boost with [`SCENERY_TEXTURE_ALBEDO_BOOST`]).
const SCENERY_TEXTURE_POST_BRIGHTEN_TINT: f32 = 1.25;

/// Open Rails lights its world with a sun + ambient and tone-maps it; MSTS `.ace`
/// albedos look right under that model. This OR-style lit path (sun shading + shadow
/// receive, neutral albedo) is the **default** and matches the camera's physical
/// `Exposure::SUNLIGHT` + 75 klux sun + ambient fill.
///
/// The legacy fixed-function path draws scenery `unlit` and claws brightness back with
/// [`SCENERY_TEXTURE_ALBEDO_BOOST`] and [`brighten_dark_ace_rgba`]; it is internally
/// inconsistent with that exposure, so surfaces stay flat and never receive shadows.
/// Opt back into it with `OPENRAILSRS_UNLIT_SCENERY=1` (or `OPENRAILSRS_OR_LIGHTING=0`).
pub fn or_lighting_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        resolve_or_lighting(
            std::env::var("OPENRAILSRS_UNLIT_SCENERY").ok().as_deref(),
            std::env::var("OPENRAILSRS_OR_LIGHTING").ok().as_deref(),
        )
    })
}

/// Pure resolver for the lighting mode (unit-tested without global env state).
///
/// Lit (OR-style) is the default. `OPENRAILSRS_UNLIT_SCENERY` (truthy) forces the
/// legacy unlit path; otherwise `OPENRAILSRS_OR_LIGHTING` may explicitly disable it
/// with `"0"`/empty.
fn resolve_or_lighting(unlit_opt_out: Option<&str>, or_flag: Option<&str>) -> bool {
    let truthy = |v: &str| !v.is_empty() && v != "0";
    if unlit_opt_out.is_some_and(truthy) {
        return false;
    }
    match or_flag {
        Some(v) => truthy(v),
        None => true,
    }
}

fn scenery_texture_tint() -> Color {
    let b = SCENERY_TEXTURE_ALBEDO_BOOST;
    Color::linear_rgb(b, b, b)
}

/// Albedo tint for a scenery texture, honouring the OR-style lit path.
///
/// In the lit path the sun/ambient provide brightness, so the fixed-function
/// `×SCENERY_TEXTURE_ALBEDO_BOOST` tint must collapse to white to avoid a washed-out look.
fn scenery_albedo_tint(pixel_brightened: bool, lit: bool) -> Color {
    if lit {
        Color::WHITE
    } else {
        scenery_material_tint_for_ace(pixel_brightened)
    }
}

/// Base (untextured / DDS) tint, honouring the OR-style lit path.
fn scenery_base_tint(lit: bool) -> Color {
    if lit {
        Color::WHITE
    } else {
        scenery_texture_tint()
    }
}

fn scenery_texture_tint_scaled(multiplier: f32) -> Color {
    Color::linear_rgb(multiplier, multiplier, multiplier)
}

/// Mean sRGB luma of opaque pixels in decoded ACE mip0 (0–255).
pub fn ace_mean_luma(rgba: &[u8]) -> f32 {
    if rgba.len() < 4 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for px in rgba.chunks_exact(4) {
        if px[3] < 8 {
            continue;
        }
        sum += 0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2]);
        n += 1;
    }
    if n == 0 { 0.0 } else { (sum / n as f64) as f32 }
}

fn scale_ace_channel(v: u8, scale: f32) -> u8 {
    (f32::from(v) * scale).min(255.0).round() as u8
}

/// Lift dark MSTS atlases toward [`SCENERY_TEXTURE_TARGET_LUMA`]. Returns `(rgba, was_brightened)`.
pub fn brighten_dark_ace_rgba(rgba: &[u8]) -> (Vec<u8>, bool) {
    let mean = ace_mean_luma(rgba);
    if mean >= DARK_TEXTURE_LUMA_THRESHOLD {
        return (rgba.to_vec(), false);
    }
    let scale = (SCENERY_TEXTURE_TARGET_LUMA / mean.max(1.0)).min(SCENERY_TEXTURE_MAX_PIXEL_SCALE);
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        if px[3] < 8 {
            continue;
        }
        px[0] = scale_ace_channel(px[0], scale);
        px[1] = scale_ace_channel(px[1], scale);
        px[2] = scale_ace_channel(px[2], scale);
    }
    (out, true)
}

/// Material tint for a scenery texture (full boost, or modest tint after pixel normalization).
pub fn scenery_material_tint_for_ace(pixel_brightened: bool) -> Color {
    if pixel_brightened {
        scenery_texture_tint_scaled(SCENERY_TEXTURE_POST_BRIGHTEN_TINT)
    } else {
        scenery_texture_tint()
    }
}

/// Emissive lift for atlases that stay near-black after pixel normalization (MSTS night/signal tex).
const SCENERY_DARK_EMISSIVE: LinearRgba = LinearRgba::new(0.4, 0.4, 0.45, 1.0);

fn scenery_needs_emissive_texture(rgba: &[u8]) -> bool {
    ace_mean_luma(rgba) < DARK_TEXTURE_LUMA_THRESHOLD
}

fn scenery_shape_material_with_texture(
    tint: Color,
    handle: Handle<Image>,
    rgba_for_luma: &[u8],
    alpha_mode: AlphaMode,
    z_bias: f32,
) -> StandardMaterial {
    let lit = or_lighting_enabled();
    let mut mat = StandardMaterial {
        base_color: tint,
        base_color_texture: Some(handle.clone()),
        perceptual_roughness: 0.85,
        metallic: 0.05,
        double_sided: true,
        alpha_mode,
        depth_bias: z_bias,
        ..default()
    };
    // The dark-atlas emissive lift only compensates for the unlit path; under real
    // lighting it would make night/signal textures self-glow, so skip it when lit.
    if !lit && scenery_needs_emissive_texture(rgba_for_luma) {
        mat.emissive = SCENERY_DARK_EMISSIVE;
        mat.emissive_texture = Some(handle);
    }
    scenery_shape_material(mat)
}

/// Finalise a scenery material for the active lighting path.
///
/// - OR-style ([`or_lighting_enabled`], the default): keep the material lit so the directional
///   sun shades it and it receives shadows, matching Open Rails' `SceneryShader`.
/// - Legacy (unlit, opt-in via `OPENRAILSRS_UNLIT_SCENERY=1`): MSTS `.ace` albedo is authored for
///   fixed-function drawing; drawn `unlit` with a brightness boost, never receiving shadows.
fn scenery_shape_material(base: StandardMaterial) -> StandardMaterial {
    finalize_scenery_material(base, or_lighting_enabled())
}

/// Pure lighting-mode finaliser (unit-tested without env state).
fn finalize_scenery_material(mut base: StandardMaterial, lit: bool) -> StandardMaterial {
    if lit {
        base.unlit = false;
        base.fog_enabled = true;
    } else {
        base.unlit = true;
        base.fog_enabled = false;
    }
    base
}

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

fn load_tsection_catalog(route_dir: &Path) -> openrailsrs_formats::TSectionCatalog {
    if let Ok(catalog) = openrailsrs_formats::TSectionCatalog::load_for_route(route_dir) {
        if !catalog.shapes.is_empty() {
            return catalog;
        }
    }
    if let Some(msts_route) = resolve_msts_route_dir(route_dir) {
        if let Ok(catalog) = openrailsrs_formats::TSectionCatalog::load_for_route(&msts_route) {
            if !catalog.shapes.is_empty() {
                return catalog;
            }
        }
    }
    openrailsrs_formats::TSectionCatalog::default()
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
    shape_path_index: HashMap<String, PathBuf>,
    tsection: openrailsrs_formats::TSectionCatalog,
    track_db: Option<openrailsrs_formats::TrackDbFile>,
    tdb_sections_by_shape: HashMap<u32, Vec<TdbSectionAnchor>>,
}

impl RouteAssets {
    pub fn new(route_dir: impl Into<PathBuf>) -> Self {
        let route_dir = route_dir.into();
        let shape_path_index = build_shape_path_index(&shape_search_dirs(&route_dir));
        let tsection = load_tsection_catalog(&route_dir);
        if !tsection.shapes.is_empty() {
            let junction_clearance = tsection
                .shapes
                .iter()
                .filter(|(id, shape)| {
                    shape.is_junction() && tsection.clearance_dist_m(**id).is_some()
                })
                .count();
            crate::viewer_log!(
                "openrailsrs-viewer3d: tsection — {} shape(s), {} section(s), {} junction(s) with ClearanceDist",
                tsection.shapes.len(),
                tsection.sections.len(),
                junction_clearance
            );
        }
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
        Self {
            route_dir,
            shape_path_index,
            tsection,
            track_db,
            tdb_sections_by_shape,
        }
    }

    pub fn track_db(&self) -> Option<&openrailsrs_formats::TrackDbFile> {
        self.track_db.as_ref()
    }

    pub fn tsection(&self) -> &openrailsrs_formats::TSectionCatalog {
        &self.tsection
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
        if file_name.is_empty() {
            return None;
        }
        let base = shape_file_basename(file_name);
        self.shape_path_index
            .get(&base.to_ascii_lowercase())
            .cloned()
    }

    /// Resolve scenery shapes; `TrackObj` prefers route-pack `GLOBAL/SHAPES/` (Open Rails layout).
    pub fn resolve_world_shape(&self, kind: &str, file_name: &str) -> Option<PathBuf> {
        if file_name.is_empty() {
            return None;
        }
        let base = shape_file_basename(file_name);
        if kind == "TrackObj" {
            for global in global_assets_dirs(&self.route_dir) {
                if let Some(path) = resolve_shape_path(&global, base) {
                    return Some(path);
                }
            }
        }
        self.resolve_shape(base)
    }

    /// Resolve `TrackObj` mesh path using `.w` `FileName` and/or `SectionIdx` → `tsection.dat`.
    pub fn resolve_trackobj_shape(
        &self,
        file_name: Option<&str>,
        section_idx: Option<u32>,
    ) -> Option<PathBuf> {
        let name = file_name
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                section_idx.and_then(|idx| self.tsection.shape_file_name(idx).map(str::to_string))
            })?;
        self.resolve_world_shape("TrackObj", &name)
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
    /// From `prim_state.alpha_test_mode`: -1 = unknown, 0 = opaque, 1 = test, 2 = blend.
    pub alpha_test_mode: i32,
    pub z_bias: Option<f32>,
    pub z_buf_mode: i32,
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
///
/// Delegates to [`crate::coordinates::shape_point_to_bevy`]; kept as a public
/// re-export so existing callers in this module and elsewhere don't break.
pub use crate::coordinates::shape_point_to_bevy;

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

    let front = Vec3::new(center.x, center.y, min.z);
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

/// Lead-vehicle placement for 3D cab (same origin/rotation as exterior `.s`, unit scale).
///
/// Open Rails keeps `ORTS3DCabHeadPos` and `CABVIEW3D` meshes in unscaled MSTS shape
/// metres; only the exterior body is length-fitted via a child scale node.
pub fn cab_shape_placement_transform(mesh: &Mesh, offset_m: f32, _length_m: f32) -> Transform {
    let rotation = msts_shape_to_train_rotation();
    let (min, max) = mesh_aabb(mesh).unwrap_or((Vec3::ZERO, Vec3::splat(0.01)));
    let center = (min + max) * 0.5;

    let front = Vec3::new(center.x, center.y, min.z);
    let front_local_x = (rotation * front).x;

    let min_y = aabb_corners(min, max)
        .iter()
        .map(|p| (rotation * *p).y)
        .fold(f32::INFINITY, f32::min);

    Transform {
        translation: Vec3::new(offset_m - front_local_x, -min_y, 0.0),
        rotation,
        scale: Vec3::ONE,
    }
}

/// Cab frame (unit MSTS scale) plus uniform length-fit scale for the exterior mesh child.
pub fn vehicle_cab_frame_and_exterior_scale(
    mesh: &Mesh,
    offset_m: f32,
    length_m: f32,
) -> (Transform, f32) {
    let (min, max) = mesh_aabb(mesh).unwrap_or((Vec3::ZERO, Vec3::splat(0.01)));
    let scale = vehicle_shape_fit_scale(max - min, length_m);
    (
        cab_shape_placement_transform(mesh, offset_m, length_m),
        scale,
    )
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
            append_primitive_mesh_buffers(shape, level, sub, prim, default_normal, &mut buffers);
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
            append_primitive_mesh_buffers(shape, level, sub, prim, default_normal, buffers);
        }
    }

    parts
        .into_iter()
        .filter_map(|(prim_state_idx, buffers)| {
            let mesh = buffers.into_mesh()?;
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
                mesh,
                texture_file: texture_filename_for_prim_state(shape, prim_state_idx),
                shader_name: shader_name_for_prim_state(shape, prim_state_idx),
                alpha_test_mode,
                z_bias,
                z_buf_mode,
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
    level: &DistanceLevel,
    sub: &openrailsrs_formats::SubObject,
    prim: &openrailsrs_formats::Primitive,
    default_normal: ShapeVec3,
    buffers: &mut MeshBuffers,
) {
    let matrix_chain = primitive_matrix_chain(shape, level, prim.prim_state_idx);
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
        }
    }
}

#[derive(Clone, Copy)]
struct ShapeMatrixRef<'a> {
    matrix: &'a Matrix43,
    zero_translation: bool,
}

fn primitive_matrix_chain<'a>(
    shape: &'a ShapeFile,
    level: &DistanceLevel,
    prim_state_idx: i32,
) -> Vec<ShapeMatrixRef<'a>> {
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
    out
}

// matrix43_transform_point_xna and matrix43_transform_vector_xna are imported from
// crate::coordinates at the top of this file.

fn transform_shape_point(mut point: Vec3, matrices: &[ShapeMatrixRef<'_>]) -> Vec3 {
    for matrix in matrices {
        point = matrix43_transform_point_xna(matrix.matrix, point, matrix.zero_translation);
    }
    point
}

fn transform_shape_normal(mut normal: Vec3, matrices: &[ShapeMatrixRef<'_>]) -> Vec3 {
    for matrix in matrices {
        normal = matrix43_transform_vector_xna(matrix.matrix, normal);
    }
    normal.try_normalize().unwrap_or(Vec3::Y)
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

/// Convert decoded ACE mip 0 (RGBA8) into a Bevy GPU image (raw mip0, no brightening).
pub fn ace_to_image(ace: &AceFile) -> Image {
    ace_rgba_to_image(ace.width, ace.height, &ace.mip0)
}

/// ACE → GPU image with dark-atlas normalization for world / train scenery.
pub fn ace_to_scenery_image(ace: &AceFile) -> (Image, bool) {
    let (rgba, brightened) = brighten_dark_ace_rgba(&ace.mip0);
    (ace_rgba_to_image(ace.width, ace.height, &rgba), brightened)
}

fn ace_rgba_to_image(width: u32, height: u32, rgba: &[u8]) -> Image {
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Decode a DDS file from raw bytes into a Bevy GPU image.
pub fn decode_dds_to_image(bytes: &[u8]) -> Result<Image, String> {
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    Image::from_buffer(
        bytes,
        ImageType::Extension("dds"),
        CompressedImageFormats::all(),
        false,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .map_err(|e| e.to_string())
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

/// All `GLOBAL/` asset roots under MSTS Content (OR uses per-route-pack trees like `Chiltern/GLOBAL/`).
pub fn global_assets_dirs(route_dir: &Path) -> Vec<PathBuf> {
    let Some(content) = msts_content_root() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        let has_shapes = p.join("SHAPES").is_dir() || p.join("shapes").is_dir();
        if has_shapes && !out.iter().any(|existing| existing == &p) {
            out.push(p);
        }
    };
    push(content.join("GLOBAL"));
    let Some(stem) = route_dir.file_name().and_then(|s| s.to_str()) else {
        return out;
    };
    push(content.join(stem).join("GLOBAL"));
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
                push(entry.path().join("GLOBAL"));
            }
        }
    }
    out
}

/// Route directory plus route-pack and root `GLOBAL` trees from [`msts_content_root`].
pub fn shape_search_dirs(route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(content) = msts_content_root() {
        if let Some(stem) = route_dir.file_name().and_then(|s| s.to_str()) {
            let pack = content.join(stem);
            if pack.is_dir() {
                dirs.push(pack);
            } else if let Ok(rd) = std::fs::read_dir(&content) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && path
                            .file_name()
                            .is_some_and(|n| n.eq_ignore_ascii_case(stem))
                    {
                        dirs.push(path);
                        break;
                    }
                }
            }
        }
    }
    for global in global_assets_dirs(route_dir) {
        dirs.push(global);
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// First `GLOBAL/` root, if any (legacy helper).
pub fn global_assets_dir() -> Option<PathBuf> {
    msts_content_root()
        .map(|root| root.join("GLOBAL"))
        .filter(|p| p.is_dir())
}

/// Directories to search for `.ace` textures given a resolved shape path.
pub fn texture_search_dirs_for_shape(shape_path: &Path, route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(parent) = shape_path.parent() {
        let in_asset_subdir = parent.file_name().is_some_and(|n| {
            n.eq_ignore_ascii_case("shapes")
                || n.eq_ignore_ascii_case("cabview3d")
                || n.eq_ignore_ascii_case("cabview")
        });
        if in_asset_subdir {
            dirs.push(parent.to_path_buf());
            if let Some(asset_root) = parent.parent() {
                if asset_root != route_dir {
                    dirs.push(asset_root.to_path_buf());
                }
            }
        }
    }
    for global in global_assets_dirs(route_dir) {
        dirs.push(global);
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Basename of a `.w` `FileName` (strips `SHAPES\\foo.s` / `foo.s`).
pub fn shape_file_basename(file_name: &str) -> &str {
    file_name
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(file_name)
}

/// Resolve `SHAPES/foo.s` under the route directory (case-insensitive on Linux).
pub fn resolve_shape_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let base = shape_file_basename(file_name);
    for subdir in ["SHAPES", "shapes"] {
        let path = route_dir.join(subdir).join(base);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    // Open Rails / MSTS trainsets sometimes store `.s` in the trainset root.
    let direct = route_dir.join(base);
    if direct.is_file() {
        return Some(direct);
    }
    openrailsrs_formats::resolve_path_case_insensitive(&direct)
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
    let base = shape_file_basename(file_name);
    if let Some(p) = resolve_texture_path_exact(route_dir, base) {
        return Some(p);
    }
    if !base.eq_ignore_ascii_case(file_name)
        && let Some(p) = resolve_texture_path_exact(route_dir, file_name)
    {
        return Some(p);
    }
    let path_obj = Path::new(base);
    if path_obj.extension().map(|e| e.to_ascii_lowercase()) == Some(std::ffi::OsString::from("ace"))
    {
        let dds_name = path_obj
            .with_extension("dds")
            .to_string_lossy()
            .into_owned();
        if let Some(p) = resolve_texture_path_exact(route_dir, &dds_name) {
            return Some(p);
        }
    }
    None
}

fn resolve_texture_path_exact(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let direct = route_dir.join(file_name);
    if direct.is_file() {
        return Some(direct);
    }
    if let Some(p) = openrailsrs_formats::resolve_path_case_insensitive(&direct) {
        return Some(p);
    }
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

/// Root folder for a vehicle shape's textures.
///
/// MSTS/Open Rails trainsets appear in both layouts:
/// - `<trainset>/SHAPES/car.s` with textures in `<trainset>/TEXTURES/`
/// - `<trainset>/car.s` with textures directly in `<trainset>/`
pub fn vehicle_texture_root_for_shape_path(shape_path: &Path) -> Option<&Path> {
    let parent = shape_path.parent()?;
    if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SHAPES"))
    {
        parent.parent()
    } else {
        Some(parent)
    }
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
            -1, // no prim_state for combined fallback
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
            part.alpha_test_mode,
            part.z_bias,
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
    alpha_test_mode: i32,
    z_bias: Option<f32>,
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
                let is_dds = tex_path.extension().map(|e| e.to_ascii_lowercase())
                    == Some(std::ffi::OsString::from("dds"));
                if is_dds {
                    if let Ok(bytes) = std::fs::read(&tex_path) {
                        if let Ok(image) = decode_dds_to_image(&bytes) {
                            let handle = texture_cache
                                .entry(tex_path.clone())
                                .or_insert_with(|| images.add(image))
                                .clone();
                            let material = materials.add(scenery_shape_material_with_texture(
                                scenery_base_tint(or_lighting_enabled()),
                                handle,
                                &[],
                                AlphaMode::Blend,
                                z_bias.unwrap_or(0.0),
                            ));
                            return (material, true, true);
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
                    let alpha_mode =
                        alpha_mode_from_prim_state(&ace, tex_name, shader_name, alpha_test_mode);
                    let is_transparent = !matches!(alpha_mode, AlphaMode::Opaque);
                    let lit = or_lighting_enabled();
                    // MSTS `.ace` atlases are authored dark for fixed-function drawing; as a raw
                    // PBR albedo they render as black silhouettes under the sun. Normalizing the
                    // near-black ones to a plausible reflectance fixes that in BOTH paths. The lit
                    // path then relies on the sun/ambient (white tint, no emissive) rather than the
                    // unlit ×boost to set final brightness — see `scenery_albedo_tint` /
                    // `scenery_shape_material_with_texture`.
                    let (rgba, pixel_brightened) = brighten_dark_ace_rgba(&ace.mip0);
                    let tint = scenery_albedo_tint(pixel_brightened, lit);
                    let image = ace_rgba_to_image(ace.width, ace.height, &rgba);
                    let handle = texture_cache
                        .entry(tex_path)
                        .or_insert_with(|| images.add(image))
                        .clone();
                    let material = materials.add(scenery_shape_material_with_texture(
                        tint,
                        handle,
                        &rgba,
                        alpha_mode,
                        z_bias.unwrap_or(0.0),
                    ));
                    return (material, true, is_transparent);
                }
            }
        }
    }

    // Untextured fallback: the emissive lift fakes brightness for the unlit path only.
    let fallback_emissive = if or_lighting_enabled() {
        LinearRgba::BLACK
    } else {
        LinearRgba::from(fallback_color) * 0.35
    };
    let material = materials.add(scenery_shape_material(StandardMaterial {
        base_color: fallback_color,
        emissive: fallback_emissive,
        perceptual_roughness: 0.75,
        metallic: 0.1,
        double_sided: true,
        depth_bias: z_bias.unwrap_or(0.0),
        ..default()
    }));
    (material, false, false)
}

/// Determine the Bevy [`AlphaMode`] for a texture+shader combination.
///
/// Priority order:
/// 1. `prim_state.alpha_test_mode` when explicitly set (0 = opaque, 1 = test, 2 = blend).
/// 2. Texture pixel analysis (semi-transparent pixels → blend, alpha-only → mask).
/// 3. Shader name / texture name heuristics.
pub fn alpha_mode_from_prim_state(
    ace: &AceFile,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> AlphaMode {
    // Honour the explicit prim_state flag first.
    match alpha_test_mode {
        0 => return AlphaMode::Opaque,
        1 => return AlphaMode::Mask(0.5),
        2 => return AlphaMode::Blend,
        _ => {}
    }
    // Fall back to the per-texture heuristic.
    shape_alpha_mode(ace, texture_file, shader_name)
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

        let (handle, has_texture, is_transparent) = material_for_shape_texture(
            &[route.as_path()],
            Some("alpha_test.ace"),
            Some("BlendATexDiff"),
            -1, // no explicit alpha_test_mode → heuristic path
            None,
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
            -1, // no explicit alpha_test_mode → heuristic path
            None,
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
        let (frame, fit_scale) = vehicle_cab_frame_and_exterior_scale(&mesh, 0.0, 18.0);
        assert!((frame.scale.x - 1.0).abs() < 1e-4);
        assert!((fit_scale - 18.0).abs() < 1e-3);
        let full = vehicle_shape_local_transform(&mesh, 0.0, 18.0);
        assert!((full.scale.x - fit_scale).abs() < 1e-3);
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
        let asset = load_shape_render_asset_from_path(
            &path,
            &[route.as_path(), texture_root],
            Some(crate::launch::LIVE_TRAIN_LOD_DISTANCE_M),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            Color::srgb(0.55, 0.58, 0.62),
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
