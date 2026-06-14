//! Resolución de texturas MSTS/Open Rails (paridad con `openrailsrs-viewer3d`).

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_ace::{AceFile, read_ace};
use openrailsrs_formats::resolve_path_case_insensitive;

/// Basename de un path MSTS (`TEXTURES\foo.ace` → `foo.ace`).
pub fn texture_file_basename(file_name: &str) -> &str {
    file_name
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(file_name)
}

/// Raíz de contenido MSTS/OR (`OPENRAILSRS_MSTS_CONTENT` o rutas típicas).
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

/// Árboles `GLOBAL/` bajo el content pack de la ruta.
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

/// Directorios donde buscar texturas dado el path del `.s` resuelto.
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

/// Directorios para resolver shapes (ruta + pack MSTS + GLOBAL).
pub fn shape_search_dirs(route_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![route_dir.to_path_buf()];
    if let Some(content) = msts_content_root() {
        if let Some(stem) = route_dir.file_name() {
            let pack = content.join(stem);
            if pack.is_dir() {
                dirs.push(pack);
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

pub fn shape_file_basename(file_name: &str) -> &str {
    texture_file_basename(file_name)
}

/// Resuelve `SHAPES/foo.s` bajo una raíz de assets.
pub fn resolve_shape_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let base = shape_file_basename(file_name);
    for subdir in ["SHAPES", "shapes"] {
        let path = route_dir.join(subdir).join(base);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    let direct = route_dir.join(base);
    if direct.is_file() {
        return Some(direct);
    }
    resolve_path_case_insensitive(&direct)
}

pub fn resolve_shape_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(path) = resolve_shape_path(dir, file_name) {
            return Some(path);
        }
    }
    None
}

/// Resuelve `TEXTURES/foo.ace` bajo una raíz (incluye subcarpetas estacionales y `.dds`).
pub fn resolve_texture_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let base = texture_file_basename(file_name);
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
    if let Some(p) = resolve_path_case_insensitive(&direct) {
        return Some(p);
    }
    for subdir in ["TEXTURES", "textures"] {
        let textures_root = route_dir.join(subdir);
        let direct = textures_root.join(file_name);
        if direct.is_file() {
            return Some(direct);
        }
        if let Some(p) = resolve_path_case_insensitive(&direct) {
            return Some(p);
        }
        if let Ok(entries) = std::fs::read_dir(&textures_root) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let candidate = entry.path().join(file_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
                if let Some(p) = resolve_path_case_insensitive(&candidate) {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn resolve_texture_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(p) = resolve_texture_path(dir, file_name) {
            return Some(p);
        }
    }
    None
}

pub fn decode_dds_to_image(bytes: &[u8]) -> Result<Image, String> {
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

/// Decodifica `.ace` o `.dds` a `Image` Bevy.
pub fn load_texture_image(path: &Path) -> Option<Image> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "dds" {
        let bytes = std::fs::read(path).ok()?;
        return decode_dds_to_image(&bytes).ok();
    }
    let ace = read_ace(path).ok()?;
    Some(ace_to_image(&ace))
}

pub fn load_ace_file(path: &Path) -> Option<AceFile> {
    read_ace(path).ok()
}

pub fn ace_to_image(ace: &AceFile) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: ace.width,
            height: ace.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        ace.mip0.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        ..default()
    });
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chiltern_route() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    #[test]
    fn resolve_texture_strips_msts_prefix() {
        let route = chiltern_route();
        if !route.is_dir() {
            return;
        }
        assert!(resolve_texture_path(&route, r"TEXTURES\poplar15_1.ace").is_some());
    }

    #[test]
    fn resolve_texture_finds_seasonal_subdir() {
        let route = chiltern_route();
        if !route.is_dir() {
            return;
        }
        assert!(resolve_texture_path(&route, "poplar15_1.ace").is_some());
    }

    #[test]
    fn resolve_shape_in_chiltern_shapes() {
        let route = chiltern_route();
        if !route.is_dir() {
            return;
        }
        assert!(resolve_shape_path(&route, "smoke1.s").is_some());
    }
}
