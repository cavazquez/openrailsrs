//! Resolve MSTS `TERRTEX/` terrain textures for patch shaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_formats::TerrainShader;

use crate::shapes::{ace_to_image, load_ace_image};
use openrailsrs_ace::read_ace;

const DEFAULT_MICROTEX: &str = "microtex.ace";

/// Overlay UV scale from OR `terrain_uvcalcs[1].d` when non-zero and not 32.
pub fn overlay_scale_from_shader(shader: &TerrainShader) -> f32 {
    shader
        .uvcalcs
        .get(1)
        .map(|c| c.d)
        .filter(|d| *d != 0.0 && (*d - 32.0).abs() > 1e-3)
        .map(|d| d as f32)
        .unwrap_or(32.0)
}

pub fn resolve_terrtex_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let base = Path::new(file_name).file_name()?.to_str()?;
    for subdir in ["TERRTEX", "terrtex"] {
        let path = route_dir.join(subdir).join(base);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn load_terrtex_image(route_dir: &Path, file_name: &str) -> Option<Image> {
    if let Some(path) = resolve_terrtex_path(route_dir, file_name) {
        let ace = read_ace(&path).ok()?;
        return Some(ace_to_image(&ace));
    }
    if !file_name.eq_ignore_ascii_case(DEFAULT_MICROTEX) {
        return load_terrtex_image(route_dir, DEFAULT_MICROTEX);
    }
    load_ace_image(route_dir, file_name)
}

/// Load/cache base + overlay handles for one terrain shader.
pub fn terrain_material_textures(
    route_dir: &Path,
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    shader: &TerrainShader,
    fallback: Handle<Image>,
) -> (Handle<Image>, Handle<Image>, f32) {
    let base_name = shader
        .texslots
        .first()
        .map(|s| s.filename.as_str())
        .unwrap_or("grass.ace");
    let overlay_name = shader
        .texslots
        .get(1)
        .map(|s| s.filename.as_str())
        .unwrap_or(DEFAULT_MICROTEX);

    let base =
        texture_handle(route_dir, images, cache, base_name).unwrap_or_else(|| fallback.clone());
    let overlay = texture_handle(route_dir, images, cache, overlay_name)
        .or_else(|| texture_handle(route_dir, images, cache, DEFAULT_MICROTEX))
        .unwrap_or_else(|| base.clone());

    (base, overlay, overlay_scale_from_shader(shader))
}

fn texture_handle(
    route_dir: &Path,
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    file_name: &str,
) -> Option<Handle<Image>> {
    if let Some(handle) = cache.get(file_name) {
        return Some(handle.clone());
    }
    resolve_terrtex_path(route_dir, file_name)?;
    let image = load_terrtex_image(route_dir, file_name)?;
    let handle = images.add(image);
    cache.insert(file_name.to_string(), handle.clone());
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TerrainUvCalc;

    #[test]
    fn overlay_scale_defaults_to_32() {
        let shader = TerrainShader {
            name: "t".into(),
            texslots: vec![],
            uvcalcs: vec![TerrainUvCalc {
                a: 0,
                b: 0,
                c: 0,
                d: 0.0,
            }],
        };
        assert!((overlay_scale_from_shader(&shader) - 32.0).abs() < 1e-3);
    }

    #[test]
    fn smoke_route_has_terrtex_grass() {
        let route =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        assert!(resolve_terrtex_path(&route, "grass.ace").is_some());
    }
}
