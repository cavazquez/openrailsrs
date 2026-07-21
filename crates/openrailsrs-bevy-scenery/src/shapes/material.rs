//! Scenery material helpers (OR-style lit vs legacy unlit StandardMaterial).

use std::sync::OnceLock;

use bevy::prelude::*;
use bevy::render::render_resource::Face;
use openrailsrs_ace::AceFile;
use openrailsrs_or_shader::OR_MSTS_ALPHA_TEST_CUTOFF;
use openrailsrs_or_shader::standard_pbr::{apply_albedo_scale, resolve_or_material_pbr_ex};

pub use crate::materials::or_scenery_shaders_enabled;

/// HDR multiplier on textured scenery whose `.ace` mip0 is already reasonably bright.
pub const SCENERY_TEXTURE_ALBEDO_BOOST: f32 = 4.0;

/// Mean sRGB luma below this → MSTS atlas looks black even with unlit + albedo boost.
pub const DARK_TEXTURE_LUMA_THRESHOLD: f32 = 32.0;

/// Target mean luma after normalizing dark MSTS `.ace` mip0 (Open Rails draws these unlit).
pub const SCENERY_TEXTURE_TARGET_LUMA: f32 = 112.0;

/// Max per-pixel scale when lifting near-black atlases (signals, tunnels, night textures).
const SCENERY_TEXTURE_MAX_PIXEL_SCALE: f32 = 128.0;

/// Small extra tint after pixel normalization (avoid double-boost with [`SCENERY_TEXTURE_ALBEDO_BOOST`]).
const SCENERY_TEXTURE_POST_BRIGHTEN_TINT: f32 = 1.25;

/// Force legacy Bevy `StandardMaterial` scenery (unlit + albedo boost), skipping OR WGSL shaders.
pub fn legacy_standard_scenery_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        matches!(
            std::env::var("OPENRAILSRS_LEGACY_STANDARD_SCENERY")
                .ok()
                .as_deref(),
            Some("1") | Some("true") | Some("on")
        )
    })
}

/// Effective lit path for scenery: OR-style sun shading unless legacy/unlit opt-outs apply.
pub fn scenery_materials_lit() -> bool {
    or_lighting_enabled()
        && !legacy_standard_scenery_enabled()
        && !crate::shapes::debug::debug_force_unlit()
}

/// Whether to use `OrSceneryMaterial` WGSL for world shapes.
pub fn scenery_uses_or_wgsl_shaders() -> bool {
    or_scenery_shaders_enabled(scenery_materials_lit()) && !legacy_standard_scenery_enabled()
}

/// Open Rails lights its world with a sun + ambient and tone-maps it; MSTS `.ace`
/// albedos look right under that model. This OR-style lit path (sun shading + shadow
/// receive, neutral albedo) is the **default** and matches the camera's physical
/// `Exposure::SUNLIGHT` + 75 klux sun + ambient fill.
///
/// The legacy fixed-function path draws scenery `unlit` and claws brightness back with
/// [`SCENERY_TEXTURE_ALBEDO_BOOST`] and [`brighten_dark_ace_rgba`]; it is internally
/// inconsistent with that exposure, so surfaces stay flat and never receive shadows.
/// Opt back into it with `OPENRAILSRS_UNLIT_SCENERY=1` (or `OPENRAILSRS_OR_LIGHTING=0`).
pub fn or_lighting_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        if legacy_standard_scenery_enabled() {
            return false;
        }
        resolve_or_lighting(
            std::env::var("OPENRAILSRS_UNLIT_SCENERY").ok().as_deref(),
            std::env::var("OPENRAILSRS_OR_LIGHTING").ok().as_deref(),
        )
    })
}

/// Pure resolver for the lighting mode (unit-tested without global env state).
///
/// Lit (OR-style) is the default. `OPENRAILSRS_UNLIT_SCENERY` (truthy) forces the
/// legacy unlit path; otherwise `OPENRAILSRS_OR_LIGHTING` may explicitly disable it
/// with `"0"`/empty.
pub fn resolve_or_lighting(unlit_opt_out: Option<&str>, or_flag: Option<&str>) -> bool {
    let truthy = |v: &str| !v.is_empty() && v != "0";
    if unlit_opt_out.is_some_and(truthy) {
        return false;
    }
    match or_flag {
        Some(v) => truthy(v),
        None => true,
    }
}

pub(crate) fn scenery_texture_tint() -> Color {
    let b = SCENERY_TEXTURE_ALBEDO_BOOST;
    Color::linear_rgb(b, b, b)
}

/// Albedo tint for a scenery texture, honouring the OR-style lit path.
///
/// In the lit path the sun/ambient provide brightness, so the fixed-function
/// `×SCENERY_TEXTURE_ALBEDO_BOOST` tint must collapse to white to avoid a washed-out look.
pub fn scenery_albedo_tint(pixel_brightened: bool, lit: bool) -> Color {
    if lit {
        Color::WHITE
    } else {
        scenery_material_tint_for_ace(pixel_brightened)
    }
}

/// Base (untextured / DDS) tint, honouring the OR-style lit path.
pub fn scenery_base_tint(lit: bool) -> Color {
    if lit {
        Color::WHITE
    } else {
        scenery_texture_tint()
    }
}

fn scenery_texture_tint_scaled(multiplier: f32) -> Color {
    Color::linear_rgb(multiplier, multiplier, multiplier)
}

/// Mean sRGB luma of opaque pixels in decoded ACE mip0 (0–255).
pub fn ace_mean_luma(rgba: &[u8]) -> f32 {
    if rgba.len() < 4 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for px in rgba.chunks_exact(4) {
        if px[3] < 8 {
            continue;
        }
        sum += 0.299 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.114 * f64::from(px[2]);
        n += 1;
    }
    if n == 0 { 0.0 } else { (sum / n as f64) as f32 }
}

pub(crate) fn scale_ace_channel(v: u8, scale: f32) -> u8 {
    (f32::from(v) * scale).min(255.0).round() as u8
}

/// Lift dark MSTS atlases toward [`SCENERY_TEXTURE_TARGET_LUMA`]. Returns `(rgba, was_brightened)`.
pub fn brighten_dark_ace_rgba(rgba: &[u8]) -> (Vec<u8>, bool) {
    let mean = ace_mean_luma(rgba);
    if mean >= DARK_TEXTURE_LUMA_THRESHOLD {
        return (rgba.to_vec(), false);
    }
    let scale = (SCENERY_TEXTURE_TARGET_LUMA / mean.max(1.0)).min(SCENERY_TEXTURE_MAX_PIXEL_SCALE);
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        if px[3] < 8 {
            continue;
        }
        px[0] = scale_ace_channel(px[0], scale);
        px[1] = scale_ace_channel(px[1], scale);
        px[2] = scale_ace_channel(px[2], scale);
    }
    (out, true)
}

/// Material tint for a scenery texture (full boost, or modest tint after pixel normalization).
pub fn scenery_material_tint_for_ace(pixel_brightened: bool) -> Color {
    if pixel_brightened {
        scenery_texture_tint_scaled(SCENERY_TEXTURE_POST_BRIGHTEN_TINT)
    } else {
        scenery_texture_tint()
    }
}

/// Emissive lift for atlases that stay near-black after pixel normalization (MSTS night/signal tex).
const SCENERY_DARK_EMISSIVE: LinearRgba = LinearRgba::new(0.4, 0.4, 0.45, 1.0);

pub(crate) fn scenery_needs_emissive_texture(rgba: &[u8]) -> bool {
    ace_mean_luma(rgba) < DARK_TEXTURE_LUMA_THRESHOLD
}

#[allow(clippy::too_many_arguments)]
pub fn cab_or_scenery_material_with_texture(
    tint: Color,
    handle: Handle<Image>,
    rgba_for_luma: &[u8],
    alpha_mode: AlphaMode,
    z_bias: f32,
    lit: bool,
    shader_name: Option<&str>,
    solid_color: Option<[f32; 3]>,
    cab_interior: bool,
) -> StandardMaterial {
    cab_or_scenery_material_with_texture_ex(
        tint,
        handle,
        rgba_for_luma,
        alpha_mode,
        z_bias,
        lit,
        shader_name,
        None,
        "",
        solid_color,
        cab_interior,
    )
}

/// Like [`cab_or_scenery_material_with_texture`], with shared OR PBR hints (#47).
#[allow(clippy::too_many_arguments)]
pub fn cab_or_scenery_material_with_texture_ex(
    tint: Color,
    handle: Handle<Image>,
    rgba_for_luma: &[u8],
    alpha_mode: AlphaMode,
    z_bias: f32,
    lit: bool,
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
    texture_name: &str,
    solid_color: Option<[f32; 3]>,
    cab_interior: bool,
) -> StandardMaterial {
    let tint = apply_msts_vertex_tint(tint, solid_color, shader_name);
    let pbr = if cab_interior {
        None
    } else {
        Some(resolve_or_material_pbr_ex(
            texture_name,
            shader_name,
            light_mat_idx,
            lit,
            0.85,
        ))
    };
    let material_lit = if cab_interior {
        // Bevy 0.18 forward: `unlit` skips PBR lighting; cab needs lit + point lights.
        true
    } else {
        lit && !pbr.is_some_and(|p| p.force_unlit)
    };
    let (roughness, metallic, reflectance, base_color) = if let Some(p) = pbr {
        (
            p.roughness,
            p.metallic,
            p.reflectance,
            apply_albedo_scale(tint, p.albedo_scale),
        )
    } else {
        (0.92, 0.0, 0.5, Color::WHITE)
    };
    let mut mat = StandardMaterial {
        base_color: if cab_interior {
            Color::WHITE
        } else {
            base_color
        },
        base_color_texture: Some(handle),
        perceptual_roughness: if cab_interior { 0.92 } else { roughness },
        metallic: if cab_interior { 0.0 } else { metallic },
        reflectance: if cab_interior { 0.5 } else { reflectance },
        double_sided: true,
        cull_mode: None,
        alpha_mode,
        depth_bias: z_bias,
        fog_enabled: false,
        ..default()
    };
    if cab_interior && matches!(alpha_mode, AlphaMode::Opaque) {
        // OR HalfBright-style ambient fill (no emissive_texture — PBR uses base_color_texture).
        // Skip on blend/mask glass — emissive washes out transparent cab windows (.dds).
        mat.emissive = LinearRgba::new(0.22, 0.23, 0.25, 1.0);
    } else if !material_lit && scenery_needs_emissive_texture(rgba_for_luma) {
        mat.emissive = SCENERY_DARK_EMISSIVE;
        mat.emissive_texture = mat.base_color_texture.clone();
    } else if let Some(p) = pbr {
        if material_lit && p.ambient_fill != LinearRgba::new(0.0, 0.0, 0.0, 1.0) {
            mat.emissive = p.ambient_fill;
        }
    }
    finalize_scenery_material(mat, material_lit)
}

/// Assign a normal map on Bevy [`StandardMaterial`] (OpenGL Y by default) — #44.
///
/// Only call after [`crate::shapes::ensure_tangents_for_normal_mapping`] succeeded.
pub fn apply_standard_normal_map(
    mat: &mut StandardMaterial,
    normal_map: Handle<Image>,
    flip_normal_map_y: bool,
) {
    mat.normal_map_texture = Some(normal_map);
    mat.flip_normal_map_y = flip_normal_map_y;
}

/// Rolling-stock exterior: Open Rails `CullCounterClockwise` (back-face cull, CCW front).
pub fn apply_train_exterior_culling(mat: &mut StandardMaterial) {
    mat.double_sided = false;
    mat.cull_mode = Some(Face::Back);
}

/// Textured exterior body for live/replay train `.s` meshes (single-sided + back cull).
#[allow(clippy::too_many_arguments)]
pub fn train_exterior_material_with_texture(
    tint: Color,
    handle: Handle<Image>,
    rgba_for_luma: &[u8],
    alpha_mode: AlphaMode,
    z_bias: f32,
    lit: bool,
    shader_name: Option<&str>,
    solid_color: Option<[f32; 3]>,
) -> StandardMaterial {
    train_exterior_material_with_texture_ex(
        tint,
        handle,
        rgba_for_luma,
        alpha_mode,
        z_bias,
        lit,
        shader_name,
        None,
        "",
        solid_color,
    )
}

/// Like [`train_exterior_material_with_texture`], with shared OR PBR hints (#47).
#[allow(clippy::too_many_arguments)]
pub fn train_exterior_material_with_texture_ex(
    tint: Color,
    handle: Handle<Image>,
    rgba_for_luma: &[u8],
    alpha_mode: AlphaMode,
    z_bias: f32,
    lit: bool,
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
    texture_name: &str,
    solid_color: Option<[f32; 3]>,
) -> StandardMaterial {
    let mut mat = cab_or_scenery_material_with_texture_ex(
        tint,
        handle,
        rgba_for_luma,
        alpha_mode,
        z_bias,
        lit,
        shader_name,
        light_mat_idx,
        texture_name,
        solid_color,
        false,
    );
    apply_train_exterior_culling(&mut mat);
    mat
}

/// OR `TexDiff` / `Tex`: vertex colour × texture albedo.
pub fn shader_uses_vertex_color_multiply(shader_name: Option<&str>) -> bool {
    shader_name.is_some_and(|s| {
        let n = s.to_ascii_lowercase();
        n.contains("diff") || n == "tex"
    })
}

pub fn apply_msts_vertex_tint(
    tint: Color,
    solid_color: Option<[f32; 3]>,
    shader_name: Option<&str>,
) -> Color {
    let Some(rgb) = solid_color else {
        return tint;
    };
    if !shader_uses_vertex_color_multiply(shader_name) {
        return tint;
    }
    let c = tint.to_linear();
    Color::linear_rgba(c.red * rgb[0], c.green * rgb[1], c.blue * rgb[2], c.alpha)
}

/// Target mean luma when [`cab_ace_brighten_enabled`] lifts dark cab atlases.
const CAB_TEXTURE_TARGET_LUMA: f32 = 140.0;

/// Lift dark MSTS cab atlases — opt-in via `OPENRAILSRS_CAB_BRIGHTEN=1` (default off).
pub fn cab_ace_brighten_enabled() -> bool {
    matches!(
        std::env::var("OPENRAILSRS_CAB_BRIGHTEN").ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    )
}

pub fn brighten_cab_ace_rgba(rgba: &[u8]) -> (Vec<u8>, bool) {
    if !cab_ace_brighten_enabled() {
        return (rgba.to_vec(), false);
    }
    let mean = ace_mean_luma(rgba);
    if mean >= DARK_TEXTURE_LUMA_THRESHOLD {
        return (rgba.to_vec(), false);
    }
    let scale = (CAB_TEXTURE_TARGET_LUMA / mean.max(1.0)).min(SCENERY_TEXTURE_MAX_PIXEL_SCALE);
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        if px[3] < 8 {
            continue;
        }
        px[0] = scale_ace_channel(px[0], scale);
        px[1] = scale_ace_channel(px[1], scale);
        px[2] = scale_ace_channel(px[2], scale);
    }
    (out, true)
}

/// Cab / interior albedo multiplier (`OPENRAILSRS_CAB_ALBEDO`, default 1 — raw `.ace`).
pub fn cab_interior_albedo_boost() -> f32 {
    std::env::var("OPENRAILSRS_CAB_ALBEDO")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(1.0, 6.0)
}

pub fn cab_albedo_tint(pixel_brightened: bool) -> Color {
    let boost = cab_interior_albedo_boost();
    if pixel_brightened && cab_ace_brighten_enabled() {
        Color::linear_rgb(boost * 0.65, boost * 0.65, boost * 0.65)
    } else {
        Color::linear_rgb(boost, boost, boost)
    }
}

/// Finalise a scenery material for the active lighting path.
///
/// - OR-style ([`or_lighting_enabled`], the default): keep the material lit so the directional
///   sun shades it and it receives shadows, matching Open Rails' `SceneryShader`.
/// - Legacy (unlit, opt-in via `OPENRAILSRS_UNLIT_SCENERY=1`): MSTS `.ace` albedo is authored for
///   fixed-function drawing; drawn `unlit` with a brightness boost, never receiving shadows.
pub fn finalize_scenery_material(mut base: StandardMaterial, lit: bool) -> StandardMaterial {
    if lit {
        base.unlit = false;
        base.fog_enabled = true;
    } else {
        base.unlit = true;
        base.fog_enabled = false;
    }
    base
}

/// Apply MSTS `prim_state.z_buf_mode` to a Bevy material (best-effort on 0.19).
///
/// Bevy `StandardMaterial` has no direct depth-write toggle; we nudge `depth_bias`
/// for the common MSTS read-only-depth case (mode 1).
pub fn apply_z_buf_mode(mat: &mut StandardMaterial, z_buf_mode: i32) {
    match z_buf_mode {
        1 => mat.depth_bias += 0.001,
        2 => mat.depth_bias -= 0.001,
        _ => {}
    }
}

/// Open Rails dual-pass cutoffs for BlendATex* (`ReferenceAlpha` 250 then 10).
pub const OR_BLEND_PASS_OPAQUE_CUTOFF: f32 = 250.0 / 255.0;
pub const OR_BLEND_PASS_TRANS_CUTOFF: f32 = 10.0 / 255.0;

/// One draw for Open Rails BlendATex* dual-pass (#101).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlendAlphaPass {
    pub alpha_mode: AlphaMode,
    /// True for the nearly-opaque pass (depth write via Mask).
    pub depth_write_pass: bool,
}

/// Determine the Bevy [`AlphaMode`] for a texture+shader combination.
///
/// Priority order:
/// 1. `prim_state.alpha_test_mode` when explicitly set (0 = opaque, 1 = test, 2 = blend).
/// 2. Texture pixel analysis (semi-transparent pixels → blend, alpha-only → mask).
/// 3. Shader name / texture name heuristics.
///
/// For BlendATex* with semi-transparent texels, prefer [`blend_alpha_passes_from_prim_state`]
/// so callers can spawn Open Rails' dual draw (Mask 250 + Blend).
pub fn alpha_mode_from_prim_state(
    ace: &AceFile,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> AlphaMode {
    blend_alpha_passes_from_prim_state(ace, texture_file, shader_name, alpha_test_mode)[0]
        .alpha_mode
}

/// Open Rails dual-pass for BlendATex / BlendATexDiff (#101).
///
/// Returns two passes when the shader is blend-capable and the ACE has
/// semi-transparent texels: (1) `Mask(250/255)` depth-write, (2) `Blend`
/// for the soft edges. Otherwise a single pass.
pub fn blend_alpha_passes_from_prim_state(
    ace: &AceFile,
    texture_file: &str,
    shader_name: Option<&str>,
    alpha_test_mode: i32,
) -> Vec<BlendAlphaPass> {
    let blend_shader = shader_name
        .map(shape_shader_requests_blending)
        .unwrap_or(false);

    // Honour explicit prim_state flags, except alpha-test on blend-capable shaders:
    // OR still runs BlendATexDiff via dual-pass blend (ReferenceAlpha 250/10), not cutout.
    let single = match alpha_test_mode {
        0 if !blend_shader => AlphaMode::Opaque,
        1 if !blend_shader => AlphaMode::Mask(OR_MSTS_ALPHA_TEST_CUTOFF),
        2 => AlphaMode::Blend,
        _ => shape_alpha_mode(ace, texture_file, shader_name),
    };

    if blend_shader && matches!(single, AlphaMode::Blend) {
        let stats = shape_alpha_stats(ace);
        if stats.has_semitransparent || texture_name_suggests_transparency(texture_file) {
            return vec![
                BlendAlphaPass {
                    alpha_mode: AlphaMode::Mask(OR_BLEND_PASS_OPAQUE_CUTOFF),
                    depth_write_pass: true,
                },
                BlendAlphaPass {
                    alpha_mode: AlphaMode::Blend,
                    depth_write_pass: false,
                },
            ];
        }
    }

    vec![BlendAlphaPass {
        alpha_mode: single,
        depth_write_pass: matches!(single, AlphaMode::Opaque | AlphaMode::Mask(_)),
    }]
}

pub fn shape_alpha_mode(ace: &AceFile, texture_file: &str, shader_name: Option<&str>) -> AlphaMode {
    let blend_shader = shader_name
        .map(shape_shader_requests_blending)
        .unwrap_or(false);

    // Open Rails: Tex/TexDiff only alpha-test when `prim_state` requests it (see Shapes.cs).
    if !blend_shader {
        return AlphaMode::Opaque;
    }

    let alpha = shape_alpha_stats(ace);
    if !alpha.has_any {
        return AlphaMode::Opaque;
    }

    if alpha.has_semitransparent || texture_name_suggests_transparency(texture_file) {
        AlphaMode::Blend
    } else {
        // BlendATex on solid panels (bp01, wheels): OR draws them opaque unless alpha-tested.
        AlphaMode::Opaque
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShapeAlphaStats {
    has_any: bool,
    has_semitransparent: bool,
}

pub(crate) fn shape_alpha_stats(ace: &AceFile) -> ShapeAlphaStats {
    let mut stats = ShapeAlphaStats {
        has_any: ace.has_mask_channel,
        has_semitransparent: false,
    };
    for rgba in ace.mip0.chunks_exact(4) {
        let a = rgba[3];
        if a < 250 {
            stats.has_any = true;
        }
        if (9..248).contains(&a) {
            stats.has_semitransparent = true;
        }
    }
    stats
}

pub fn texture_name_suggests_transparency(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    ["glass", "window", "alpha", "trans", "transp"]
        .iter()
        .any(|needle| lower.contains(needle))
}

pub fn shape_shader_requests_blending(shader_name: &str) -> bool {
    matches!(
        shader_name,
        "BlendATex" | "BlendATexDiff" | "AddATex" | "AddATexDiff"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_ace::AceFile;

    fn ace_with_semitransparent() -> AceFile {
        // 2×2 RGBA with mid-alpha texels.
        let mut mip0 = Vec::new();
        for a in [255u8, 128, 64, 255] {
            mip0.extend_from_slice(&[200, 200, 200, a]);
        }
        AceFile {
            width: 2,
            height: 2,
            format: openrailsrs_ace::AceFormat::Rgba8,
            mips_count: 1,
            mip0,
            mips: Vec::new(),
            has_mask_channel: false,
            alpha_bits: 8,
        }
    }

    #[test]
    fn blend_atex_diff_dual_pass_when_semitransparent() {
        let ace = ace_with_semitransparent();
        let passes =
            blend_alpha_passes_from_prim_state(&ace, "glass.ace", Some("BlendATexDiff"), -1);
        assert_eq!(passes.len(), 2);
        assert!(matches!(
            passes[0].alpha_mode,
            AlphaMode::Mask(c) if (c - OR_BLEND_PASS_OPAQUE_CUTOFF).abs() < 1e-5
        ));
        assert!(matches!(passes[1].alpha_mode, AlphaMode::Blend));
        assert!(passes[0].depth_write_pass);
        assert!(!passes[1].depth_write_pass);
    }

    #[test]
    fn texdiff_stays_single_opaque_pass() {
        let ace = ace_with_semitransparent();
        let passes = blend_alpha_passes_from_prim_state(&ace, "body.ace", Some("TexDiff"), -1);
        assert_eq!(passes.len(), 1);
        assert!(matches!(passes[0].alpha_mode, AlphaMode::Opaque));
    }
}
