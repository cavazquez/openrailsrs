//! CABVIEW3D diagnostics — per-part logs and debug shader views (`OPENRAILSRS_CAB_DEBUG`).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_ace::AceFile;

use crate::or_cab_material::OrCabMaterial;
use crate::or_cab_material::cab_interior_sun_enabled;
use crate::shapes::{
    DARK_TEXTURE_LUMA_THRESHOLD, MeshVertexColorMode, ShapePartAsset, ShapeRenderAsset,
    cab_ace_brighten_enabled, cab_interior_albedo_boost, mesh_aabb, mesh_has_uvs, mesh_uv_aabb,
    mesh_uv_degenerate, mesh_vertex_color_stats, resolve_texture_path_in_dirs,
    shader_uses_vertex_color_multiply,
};
use crate::viewer_log;

/// Debug fragment path for [`or_cab.wgsl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CabDebugView {
    #[default]
    Off,
    /// `fract(uv)` → red/green (flat UV = solid color).
    Uv,
    /// Raw `textureSample × tint` (no lighting).
    Albedo,
    /// Per-vertex `ATTRIBUTE_COLOR` (no texture/lighting).
    VertexColor,
}

impl CabDebugView {
    pub fn shader_def(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Uv => Some("OR_CAB_DEBUG_UV"),
            Self::Albedo => Some("OR_CAB_DEBUG_ALBEDO"),
            Self::VertexColor => Some("OR_CAB_DEBUG_VCOLOR"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Uv => "uv",
            Self::Albedo => "albedo",
            Self::VertexColor => "vcolor",
        }
    }
}

/// `OPENRAILSRS_CAB_DEBUG`: `uv` | `albedo` | `1`/`on` (albedo).
pub fn cab_debug_view() -> CabDebugView {
    match std::env::var("OPENRAILSRS_CAB_DEBUG")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        None => CabDebugView::Off,
        Some("") => CabDebugView::Off,
        Some(s) if s.eq_ignore_ascii_case("uv") || s.eq_ignore_ascii_case("uvs") => {
            CabDebugView::Uv
        }
        Some(s)
            if s.eq_ignore_ascii_case("albedo")
                || s.eq_ignore_ascii_case("tex")
                || s.eq_ignore_ascii_case("texture")
                || s == "1"
                || s.eq_ignore_ascii_case("on") =>
        {
            CabDebugView::Albedo
        }
        Some(s)
            if s.eq_ignore_ascii_case("vcolor")
                || s.eq_ignore_ascii_case("vc")
                || s.eq_ignore_ascii_case("vertex")
                || s.eq_ignore_ascii_case("vertexcolor") =>
        {
            CabDebugView::VertexColor
        }
        Some(other) => {
            crate::viewer_log!(
                "openrailsrs-viewer3d: cab debug — unknown OPENRAILSRS_CAB_DEBUG={other:?}, use uv|albedo|vcolor"
            );
            CabDebugView::Off
        }
    }
}

/// Log once when driver cab spawns (39 parts max — OK at spawn).
#[allow(clippy::too_many_arguments)]
pub fn log_cab_interior_part_diagnostics(
    cab_shape: &Path,
    texture_dirs: &[PathBuf],
    asset: &ShapeRenderAsset,
    meshes: &Assets<Mesh>,
    materials: &Assets<StandardMaterial>,
    or_materials: &Assets<OrCabMaterial>,
) {
    let tex_refs: Vec<&Path> = texture_dirs.iter().map(|p| p.as_path()).collect();
    let debug = cab_debug_view();
    viewer_log!(
        "openrailsrs-viewer3d: cab diag — shape {} parts={} debug={}",
        cab_shape.display(),
        asset.parts.len(),
        debug.label(),
    );
    if debug != CabDebugView::Off {
        viewer_log!(
            "openrailsrs-viewer3d: cab diag — OPENRAILSRS_CAB_DEBUG={} (uv=fract(uv), albedo=texture×tint, vcolor=vertex colour)",
            debug.label(),
        );
    }
    viewer_log!(
        "openrailsrs-viewer3d: cab diag — env albedo={:.2} brighten={} sun={} min_bright={:.2} brighten_if_luma<{:.0} or_shader={}",
        cab_interior_albedo_boost(),
        cab_ace_brighten_enabled(),
        cab_interior_sun_enabled(),
        cab_min_brightness_env(),
        DARK_TEXTURE_LUMA_THRESHOLD,
        crate::or_cab_material::or_cab_shaders_enabled(),
    );

    for (pi, part) in asset.parts.iter().enumerate() {
        log_one_cab_part(pi, part, &tex_refs, meshes, materials, or_materials);
    }
}

#[allow(clippy::too_many_arguments)]
fn log_one_cab_part(
    pi: usize,
    part: &ShapePartAsset,
    texture_dirs: &[&Path],
    meshes: &Assets<Mesh>,
    materials: &Assets<StandardMaterial>,
    or_materials: &Assets<OrCabMaterial>,
) {
    let mesh = meshes.get(&part.mesh);
    let (uv_min, uv_max, uv_deg) = mesh
        .and_then(mesh_uv_aabb)
        .map(|(mn, mx)| (Some(mn), Some(mx), mesh_uv_degenerate(mn, mx)))
        .unwrap_or((None, None, true));
    let no_uv = mesh.is_some_and(|m| !mesh_has_uvs(m));
    let extent = mesh.and_then(mesh_aabb).map(|(mn, mx)| mx - mn);

    let vtx_mult = shader_uses_vertex_color_multiply(part.shader_name.as_deref());
    let solid = part
        .solid_color
        .map(|c| format!("({:.3},{:.3},{:.3})", c[0], c[1], c[2]))
        .unwrap_or_else(|| "none".into());
    let vtx_line = mesh
        .map(mesh_vertex_color_stats)
        .map(format_vertex_color_stats)
        .unwrap_or_else(|| "mesh?".into());

    let mat_kind = if part.or_cab_material.is_some() {
        part.or_cab_material
            .as_ref()
            .and_then(|h| or_materials.get(h))
            .map(|m| {
                let p = &m.params;
                format!(
                    "OR kind={:.0} alpha={:?} tint=({:.3},{:.3},{:.3},{:.3}) flags={:.0} min_b={:.2} sh={:.2} fb={:.2}",
                    p.shader_kind,
                    m.alpha_mode,
                    p.tint_r,
                    p.tint_g,
                    p.tint_b,
                    p.tint_a,
                    p.flags,
                    p.cab_min_brightness,
                    p.shadow_brightness,
                    p.full_brightness,
                )
            })
            .unwrap_or_else(|| "OR (missing asset)".into())
    } else {
        materials
            .get(&part.material)
            .map(|m| {
                format!(
                    "Std unlit={} tex={} alpha={:?}",
                    m.unlit,
                    m.base_color_texture.is_some(),
                    m.alpha_mode
                )
            })
            .unwrap_or_else(|| "Std (missing)".into())
    };

    let tex = part.texture_name.as_deref().unwrap_or("-");
    let shader = part.shader_name.as_deref().unwrap_or("-");
    let light_mat = part
        .light_mat_idx
        .map(|i| i.to_string())
        .unwrap_or_else(|| "-".into());
    let ace_line = part
        .texture_name
        .as_deref()
        .and_then(|name| resolve_texture_path_in_dirs(texture_dirs, name))
        .map(|p| {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dds")) {
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                format!("dds {name} alpha={:?}", crate::shapes::dds_alpha_type(&p))
            } else {
                openrailsrs_ace::read_ace(&p)
                    .ok()
                    .map(|ace| ace_diag_line(&ace))
                    .unwrap_or_else(|| "ace: decode fail".into())
            }
        })
        .unwrap_or_else(|| "tex: n/a".into());

    viewer_log!(
        "openrailsrs-viewer3d: cab diag part {pi:2} prim={:3} tex={tex} shader={shader} light_mat={light_mat} | has_tex={} or={} transp={} | has_uv={} range={uv_min:?}..{uv_max:?} deg={uv_deg} ext={extent:?}",
        part.prim_state_idx,
        part.has_texture,
        part.or_cab_material.is_some(),
        part.is_transparent,
        !no_uv,
    );
    viewer_log!(
        "openrailsrs-viewer3d: cab diag part {pi:2}   vtx_mult={vtx_mult} solid={solid} {vtx_line} | {mat_kind} | {ace_line}",
    );
}

fn cab_min_brightness_env() -> f32 {
    std::env::var("OPENRAILSRS_CAB_MIN_BRIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            if cab_interior_sun_enabled() {
                0.72_f32
            } else {
                0.0_f32
            }
        })
        .clamp(0.0, 1.0)
}

fn format_vertex_color_stats(stats: crate::shapes::MeshVertexColorStats) -> String {
    match stats.mode {
        MeshVertexColorMode::None => "vtx=none".into(),
        MeshVertexColorMode::Uniform => format!(
            "vtx=uniform n={} rgb=({:.3},{:.3},{:.3})",
            stats.count, stats.min.x, stats.min.y, stats.min.z
        ),
        MeshVertexColorMode::Varying => format!(
            "vtx=VARYING n={} rgb_min=({:.3},{:.3},{:.3}) rgb_max=({:.3},{:.3},{:.3}) span={:.3}",
            stats.count,
            stats.min.x,
            stats.min.y,
            stats.min.z,
            stats.max.x,
            stats.max.y,
            stats.max.z,
            (stats.max - stats.min).length(),
        ),
    }
}

fn ace_diag_line(ace: &AceFile) -> String {
    let (mean_luma, mean_r, mean_g, mean_b, min_a, max_a, opaque_pct) = ace_sample_stats(ace);
    let would_brighten = mean_luma < DARK_TEXTURE_LUMA_THRESHOLD;
    format!(
        "ace {}x{} rgb=({mean_r:.0},{mean_g:.0},{mean_b:.0}) luma={mean_luma:.0} brighten={would_brighten} a=[{min_a},{max_a}] opaque={opaque_pct:.0}%",
        ace.width, ace.height,
    )
}

fn ace_sample_stats(ace: &AceFile) -> (f32, f32, f32, f32, u8, u8, f32) {
    let mut luma_sum = 0u64;
    let mut r_sum = 0u64;
    let mut g_sum = 0u64;
    let mut b_sum = 0u64;
    let mut count = 0u64;
    let mut min_a = 255u8;
    let mut max_a = 0u8;
    let mut opaque = 0u64;
    for px in ace.mip0.chunks_exact(4) {
        if px[3] < 8 {
            continue;
        }
        r_sum += px[0] as u64;
        g_sum += px[1] as u64;
        b_sum += px[2] as u64;
        luma_sum += (px[0] as u64 + px[1] as u64 + px[2] as u64) / 3;
        count += 1;
        min_a = min_a.min(px[3]);
        max_a = max_a.max(px[3]);
        if px[3] >= 250 {
            opaque += 1;
        }
    }
    if count == 0 {
        return (0.0, 0.0, 0.0, 0.0, 0, 0, 0.0);
    }
    let n = count as f32;
    (
        luma_sum as f32 / n,
        r_sum as f32 / n,
        g_sum as f32 / n,
        b_sum as f32 / n,
        min_a,
        max_a,
        100.0 * opaque as f32 / n,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cab_debug_parses_uv_and_albedo() {
        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_DEBUG", "uv");
        }
        assert_eq!(cab_debug_view(), CabDebugView::Uv);
        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_DEBUG", "albedo");
        }
        assert_eq!(cab_debug_view(), CabDebugView::Albedo);
        unsafe {
            std::env::set_var("OPENRAILSRS_CAB_DEBUG", "vcolor");
        }
        assert_eq!(cab_debug_view(), CabDebugView::VertexColor);
        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_DEBUG");
        }
        assert_eq!(cab_debug_view(), CabDebugView::Off);
    }
}
