//! Resolve MSTS `TERRTEX/` terrain textures for patch shaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
pub use openrailsrs_bevy_scenery::terrain_shader_material_key;
use openrailsrs_bevy_scenery::{
    sanitize_terrain_base_rgba, set_terrain_repeat_sampler, terrain_shader_overlay_scale,
};
use openrailsrs_formats::TerrainShader;

use crate::shapes::load_ace_image;
use openrailsrs_bevy_scenery::materials::DEFAULT_MICROTEX;

/// Overlay UV scale from OR `terrain_uvcalcs[1].d` when non-zero and not 32.
pub fn overlay_scale_from_shader(shader: &TerrainShader) -> f32 {
    terrain_shader_overlay_scale(shader)
}

pub fn resolve_terrtex_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    resolve_terrtex_for_environment(
        route_dir,
        file_name,
        crate::shapes::scenery_texture_environment(
            openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(0),
        ),
    )
}

pub(crate) fn resolve_terrtex_for_environment(
    route_dir: &Path,
    file_name: &str,
    environment: openrailsrs_bevy_scenery::textures::TextureEnvironment,
) -> Option<PathBuf> {
    let base = openrailsrs_bevy_scenery::textures::texture_file_basename(file_name);
    // OR Helpers.GetTerrainTextureFile uses Snow in winter regardless of weather.
    // Fall back to the base asset when a pack has no winter variant.
    let root = route_dir.join("TERRTEX");
    let mut dirs = vec![];
    if environment.is_snow() {
        dirs.push(root.join("Snow"));
    }
    dirs.push(root);
    for dir in dirs {
        for ext in [None, Some("dds"), Some("ktx2")] {
            let candidate = match ext {
                None => dir.join(base),
                Some(ext) => dir.join(base).with_extension(ext),
            };
            if let Some(path) = openrailsrs_formats::resolve_path_case_insensitive(&candidate) {
                return Some(path);
            }
        }
    }
    None
}

pub fn load_terrtex_image(route_dir: &Path, file_name: &str) -> Option<Image> {
    if let Some(path) = resolve_terrtex_path(route_dir, file_name) {
        // Terrain base sanitization inspects RGBA, never compressed block bytes.
        let mut image = openrailsrs_bevy_scenery::texture_cache::load(
            &path,
            bevy::image::CompressedImageFormats::NONE,
            None,
            None,
        )
        .ok()?
        .image;
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

pub(crate) fn terrain_texture_key(file_name: &str, base: bool) -> String {
    format!(
        "{file_name}:{}:{}",
        crate::shapes::scenery_texture_environment(
            openrailsrs_bevy_scenery::textures::TextureFlags::from_raw(0)
        )
        .cache_key(),
        if base { "base" } else { "raw" }
    )
}

/// Prepare the same base/overlay pixels used by `texture_handle`, on a worker.
pub(crate) fn prepare_terrain_images(
    route_dir: &Path,
    shaders: &[TerrainShader],
) -> Vec<(String, Image)> {
    let mut names = std::collections::BTreeSet::new();
    names.insert((DEFAULT_MICROTEX.to_string(), false));
    for shader in shaders {
        names.insert((
            shader
                .texslots
                .first()
                .map(|s| s.filename.clone())
                .unwrap_or_else(|| "grass.ace".into()),
            true,
        ));
        names.insert((
            shader
                .texslots
                .get(1)
                .map(|s| s.filename.clone())
                .unwrap_or_else(|| DEFAULT_MICROTEX.into()),
            false,
        ));
    }
    names
        .into_iter()
        .filter_map(|(name, base)| {
            resolve_terrtex_path(route_dir, &name)?;
            let mut image = load_terrtex_image(route_dir, &name)?;
            if base {
                sanitize_terrain_base_rgba(image.data.as_mut());
            }
            Some((terrain_texture_key(&name, base), image))
        })
        .collect()
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
        .unwrap_or_else(|| {
            cache
                .entry("@neutral-terrain-overlay:raw".into())
                .or_insert_with(|| {
                    // OR terrain multiplies base × overlay × 2. A missing overlay
                    // must be half intensity in linear space, not a second base map.
                    let mut image = Image::new_fill(
                        Extent3d {
                            width: 1,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        TextureDimension::D2,
                        &[128, 128, 128, 255],
                        TextureFormat::Rgba8Unorm,
                        RenderAssetUsages::default(),
                    );
                    set_terrain_repeat_sampler(&mut image);
                    images.add(image)
                })
                .clone()
        });

    (base, overlay, overlay_scale_from_shader(shader))
}

fn texture_handle(
    route_dir: &Path,
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    file_name: &str,
    sanitize_base_alpha: bool,
) -> Option<Handle<Image>> {
    let key = terrain_texture_key(file_name, sanitize_base_alpha);
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
    fn winter_terrain_uses_native_snow_and_falls_back_for_missing_variants() {
        use openrailsrs_bevy_scenery::textures::TextureEnvironment;
        let route = tempfile::tempdir().unwrap();
        let base = route.path().join("terrtex/Grass.ACE");
        let snow = route.path().join("terrtex/SNOW/Grass.ACE");
        std::fs::create_dir_all(snow.parent().unwrap()).unwrap();
        std::fs::write(&base, []).unwrap();
        std::fs::write(&snow, []).unwrap();
        let winter = TextureEnvironment::from_cli("winter", "clear", false);
        assert_eq!(
            resolve_terrtex_for_environment(route.path(), r"TERRTEX\grass.ace", winter),
            Some(snow.clone())
        );
        assert_eq!(
            resolve_terrtex_for_environment(
                route.path(),
                "grass.ace",
                TextureEnvironment::summer_day()
            ),
            Some(base.clone())
        );
        std::fs::remove_file(snow).unwrap();
        assert_eq!(
            resolve_terrtex_for_environment(route.path(), "grass.ace", winter),
            Some(base)
        );
    }

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
    fn missing_microtexture_preserves_base_color_and_reuses_neutral_overlay() {
        let route = tempfile::tempdir().unwrap();
        let shader = TerrainShader {
            name: "t".into(),
            texslots: vec![],
            uvcalcs: vec![],
        };
        let mut images = Assets::<Image>::default();
        let fallback = images.add(Image::default());
        let mut cache = HashMap::new();
        let (base, overlay, _) = terrain_material_textures(
            route.path(),
            &mut images,
            &mut cache,
            &shader,
            fallback.clone(),
        );
        assert_eq!(base, fallback);
        assert_ne!(overlay, base);
        let image = images.get(&overlay).unwrap();
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        for channel in &image.data.as_deref().unwrap()[..3] {
            assert!((2.0 * f32::from(*channel) / 255.0 - 1.0).abs() < 0.01);
        }
        let count = images.len();
        let (_, again, _) =
            terrain_material_textures(route.path(), &mut images, &mut cache, &shader, fallback);
        assert_eq!(again, overlay);
        assert_eq!(images.len(), count);
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
