//! Spawn del mundo 3D (terreno, vía, objetos) — usado de forma progresiva desde `loading`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_ace::AceFile;
use openrailsrs_formats::TSectionCatalog;

use crate::objects::{ObjectKind, ObjectMarker};
use crate::shape_descriptor::ShapeDescriptor;
use crate::shapes;
use crate::stream::TileContent;
use crate::terrain::{PatchGeometry, TileGeometry};
use crate::textures::{
    TextureEnvironment, TextureFlags, global_assets_dirs, index_textures_tree,
    index_trainset_textures, load_ace_file, load_texture_image, resolve_shape_path,
    resolve_shape_path_in_dirs, resolve_texture_with_index, shape_search_dirs, shape_texture_flags,
    texture_search_dirs_for_shape,
};
use crate::track::TrackRibbon;
use openrailsrs_or_shader::standard_pbr::{apply_albedo_scale, resolve_or_material_pbr};

use crate::or_scenery_material::{
    OrSceneryMaterial, create_or_scenery_material, or_scenery_shaders_enabled,
};
use crate::or_terrain_material::{
    DEFAULT_MICROTEX, OrTerrainMaterial, create_or_terrain_material, or_terrain_shaders_enabled,
    set_terrain_repeat_sampler,
};
use crate::tdb_track::TdbContext;

/// Material de escena: PBR estándar o shader OR (WGSL).
#[derive(Clone)]
pub(crate) enum SceneMaterialHandle {
    Standard(Handle<StandardMaterial>),
    OrScenery(Handle<OrSceneryMaterial>),
}

/// Handles de una parte de shape ya en GPU.
#[derive(Clone)]
pub struct PartHandles {
    mesh: Handle<Mesh>,
    material: SceneMaterialHandle,
}

/// Índice case-insensitive de assets `.s` / `.ace`.
#[derive(Clone)]
pub struct AssetIndex {
    shapes: HashMap<String, PathBuf>,
    textures: HashMap<String, PathBuf>,
    pub tsection: TSectionCatalog,
}

impl AssetIndex {
    pub fn build(route_dir: &Path, msts_root: &Path) -> Self {
        let mut shapes = HashMap::new();
        let mut textures = HashMap::new();

        // 1. Indexar GLOBAL primero para que la ruta pueda sobrescribir con SHAPES/TEXTURES locales
        let global = msts_root.join("GLOBAL");
        for sub in ["SHAPES", "shapes"] {
            index_shapes_tree(&mut shapes, &global.join(sub));
        }
        index_textures_tree(&mut textures, &global);

        // 2. Indexar la ruta (sobrescribe GLOBAL)
        for sub in ["SHAPES", "shapes"] {
            index_shapes_tree(&mut shapes, &route_dir.join(sub));
        }
        index_textures_tree(&mut textures, route_dir);
        // UKFS y alias de ruta suelen vivir en `Alias/` además de `TEXTURES/`.
        for sub in ["Alias", "alias"] {
            index_textures_tree(&mut textures, &route_dir.join(sub));
        }

        // 3. Trainsets del content pack (vehículos rolling stock)
        index_trainset_textures(&mut textures, msts_root);

        let tsection = TSectionCatalog::load_for_route(route_dir).unwrap_or_default();

        Self {
            shapes,
            textures,
            tsection,
        }
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    fn shape(&self, file: &str) -> Option<&PathBuf> {
        self.shapes.get(&base_lower(file)?)
    }

    pub fn resolve_texture(
        &self,
        dirs: &[&Path],
        file_name: &str,
        env: &TextureEnvironment,
        flags: TextureFlags,
    ) -> Option<PathBuf> {
        resolve_texture_with_index(&self.textures, dirs, file_name, env, flags)
    }
}

/// Resuelve el `.s` de un objeto del `.w`.
pub fn resolve_object_shape_path(
    obj: &ObjectMarker,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
) -> Option<PathBuf> {
    if obj.kind == ObjectKind::Track {
        return resolve_trackobj_shape_path(
            route_dir,
            msts_root,
            index,
            obj.file_name.as_deref(),
            obj.section_idx,
        );
    }
    let file = obj.file_name.as_deref()?;
    resolve_shape_file(index, route_dir, msts_root, file)
}

/// `TrackObj` usa shapes de `GLOBAL/SHAPES` (Open Rails `Scenery.cs`).
pub fn resolve_trackobj_shape_path(
    route_dir: &Path,
    msts_root: &Path,
    index: &AssetIndex,
    file_name: Option<&str>,
    section_idx: Option<u32>,
) -> Option<PathBuf> {
    let name = file_name
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            section_idx.and_then(|idx| index.tsection.shape_file_name(idx).map(str::to_string))
        })?;
    for global in global_assets_dirs(route_dir, msts_root) {
        if let Some(path) = resolve_shape_path(&global, &name) {
            return Some(path);
        }
    }
    if let Some(path) = resolve_shape_path(&msts_root.join("GLOBAL"), &name) {
        return Some(path);
    }
    index.shape(&name).cloned()
}

fn resolve_shape_file(
    index: &AssetIndex,
    route: &Path,
    msts_root: &Path,
    file: &str,
) -> Option<PathBuf> {
    if let Some(path) = index.shape(file) {
        return Some(path.clone());
    }
    let dirs = shape_search_dirs(route, msts_root);
    let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
    resolve_shape_path_in_dirs(&refs, file)
}

fn base_lower(file: &str) -> Option<String> {
    Path::new(file)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn index_dir(map: &mut HashMap<String, PathBuf>, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            map.entry(name.to_ascii_lowercase()).or_insert(path);
        }
    }
}

fn index_shapes_tree(map: &mut HashMap<String, PathBuf>, dir: &Path) {
    index_dir(map, dir);
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            index_dir(map, &entry.path());
        }
    }
}

/// Texturas MSTS/OR: albedo oscuro pensado para fixed-function + unlit.
const MSTS_ALBEDO_BOOST: f32 = 4.0;
const DARK_TEXTURE_LUMA: f32 = 60.0;
/// Follaje con alpha-test suele quedar negro bajo unlit si luma < ~90.
const FOLIAGE_LUMA_THRESHOLD: f32 = 96.0;
const TARGET_TEXTURE_LUMA: f32 = 112.0;
const FOLIAGE_MASK_CUTOFF: f32 = 0.04;
const DEFAULT_MASK_CUTOFF: f32 = 0.35;
/// Lift emissive para atlases oscuros (paridad viewer3d / OR unlit).
const SCENERY_DARK_EMISSIVE: LinearRgba = LinearRgba::new(0.55, 0.55, 0.58, 1.0);

struct PreparedAce {
    image: Image,
    tint: Color,
}

fn ace_mean_luma(rgba: &[u8]) -> f32 {
    if rgba.len() < 4 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for px in rgba.chunks_exact(4) {
        // MSTS a veces deja alpha=0 en mip0 aunque el RGB sea válido; no excluir esos píxeles.
        let luma = 0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2]);
        if luma < 1.0 && px[3] < 8 {
            continue;
        }
        sum += luma;
        n += 1;
    }
    if n == 0 { 0.0 } else { (sum / n as f64) as f32 }
}

fn brighten_dark_ace_rgba(rgba: &[u8], luma_threshold: f32) -> (Vec<u8>, bool) {
    let mean = ace_mean_luma(rgba);
    if mean >= luma_threshold {
        return (rgba.to_vec(), false);
    }
    let scale = (TARGET_TEXTURE_LUMA / mean.max(1.0)).min(128.0);
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        let luma = 0.299 * f32::from(px[0]) + 0.587 * f32::from(px[1]) + 0.114 * f32::from(px[2]);
        if luma < 1.0 && px[3] < 8 {
            continue;
        }
        for c in &mut px[0..3] {
            *c = (f32::from(*c) * scale).min(255.0).round() as u8;
        }
        // Texturas opacas con alpha=0 en el archivo: forzar opaco para Bevy.
        if px[3] < 8 && luma >= 1.0 {
            px[3] = 255;
        }
    }
    (out, true)
}

fn msts_albedo_boost_factor(
    mean_luma: f32,
    texture_name: &str,
    pixel_brightened: bool,
    lit: bool,
) -> f32 {
    if lit {
        if pixel_brightened {
            return 1.02;
        }
        let lower = texture_name.to_ascii_lowercase();
        if lower.starts_with("ukfs_")
            || lower.contains("chalk")
            || lower.contains("cliff")
            || lower.contains("concrete")
            || lower.contains("white")
        {
            return 1.0;
        }
        if mean_luma >= 80.0 {
            1.0
        } else if mean_luma >= 45.0 {
            1.06
        } else {
            1.14
        }
    } else if pixel_brightened {
        1.25
    } else {
        let lower = texture_name.to_ascii_lowercase();
        if lower.starts_with("ukfs_") {
            return if mean_luma >= 72.0 { 1.0 } else { 1.5 };
        }
        if lower.contains("chalk")
            || lower.contains("cliff")
            || lower.contains("concrete")
            || lower.contains("white")
        {
            return 1.0;
        }
        if mean_luma >= 100.0 {
            1.0
        } else if mean_luma >= 72.0 {
            1.5
        } else {
            MSTS_ALBEDO_BOOST
        }
    }
}

fn texture_name_suggests_cutout(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    [
        "tree",
        "baum",
        "bush",
        "hedge",
        "leaf",
        "poplar",
        "hornbeam",
        "ash",
        "birch",
        "beech",
        "forest",
        "clump",
        "treeline",
        "nut",
        "pine",
        "spruce",
        "willow",
        "eboshi",
        "poplars",
        "woody",
        "fira",
        "plumtree",
        "scrub",
        "grassscrub",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Open Rails `TexDiff` / `BlendATexDiff`: color de vértice × textura (sin vertex attrs en Bevy).
fn shader_uses_vertex_color_multiply(shader_name: Option<&str>) -> bool {
    shader_name.is_some_and(|s| {
        let n = s.to_ascii_lowercase();
        n.contains("diff") || n == "tex"
    })
}

fn apply_shader_vertex_tint(
    tint: Color,
    solid_color: Option<[f32; 3]>,
    shader_name: Option<&str>,
) -> Color {
    let Some(rgb) = solid_color else {
        return tint;
    };
    if !shader_uses_vertex_color_multiply(shader_name) {
        return tint;
    }
    let c = tint.to_linear();
    Color::linear_rgba(c.red * rgb[0], c.green * rgb[1], c.blue * rgb[2], c.alpha)
}

/// Ajustes finales por nombre de textura (unlit exagera algunos ACE de vía/follaje).
fn finalize_scenery_tint(texture_name: &str, tint: Color, lit: bool) -> Color {
    let lower = texture_name.to_ascii_lowercase();
    if lower.contains("ukfs") && lower.contains("rail") {
        let c = tint.to_linear();
        if lit {
            // Acero envejecido bajo sol direccional (PBR metálico).
            return Color::linear_rgba(
                (c.red * 0.78).min(0.72),
                (c.green * 0.76).min(0.70),
                (c.blue * 0.74).min(0.68),
                c.alpha,
            );
        }
        return Color::linear_rgba(
            (c.red * 0.62).min(0.82),
            (c.green * 0.58).min(0.72),
            (c.blue * 0.45).min(0.52),
            c.alpha,
        );
    }
    if lower.contains("grassscrub") || lower.contains("scrub") {
        let c = tint.to_linear();
        let luma = 0.299 * c.red + 0.587 * c.green + 0.114 * c.blue;
        if luma > 0.9 {
            return Color::linear_rgba(c.red * 0.78, c.green * 0.78, c.blue * 0.78, c.alpha);
        }
    }
    tint
}

fn msts_albedo_tint(
    pixel_brightened: bool,
    additive: bool,
    mean_luma: f32,
    texture_name: &str,
    lit: bool,
) -> Color {
    if additive {
        return Color::WHITE;
    }
    let boost = msts_albedo_boost_factor(mean_luma, texture_name, pixel_brightened, lit);
    Color::linear_rgb(boost, boost, boost)
}

fn scenery_needs_emissive_texture(
    rgba: &[u8],
    alpha_mode: AlphaMode,
    texture_name: &str,
    lit: bool,
) -> bool {
    if lit && matches!(alpha_mode, AlphaMode::Opaque) {
        return false;
    }
    let luma = ace_mean_luma(rgba);
    if matches!(alpha_mode, AlphaMode::Add) {
        return luma < FOLIAGE_LUMA_THRESHOLD || texture_name_suggests_additive(texture_name);
    }
    if matches!(alpha_mode, AlphaMode::Mask(_)) && texture_name_suggests_cutout(texture_name) {
        return luma < FOLIAGE_LUMA_THRESHOLD;
    }
    luma < 32.0
}

fn brighten_luma_threshold(texture_name: &str, alpha_mode: AlphaMode) -> f32 {
    if matches!(alpha_mode, AlphaMode::Mask(_)) && texture_name_suggests_cutout(texture_name) {
        return FOLIAGE_LUMA_THRESHOLD;
    }
    DARK_TEXTURE_LUMA
}

fn normalize_alpha_mode(mode: AlphaMode, texture_name: &str) -> AlphaMode {
    match mode {
        AlphaMode::Mask(_) if texture_name_suggests_cutout(texture_name) => {
            AlphaMode::Mask(FOLIAGE_MASK_CUTOFF)
        }
        AlphaMode::Mask(_) => AlphaMode::Mask(DEFAULT_MASK_CUTOFF),
        other => other,
    }
}

fn prepared_ace(
    ace: &AceFile,
    texture_name: &str,
    alpha_mode: AlphaMode,
    lit: bool,
) -> PreparedAce {
    let threshold = brighten_luma_threshold(texture_name, alpha_mode);
    let mean_luma = ace_mean_luma(&ace.mip0);
    let (mip0, brightened) = brighten_dark_ace_rgba(&ace.mip0, threshold);
    let mut prepared = ace.clone();
    prepared.mip0 = mip0;
    let additive = matches!(alpha_mode, AlphaMode::Add);
    PreparedAce {
        image: ace_to_image(&prepared),
        tint: msts_albedo_tint(brightened, additive, mean_luma, texture_name, lit),
    }
}

#[allow(clippy::too_many_arguments)]
fn msts_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Handle<Image>,
    tint: Color,
    alpha_mode: AlphaMode,
    roughness: f32,
    emissive_texture: bool,
    lit: bool,
    texture_name: &str,
    shader_name: Option<&str>,
) -> Handle<StandardMaterial> {
    let pbr = resolve_or_material_pbr(texture_name, shader_name, lit, roughness);
    let material_lit = lit && !pbr.force_unlit;
    let mut mat = StandardMaterial {
        base_color: apply_albedo_scale(tint, pbr.albedo_scale),
        base_color_texture: Some(texture.clone()),
        perceptual_roughness: pbr.roughness,
        metallic: pbr.metallic,
        reflectance: pbr.reflectance,
        alpha_mode,
        double_sided: true,
        cull_mode: None,
        unlit: !material_lit,
        fog_enabled: material_lit,
        ..default()
    };
    if emissive_texture {
        mat.emissive = if material_lit {
            SCENERY_DARK_EMISSIVE * 0.35
        } else {
            SCENERY_DARK_EMISSIVE
        };
        mat.emissive_texture = Some(texture);
    } else if pbr.ambient_fill != LinearRgba::new(0.0, 0.0, 0.0, 1.0) && material_lit {
        mat.emissive = pbr.ambient_fill;
    }
    materials.add(mat)
}

/// Caches de materiales de terreno (TERRTEX) estilo OR PSTerrain.
#[derive(Clone)]
pub struct TerrainSpawnCtx {
    pub fallback: Handle<OrTerrainMaterial>,
    pub mat_cache: HashMap<String, Handle<OrTerrainMaterial>>,
    pub tex_cache: HashMap<String, Handle<Image>>,
    pub materials_lit: bool,
    pub night: bool,
    pub use_or_shaders: bool,
}

impl TerrainSpawnCtx {
    pub fn new(
        or_materials: &mut Assets<OrTerrainMaterial>,
        images: &mut Assets<Image>,
        materials_lit: bool,
        night: bool,
    ) -> Self {
        let fallback = or_terrain_fallback(or_materials, images, materials_lit, night);
        Self {
            fallback,
            mat_cache: HashMap::new(),
            tex_cache: HashMap::new(),
            materials_lit,
            night,
            use_or_shaders: or_terrain_shaders_enabled(materials_lit),
        }
    }

    fn material_for_patch(
        &mut self,
        or_materials: &mut Assets<OrTerrainMaterial>,
        images: &mut Assets<Image>,
        route: &Path,
        patch: &PatchGeometry,
    ) -> Handle<OrTerrainMaterial> {
        let base_name = patch.texture.as_deref().unwrap_or("grass.ace");
        let overlay_name = patch.overlay_texture.as_deref().unwrap_or(DEFAULT_MICROTEX);
        let cache_key = format!(
            "{base_name}|{overlay_name}|{:.3}|lit={}|night={}|or={}",
            patch.overlay_scale, self.materials_lit, self.night, self.use_or_shaders
        );
        if let Some(mat) = self.mat_cache.get(&cache_key) {
            return mat.clone();
        }
        let base = self.load_terrtex(route, images, base_name, true);
        let overlay = self
            .load_terrtex(route, images, overlay_name, false)
            .or_else(|| self.load_terrtex(route, images, DEFAULT_MICROTEX, false));
        let mat = match (base, overlay) {
            (Some(base_h), Some(overlay_h)) => create_or_terrain_material(
                or_materials,
                base_h,
                overlay_h,
                patch.overlay_scale,
                self.materials_lit,
                self.night,
            ),
            _ => self.fallback.clone(),
        };
        self.mat_cache.insert(cache_key, mat.clone());
        mat
    }

    fn load_terrtex(
        &mut self,
        route: &Path,
        images: &mut Assets<Image>,
        file_name: &str,
        sanitize_base: bool,
    ) -> Option<Handle<Image>> {
        let key = format!("{file_name}:{}", if sanitize_base { "base" } else { "raw" });
        if let Some(handle) = self.tex_cache.get(&key) {
            return Some(handle.clone());
        }
        let path = crate::terrain::resolve_terrtex_path(route, file_name)?;
        let is_dds = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dds"));
        let image = if is_dds {
            load_texture_image(&path)?
        } else {
            let ace = load_terrtex_ace(route, file_name)?;
            ace_to_image(&ace)
        };
        let mut image = image;
        set_terrain_repeat_sampler(&mut image);
        if sanitize_base {
            sanitize_terrain_base_rgba(image.data.as_mut());
        }
        let handle = images.add(image);
        self.tex_cache.insert(key, handle.clone());
        Some(handle)
    }
}

fn or_terrain_fallback(
    or_materials: &mut Assets<OrTerrainMaterial>,
    images: &mut Assets<Image>,
    lit: bool,
    night: bool,
) -> Handle<OrTerrainMaterial> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut base = Image::new_fill(
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[72, 107, 56, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    set_terrain_repeat_sampler(&mut base);
    let mut overlay = Image::new_fill(
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[128, 128, 128, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    set_terrain_repeat_sampler(&mut overlay);
    create_or_terrain_material(
        or_materials,
        images.add(base),
        images.add(overlay),
        32.0,
        lit,
        night,
    )
}

fn sanitize_terrain_base_rgba(data: Option<&mut Vec<u8>>) {
    let Some(data) = data else {
        return;
    };
    if data.chunks_exact(4).all(|rgba| rgba[3] >= 250) {
        return;
    }
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for rgba in data.chunks_exact(4) {
        if rgba[3] >= 250 && !looks_like_terrain_chroma_key(rgba) {
            sum[0] += rgba[0] as u64;
            sum[1] += rgba[1] as u64;
            sum[2] += rgba[2] as u64;
            count += 1;
        }
    }
    let fill = count
        .checked_sub(1)
        .map(|_| {
            [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ]
        })
        .unwrap_or([72, 107, 56]);
    for rgba in data.chunks_exact_mut(4) {
        if rgba[3] < 16 || looks_like_terrain_chroma_key(rgba) {
            rgba[0] = fill[0];
            rgba[1] = fill[1];
            rgba[2] = fill[2];
        }
        rgba[3] = 255;
    }
}

fn looks_like_terrain_chroma_key(rgba: &[u8]) -> bool {
    let [r, g, b, _] = [rgba[0], rgba[1], rgba[2], rgba[3]];
    b > 135 && g > 115 && r < 170 && b.saturating_sub(r) > 25 && b >= g
}

/// Contadores de resolución de texturas (diagnóstico al cargar objetos).
#[derive(Resource, Default)]
pub struct TextureLoadStats {
    pub resolved: u32,
    pub unresolved: u32,
    pub decode_failed: u32,
    pub no_texture_part: u32,
    pub vertex_color_part: u32,
    /// Primeras entradas para el log (shape, textura).
    pub unresolved_samples: Vec<(String, String)>,
    pub decode_failed_samples: Vec<(String, String)>,
    /// Texturas con luminosidad baja o alpha no-opaco (diagnóstico de manchas negras).
    pub dark_or_blend_samples: Vec<(String, String, f32, String)>,
}

impl TextureLoadStats {
    const MAX_SAMPLES: usize = 25;

    pub fn record_unresolved(&mut self, shape_file: &str, texture: &str, shape_path: &Path) {
        self.unresolved += 1;
        if self.unresolved_samples.len() < Self::MAX_SAMPLES {
            self.unresolved_samples.push((
                shape_file.to_string(),
                format!("{texture}  [{}]", shape_path.display()),
            ));
        }
        if texture_debug_enabled() {
            eprintln!(
                "[textura] NO RESUELTA: {texture}  (shape {}  file {shape_file})",
                shape_path.display()
            );
        }
    }

    pub fn record_decode_failed(&mut self, shape_file: &str, texture: &str, path: &Path) {
        self.decode_failed += 1;
        if self.decode_failed_samples.len() < Self::MAX_SAMPLES {
            self.decode_failed_samples.push((
                shape_file.to_string(),
                format!("{texture}  [{}]", path.display()),
            ));
        }
        if texture_debug_enabled() {
            eprintln!(
                "[textura] DECODE FALLÓ: {texture}  ({})  shape {shape_file}",
                path.display()
            );
        }
    }

    pub fn record_resolved(&mut self) {
        self.resolved += 1;
    }

    pub fn record_material_diagnostic(
        &mut self,
        shape_file: &str,
        texture: &str,
        luma: f32,
        alpha_mode_str: &str,
    ) {
        if self.dark_or_blend_samples.len() < Self::MAX_SAMPLES * 2 {
            self.dark_or_blend_samples.push((
                shape_file.to_string(),
                texture.to_string(),
                luma,
                alpha_mode_str.to_string(),
            ));
        }
    }

    pub fn report(&self) {
        let total = self.resolved + self.unresolved + self.decode_failed;
        if total == 0 && self.no_texture_part == 0 && self.vertex_color_part == 0 {
            return;
        }
        println!(
            "texturas: {} ok, {} archivo no encontrado, {} decode falló, {} partes sin textura en el shape, {} con color vértice",
            self.resolved,
            self.unresolved,
            self.decode_failed,
            self.no_texture_part,
            self.vertex_color_part
        );
        if texture_debug_enabled() || self.unresolved > 0 || self.decode_failed > 0 {
            for (shape, detail) in &self.unresolved_samples {
                println!("  · falta: {shape} → {detail}");
            }
            for (shape, detail) in &self.decode_failed_samples {
                println!("  · decode: {shape} → {detail}");
            }
            if self.unresolved > self.unresolved_samples.len() as u32 {
                println!(
                    "  … (+{} faltantes más)",
                    self.unresolved - self.unresolved_samples.len() as u32
                );
            }
        }
        if !self.dark_or_blend_samples.is_empty() {
            println!(
                "  diagnóstico manchas negras: {} texturas con luma < 60 ó alpha no-opaco:",
                self.dark_or_blend_samples.len()
            );
            for (shape, tex, luma, mode) in &self.dark_or_blend_samples {
                let luma_str = if *luma >= 0.0 {
                    format!("{luma:.0}")
                } else {
                    "n/a".to_string()
                };
                println!("    · {shape} → {tex}  luma={luma_str}  alpha={mode}");
            }
        }
        if self.unresolved == 0 && self.decode_failed == 0 && self.dark_or_blend_samples.is_empty()
        {
            println!("texturas: todas ok, sin candidatos a manchas negras");
        }
    }
}

/// Contadores de spawn de vía (UKFS, procedural, cinta) para diagnóstico al cargar.
#[derive(Resource, Default)]
pub struct TrackSpawnStats {
    pub uses_tdb: bool,
    pub ukfs_shapes_enabled: bool,
    pub shaped_chords: u32,
    pub ukfs_instances_attempted: u32,
    pub ukfs_shapes_spawned: u32,
    pub ukfs_shape_missing: u32,
    pub ukfs_mesh_empty: u32,
    pub ukfs_missing_samples: Vec<String>,
    pub procedural_tdb_segments: u32,
    pub procedural_trackobj_segments: u32,
    pub ribbon_fallback: bool,
    pub tiles_suppressed: u32,
    pub tiles_suppressed_bypassed: u32,
    pub tiles_skipped_non_center: u32,
}

impl TrackSpawnStats {
    const MAX_SAMPLES: usize = 12;

    pub fn record_ukfs_shape_missing(&mut self, label: impl Into<String>) {
        self.ukfs_shape_missing += 1;
        if self.ukfs_missing_samples.len() < Self::MAX_SAMPLES {
            self.ukfs_missing_samples.push(label.into());
        }
    }

    pub fn report(&self) {
        if !self.uses_tdb
            && self.procedural_trackobj_segments == 0
            && !self.ribbon_fallback
            && self.tiles_suppressed == 0
        {
            return;
        }

        let mode = if !self.uses_tdb {
            "graph / cinta"
        } else if self.ukfs_shapes_enabled {
            "UKFS .s + relleno procedural"
        } else {
            "solo procedural (OPENRAILSRS_TDB_UKFS=procedural|0)"
        };
        println!("vía [{mode}]:");
        if self.uses_tdb {
            println!(
                "  .tdb: {} acordes con shape, {} instancias UKFS, {} shapes ok, {} shape no encontrado, {} mesh vacío/falló",
                self.shaped_chords,
                self.ukfs_instances_attempted,
                self.ukfs_shapes_spawned,
                self.ukfs_shape_missing,
                self.ukfs_mesh_empty,
            );
            println!(
                "  procedural: {} segmentos (.tdb) | {} segmentos (TrackObj .w)",
                self.procedural_tdb_segments, self.procedural_trackobj_segments,
            );
            if self.tiles_suppressed > 0 {
                println!(
                    "  tiles omitidos (TrackObj en .w suprime .tdb): {}",
                    self.tiles_suppressed
                );
            }
            if self.tiles_suppressed_bypassed > 0 {
                println!(
                    "  tiles con TrackObj pero .tdb procedural forzado: {}",
                    self.tiles_suppressed_bypassed
                );
            }
            if self.tiles_skipped_non_center > 0 {
                println!(
                    "  tiles sin spawn .tdb (solo tile central): {}",
                    self.tiles_skipped_non_center
                );
            }
        } else if self.procedural_trackobj_segments > 0 {
            println!(
                "  procedural: {} segmentos (TrackObj .w)",
                self.procedural_trackobj_segments
            );
        } else if self.ribbon_fallback {
            println!("  cinta TrackRibbon (fallback sin .tdb UKFS)");
        }
        if self.ukfs_shape_missing > 0 {
            println!("  UKFS shape no resuelto (muestra):");
            for name in &self.ukfs_missing_samples {
                println!("    · {name}");
            }
            let extra = self.ukfs_shape_missing as usize - self.ukfs_missing_samples.len();
            if extra > 0 {
                println!("    … (+{extra} más)");
            }
        }
        let any_3d = self.ukfs_shapes_spawned > 0
            || self.procedural_tdb_segments > 0
            || self.procedural_trackobj_segments > 0;
        if self.uses_tdb && !any_3d && !self.ribbon_fallback {
            println!(
                "  aviso: no se spawneó geometría de vía 3D; revisá GLOBAL/SHAPES y tsection.dat"
            );
        }
        if self.ukfs_shapes_enabled
            && self.ukfs_shapes_spawned == 0
            && self.procedural_tdb_segments == 0
        {
            println!(
                "  tip: probá `OPENRAILSRS_TDB_UKFS=procedural` para rieles 3D de prueba (dyntrack)"
            );
        }
    }
}

pub fn texture_debug_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_TEXTURE_DEBUG").is_some()
}

/// Caches de shapes/objetos.
#[derive(Clone)]
pub struct ObjectSpawnCtx {
    pub shape_cache: HashMap<String, Vec<PartHandles>>,
    pub tex_mat_cache: HashMap<String, Handle<StandardMaterial>>,
    pub or_tex_mat_cache: HashMap<String, Handle<OrSceneryMaterial>>,
    pub color_mat_cache: HashMap<[u8; 3], Handle<StandardMaterial>>,
    pub untextured: Handle<StandardMaterial>,
    pub materials_lit: bool,
    pub use_or_shaders: bool,
    pub moment_atlas: Handle<Image>,
    pub shadow_map_limits: [f32; 4],
}

impl ObjectSpawnCtx {
    pub fn new(
        materials: &mut Assets<StandardMaterial>,
        materials_lit: bool,
        moment_atlas: Handle<Image>,
        shadow_map_limits: [f32; 4],
    ) -> Self {
        let untextured = materials.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.70, 0.66),
            perceptual_roughness: 0.85,
            double_sided: true,
            cull_mode: None,
            unlit: !materials_lit,
            ..default()
        });
        Self {
            shape_cache: HashMap::new(),
            tex_mat_cache: HashMap::new(),
            or_tex_mat_cache: HashMap::new(),
            color_mat_cache: HashMap::new(),
            untextured,
            materials_lit,
            use_or_shaders: or_scenery_shaders_enabled(materials_lit),
            moment_atlas,
            shadow_map_limits,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_terrain_patches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    ctx: &mut TerrainSpawnCtx,
    or_materials: &mut Assets<OrTerrainMaterial>,
    images: &mut Assets<Image>,
    route: &Path,
    tile: &TileGeometry,
    from: usize,
    to: usize,
    tile_offset: Vec3,
    tile_x: i32,
    tile_z: i32,
) {
    for (i, patch) in tile
        .patches
        .iter()
        .enumerate()
        .skip(from)
        .take(to.saturating_sub(from))
    {
        let mesh_handle = meshes.add(patch_mesh(patch));
        let material = ctx.material_for_patch(or_materials, images, route, patch);
        commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::from_array(patch.offset) + tile_offset),
            TileContent { tile_x, tile_z },
            Name::new(format!("terrain_patch_{i}")),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_tile_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    index: Option<&AssetIndex>,
    obj_ctx: Option<&mut ObjectSpawnCtx>,
    route_dir: &Path,
    msts_root: &Path,
    tdb: Option<&crate::tdb_track::TdbContext>,
    shaped_chords: &[(Vec3, Vec3, u32)],
    ribbon: &TrackRibbon,
    objects: &[ObjectMarker],
    center_tile: (i32, i32),
    heights: &crate::tdb_track::TileHeightIndex<'_>,
    tile_offset: Vec3,
    materials_lit: bool,
    tile_x: i32,
    tile_z: i32,
    tex_stats: &mut TextureLoadStats,
    texture_env: &TextureEnvironment,
    viewer_pos: Vec3,
    mut track_stats: Option<&mut TrackSpawnStats>,
) {
    let suppress = crate::objects::tile_suppresses_tdb_ribbon(objects);
    let force_procedural = tdb_procedural_forced();
    if suppress && !force_procedural {
        if let Some(stats) = track_stats.as_deref_mut() {
            stats.tiles_suppressed += 1;
        }
        return;
    }
    if suppress && force_procedural {
        if let Some(stats) = track_stats.as_deref_mut() {
            stats.tiles_suppressed_bypassed += 1;
        }
    }
    if let Some(ctx) = tdb.filter(|c| crate::tdb_track::route_has_ukfs_tsection(&c.tsection)) {
        let ukfs_on = tdb_ukfs_shapes_enabled();
        if let Some(stats) = track_stats.as_deref_mut() {
            stats.uses_tdb = true;
            stats.ukfs_shapes_enabled = ukfs_on;
        }
        if (tile_x, tile_z) != center_tile {
            if let Some(stats) = track_stats.as_deref_mut() {
                stats.tiles_skipped_non_center += 1;
            }
            return;
        }
        let shaped = shaped_chords;
        if let Some(stats) = track_stats.as_deref_mut() {
            stats.shaped_chords = stats.shaped_chords.max(shaped.len() as u32);
        }
        let ukfs_instances = crate::tdb_track::tdb_ukfs_instances_for_tile(
            shaped,
            &ctx.tsection,
            center_tile,
            heights,
        );
        let ukfs_spawned = if ukfs_on {
            if let (Some(index), Some(obj_ctx), Some(stats)) =
                (index, obj_ctx, track_stats.as_deref_mut())
            {
                spawn_tdb_ukfs_shapes(
                    commands,
                    meshes,
                    materials,
                    or_materials,
                    images,
                    index,
                    obj_ctx,
                    route_dir,
                    msts_root,
                    &ukfs_instances,
                    &ctx.tsection,
                    tex_stats,
                    texture_env,
                    viewer_pos,
                    tile_x,
                    tile_z,
                    stats,
                )
            } else if let Some(stats) = track_stats.as_deref_mut() {
                stats.ukfs_instances_attempted += ukfs_instances.len() as u32;
                stats.ukfs_mesh_empty += ukfs_instances.len() as u32;
                0
            } else {
                0
            }
        } else {
            if let Some(stats) = track_stats.as_deref_mut() {
                stats.ukfs_instances_attempted += ukfs_instances.len() as u32;
            }
            0
        };

        let procedural_chords = if ukfs_spawned > 0 {
            crate::tdb_track::tdb_procedural_chords_for_tile(shaped, &ctx.tsection)
        } else {
            shaped.to_vec()
        };
        let segments = crate::tdb_track::tdb_procedural_segments_for_tile(
            &procedural_chords,
            &ctx.tsection,
            center_tile,
            heights,
        );
        if ukfs_spawned > 0 || !segments.is_empty() {
            if !segments.is_empty() {
                if let Some(stats) = track_stats.as_deref_mut() {
                    stats.procedural_tdb_segments += segments.len() as u32;
                }
                crate::dyntrack::spawn_procedural_track_batch(
                    commands,
                    meshes,
                    materials,
                    &segments,
                    materials_lit,
                    tile_x,
                    tile_z,
                    if ukfs_spawned > 0 {
                        "tdb_ukfs_fill"
                    } else {
                        "tdb_ukfs"
                    },
                );
            }
            return;
        }
    }
    if !ribbon.positions.is_empty() {
        if let Some(stats) = track_stats {
            stats.ribbon_fallback = true;
        }
    }
    spawn_track(
        commands,
        meshes,
        materials,
        ribbon,
        tile_offset,
        materials_lit,
        tile_x,
        tile_z,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ribbon: &TrackRibbon,
    tile_offset: Vec3,
    materials_lit: bool,
    tile_x: i32,
    tile_z: i32,
) {
    if ribbon.positions.is_empty() {
        return;
    }
    let track_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.16, 0.14),
        emissive: if materials_lit {
            LinearRgba::new(0.0, 0.0, 0.0, 1.0)
        } else {
            LinearRgba::new(0.06, 0.05, 0.04, 1.0)
        },
        perceptual_roughness: 0.92,
        double_sided: true,
        cull_mode: None,
        unlit: !materials_lit,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(track_ribbon_mesh(ribbon))),
        MeshMaterial3d(track_mat),
        Transform::from_translation(tile_offset),
        TileContent { tile_x, tile_z },
        Name::new("track"),
    ));
}

/// Spawnea las partes de un shape de vehículo (consist estático).
#[allow(clippy::too_many_arguments)]
pub fn spawn_consist_vehicle_shape(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    ctx: &mut ObjectSpawnCtx,
    route_dir: &Path,
    msts_root: &Path,
    obj: &ObjectMarker,
    world_transform: Transform,
    viewer_pos: Vec3,
    texture_env: &TextureEnvironment,
    tex_stats: &mut TextureLoadStats,
    name_prefix: &str,
) -> usize {
    let tile_offset = world_transform.translation;
    let Some(parts) = build_shape(
        obj,
        index,
        route_dir,
        msts_root,
        meshes,
        materials,
        or_materials,
        images,
        ctx,
        tex_stats,
        texture_env,
        viewer_pos,
        tile_offset,
    ) else {
        return 0;
    };
    let mut count = 0usize;
    for (pi, part) in parts.into_iter().enumerate() {
        match &part.material {
            SceneMaterialHandle::Standard(mat) => {
                commands.spawn((
                    Mesh3d(part.mesh),
                    MeshMaterial3d(mat.clone()),
                    world_transform,
                    Name::new(format!("{name_prefix}:part{pi}")),
                ));
            }
            SceneMaterialHandle::OrScenery(mat) => {
                commands.spawn((
                    Mesh3d(part.mesh),
                    MeshMaterial3d(mat.clone()),
                    world_transform,
                    Name::new(format!("{name_prefix}:part{pi}")),
                ));
            }
        }
        count += 1;
    }
    count
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_object_shape(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    ctx: &mut ObjectSpawnCtx,
    route_dir: &Path,
    msts_root: &Path,
    obj: &ObjectMarker,
    transform: Transform,
    tex_stats: &mut TextureLoadStats,
    texture_env: &TextureEnvironment,
    viewer_pos: Vec3,
    tile_offset: Vec3,
    tile_x: i32,
    tile_z: i32,
) -> bool {
    let Some(parts) = build_shape(
        obj,
        index,
        route_dir,
        msts_root,
        meshes,
        materials,
        or_materials,
        images,
        ctx,
        tex_stats,
        texture_env,
        viewer_pos,
        tile_offset,
    ) else {
        return false;
    };
    if parts.is_empty() {
        return false;
    }
    for part in parts {
        match &part.material {
            SceneMaterialHandle::Standard(mat) => {
                commands.spawn((
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    transform,
                    TileContent { tile_x, tile_z },
                    Name::new("object_shape"),
                ));
            }
            SceneMaterialHandle::OrScenery(mat) => {
                commands.spawn((
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    transform,
                    TileContent { tile_x, tile_z },
                    Name::new("object_shape"),
                ));
            }
        }
    }
    true
}

pub fn tdb_ukfs_shapes_enabled() -> bool {
    !tdb_procedural_forced()
}

/// Fuerza vía 3D procedural (dyntrack) desde `.tdb`, sin shapes UKFS.
pub fn tdb_procedural_forced() -> bool {
    matches!(
        std::env::var("OPENRAILSRS_TDB_UKFS").ok().as_deref(),
        Some("0") | Some("procedural")
    )
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_tdb_ukfs_shapes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    ctx: &mut ObjectSpawnCtx,
    route_dir: &Path,
    msts_root: &Path,
    instances: &[crate::tdb_track::TdbUkfsInstance],
    tsection: &openrailsrs_formats::TSectionCatalog,
    tex_stats: &mut TextureLoadStats,
    texture_env: &TextureEnvironment,
    viewer_pos: Vec3,
    tile_x: i32,
    tile_z: i32,
    track_stats: &mut TrackSpawnStats,
) -> usize {
    let mut spawned = 0usize;
    for inst in instances {
        track_stats.ukfs_instances_attempted += 1;
        let file_name = tsection
            .shape_file_name(inst.section_idx)
            .map(str::to_string);
        let shape_label = file_name
            .clone()
            .unwrap_or_else(|| format!("tsection:{}", inst.section_idx));
        let obj = ObjectMarker {
            position: inst.position,
            rotation: inst.rotation,
            scale: Vec3::ONE,
            kind: ObjectKind::Track,
            file_name,
            section_idx: Some(inst.section_idx),
            forest: None,
            hwater: None,
            transfer: None,
        };
        let transform = Transform {
            translation: inst.position,
            rotation: inst.rotation,
            scale: Vec3::ONE,
        };
        if resolve_object_shape_path(&obj, index, route_dir, msts_root).is_none() {
            track_stats.record_ukfs_shape_missing(shape_label);
            continue;
        }
        if spawn_object_shape(
            commands,
            meshes,
            materials,
            or_materials,
            images,
            index,
            ctx,
            route_dir,
            msts_root,
            &obj,
            transform,
            tex_stats,
            texture_env,
            viewer_pos,
            Vec3::ZERO,
            tile_x,
            tile_z,
        ) {
            spawned += 1;
            track_stats.ukfs_shapes_spawned += 1;
        } else {
            track_stats.ukfs_mesh_empty += 1;
        }
    }
    spawned
}

/// Fallback procedural cuando no hay `.s` resoluble en `GLOBAL/SHAPES`.
pub fn trackobj_prefers_procedural_mesh(
    obj: &ObjectMarker,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
) -> bool {
    if obj.kind != ObjectKind::Track {
        return false;
    }
    if resolve_object_shape_path(obj, index, route_dir, msts_root).is_some() {
        return false;
    }
    obj.section_idx.is_some()
}

/// Vía procedural para anclas `TrackObj` sin shape `.s` resoluble.
#[allow(clippy::too_many_arguments)]
pub fn spawn_trackobj_procedural_for_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    objects: &[ObjectMarker],
    tile_offset: Vec3,
    tsection: &openrailsrs_formats::TSectionCatalog,
    tdb: Option<&TdbContext>,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
    materials_lit: bool,
    tile_x: i32,
    tile_z: i32,
    track_stats: Option<&mut TrackSpawnStats>,
) -> usize {
    let mut segments = Vec::new();
    for obj in objects {
        if !trackobj_prefers_procedural_mesh(obj, index, route_dir, msts_root) {
            continue;
        }
        let rotation = tdb
            .map(|ctx| {
                crate::tdb_track::refine_trackobj_rotation(
                    &ctx.sections_by_shape,
                    tile_x,
                    tile_z,
                    obj,
                )
            })
            .unwrap_or(obj.rotation);
        segments.extend(crate::dyntrack::trackobj_procedural_segments(
            obj,
            tile_offset,
            tsection,
            rotation,
        ));
    }
    if segments.is_empty() {
        return 0;
    }
    if let Some(stats) = track_stats {
        stats.procedural_trackobj_segments += segments.len() as u32;
    }
    crate::dyntrack::spawn_procedural_track_batch(
        commands,
        meshes,
        materials,
        &segments,
        materials_lit,
        tile_x,
        tile_z,
        "trackobj",
    )
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_objects_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    ctx: &mut ObjectSpawnCtx,
    route_dir: &Path,
    msts_root: &Path,
    batch: &[ObjectMarker],
    tex_stats: &mut TextureLoadStats,
    tile_offset: Vec3,
    texture_env: &TextureEnvironment,
    viewer_pos: Vec3,
    tile_x: i32,
    tile_z: i32,
    materials_lit: bool,
    track_stats: Option<&mut TrackSpawnStats>,
    tdb: Option<&crate::tdb_track::TdbContext>,
) {
    let mut procedural_track: Vec<crate::dyntrack::ProceduralTrackSegment> = Vec::new();
    let skip_track_shapes = tdb_procedural_forced();
    for obj in batch {
        let rotation = if obj.kind == ObjectKind::Track {
            tdb.map(|ctx| {
                crate::tdb_track::refine_trackobj_rotation(
                    &ctx.sections_by_shape,
                    tile_x,
                    tile_z,
                    obj,
                )
            })
            .unwrap_or(obj.rotation)
        } else {
            obj.rotation
        };
        let transform = Transform {
            translation: obj.position + tile_offset,
            rotation,
            scale: obj.scale,
        };
        if !skip_track_shapes && trackobj_prefers_procedural_mesh(obj, index, route_dir, msts_root)
        {
            procedural_track.extend(crate::dyntrack::trackobj_procedural_segments(
                obj,
                tile_offset,
                &index.tsection,
                rotation,
            ));
            continue;
        }
        if skip_track_shapes && obj.kind == ObjectKind::Track {
            procedural_track.extend(crate::dyntrack::trackobj_procedural_segments(
                obj,
                tile_offset,
                &index.tsection,
                rotation,
            ));
            continue;
        }
        if spawn_object_shape(
            commands,
            meshes,
            materials,
            or_materials,
            images,
            index,
            ctx,
            route_dir,
            msts_root,
            obj,
            transform,
            tex_stats,
            texture_env,
            viewer_pos,
            tile_offset,
            tile_x,
            tile_z,
        ) {
            continue;
        }
        if obj.kind == ObjectKind::Track {
            procedural_track.extend(crate::dyntrack::trackobj_procedural_segments(
                obj,
                tile_offset,
                &index.tsection,
                rotation,
            ));
        }
    }
    if !procedural_track.is_empty() {
        if let Some(stats) = track_stats {
            stats.procedural_trackobj_segments += procedural_track.len() as u32;
        }
        crate::dyntrack::spawn_procedural_track_batch(
            commands,
            meshes,
            materials,
            &procedural_track,
            materials_lit,
            tile_x,
            tile_z,
            "trackobj",
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn build_shape(
    obj: &ObjectMarker,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    ctx: &mut ObjectSpawnCtx,
    tex_stats: &mut TextureLoadStats,
    texture_env: &TextureEnvironment,
    viewer_pos: Vec3,
    tile_offset: Vec3,
) -> Option<Vec<PartHandles>> {
    let path = resolve_object_shape_path(obj, index, route_dir, msts_root)?;
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("shape.s");
    let view_distance = viewer_pos.distance(obj.position + tile_offset);
    let lod_key = shapes::lod_cache_key(view_distance);
    let cache_key = format!(
        "{:?}:{}:lod={lod_key}:{}",
        obj.kind,
        file.to_ascii_lowercase(),
        texture_env.cache_key()
    );
    if let Some(cached) = ctx.shape_cache.get(&cache_key) {
        return Some(cached.clone());
    }
    let descriptor = ShapeDescriptor::load_for_shape(&path);
    let shape_flags = shape_texture_flags(&path, descriptor.alternative_texture);
    let ukfs_track = is_ukfs_track_shape(file);
    let parts = shapes::load_shape_parts_at_distance(&path, view_distance)?
        .into_iter()
        .filter(|p| shapes::part_visible(&descriptor, p, texture_env))
        .collect::<Vec<_>>();
    let handles: Vec<PartHandles> = parts
        .into_iter()
        .filter(|p| !p.positions.is_empty())
        .map(|p| {
            let material = match &p.texture {
                Some(name) => texture_material(
                    file,
                    name,
                    p.alpha_test_mode,
                    p.shader_name.as_deref(),
                    p.solid_color,
                    &path,
                    index,
                    route_dir,
                    msts_root,
                    shape_flags,
                    texture_env,
                    &mut ctx.tex_mat_cache,
                    or_materials,
                    &mut ctx.or_tex_mat_cache,
                    ctx.use_or_shaders,
                    materials,
                    images,
                    &ctx.untextured,
                    tex_stats,
                    ctx.materials_lit,
                    &ctx.moment_atlas,
                    ctx.shadow_map_limits,
                ),
                None if ukfs_track => ukfs_untextured_material(
                    file,
                    &p,
                    &path,
                    index,
                    route_dir,
                    msts_root,
                    shape_flags,
                    texture_env,
                    &mut ctx.tex_mat_cache,
                    or_materials,
                    &mut ctx.or_tex_mat_cache,
                    ctx.use_or_shaders,
                    materials,
                    images,
                    &ctx.untextured,
                    tex_stats,
                    ctx.materials_lit,
                    &ctx.moment_atlas,
                    ctx.shadow_map_limits,
                ),
                None => material_for_untextured_part(
                    &p,
                    materials,
                    &mut ctx.color_mat_cache,
                    &ctx.untextured,
                    tex_stats,
                    ctx.materials_lit,
                ),
            };
            PartHandles {
                mesh: meshes.add(shape_part_mesh(&p, p.texture.is_some(), ukfs_track)),
                material,
            }
        })
        .collect();
    ctx.shape_cache.insert(cache_key, handles.clone());
    Some(handles)
}

fn is_ukfs_track_shape(file: &str) -> bool {
    base_lower(file).is_some_and(|n| n.starts_with("ukfs_"))
}

fn ukfs_untextured_texture_hint(solid: Option<[f32; 3]>) -> &'static str {
    if let Some(rgb) = solid {
        let luma = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
        if luma > 0.55 {
            return "ukfs_rail.ace";
        }
        if luma < 0.25 {
            return "ballast.ace";
        }
    }
    "ukfs_tie.ace"
}

#[allow(clippy::too_many_arguments)]
fn ukfs_untextured_material(
    shape_file: &str,
    part: &shapes::ShapePart,
    shape_path: &Path,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
    shape_flags: TextureFlags,
    texture_env: &TextureEnvironment,
    tex_mat_cache: &mut HashMap<String, Handle<StandardMaterial>>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    or_tex_mat_cache: &mut HashMap<String, Handle<OrSceneryMaterial>>,
    use_or_shaders: bool,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    untextured: &Handle<StandardMaterial>,
    tex_stats: &mut TextureLoadStats,
    lit: bool,
    moment_atlas: &Handle<Image>,
    shadow_map_limits: [f32; 4],
) -> SceneMaterialHandle {
    let tex_name = ukfs_untextured_texture_hint(part.solid_color);
    texture_material(
        shape_file,
        tex_name,
        part.alpha_test_mode,
        Some("TexDiff"),
        None,
        shape_path,
        index,
        route_dir,
        msts_root,
        shape_flags,
        texture_env,
        tex_mat_cache,
        or_materials,
        or_tex_mat_cache,
        use_or_shaders,
        materials,
        images,
        untextured,
        tex_stats,
        lit,
        moment_atlas,
        shadow_map_limits,
    )
}

fn clamp_lit_vertex_color(color: Color) -> Color {
    let l = color.to_linear();
    let luma = 0.299 * l.red + 0.587 * l.green + 0.114 * l.blue;
    if luma <= 0.72 {
        return color;
    }
    let scale = 0.72 / luma;
    Color::linear_rgba(
        (l.red * scale).min(1.0),
        (l.green * scale).min(1.0),
        (l.blue * scale).min(1.0),
        l.alpha,
    )
}

fn material_for_untextured_part(
    part: &shapes::ShapePart,
    materials: &mut Assets<StandardMaterial>,
    color_mat_cache: &mut HashMap<[u8; 3], Handle<StandardMaterial>>,
    untextured: &Handle<StandardMaterial>,
    tex_stats: &mut TextureLoadStats,
    lit: bool,
) -> SceneMaterialHandle {
    if let Some(rgb) = part.solid_color {
        let key = [
            (rgb[0].clamp(0.0, 1.0) * 255.0) as u8,
            (rgb[1].clamp(0.0, 1.0) * 255.0) as u8,
            (rgb[2].clamp(0.0, 1.0) * 255.0) as u8,
        ];
        if let Some(mat) = color_mat_cache.get(&key) {
            tex_stats.vertex_color_part += 1;
            return SceneMaterialHandle::Standard(mat.clone());
        }
        let mat = materials.add(StandardMaterial {
            base_color: if lit {
                clamp_lit_vertex_color(Color::linear_rgb(rgb[0], rgb[1], rgb[2]))
            } else {
                Color::linear_rgb(rgb[0], rgb[1], rgb[2])
            },
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            unlit: !lit,
            ..default()
        });
        color_mat_cache.insert(key, mat.clone());
        tex_stats.vertex_color_part += 1;
        return SceneMaterialHandle::Standard(mat);
    }
    if part.colors.is_some() {
        tex_stats.vertex_color_part += 1;
        return SceneMaterialHandle::Standard(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            double_sided: true,
            cull_mode: None,
            unlit: !lit,
            ..default()
        }));
    }
    tex_stats.no_texture_part += 1;
    SceneMaterialHandle::Standard(untextured.clone())
}

#[allow(clippy::too_many_arguments)]
fn texture_material(
    shape_file: &str,
    name: &str,
    alpha_test_mode: i32,
    shader_name: Option<&str>,
    solid_color: Option<[f32; 3]>,
    shape_path: &Path,
    index: &AssetIndex,
    route_dir: &Path,
    msts_root: &Path,
    shape_flags: TextureFlags,
    texture_env: &TextureEnvironment,
    tex_mat_cache: &mut HashMap<String, Handle<StandardMaterial>>,
    or_materials: &mut Assets<OrSceneryMaterial>,
    or_tex_mat_cache: &mut HashMap<String, Handle<OrSceneryMaterial>>,
    use_or_shaders: bool,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    untextured: &Handle<StandardMaterial>,
    tex_stats: &mut TextureLoadStats,
    lit: bool,
    moment_atlas: &Handle<Image>,
    shadow_map_limits: [f32; 4],
) -> SceneMaterialHandle {
    let tex_dirs = texture_search_dirs_for_shape(shape_path, route_dir, msts_root);
    let dir_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let Some(tex_path) = index.resolve_texture(&dir_refs, name, texture_env, shape_flags) else {
        tex_stats.record_unresolved(shape_file, name, shape_path);
        return SceneMaterialHandle::Standard(untextured.clone());
    };

    let is_dds = tex_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"));

    let alpha_mode = if is_dds {
        use crate::textures::{DdsAlpha, dds_alpha_type};
        let dds_alpha = dds_alpha_type(&tex_path).unwrap_or(DdsAlpha::Full);
        let has_alpha = matches!(dds_alpha, DdsAlpha::Full);
        let has_semitransparent = has_alpha;

        alpha_mode_from_shader(
            shader_name,
            name,
            alpha_test_mode,
            has_semitransparent,
            has_alpha,
        )
    } else {
        let Some(ace) = load_ace_file(&tex_path) else {
            tex_stats.record_decode_failed(shape_file, name, &tex_path);
            return SceneMaterialHandle::Standard(untextured.clone());
        };

        let mut has_alpha = ace.has_mask_channel;
        let mut has_semitransparent = false;
        for rgba in ace.mip0.chunks_exact(4) {
            let alpha = rgba[3];
            if alpha < 250 {
                has_alpha = true;
            }
            if (9..248).contains(&alpha) {
                has_semitransparent = true;
            }
        }

        alpha_mode_from_shader(
            shader_name,
            name,
            alpha_test_mode,
            has_semitransparent,
            has_alpha,
        )
    };

    let cache_key = {
        let vtx = solid_color
            .filter(|_| shader_uses_vertex_color_multiply(shader_name))
            .map(|c| format!("{:.3},{:.3},{:.3}", c[0], c[1], c[2]))
            .unwrap_or_default();
        let sh = shader_name.unwrap_or("");
        format!(
            "{}:{alpha_mode:?}:{vtx}:lit={lit}:sh={sh}:or={}",
            tex_path.display(),
            use_or_shaders as u8
        )
    };

    if use_or_shaders {
        let handle = or_tex_mat_cache
            .entry(cache_key)
            .or_insert_with(|| {
                build_textured_or_material(
                    shape_file,
                    name,
                    shader_name,
                    solid_color,
                    is_dds,
                    &tex_path,
                    alpha_mode,
                    texture_env,
                    or_materials,
                    images,
                    tex_stats,
                    lit,
                    moment_atlas,
                    shadow_map_limits,
                )
            })
            .clone();
        return SceneMaterialHandle::OrScenery(handle);
    }

    let handle = tex_mat_cache
        .entry(cache_key)
        .or_insert_with(|| {
            build_textured_standard_material(
                shape_file,
                name,
                shader_name,
                solid_color,
                is_dds,
                &tex_path,
                alpha_mode,
                materials,
                images,
                untextured,
                tex_stats,
                lit,
            )
        })
        .clone();
    SceneMaterialHandle::Standard(handle)
}

#[allow(clippy::too_many_arguments)]
fn build_textured_standard_material(
    shape_file: &str,
    name: &str,
    shader_name: Option<&str>,
    solid_color: Option<[f32; 3]>,
    is_dds: bool,
    tex_path: &Path,
    alpha_mode: AlphaMode,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    untextured: &Handle<StandardMaterial>,
    tex_stats: &mut TextureLoadStats,
    lit: bool,
) -> Handle<StandardMaterial> {
    if is_dds {
        let Some(image) = load_texture_image(tex_path) else {
            tex_stats.record_decode_failed(shape_file, name, tex_path);
            return untextured.clone();
        };
        tex_stats.record_resolved();
        let final_alpha = normalize_alpha_mode(
            if alpha_mode == AlphaMode::Add || texture_name_suggests_additive(name) {
                AlphaMode::Add
            } else {
                alpha_mode
            },
            name,
        );
        if !matches!(final_alpha, AlphaMode::Opaque) {
            tex_stats.record_material_diagnostic(
                shape_file,
                name,
                -1.0,
                &format!("{final_alpha:?} (DDS)"),
            );
        }
        let tex = images.add(image);
        let boost = msts_albedo_boost_factor(128.0, name, false, lit);
        let tint = apply_shader_vertex_tint(
            Color::linear_rgb(boost, boost, boost),
            solid_color,
            shader_name,
        );
        msts_material(
            materials,
            tex,
            tint,
            final_alpha,
            0.85,
            false,
            lit,
            name,
            shader_name,
        )
    } else {
        let ace = match load_ace_file(tex_path) {
            Some(a) => a,
            None => {
                tex_stats.record_decode_failed(shape_file, name, tex_path);
                return untextured.clone();
            }
        };
        tex_stats.record_resolved();
        let raw_luma = ace_mean_luma(&ace.mip0);
        let final_alpha = normalize_alpha_mode(
            if alpha_mode == AlphaMode::Add
                || (raw_luma < 30.0 && texture_name_suggests_additive(name))
            {
                AlphaMode::Add
            } else {
                alpha_mode
            },
            name,
        );
        if raw_luma < 60.0 || !matches!(final_alpha, AlphaMode::Opaque) {
            tex_stats.record_material_diagnostic(
                shape_file,
                name,
                raw_luma,
                &format!("{final_alpha:?}"),
            );
        }
        let prep = prepared_ace(&ace, name, final_alpha, lit);
        let tint = finalize_scenery_tint(
            name,
            apply_shader_vertex_tint(prep.tint, solid_color, shader_name),
            lit,
        );
        let emissive = scenery_needs_emissive_texture(&ace.mip0, final_alpha, name, lit);
        let tex = images.add(prep.image);
        msts_material(
            materials,
            tex,
            tint,
            final_alpha,
            0.85,
            emissive,
            lit,
            name,
            shader_name,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn or_scenery_fallback(
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    moment_atlas: Handle<Image>,
    shadow_map_limits: [f32; 4],
    shader_name: Option<&str>,
    lit: bool,
    night: bool,
) -> Handle<OrSceneryMaterial> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let tex = images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[220, 215, 205, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));
    create_or_scenery_material(
        or_materials,
        tex,
        moment_atlas.clone(),
        shadow_map_limits,
        Color::srgb(0.72, 0.70, 0.66),
        AlphaMode::Opaque,
        shader_name,
        "fallback.ace",
        lit,
        night,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_textured_or_material(
    shape_file: &str,
    name: &str,
    shader_name: Option<&str>,
    solid_color: Option<[f32; 3]>,
    is_dds: bool,
    tex_path: &Path,
    alpha_mode: AlphaMode,
    texture_env: &TextureEnvironment,
    or_materials: &mut Assets<OrSceneryMaterial>,
    images: &mut Assets<Image>,
    tex_stats: &mut TextureLoadStats,
    lit: bool,
    moment_atlas: &Handle<Image>,
    shadow_map_limits: [f32; 4],
) -> Handle<OrSceneryMaterial> {
    let night_texture = name.to_ascii_lowercase().contains("night");
    if is_dds {
        let Some(image) = load_texture_image(tex_path) else {
            tex_stats.record_decode_failed(shape_file, name, tex_path);
            return or_scenery_fallback(
                or_materials,
                images,
                moment_atlas.clone(),
                shadow_map_limits,
                shader_name,
                lit,
                texture_env.night,
            );
        };
        tex_stats.record_resolved();
        let final_alpha = normalize_alpha_mode(
            if alpha_mode == AlphaMode::Add || texture_name_suggests_additive(name) {
                AlphaMode::Add
            } else {
                alpha_mode
            },
            name,
        );
        let tex = images.add(image);
        let boost = msts_albedo_boost_factor(128.0, name, false, lit);
        let tint = apply_shader_vertex_tint(
            Color::linear_rgb(boost, boost, boost),
            solid_color,
            shader_name,
        );
        create_or_scenery_material(
            or_materials,
            tex,
            moment_atlas.clone(),
            shadow_map_limits,
            tint,
            final_alpha,
            shader_name,
            name,
            lit,
            texture_env.night,
            night_texture,
        )
    } else {
        let ace = match load_ace_file(tex_path) {
            Some(a) => a,
            None => {
                tex_stats.record_decode_failed(shape_file, name, tex_path);
                return or_scenery_fallback(
                    or_materials,
                    images,
                    moment_atlas.clone(),
                    shadow_map_limits,
                    shader_name,
                    lit,
                    texture_env.night,
                );
            }
        };
        tex_stats.record_resolved();
        let raw_luma = ace_mean_luma(&ace.mip0);
        let final_alpha = normalize_alpha_mode(
            if alpha_mode == AlphaMode::Add
                || (raw_luma < 30.0 && texture_name_suggests_additive(name))
            {
                AlphaMode::Add
            } else {
                alpha_mode
            },
            name,
        );
        let prep = prepared_ace(&ace, name, final_alpha, lit);
        let tint = finalize_scenery_tint(
            name,
            apply_shader_vertex_tint(prep.tint, solid_color, shader_name),
            lit,
        );
        let tex = images.add(prep.image);
        create_or_scenery_material(
            or_materials,
            tex,
            moment_atlas.clone(),
            shadow_map_limits,
            tint,
            final_alpha,
            shader_name,
            name,
            lit,
            texture_env.night,
            night_texture,
        )
    }
}

fn alpha_mode_from_shader(
    shader_name: Option<&str>,
    texture_name: &str,
    alpha_test_mode: i32,
    has_semitransparent: bool,
    has_alpha: bool,
) -> AlphaMode {
    // Igual que viewer3d: prim_state explícito manda.
    match alpha_test_mode {
        0 => return AlphaMode::Opaque,
        1 => return AlphaMode::Mask(0.5),
        2 => return AlphaMode::Blend,
        _ => {}
    }

    let alpha_test_requested = alpha_test_mode == 1;

    let Some(shader) = shader_name else {
        if alpha_test_requested {
            return AlphaMode::Mask(0.5);
        }
        if !has_alpha {
            return AlphaMode::Opaque;
        }
        if texture_name_suggests_blend(texture_name) && has_semitransparent {
            return AlphaMode::Blend;
        }
        if texture_name_suggests_cutout(texture_name) {
            return AlphaMode::Mask(0.5);
        }
        return AlphaMode::Opaque;
    };

    if shader.eq_ignore_ascii_case("AddATex") || shader.eq_ignore_ascii_case("AddATexDiff") {
        return AlphaMode::Add;
    }

    let alpha_blend_requested =
        shader.eq_ignore_ascii_case("BlendATex") || shader.eq_ignore_ascii_case("BlendATexDiff");

    // Lógica inteligente (diferente de OR pero mejor para Bevy):
    // 1. Si no hay pixeles transparentes (<250), es Opaque.
    // 2. Si es blend-shader y hay píxeles semi-transparentes Y su nombre sugiere blend, es Blend.
    // 3. De lo contrario, usamos Mask(0.5) o Opaque dependiendo de los flags.

    if !has_alpha {
        AlphaMode::Opaque
    } else if alpha_blend_requested {
        if has_semitransparent && texture_name_suggests_blend(texture_name) {
            AlphaMode::Blend
        } else {
            // Solo tiene máscara binaria, o es un objeto que debe usar Z-sorting (árboles, vías)
            AlphaMode::Mask(0.5)
        }
    } else {
        // Shader no es de blend (TexDiff).
        if alpha_test_requested {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        }
    }
}

fn texture_name_suggests_blend(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    [
        "glass", "window", "alpha", "trans", "transp", "steam", "smoke", "exhaust", "fume",
        "cloud", "mist", "vapor", "vapour", "blank", "black", "shadow", "cloud", "mist", "vapor",
        "vapour", "blank", "black", "shadow",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn texture_name_suggests_additive(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    if lower.contains("lightpool") || lower.contains("lightglow") {
        return true;
    }
    if lower.contains("post")
        || lower.contains("lad")
        || lower.contains("gantry")
        || lower.contains("bracket")
        || lower.contains("pole")
        || lower.contains("frame")
    {
        return false;
    }
    [
        "led",
        "light",
        "glow",
        "dolly",
        "corona",
        "lamp",
        "lantern",
        "cls",
        "sig",
        "feather",
        "flare",
        "headlight",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn patch_mesh(patch: &PatchGeometry) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, patch.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, patch.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, patch.uvs.clone());
    mesh.insert_indices(Indices::U32(patch.indices.clone()));
    mesh
}

fn shape_part_mesh(part: &shapes::ShapePart, textured: bool, ukfs_track: bool) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, part.uvs.clone());
    // OR multiplica color de vértice × textura; MSTS suele dejar blanco en shapes texturizados.
    // Colores no blancos en partes texturizadas tiñen de amarillo/marrón (UKFS track).
    if textured && ukfs_track {
        let white = vec![[1.0_f32, 1.0, 1.0, 1.0]; part.positions.len()];
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, white);
    } else if !textured {
        if let Some(colors) = &part.colors {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors.clone());
        }
    }
    mesh
}

fn track_ribbon_mesh(ribbon: &TrackRibbon) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, ribbon.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, ribbon.normals.clone());
    mesh.insert_indices(Indices::U32(ribbon.indices.clone()));
    mesh
}

fn load_terrtex_ace(route_dir: &Path, file_name: &str) -> Option<AceFile> {
    openrailsrs_ace::read_ace(&crate::terrain::resolve_terrtex_path(route_dir, file_name)?).ok()
}

fn ace_to_image(ace: &AceFile) -> Image {
    crate::textures::ace_to_image(ace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_lit_vertex_color_limits_bright_cream() {
        let bright = Color::linear_rgb(0.95, 0.92, 0.75);
        let c = clamp_lit_vertex_color(bright).to_linear();
        let luma = 0.299 * c.red + 0.587 * c.green + 0.114 * c.blue;
        assert!(luma <= 0.73, "luma={luma}");
    }

    #[test]
    fn texdiff_vertex_tint_darkens_bright_rail() {
        let bright = Color::linear_rgb(1.0, 1.0, 1.0);
        let tinted = apply_shader_vertex_tint(bright, Some([0.588, 0.588, 0.588]), Some("TexDiff"));
        let c = tinted.to_linear();
        assert!(c.red < 0.65 && c.green < 0.65);
        let rail_unlit = finalize_scenery_tint("ukfs_rail.ace", tinted, false);
        let r = rail_unlit.to_linear();
        assert!(r.red < 0.45 && r.green < 0.45);
        let rail_lit = finalize_scenery_tint("ukfs_rail.ace", tinted, true);
        let rl = rail_lit.to_linear();
        assert!(rl.red < 0.75 && rl.green < 0.75);
        assert!(
            rl.red > r.red * 0.9,
            "lit rail should be less crushed than unlit"
        );
    }

    #[test]
    fn ukfs_rail_uses_metallic_pbr_when_lit() {
        let p = resolve_or_material_pbr("ukfs_rail.ace", Some("TexDiff"), true, 0.85);
        assert!(p.metallic > 0.7);
    }

    #[test]
    fn halfbright_shader_adds_shadow_fill() {
        let p = resolve_or_material_pbr("wall.ace", Some("HalfBright"), true, 0.85);
        assert!(p.ambient_fill.red > 0.0);
    }

    #[test]
    fn lit_albedo_boost_stays_near_unity() {
        assert_eq!(
            msts_albedo_boost_factor(200.0, "ChalkCliff.ace", false, true),
            1.0
        );
        assert!((msts_albedo_boost_factor(40.0, "brick.ace", false, true) - 1.14).abs() < 0.01);
    }

    #[test]
    fn bright_chalk_texture_uses_unit_albedo_boost() {
        let rgba = vec![200u8, 190, 175, 255];
        let tint = msts_albedo_tint(false, false, ace_mean_luma(&rgba), "ChalkCliff.ace", false);
        assert_eq!(tint, Color::linear_rgb(1.0, 1.0, 1.0));
    }

    #[test]
    fn ace_with_zero_alpha_channel_still_brightens_rgb() {
        let rgba = vec![20u8, 25, 18, 0];
        let (out, brightened) = brighten_dark_ace_rgba(&rgba, DARK_TEXTURE_LUMA);
        assert!(brightened);
        assert!(ace_mean_luma(&out) > 40.0);
        assert_eq!(out[3], 255);
    }

    #[test]
    fn inferred_shape_alpha_uses_ace_cutout_when_prim_state_unspecified() {
        assert!(matches!(
            alpha_mode_from_shader(None, "tree.ace", -1, false, true),
            AlphaMode::Mask(_)
        ));
        assert!(matches!(
            alpha_mode_from_shader(None, "brickwall.ace", -1, false, true),
            AlphaMode::Opaque
        ));
    }

    #[test]
    fn explicit_opaque_prim_state_wins_over_texture_alpha() {
        assert!(matches!(
            alpha_mode_from_shader(None, "glass.ace", 0, true, true),
            AlphaMode::Opaque
        ));
    }

    #[test]
    fn brighten_dark_terrtex_lifts_mean_luma() {
        let ace = AceFile {
            width: 1,
            height: 1,
            format: openrailsrs_ace::AceFormat::Rgba8,
            mips_count: 1,
            mip0: vec![8, 8, 8, 255],
            mips: Vec::new(),
            has_mask_channel: false,
            alpha_bits: 8,
        };
        let (out, brightened) = brighten_dark_ace_rgba(&ace.mip0, DARK_TEXTURE_LUMA);
        assert!(brightened);
        assert!(ace_mean_luma(&out) > DARK_TEXTURE_LUMA);
    }

    #[test]
    fn smoke_texture_name_uses_blend() {
        assert!(texture_name_suggests_blend("factory_steam.ace"));
    }

    #[test]
    fn bright_textures_keep_four_x_tint() {
        let ace = AceFile {
            width: 1,
            height: 1,
            format: openrailsrs_ace::AceFormat::Rgba8,
            mips_count: 1,
            mip0: vec![180, 180, 180, 255],
            mips: Vec::new(),
            has_mask_channel: false,
            alpha_bits: 8,
        };
        let prep = prepared_ace(&ace, "brick.ace", AlphaMode::Opaque, false);
        assert_eq!(
            prep.tint,
            msts_albedo_tint(false, false, ace_mean_luma(&ace.mip0), "brick.ace", false)
        );
    }

    #[test]
    fn foliage_mask_uses_low_cutoff() {
        assert!(matches!(
            normalize_alpha_mode(AlphaMode::Mask(0.5), "MSTreeline.ace"),
            AlphaMode::Mask(c) if (c - FOLIAGE_MASK_CUTOFF).abs() < 0.001
        ));
    }

    #[test]
    fn dark_foliage_requests_emissive() {
        let rgba = vec![20u8, 40, 15, 255];
        assert!(scenery_needs_emissive_texture(
            &rgba,
            AlphaMode::Mask(0.04),
            "MSTreeline.ace",
            false,
        ));
    }

    #[test]
    fn foliage_brightens_below_higher_threshold() {
        let rgba = vec![30u8, 70, 25, 255];
        let (_, brightened) = brighten_dark_ace_rgba(&rgba, FOLIAGE_LUMA_THRESHOLD);
        assert!(brightened);
    }
    #[test]
    fn trackobj_resolves_from_global_shapes() {
        use crate::objects::{ObjectKind, load_objects};

        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let msts = std::env::var_os("OPENRAILSRS_MSTS_CONTENT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
            });
        let index = AssetIndex::build(&route, &msts);
        let objs = load_objects(&route, -6082, 14925, 100.0);
        if objs.is_empty() {
            eprintln!("skip: tile Chiltern no disponible en examples");
            return;
        }
        let track = objs.iter().find(|o| o.kind == ObjectKind::Track);
        let Some(track) = track else {
            eprintln!("skip: sin TrackObj en tile Chiltern");
            return;
        };
        let path = resolve_trackobj_shape_path(
            &route,
            &msts,
            &index,
            track.file_name.as_deref(),
            track.section_idx,
        );
        let Some(path) = path else {
            eprintln!("skip: TrackObj sin shape resoluble (content MSTS ausente)");
            return;
        };
        assert!(
            path.is_file(),
            "shape resuelto debe existir: {}",
            path.display()
        );
    }

    #[test]
    fn new_forest_trackobj_resolves_when_route_present() {
        use crate::objects::{ObjectKind, load_objects};

        let route = std::env::var_os("NEW_FOREST_V3_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home).join("routes/NewForestRouteV3/Routes/Watersnake");
                p.join("world").is_dir().then_some(p)
            });
        let Some(route) = route else {
            return;
        };
        let msts = std::env::var_os("OPENRAILSRS_MSTS_CONTENT")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                Some(PathBuf::from(home).join("routes/NewForestRouteV3"))
            });
        let Some(msts) = msts else {
            return;
        };
        let index = AssetIndex::build(&route, &msts);
        let objs = load_objects(&route, -6131, 14898, 0.0);
        let track_count = objs.iter().filter(|o| o.kind == ObjectKind::Track).count();
        let resolved = objs
            .iter()
            .filter(|o| o.kind == ObjectKind::Track)
            .filter(|o| {
                resolve_trackobj_shape_path(
                    &route,
                    &msts,
                    &index,
                    o.file_name.as_deref(),
                    o.section_idx,
                )
                .is_some()
            })
            .count();
        eprintln!("NF tile TrackObj: {track_count} total, {resolved} shapes resueltos");
        assert!(track_count > 0, "tile NF estacion deberia tener TrackObj");
        assert!(
            resolved > track_count / 2,
            "mayoria de TrackObj deberian resolver shape, got {resolved}/{track_count}"
        );
    }
}
