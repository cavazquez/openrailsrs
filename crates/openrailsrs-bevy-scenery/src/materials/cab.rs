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

/// Floor so underside / sun-facing-away cab panels keep albedo detail (labels, plaques).
/// Pure OR SceneryShader has no floor (#153); Bevy half-Lambert + no cab ambient otherwise
/// crushes dark ACE lettering. Override with `OPENRAILSRS_CAB_MIN_BRIGHT` (`0` = OR-strict).
fn cab_min_brightness_default() -> f32 {
    0.55
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

/// Pipeline key: polygon depth bias + whether to write the depth buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OrCabMaterialKey {
    pub depth_bias: i32,
    pub depth_write: bool,
}

impl From<&OrCabMaterial> for OrCabMaterialKey {
    fn from(material: &OrCabMaterial) -> Self {
        Self {
            depth_bias: material.depth_bias as i32,
            depth_write: material.depth_write,
        }
    }
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
        // Slightly above OR outdoor 0.5 so enclosed cab shade still reads textures.
        shadow_brightness: 0.72,
        full_brightness: 1.0,
        half_shadow_brightness: 0.85,
        shader_kind: or_shader_kind_gpu_id(kind),
        cab_min_brightness: cab_min,
        flags: cab_material_flags(),
    }
}

/// MSTS `z_buf_mode` → OrCab polygon bias + depth-write hint.
///
/// Pullman `PULLMAN_GR.s` marks **every** prim (including opaque desk/floor) as
/// `ZBufMode=1`. Open Rails still occludes the world because the cab is drawn in a
/// late pass; Bevy shares one opaque pass with scenery, so callers must treat
/// opaque/mask materials as depth-writing via [`or_cab_depth_write_for_alpha`].
pub fn or_cab_depth_from_z_buf(z_buf_mode: i32, depth_bias: f32) -> (f32, bool) {
    match z_buf_mode {
        // Depth test, no write (OR typical for overlays / needles / late cab pass).
        1 => (depth_bias.max(1.0), false),
        // Prefer drawing on top.
        2 => (depth_bias.max(2.0), false),
        _ => (depth_bias, true),
    }
}

/// Effective depth-write for cab materials in Bevy's shared opaque pass.
///
/// Opaque/Mask always write depth so WORLD TrackObj cannot stomp the desk/floor.
/// Blend/Add keep MSTS `z_buf_mode` (glass/overlays).
pub fn or_cab_depth_write_for_alpha(alpha_mode: AlphaMode, z_buf_mode: i32) -> bool {
    match alpha_mode {
        AlphaMode::Opaque | AlphaMode::Mask(_) => true,
        _ => or_cab_depth_from_z_buf(z_buf_mode, 0.0).1,
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(OrCabMaterialKey)]
pub struct OrCabMaterial {
    #[uniform(0)]
    pub params: OrCabGpuParams,
    #[texture(1)]
    #[sampler(2)]
    pub base_texture: Handle<Image>,
    pub alpha_mode: AlphaMode,
    /// Polygon / sort depth bias (Bevy `Material::depth_bias`; integer part → GPU constant).
    pub depth_bias: f32,
    /// When false, match MSTS `z_buf_mode=1` (read depth, do not write).
    pub depth_write: bool,
}

impl Material for OrCabMaterial {
    fn fragment_shader() -> ShaderRef {
        OR_CAB_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn depth_bias(&self) -> f32 {
        self.depth_bias
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth_stencil) = descriptor.depth_stencil.as_mut() {
            depth_stencil.bias.constant = key.bind_group_data.depth_bias;
            depth_stencil.depth_write_enabled = Some(key.bind_group_data.depth_write);
        }
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
    create_or_cab_material_ex(
        materials,
        texture,
        tint,
        alpha_mode,
        shader_name,
        light_mat_idx,
        0.0,
        -1,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_or_cab_material_ex(
    materials: &mut Assets<OrCabMaterial>,
    texture: Handle<Image>,
    tint: Color,
    alpha_mode: AlphaMode,
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
    depth_bias: f32,
    z_buf_mode: i32,
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
    let (raw_bias, _) = or_cab_depth_from_z_buf(z_buf_mode, depth_bias);
    // Opaque cab must write depth (shared pass with WORLD). Keep overlay bias only
    // for transparent / non-writing paths; solid geometry uses authored bias as-is.
    let depth_write = or_cab_depth_write_for_alpha(alpha_mode, z_buf_mode);
    let depth_bias = if depth_write {
        depth_bias
    } else {
        raw_bias
    };
    materials.add(OrCabMaterial {
        params,
        base_texture: texture,
        alpha_mode,
        depth_bias,
        depth_write,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfbright_kind_id() {
        let p = build_or_cab_params(OrShaderKind::HalfBright, 0.01);
        assert_eq!(p.shader_kind, 2.0);
        assert!((p.half_shadow_brightness - 0.85).abs() < 1e-3);
        assert!((p.shadow_brightness - 0.72).abs() < 1e-3);
    }

    #[test]
    fn z_buf_mode_one_disables_depth_write() {
        let (bias, write) = or_cab_depth_from_z_buf(1, 0.0);
        assert!(!write);
        assert!(bias >= 1.0);
    }

    #[test]
    fn opaque_cab_writes_depth_despite_z_buf_mode_one() {
        // Pullman marks floor/DESK1 as ZBufMode=1; Bevy still needs depth write.
        assert!(or_cab_depth_write_for_alpha(AlphaMode::Opaque, 1));
        assert!(or_cab_depth_write_for_alpha(AlphaMode::Mask(0.5), 1));
        assert!(!or_cab_depth_write_for_alpha(AlphaMode::Blend, 1));
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
        assert!((default.cab_min_brightness - 0.55).abs() < 1e-3);

        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_OR_LIKE", "1");
            std::env::remove_var("OPENRAILSRS_CAB_SUN");
            std::env::remove_var("OPENRAILSRS_CAB_RAW");
            std::env::remove_var("OPENRAILSRS_CAB_MIN_BRIGHT");
        }
        let or_like = build_or_cab_params(OrShaderKind::TexDiff, 0.01);
        assert_eq!(or_like.flags, OR_FLAG_OR_LIKE);
        assert!((or_like.cab_min_brightness - 0.55).abs() < 1e-3);

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
