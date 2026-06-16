//! Material WGSL estilo Open Rails PSTerrain (SceneryShader.fx TerrainLevel9_3).

use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

pub const OR_TERRAIN_SHADER_PATH: &str = "shaders/or_terrain.wgsl";
pub const DEFAULT_MICROTEX: &str = "microtex.ace";

#[derive(Clone, Copy, Debug, Default, bevy::render::render_resource::ShaderType)]
pub struct OrTerrainGpuParams {
    pub shadow_brightness: f32,
    pub full_brightness: f32,
    pub night_color_modifier: f32,
    pub image_texture_is_night: f32,
    pub overlay_scale: f32,
    pub lit: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

pub fn or_terrain_shaders_enabled(_materials_lit: bool) -> bool {
    !matches!(
        std::env::var("OPENRAILSRS_OR_TERRAIN").ok().as_deref(),
        Some("0")
    )
}

/// Escala UV del overlay (`terrain_uvcalcs[1].d`), default 32 como OR.
pub fn overlay_scale_from_uvcalc(d: f64) -> f32 {
    if d != 0.0 && (d - 32.0).abs() > 1e-3 {
        d as f32
    } else {
        32.0
    }
}

pub fn build_or_terrain_params(lit: bool, night: bool) -> OrTerrainGpuParams {
    let (night_mod, image_night) = if night { (0.35, 0.0) } else { (1.0, 0.0) };
    OrTerrainGpuParams {
        shadow_brightness: 0.5,
        full_brightness: 1.0,
        night_color_modifier: night_mod,
        image_texture_is_night: image_night,
        overlay_scale: 32.0,
        lit: if lit { 1.0 } else { 0.0 },
        _pad0: 0.0,
        _pad1: 0.0,
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct OrTerrainMaterial {
    #[uniform(0)]
    pub params: OrTerrainGpuParams,
    #[texture(1)]
    #[sampler(2)]
    pub base_texture: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub overlay_texture: Handle<Image>,
}

impl Material for OrTerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        OR_TERRAIN_SHADER_PATH.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub fn set_terrain_repeat_sampler(image: &mut Image) {
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
}

pub fn create_or_terrain_material(
    materials: &mut Assets<OrTerrainMaterial>,
    base_texture: Handle<Image>,
    overlay_texture: Handle<Image>,
    overlay_scale: f32,
    lit: bool,
    night: bool,
) -> Handle<OrTerrainMaterial> {
    let mut params = build_or_terrain_params(lit, night);
    params.overlay_scale = overlay_scale;
    materials.add(OrTerrainMaterial {
        params,
        base_texture,
        overlay_texture,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_scale_defaults_to_32() {
        assert!((overlay_scale_from_uvcalc(0.0) - 32.0).abs() < 1e-3);
        assert!((overlay_scale_from_uvcalc(32.0) - 32.0).abs() < 1e-3);
    }

    #[test]
    fn overlay_scale_honors_uvcalc_d() {
        assert!((overlay_scale_from_uvcalc(48.0) - 48.0).abs() < 1e-3);
    }

    #[test]
    fn env_disables_or_terrain_shader() {
        unsafe {
            std::env::set_var("OPENRAILSRS_OR_TERRAIN", "0");
        }
        assert!(!or_terrain_shaders_enabled(true));
        unsafe {
            std::env::remove_var("OPENRAILSRS_OR_TERRAIN");
        }
    }

    #[test]
    fn terrain_params_match_or_psterrain_constants() {
        let p = build_or_terrain_params(true, false);
        assert_eq!(p.shadow_brightness, 0.5);
        assert_eq!(p.full_brightness, 1.0);
        assert_eq!(p.lit, 1.0);
    }
}
