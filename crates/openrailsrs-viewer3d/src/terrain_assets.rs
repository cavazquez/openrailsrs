//! Resolve MSTS `TERRTEX/` terrain textures for patch shaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
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
        let mut image = ace_to_image(&ace);
        set_terrain_sampler_wrap(&mut image);
        return Some(image);
    }
    if !file_name.eq_ignore_ascii_case(DEFAULT_MICROTEX) {
        return load_terrtex_image(route_dir, DEFAULT_MICROTEX);
    }
    let mut image = load_ace_image(route_dir, file_name)?;
    set_terrain_sampler_wrap(&mut image);
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

fn set_terrain_sampler_wrap(image: &mut Image) {
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
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
        set_terrain_sampler_wrap(&mut image);
        let ImageSampler::Descriptor(desc) = image.sampler else {
            panic!("expected explicit sampler");
        };
        assert_eq!(desc.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(desc.address_mode_v, ImageAddressMode::Repeat);
    }
}
