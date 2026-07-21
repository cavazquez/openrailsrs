//! Resolve MSTS `TERRTEX/` terrain textures for patch shaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_ace::read_ace;
pub use openrailsrs_bevy_scenery::terrain_shader_material_key;
use openrailsrs_bevy_scenery::{
    sanitize_terrain_base_rgba, set_terrain_repeat_sampler, terrain_shader_overlay_scale,
};
use openrailsrs_formats::TerrainShader;

use crate::shapes::{ace_to_image, load_ace_image};
use openrailsrs_bevy_scenery::materials::DEFAULT_MICROTEX;

/// Overlay UV scale from OR `terrain_uvcalcs[1].d` when non-zero and not 32.
pub fn overlay_scale_from_shader(shader: &TerrainShader) -> f32 {
    terrain_shader_overlay_scale(shader)
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
        let mut image = ace_to_image(&ace);
        set_terrain_repeat_sampler(&mut image);
        return Some(image);
    }
    if !file_name.eq_ignore_ascii_case(DEFAULT_MICROTEX) {
        return load_terrtex_image(route_dir, DEFAULT_MICROTEX);
    }
    let mut image = load_ace_image(route_dir, file_name)?;
    set_terrain_repeat_sampler(&mut image);
    Some(image)
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

    let base = texture_handle(route_dir, images, cache, base_name, true)
        .unwrap_or_else(|| fallback.clone());
    let overlay = texture_handle(route_dir, images, cache, overlay_name, false)
        .or_else(|| texture_handle(route_dir, images, cache, DEFAULT_MICROTEX, false))
        .unwrap_or_else(|| base.clone());

    (base, overlay, overlay_scale_from_shader(shader))
}

fn texture_handle(
    route_dir: &Path,
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    file_name: &str,
    sanitize_base_alpha: bool,
) -> Option<Handle<Image>> {
    let key = format!(
        "{file_name}:{}",
        if sanitize_base_alpha { "base" } else { "raw" }
    );
    if let Some(handle) = cache.get(&key) {
        return Some(handle.clone());
    }
    resolve_terrtex_path(route_dir, file_name)?;
    let mut image = load_terrtex_image(route_dir, file_name)?;
    if sanitize_base_alpha {
        sanitize_terrain_base_rgba(image.data.as_mut());
    }
    let handle = images.add(image);
    cache.insert(key, handle.clone());
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::image::{ImageAddressMode, ImageSampler};
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
        let _ = terrain_shader_material_key(&shader);
    }

    #[test]
    fn smoke_route_has_terrtex_grass() {
        let route =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        assert!(resolve_terrtex_path(&route, "grass.ace").is_some());
    }

    #[test]
    fn terrain_base_sanitizer_fills_transparent_pixels() {
        let mut rgba = vec![
            10, 20, 30, 255, //
            200, 210, 220, 0,
        ];
        sanitize_terrain_base_rgba(Some(&mut rgba));
        assert_eq!(&rgba[0..4], &[10, 20, 30, 255]);
        assert_eq!(&rgba[4..8], &[10, 20, 30, 255]);
    }

    #[test]
    fn terrain_textures_use_repeat_sampler_like_open_rails() {
        let mut image = Image::default();
        set_terrain_repeat_sampler(&mut image);
        let ImageSampler::Descriptor(desc) = image.sampler else {
            panic!("expected explicit sampler");
        };
        assert_eq!(desc.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(desc.address_mode_v, ImageAddressMode::Repeat);
    }
}
