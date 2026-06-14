//! Spawn del mundo 3D (terreno, vía, objetos) — usado de forma progresiva desde `loading`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_ace::AceFile;

use crate::objects::ObjectMarker;
use crate::shapes;
use crate::terrain::{PatchGeometry, TileGeometry};
use crate::textures::{
    load_ace_file, load_texture_image, msts_content_root, resolve_shape_path_in_dirs,
    resolve_texture_path_in_dirs, shape_search_dirs, texture_search_dirs_for_shape,
};
use crate::track::TrackRibbon;

/// Handles de una parte de shape ya en GPU.
#[derive(Clone)]
pub(crate) struct PartHandles {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Índice case-insensitive de assets `.s` / `.ace`.
#[derive(Clone)]
pub struct AssetIndex {
    shapes: HashMap<String, PathBuf>,
    textures: HashMap<String, PathBuf>,
}

impl AssetIndex {
    pub fn build(route_dir: &Path) -> Self {
        let mut shapes = HashMap::new();
        let mut textures = HashMap::new();

        for sub in ["SHAPES", "shapes"] {
            index_dir(&mut shapes, &route_dir.join(sub));
        }
        let tex = route_dir.join("TEXTURES");
        index_dir(&mut textures, &tex);
        for season in ["SPRING", "AUTUMN", "WINTER", "SNOW", "SUMMER"] {
            index_dir(&mut textures, &tex.join(season));
        }

        if let Some(root) = msts_content_root() {
            index_tree(&mut shapes, &mut textures, &root, 6);
        }

        Self { shapes, textures }
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
}

fn resolve_shape_file(index: &AssetIndex, route: &Path, file: &str) -> Option<PathBuf> {
    if let Some(path) = index.shape(file) {
        return Some(path.clone());
    }
    let dirs = shape_search_dirs(route);
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

fn index_tree(
    shapes: &mut HashMap<String, PathBuf>,
    textures: &mut HashMap<String, PathBuf>,
    dir: &Path,
    depth: usize,
) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if ft.is_dir() {
            index_tree(shapes, textures, &path, depth - 1);
        } else if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".s") {
                shapes.entry(lower).or_insert(path);
            } else if lower.ends_with(".ace") || lower.ends_with(".dds") {
                textures.entry(lower).or_insert(path);
            }
        }
    }
}

/// Texturas MSTS/OR: albedo oscuro pensado para fixed-function + unlit.
const MSTS_ALBEDO_BOOST: f32 = 4.0;
const DARK_TEXTURE_LUMA: f32 = 60.0;
const TARGET_TEXTURE_LUMA: f32 = 112.0;

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
        if px[3] < 8 {
            continue;
        }
        sum += 0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2]);
        n += 1;
    }
    if n == 0 { 0.0 } else { (sum / n as f64) as f32 }
}

fn brighten_dark_ace_rgba(rgba: &[u8]) -> (Vec<u8>, bool) {
    let mean = ace_mean_luma(rgba);
    if mean >= DARK_TEXTURE_LUMA {
        return (rgba.to_vec(), false);
    }
    let scale = (TARGET_TEXTURE_LUMA / mean.max(1.0)).min(128.0);
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        if px[3] < 8 {
            continue;
        }
        for c in &mut px[0..3] {
            *c = (f32::from(*c) * scale).min(255.0).round() as u8;
        }
    }
    (out, true)
}

fn msts_albedo_tint(pixel_brightened: bool) -> Color {
    if pixel_brightened {
        Color::linear_rgb(1.25, 1.25, 1.25)
    } else {
        Color::linear_rgb(MSTS_ALBEDO_BOOST, MSTS_ALBEDO_BOOST, MSTS_ALBEDO_BOOST)
    }
}

fn prepared_ace(ace: &AceFile) -> PreparedAce {
    let (mip0, brightened) = brighten_dark_ace_rgba(&ace.mip0);
    let mut prepared = ace.clone();
    prepared.mip0 = mip0;
    PreparedAce {
        image: ace_to_image(&prepared),
        tint: msts_albedo_tint(brightened),
    }
}

fn msts_unlit_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Handle<Image>,
    tint: Color,
    alpha_mode: AlphaMode,
    roughness: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: tint,
        base_color_texture: Some(texture),
        perceptual_roughness: roughness,
        alpha_mode,
        double_sided: true,
        cull_mode: None,
        unlit: true,
        ..default()
    })
}

/// Caches de materiales de terreno (TERRTEX).
#[derive(Clone)]
pub struct TerrainSpawnCtx {
    pub fallback: Handle<StandardMaterial>,
    pub mat_cache: HashMap<String, Handle<StandardMaterial>>,
}

impl TerrainSpawnCtx {
    pub fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let fallback = materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.55, 0.32),
            perceptual_roughness: 0.95,
            double_sided: true,
            cull_mode: None,
            unlit: true,
            ..default()
        });
        Self {
            fallback,
            mat_cache: HashMap::new(),
        }
    }

    fn material_for_patch(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        route: &Path,
        name: &str,
    ) -> Handle<StandardMaterial> {
        self.mat_cache
            .entry(name.to_string())
            .or_insert_with(|| {
                load_terrtex_ace(route, name)
                    .map(|ace| {
                        let prep = prepared_ace(&ace);
                        let tex = images.add(prep.image);
                        msts_unlit_material(materials, tex, prep.tint, AlphaMode::Opaque, 0.95)
                    })
                    .unwrap_or_else(|| self.fallback.clone())
            })
            .clone()
    }
}

/// Contadores de resolución de texturas (diagnóstico al cargar objetos).
#[derive(Resource, Default)]
pub struct TextureLoadStats {
    pub resolved: u32,
    pub unresolved: u32,
    pub decode_failed: u32,
    pub no_texture_part: u32,
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
        if total == 0 && self.no_texture_part == 0 {
            return;
        }
        println!(
            "texturas: {} ok, {} sin archivo, {} decode falló, {} partes sin textura",
            self.resolved, self.unresolved, self.decode_failed, self.no_texture_part
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

pub fn texture_debug_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_TEXTURE_DEBUG").is_some()
}

/// Caches de shapes/objetos.
#[derive(Clone)]
pub struct ObjectSpawnCtx {
    pub shape_cache: HashMap<String, Vec<PartHandles>>,
    pub tex_mat_cache: HashMap<String, Handle<StandardMaterial>>,
    pub untextured: Handle<StandardMaterial>,
}

impl ObjectSpawnCtx {
    pub fn new(materials: &mut Assets<StandardMaterial>) -> Self {
        let untextured = materials.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.70, 0.66),
            perceptual_roughness: 0.85,
            double_sided: true,
            cull_mode: None,
            unlit: true,
            ..default()
        });
        Self {
            shape_cache: HashMap::new(),
            tex_mat_cache: HashMap::new(),
            untextured,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_terrain_patches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    ctx: &mut TerrainSpawnCtx,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    route: &Path,
    tile: &TileGeometry,
    from: usize,
    to: usize,
) {
    for (i, patch) in tile
        .patches
        .iter()
        .enumerate()
        .skip(from)
        .take(to.saturating_sub(from))
    {
        let mesh_handle = meshes.add(patch_mesh(patch));
        let material = match &patch.texture {
            Some(name) => ctx.material_for_patch(materials, images, route, name),
            None => ctx.fallback.clone(),
        };
        commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::from_array(patch.offset)),
            Name::new(format!("terrain_patch_{i}")),
        ));
    }
}

pub fn spawn_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ribbon: &TrackRibbon,
) {
    if ribbon.positions.is_empty() {
        return;
    }
    let track_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.52, 0.48),
        perceptual_roughness: 0.9,
        double_sided: true,
        cull_mode: None,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(track_ribbon_mesh(ribbon))),
        MeshMaterial3d(track_mat),
        Transform::default(),
        Name::new("track"),
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_objects_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    index: &AssetIndex,
    ctx: &mut ObjectSpawnCtx,
    route: &Path,
    batch: &[ObjectMarker],
    tex_stats: &mut TextureLoadStats,
) {
    for obj in batch {
        let Some(file) = obj.file_name.as_deref() else {
            continue;
        };
        let Some(parts) = build_shape(
            file,
            index,
            route,
            meshes,
            materials,
            images,
            &mut ctx.shape_cache,
            &mut ctx.tex_mat_cache,
            &ctx.untextured,
            tex_stats,
        ) else {
            continue;
        };
        if parts.is_empty() {
            continue;
        }
        let transform = Transform {
            translation: obj.position,
            rotation: obj.rotation,
            scale: obj.scale,
        };
        for part in parts {
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(part.material.clone()),
                transform,
                Name::new("object_shape"),
            ));
        }
    }
}

pub fn spawn_world_lights(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 1.0, 0.0).looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
        Name::new("sun"),
    ));
}

#[allow(clippy::too_many_arguments)]
fn build_shape(
    file: &str,
    index: &AssetIndex,
    route: &Path,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    shape_cache: &mut HashMap<String, Vec<PartHandles>>,
    tex_mat_cache: &mut HashMap<String, Handle<StandardMaterial>>,
    untextured: &Handle<StandardMaterial>,
    tex_stats: &mut TextureLoadStats,
) -> Option<Vec<PartHandles>> {
    if let Some(cached) = shape_cache.get(file) {
        return Some(cached.clone());
    }
    let path = resolve_shape_file(index, route, file)?;
    let parts = shapes::load_shape_parts(&path)?;
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
                    &path,
                    route,
                    tex_mat_cache,
                    materials,
                    images,
                    untextured,
                    tex_stats,
                ),
                None => {
                    tex_stats.no_texture_part += 1;
                    untextured.clone()
                }
            };
            PartHandles {
                mesh: meshes.add(shape_part_mesh(&p)),
                material,
            }
        })
        .collect();
    shape_cache.insert(file.to_string(), handles.clone());
    Some(handles)
}

#[allow(clippy::too_many_arguments)]
fn texture_material(
    shape_file: &str,
    name: &str,
    alpha_test_mode: i32,
    shader_name: Option<&str>,
    shape_path: &Path,
    route_dir: &Path,
    tex_mat_cache: &mut HashMap<String, Handle<StandardMaterial>>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    untextured: &Handle<StandardMaterial>,
    tex_stats: &mut TextureLoadStats,
) -> Handle<StandardMaterial> {
    let tex_dirs = texture_search_dirs_for_shape(shape_path, route_dir);
    let dir_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let Some(tex_path) = resolve_texture_path_in_dirs(&dir_refs, name) else {
        tex_stats.record_unresolved(shape_file, name, shape_path);
        return untextured.clone();
    };

    let is_dds = tex_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"));

    let mut alpha_mode = if is_dds {
        alpha_mode_from_name(name, alpha_test_mode)
    } else {
        let Some(ace) = load_ace_file(&tex_path) else {
            tex_stats.record_decode_failed(shape_file, name, &tex_path);
            return untextured.clone();
        };
        alpha_mode_from_ace(&ace, name, alpha_test_mode)
    };

    if let Some(shader) = shader_name {
        if shader.eq_ignore_ascii_case("AddATex") || shader.eq_ignore_ascii_case("AddATexDiff") {
            alpha_mode = AlphaMode::Add;
        } else if shader.eq_ignore_ascii_case("BlendATex")
            || shader.eq_ignore_ascii_case("BlendATexDiff")
        {
            alpha_mode = AlphaMode::Blend;
        }
    }

    tex_mat_cache
        .entry(format!("{}:{alpha_mode:?}", tex_path.display()))
        .or_insert_with(|| {
            if is_dds {
                let Some(image) = load_texture_image(&tex_path) else {
                    tex_stats.record_decode_failed(shape_file, name, &tex_path);
                    return untextured.clone();
                };
                tex_stats.record_resolved();
                let final_alpha =
                    if alpha_mode == AlphaMode::Add || texture_name_suggests_additive(name) {
                        AlphaMode::Add
                    } else {
                        alpha_mode
                    };
                if !matches!(final_alpha, AlphaMode::Opaque) {
                    tex_stats.record_material_diagnostic(
                        shape_file,
                        name,
                        -1.0,
                        &format!("{final_alpha:?} (DDS)"),
                    );
                }
                let tex = images.add(image);
                msts_unlit_material(
                    materials,
                    tex,
                    Color::linear_rgb(MSTS_ALBEDO_BOOST, MSTS_ALBEDO_BOOST, MSTS_ALBEDO_BOOST),
                    final_alpha,
                    0.85,
                )
            } else {
                let ace = match load_ace_file(&tex_path) {
                    Some(a) => a,
                    None => {
                        tex_stats.record_decode_failed(shape_file, name, &tex_path);
                        return untextured.clone();
                    }
                };
                tex_stats.record_resolved();
                let raw_luma = ace_mean_luma(&ace.mip0);
                let final_alpha = if alpha_mode == AlphaMode::Add
                    || (raw_luma < 30.0 && texture_name_suggests_additive(name))
                {
                    AlphaMode::Add
                } else {
                    alpha_mode
                };
                if raw_luma < 60.0 || !matches!(final_alpha, AlphaMode::Opaque) {
                    tex_stats.record_material_diagnostic(
                        shape_file,
                        name,
                        raw_luma,
                        &format!("{final_alpha:?}"),
                    );
                }
                let prep = prepared_ace(&ace);
                let tex = images.add(prep.image);
                msts_unlit_material(materials, tex, prep.tint, final_alpha, 0.85)
            }
        })
        .clone()
}

fn alpha_mode_from_name(texture_name: &str, alpha_test_mode: i32) -> AlphaMode {
    match alpha_test_mode {
        0 => AlphaMode::Opaque,
        1 => AlphaMode::Mask(0.5),
        2 => AlphaMode::Blend,
        _ => {
            if texture_name_suggests_blend(texture_name) {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            }
        }
    }
}

fn alpha_mode_from_ace(ace: &AceFile, texture_name: &str, alpha_test_mode: i32) -> AlphaMode {
    match alpha_test_mode {
        0 => return AlphaMode::Opaque,
        1 => return AlphaMode::Mask(0.5),
        2 => return AlphaMode::Blend,
        _ => {}
    }

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

    if !has_alpha {
        AlphaMode::Opaque
    } else if has_semitransparent && texture_name_suggests_blend(texture_name) {
        AlphaMode::Blend
    } else {
        AlphaMode::Mask(0.5)
    }
}

fn texture_name_suggests_blend(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    [
        "glass", "window", "alpha", "trans", "transp", "steam", "smoke", "exhaust", "fume",
        "cloud", "mist", "vapor", "vapour",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn texture_name_suggests_additive(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
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
        "led", "light", "glow", "dolly", "corona", "lamp", "lantern", "cls", "sig", "feather",
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

fn shape_part_mesh(part: &shapes::ShapePart) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, part.uvs.clone());
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

    fn ace_with_alpha(alphas: &[u8], has_mask_channel: bool) -> AceFile {
        let mut mip0 = Vec::with_capacity(alphas.len() * 4);
        for alpha in alphas {
            mip0.extend_from_slice(&[255, 255, 255, *alpha]);
        }
        AceFile {
            width: alphas.len() as u32,
            height: 1,
            format: openrailsrs_ace::AceFormat::Rgba8,
            mips_count: 1,
            mip0,
            has_mask_channel,
        }
    }

    #[test]
    fn inferred_shape_alpha_uses_ace_cutout_when_prim_state_unspecified() {
        let ace = ace_with_alpha(&[0, 255], true);
        assert!(matches!(
            alpha_mode_from_ace(&ace, "tree.ace", -1),
            AlphaMode::Mask(_)
        ));
    }

    #[test]
    fn explicit_opaque_prim_state_wins_over_texture_alpha() {
        let ace = ace_with_alpha(&[0, 128, 255], false);
        assert!(matches!(
            alpha_mode_from_ace(&ace, "glass.ace", 0),
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
            has_mask_channel: false,
        };
        let (out, brightened) = brighten_dark_ace_rgba(&ace.mip0);
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
            has_mask_channel: false,
        };
        let prep = prepared_ace(&ace);
        assert_eq!(prep.tint, msts_albedo_tint(false));
    }
}
