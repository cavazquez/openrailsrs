//! Material WGSL con logica de SceneryShader.fx (Open Rails).

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::render::render_resource::{RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::{ShaderDefVal, ShaderRef};

use crate::vsm::OrVsmMode;
use openrailsrs_or_shader::{OrShaderKind, classify_or_shader};

pub const OR_SCENERY_SHADER_PATH: &str = "shaders/or_scenery.wgsl";

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct OrSceneryGpuParams {
    pub tint_r: f32,
    pub tint_g: f32,
    pub tint_b: f32,
    pub tint_a: f32,
    pub reference_alpha: f32,
    pub shadow_brightness: f32,
    pub full_brightness: f32,
    pub half_shadow_brightness: f32,
    pub specular_strength: f32,
    pub specular_power: f32,
    pub night_color_modifier: f32,
    pub image_texture_is_night: f32,
    pub shader_kind: f32,
    pub flags: f32,
    pub vsm_mode: f32,
    pub shadow_map_limit_x: f32,
    pub shadow_map_limit_y: f32,
    pub shadow_map_limit_z: f32,
    pub shadow_map_limit_w: f32,
    pub debug_flags: f32,
}

const OR_KIND_TEX: f32 = 0.0;
const OR_KIND_TEX_DIFF: f32 = 1.0;
const OR_KIND_HALF_BRIGHT: f32 = 2.0;
const OR_KIND_DARK_SHADE: f32 = 3.0;
const OR_KIND_FULL_BRIGHT: f32 = 4.0;
const OR_KIND_SPECULAR25: f32 = 5.0;
const OR_KIND_SPECULAR750: f32 = 6.0;
const OR_KIND_SPECULAR: f32 = 7.0;
const OR_FLAG_LIT: f32 = 1.0;

pub fn or_scenery_shaders_enabled(materials_lit: bool) -> bool {
    match std::env::var("OPENRAILSRS_OR_SHADERS").ok().as_deref() {
        Some("0") => false,
        Some("1") => true,
        _ => materials_lit,
    }
}

fn or_kind_id(kind: OrShaderKind) -> f32 {
    match kind {
        OrShaderKind::Tex
        | OrShaderKind::Unknown
        | OrShaderKind::AddATex
        | OrShaderKind::BlendATex => OR_KIND_TEX,
        OrShaderKind::TexDiff => OR_KIND_TEX_DIFF,
        OrShaderKind::HalfBright => OR_KIND_HALF_BRIGHT,
        OrShaderKind::DarkShade | OrShaderKind::Dark => OR_KIND_DARK_SHADE,
        OrShaderKind::FullBright | OrShaderKind::Bright => OR_KIND_FULL_BRIGHT,
        OrShaderKind::Specular25 => OR_KIND_SPECULAR25,
        OrShaderKind::Specular750 => OR_KIND_SPECULAR750,
        OrShaderKind::Specular => OR_KIND_SPECULAR,
    }
}

fn reference_alpha_from_mode(alpha_mode: AlphaMode) -> f32 {
    match alpha_mode {
        AlphaMode::Mask(c) => c,
        AlphaMode::Blend
        | AlphaMode::Opaque
        | AlphaMode::Add
        | AlphaMode::Premultiplied
        | AlphaMode::AlphaToCoverage
        | AlphaMode::Multiply => 0.01,
    }
}

fn texture_name_suggests_rail(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    lower.contains("rail")
        || lower.contains("ukfs_r")
        || (lower.starts_with("ukfs_") && lower.contains("head"))
}

pub fn build_or_scenery_params(
    kind: OrShaderKind,
    lit: bool,
    reference_alpha: f32,
    night: bool,
    night_texture: bool,
    texture_name: &str,
    shadow_map_limits: [f32; 4],
) -> OrSceneryGpuParams {
    let mut specular_power = match kind {
        OrShaderKind::Specular750 => 750.0_f32,
        OrShaderKind::Specular25 => 25.0_f32,
        OrShaderKind::Specular => 128.0_f32,
        _ => 25.0_f32,
    };
    let mut specular_strength = match kind {
        OrShaderKind::Specular750 | OrShaderKind::Specular => 1.0_f32,
        OrShaderKind::Specular25 => 0.85_f32,
        OrShaderKind::HalfBright | OrShaderKind::DarkShade | OrShaderKind::Dark => 0.0_f32,
        OrShaderKind::FullBright | OrShaderKind::Bright => 0.0_f32,
        _ => {
            if kind == OrShaderKind::Unknown {
                0.35_f32
            } else {
                0.55_f32
            }
        }
    };

    if texture_name_suggests_rail(texture_name) {
        specular_strength = f32::max(specular_strength, 0.75);
        specular_power = f32::max(specular_power, 128.0);
    }

    let (night_mod, image_night) = if night_texture {
        (1.0_f32, 1.0_f32)
    } else if night {
        (0.35_f32, 0.0_f32)
    } else {
        (1.0_f32, 0.0_f32)
    };

    OrSceneryGpuParams {
        tint_r: 1.0,
        tint_g: 1.0,
        tint_b: 1.0,
        tint_a: 1.0,
        reference_alpha,
        shadow_brightness: 0.5,
        full_brightness: 1.0,
        half_shadow_brightness: 0.75,
        specular_strength,
        specular_power,
        night_color_modifier: night_mod,
        image_texture_is_night: image_night,
        shader_kind: or_kind_id(kind),
        flags: if lit { OR_FLAG_LIT } else { 0.0 },
        vsm_mode: OrVsmMode::from_env().as_gpu(),
        shadow_map_limit_x: shadow_map_limits[0],
        shadow_map_limit_y: shadow_map_limits[1],
        shadow_map_limit_z: shadow_map_limits[2],
        shadow_map_limit_w: shadow_map_limits[3],
        debug_flags: 0.0,
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct OrSceneryMaterial {
    #[uniform(0)]
    pub params: OrSceneryGpuParams,
    #[texture(1)]
    #[sampler(2)]
    pub base_texture: Handle<Image>,
    #[texture(3, dimension = "2d_array")]
    #[sampler(4)]
    pub moment_atlas: Handle<Image>,
    pub alpha_mode: AlphaMode,
}

impl Material for OrSceneryMaterial {
    fn fragment_shader() -> ShaderRef {
        OR_SCENERY_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment
                .shader_defs
                .push(ShaderDefVal::from("OR_VSM_MOMENT_ATLAS"));
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_or_scenery_material(
    materials: &mut Assets<OrSceneryMaterial>,
    texture: Handle<Image>,
    moment_atlas: Handle<Image>,
    shadow_map_limits: [f32; 4],
    tint: Color,
    alpha_mode: AlphaMode,
    shader_name: Option<&str>,
    texture_name: &str,
    lit: bool,
    night: bool,
    night_texture: bool,
) -> Handle<OrSceneryMaterial> {
    let kind = classify_or_shader(shader_name);
    let reference_alpha = reference_alpha_from_mode(alpha_mode);
    let mut params = build_or_scenery_params(
        kind,
        lit,
        reference_alpha,
        night,
        night_texture,
        texture_name,
        shadow_map_limits,
    );
    let linear = tint.to_linear();
    params.tint_r = linear.red;
    params.tint_g = linear.green;
    params.tint_b = linear.blue;
    params.tint_a = linear.alpha;
    materials.add(OrSceneryMaterial {
        params,
        base_texture: texture,
        moment_atlas,
        alpha_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfbright_has_fixed_brightness_kind() {
        let p = build_or_scenery_params(
            OrShaderKind::HalfBright,
            true,
            0.01,
            false,
            false,
            "brick.ace",
            [100.0, 200.0, 400.0, 800.0],
        );
        assert_eq!(p.shader_kind, OR_KIND_HALF_BRIGHT);
        assert_eq!(p.half_shadow_brightness, 0.75);
        assert_eq!(p.specular_strength, 0.0);
    }

    #[test]
    fn tex_uses_specular_strength() {
        let p = build_or_scenery_params(
            OrShaderKind::Tex,
            true,
            0.01,
            false,
            false,
            "brick.ace",
            [100.0, 200.0, 400.0, 800.0],
        );
        assert_eq!(p.shader_kind, OR_KIND_TEX);
        assert!(p.specular_strength > 0.0);
    }

    #[test]
    fn env_disables_or_shaders() {
        unsafe {
            std::env::set_var("OPENRAILSRS_OR_SHADERS", "0");
        }
        assert!(!or_scenery_shaders_enabled(true));
        unsafe {
            std::env::remove_var("OPENRAILSRS_OR_SHADERS");
        }
    }
}
