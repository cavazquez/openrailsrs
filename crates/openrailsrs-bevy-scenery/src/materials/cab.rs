//! Open Rails `SceneryShader.fx` path for CABVIEW3D interiors (no VSM atlas).

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::SpecializedMeshPipelineError;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType};
use bevy::shader::{ShaderDefVal, ShaderRef};

use openrailsrs_or_shader::{
    OR_OPAQUE_REFERENCE_ALPHA, OrShaderKind, or_cab_shader_kind_gpu_id, or_shader_kind_gpu_id,
    resolve_or_material_kind,
};

/// Debug fragment path for `or_cab.wgsl` (`OPENRAILSRS_CAB_DEBUG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CabDebugView {
    #[default]
    Off,
    Uv,
    Albedo,
    VertexColor,
}

impl CabDebugView {
    fn shader_def(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Uv => Some("OR_CAB_DEBUG_UV"),
            Self::Albedo => Some("OR_CAB_DEBUG_ALBEDO"),
            Self::VertexColor => Some("OR_CAB_DEBUG_VCOLOR"),
        }
    }
}

fn cab_debug_view() -> CabDebugView {
    match std::env::var("OPENRAILSRS_CAB_DEBUG")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("uv") | Some("UV") => CabDebugView::Uv,
        Some("vcolor") | Some("VCOLOR") => CabDebugView::VertexColor,
        Some("albedo") | Some("1") | Some("true") | Some("on") => CabDebugView::Albedo,
        _ => CabDebugView::Off,
    }
}

pub const OR_CAB_SHADER_PATH: &str = "shaders/or_cab.wgsl";

const OR_FLAG_LIT: f32 = 1.0;
const OR_FLAG_BLEND: f32 = 2.0;
/// Legacy fixed brightness without outdoor sun — opt-in via `OPENRAILSRS_CAB_OR_LIKE=1`.
pub const OR_FLAG_OR_LIKE: f32 = 4.0;

/// Outdoor directional sun on cab TexDiff — default ON for OR SceneryShader parity (#153).
/// Set `OPENRAILSRS_CAB_SUN=0` to disable.
pub fn cab_interior_sun_enabled() -> bool {
    match std::env::var("OPENRAILSRS_CAB_SUN")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("off") => false,
        Some("1") | Some("true") | Some("on") => true,
        _ => !cab_or_like_enabled() && !cab_interior_raw_flat_enabled(),
    }
}

/// Legacy flat albedo (`OPENRAILSRS_CAB_RAW=1`) — debugging only.
pub fn cab_interior_raw_flat_enabled() -> bool {
    matches!(
        std::env::var("OPENRAILSRS_CAB_RAW").ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    )
}

/// Legacy fixed ambient mix (`OPENRAILSRS_CAB_OR_LIKE=1`) — debugging only (#153).
pub fn cab_or_like_enabled() -> bool {
    matches!(
        std::env::var("OPENRAILSRS_CAB_OR_LIKE").ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    )
}

/// No invented brightness floor; OR SceneryShader has none (#153).
/// Optional lift via `OPENRAILSRS_CAB_MIN_BRIGHT`.
fn cab_min_brightness_default() -> f32 {
    0.0
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

pub fn reference_alpha_from_mode(alpha_mode: AlphaMode) -> f32 {
    match alpha_mode {
        AlphaMode::Opaque => OR_OPAQUE_REFERENCE_ALPHA,
        AlphaMode::Mask(c) => c,
        AlphaMode::Blend
        | AlphaMode::Add
        | AlphaMode::Premultiplied
        | AlphaMode::AlphaToCoverage
        | AlphaMode::Multiply => 0.01,
    }
}

fn cab_material_flags() -> f32 {
    if cab_interior_raw_flat_enabled() {
        0.0
    } else if cab_or_like_enabled() {
        OR_FLAG_OR_LIKE
    } else if cab_interior_sun_enabled() {
        OR_FLAG_LIT
    } else {
        0.0
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
        flags: cab_material_flags(),
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
    let kind = resolve_or_material_kind(shader_name, light_mat_idx);
    let reference_alpha = reference_alpha_from_mode(alpha_mode);
    let mut params = build_or_cab_params(kind, reference_alpha);
    params.shader_kind = or_cab_shader_kind_gpu_id(kind);
    if matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add) {
        params.flags = OR_FLAG_BLEND;
        params.reference_alpha = 0.0;
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
    fn cab_env_toggles() {
        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
            std::env::remove_var("OPENRAILSRS_CAB_RAW");
            std::env::remove_var("OPENRAILSRS_CAB_OR_LIKE");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let default = build_or_cab_params(OrShaderKind::TexDiff, OR_OPAQUE_REFERENCE_ALPHA);
        assert_eq!(default.flags, OR_FLAG_LIT);
        assert!(default.cab_min_brightness.abs() < 1e-3);

        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_OR_LIKE", "1");
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
            std::env::remove_var("OPENRAILSRS_CAB_RAW");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let or_like = build_or_cab_params(OrShaderKind::TexDiff, 0.01);
        assert_eq!(or_like.flags, OR_FLAG_OR_LIKE);
        assert!(or_like.cab_min_brightness.abs() < 1e-3);

        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_SUN", "0");
            std::env::remove_var("OPENRAILSRS_CAB_OR_LIKE");
            std::env::remove_var("OPENRAILSRS_CAB_RAW");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let sun_off = build_or_cab_params(OrShaderKind::TexDiff, 0.01);
        assert_eq!(sun_off.flags, 0.0);

        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
            std::env::set_var("OPENRAILSRS_CAB_RAW", "1");
            std::env::remove_var("OPENRAILSRS_CAB_OR_LIKE");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let raw = build_or_cab_params(OrShaderKind::TexDiff, 0.01);
        assert_eq!(raw.flags, 0.0);

        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
            std::env::remove_var("OPENRAILSRS_CAB_RAW");
            std::env::remove_var("OPENRAILSRS_CAB_OR_LIKE");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
    }

    #[test]
    fn cab_opaque_reference_alpha() {
        assert_eq!(
            reference_alpha_from_mode(AlphaMode::Opaque),
            OR_OPAQUE_REFERENCE_ALPHA
        );
    }
}
