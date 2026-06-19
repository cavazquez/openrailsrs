//! Open Rails `SceneryShader.fx` path for CABVIEW3D interiors (no VSM atlas).

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::SpecializedMeshPipelineError;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType};
use bevy::shader::{ShaderDefVal, ShaderRef};

use crate::cab_diag::cab_debug_view;
use crate::or_shader::{OrShaderKind, or_shader_kind_gpu_id};

pub const OR_CAB_SHADER_PATH: &str = "shaders/or_cab.wgsl";

const OR_FLAG_LIT: f32 = 1.0;
const OR_FLAG_BLEND: f32 = 2.0;

/// Outdoor directional sun on cab TexDiff (`OPENRAILSRS_CAB_SUN=1`). Default off — flat interior.
pub fn cab_interior_sun_enabled() -> bool {
    matches!(
        std::env::var("OPENRAILSRS_CAB_SUN").ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    )
}

fn cab_min_brightness_default() -> f32 {
    if cab_interior_sun_enabled() {
        0.72
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct OrCabGpuParams {
    pub tint_r: f32,
    pub tint_g: f32,
    pub tint_b: f32,
    pub tint_a: f32,
    pub reference_alpha: f32,
    pub shadow_brightness: f32,
    pub full_brightness: f32,
    pub half_shadow_brightness: f32,
    pub shader_kind: f32,
    pub cab_min_brightness: f32,
    pub flags: f32,
}

pub fn or_cab_shaders_enabled() -> bool {
    match std::env::var("OPENRAILSRS_OR_CAB_SHADER").ok().as_deref() {
        Some("0") => false,
        Some("1") => true,
        _ => true,
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

pub fn build_or_cab_params(kind: OrShaderKind, reference_alpha: f32) -> OrCabGpuParams {
    let cab_min = std::env::var("OPENRAILSRS_CAB_MIN_BRIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(cab_min_brightness_default)
        .clamp(0.0, 1.0);

    OrCabGpuParams {
        tint_r: 1.0,
        tint_g: 1.0,
        tint_b: 1.0,
        tint_a: 1.0,
        reference_alpha,
        shadow_brightness: 0.5,
        full_brightness: 1.0,
        half_shadow_brightness: 0.75,
        shader_kind: or_shader_kind_gpu_id(kind),
        cab_min_brightness: cab_min,
        flags: if cab_interior_sun_enabled() {
            OR_FLAG_LIT
        } else {
            0.0
        },
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct OrCabMaterial {
    #[uniform(0)]
    pub params: OrCabGpuParams,
    #[texture(1)]
    #[sampler(2)]
    pub base_texture: Handle<Image>,
    pub alpha_mode: AlphaMode,
}

impl Material for OrCabMaterial {
    fn fragment_shader() -> ShaderRef {
        OR_CAB_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if layout.0.contains(Mesh::ATTRIBUTE_COLOR) {
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment
                    .shader_defs
                    .push(ShaderDefVal::from("VERTEX_COLORS"));
            }
        }
        if let Some(def) = cab_debug_view().shader_def() {
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push(ShaderDefVal::from(def));
            }
        }
        Ok(())
    }
}

pub fn create_or_cab_material(
    materials: &mut Assets<OrCabMaterial>,
    texture: Handle<Image>,
    tint: Color,
    alpha_mode: AlphaMode,
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
) -> Handle<OrCabMaterial> {
    let kind = crate::or_shader::resolve_or_material_kind(shader_name, light_mat_idx);
    let reference_alpha = reference_alpha_from_mode(alpha_mode);
    let mut params = build_or_cab_params(kind, reference_alpha);
    params.shader_kind = crate::or_shader::or_cab_shader_kind_gpu_id(kind);
    if matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add) {
        params.flags = OR_FLAG_BLEND;
        params.reference_alpha = 0.0;
    } else if !cab_interior_sun_enabled() {
        params.flags = 0.0;
    }
    let linear = tint.to_linear();
    params.tint_r = linear.red;
    params.tint_g = linear.green;
    params.tint_b = linear.blue;
    params.tint_a = linear.alpha;
    materials.add(OrCabMaterial {
        params,
        base_texture: texture,
        alpha_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfbright_kind_id() {
        let p = build_or_cab_params(OrShaderKind::HalfBright, 0.01);
        assert_eq!(p.shader_kind, 2.0);
        assert_eq!(p.half_shadow_brightness, 0.75);
    }

    #[test]
    fn cab_interior_flat_by_default() {
        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let p = build_or_cab_params(OrShaderKind::TexDiff, 0.01);
        assert_eq!(p.flags, 0.0);
        assert_eq!(p.cab_min_brightness, 0.0);
    }

    #[test]
    fn cab_sun_opt_in() {
        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_SUN", "1");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let p = build_or_cab_params(OrShaderKind::TexDiff, 0.01);
        assert_eq!(p.flags, OR_FLAG_LIT);
        assert!((p.cab_min_brightness - 0.72).abs() < 1e-3);
        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
        }
    }
}
