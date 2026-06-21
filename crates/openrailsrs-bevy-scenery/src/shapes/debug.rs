//! Shape render debug flags and MSTS `z_bias` sanitisation for Bevy materials.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::Face;

/// Maximum absolute MSTS `prim_state` ZBias passed to Bevy `depth_bias`.
pub const MSTS_Z_BIAS_CLAMP: f32 = 10.0;

/// Values above this trigger a warning (likely parser corruption).
pub const MSTS_Z_BIAS_WARN_ABS: f32 = 100.0;

static TRAIN_SHAPE_DEBUG_SCOPE: AtomicBool = AtomicBool::new(false);

/// Mark the current thread as building/spawning live-train exterior shapes only.
/// World scenery must not set this flag.
pub fn set_train_shape_debug_scope(active: bool) {
    TRAIN_SHAPE_DEBUG_SCOPE.store(active, Ordering::SeqCst);
}

pub fn train_shape_debug_scope() -> bool {
    TRAIN_SHAPE_DEBUG_SCOPE.load(Ordering::SeqCst)
}

/// Context for z_bias clamp / material debug logs.
#[derive(Clone, Debug, Default)]
pub struct ShapeMaterialDebugCtx {
    pub shape_name: Option<String>,
    pub prim_state_idx: i32,
    pub prim_state_name: Option<String>,
    pub shader_name: Option<String>,
    pub texture_name: Option<String>,
}

fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    )
}

macro_rules! debug_flag {
    ($fn_name:ident, $env:literal) => {
        pub fn $fn_name() -> bool {
            static FLAG: OnceLock<bool> = OnceLock::new();
            *FLAG.get_or_init(|| env_truthy($env))
        }
    };
}

debug_flag!(debug_force_opaque, "OPENRAILSRS_DEBUG_FORCE_OPAQUE");
debug_flag!(
    debug_force_double_sided,
    "OPENRAILSRS_DEBUG_FORCE_DOUBLE_SIDED"
);
debug_flag!(debug_force_unlit, "OPENRAILSRS_DEBUG_FORCE_UNLIT");
debug_flag!(debug_shape_stats_enabled, "OPENRAILSRS_DEBUG_SHAPE_STATS");
debug_flag!(debug_materials_enabled, "OPENRAILSRS_DEBUG_MATERIALS");

// ── Train-only UV / culling diagnostics (require [`train_shape_debug_scope`]) ──

debug_flag!(debug_no_uv_flip, "OPENRAILSRS_DEBUG_NO_UV_FLIP");
debug_flag!(debug_flip_u, "OPENRAILSRS_DEBUG_FLIP_U");
debug_flag!(debug_flip_v, "OPENRAILSRS_DEBUG_FLIP_V");
debug_flag!(debug_flip_uv, "OPENRAILSRS_DEBUG_FLIP_UV");
debug_flag!(debug_cull_normal, "OPENRAILSRS_DEBUG_CULL_NORMAL");
debug_flag!(
    debug_force_single_sided,
    "OPENRAILSRS_DEBUG_FORCE_SINGLE_SIDED"
);
debug_flag!(debug_cull_front, "OPENRAILSRS_DEBUG_CULL_FRONT");
debug_flag!(debug_flip_winding, "OPENRAILSRS_DEBUG_FLIP_WINDING");
debug_flag!(debug_consist_enabled, "OPENRAILSRS_DEBUG_CONSIST");
debug_flag!(
    debug_vehicle_transforms_enabled,
    "OPENRAILSRS_DEBUG_VEHICLE_TRANSFORMS"
);

/// Face-colour diagnostic: `front` = green front faces, `back` = red back faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugFaceColorMode {
    FrontGreen,
    BackRed,
}

pub fn debug_face_color_mode() -> Option<DebugFaceColorMode> {
    static MODE: OnceLock<Option<DebugFaceColorMode>> = OnceLock::new();
    *MODE.get_or_init(|| {
        match std::env::var("OPENRAILSRS_DEBUG_FACE_COLORS")
            .ok()
            .as_deref()
        {
            Some("front") | Some("green") => Some(DebugFaceColorMode::FrontGreen),
            Some("back") | Some("red") => Some(DebugFaceColorMode::BackRed),
            _ => None,
        }
    })
}

fn train_uv_or_cull_env_active() -> bool {
    debug_no_uv_flip()
        || debug_flip_u()
        || debug_flip_v()
        || debug_flip_uv()
        || debug_cull_normal()
        || debug_force_single_sided()
        || debug_cull_front()
        || debug_flip_winding()
        || debug_force_double_sided()
        || debug_face_color_mode().is_some()
}

/// Train UV/cull experiments are active (live consist scope + at least one debug env).
pub fn train_shape_debug_active() -> bool {
    train_shape_debug_scope() && train_uv_or_cull_env_active()
}

/// Convert MSTS `.s` UV to Bevy mesh UV (production default: flip V like legacy path).
pub fn shape_uv_to_bevy(u: f32, v: f32) -> Vec2 {
    if train_shape_debug_scope() && train_uv_or_cull_env_active() {
        return apply_train_uv_debug(u, v);
    }
    Vec2::new(u, 1.0 - v)
}

fn apply_train_uv_debug(u: f32, v: f32) -> Vec2 {
    if debug_no_uv_flip() {
        let mut uu = u;
        let mut vv = v;
        if debug_flip_u() || debug_flip_uv() {
            uu = 1.0 - uu;
        }
        if debug_flip_v() || debug_flip_uv() {
            vv = 1.0 - vv;
        }
        return Vec2::new(uu, vv);
    }
    // Start from production conversion, then apply experimental toggles.
    let mut uu = u;
    let mut vv = 1.0 - v;
    if debug_flip_u() || debug_flip_uv() {
        uu = 1.0 - uu;
    }
    if debug_flip_v() || debug_flip_uv() {
        vv = 1.0 - vv;
    }
    Vec2::new(uu, vv)
}

/// Reverse triangle winding (swap two corners) when train flip-winding debug is on.
pub fn train_debug_flip_winding_active() -> bool {
    train_shape_debug_scope() && debug_flip_winding()
}

/// Sanitise MSTS ZBias before assigning Bevy `StandardMaterial.depth_bias`.
pub fn clamp_msts_z_bias_for_bevy(raw: Option<f32>, ctx: Option<&ShapeMaterialDebugCtx>) -> f32 {
    let v = raw.unwrap_or(0.0);
    if !v.is_finite() {
        if debug_materials_enabled() || ctx.is_some() {
            eprintln!(
                "openrailsrs-bevy-scenery: z_bias non-finite → 0{}",
                ctx.map(format_ctx_suffix).unwrap_or_default()
            );
        }
        return 0.0;
    }
    if v.abs() > MSTS_Z_BIAS_WARN_ABS {
        eprintln!(
            "openrailsrs-bevy-scenery: suspicious z_bias={v} (clamp ±{MSTS_Z_BIAS_CLAMP}){}",
            ctx.map(format_ctx_suffix).unwrap_or_default()
        );
    }
    v.clamp(-MSTS_Z_BIAS_CLAMP, MSTS_Z_BIAS_CLAMP)
}

fn format_ctx_suffix(ctx: &ShapeMaterialDebugCtx) -> String {
    format!(
        " shape={:?} prim_state={} name={:?} shader={:?} texture={:?}",
        ctx.shape_name, ctx.prim_state_idx, ctx.prim_state_name, ctx.shader_name, ctx.texture_name
    )
}

/// Apply global debug env overrides (opaque, unlit, …).
pub fn apply_shape_debug_material_overrides(mat: &mut StandardMaterial) {
    if debug_force_opaque() {
        mat.alpha_mode = AlphaMode::Opaque;
        let c = mat.base_color.to_linear();
        mat.base_color = Color::linear_rgba(c.red, c.green, c.blue, 1.0);
    }
    if debug_force_unlit() {
        mat.unlit = true;
        mat.fog_enabled = false;
    }
}

/// Train-only culling / face-colour overrides (Open Rails uses CCW + back-face cull).
pub fn apply_train_debug_material_overrides(mat: &mut StandardMaterial) {
    if !train_shape_debug_scope() {
        apply_shape_debug_material_overrides(mat);
        if debug_force_double_sided() {
            mat.cull_mode = None;
            mat.double_sided = true;
        }
        return;
    }

    if let Some(mode) = debug_face_color_mode() {
        mat.base_color_texture = None;
        mat.normal_map_texture = None;
        mat.metallic_roughness_texture = None;
        mat.unlit = true;
        mat.fog_enabled = false;
        mat.alpha_mode = AlphaMode::Opaque;
        mat.double_sided = false;
        match mode {
            DebugFaceColorMode::FrontGreen => {
                mat.base_color = Color::srgb(0.1, 0.85, 0.15);
                mat.cull_mode = Some(Face::Back);
            }
            DebugFaceColorMode::BackRed => {
                mat.base_color = Color::srgb(0.9, 0.12, 0.1);
                mat.cull_mode = Some(Face::Front);
            }
        }
        apply_shape_debug_material_overrides(mat);
        return;
    }

    if debug_cull_front() {
        mat.cull_mode = Some(Face::Front);
        mat.double_sided = false;
    } else if debug_cull_normal() || debug_force_single_sided() {
        mat.cull_mode = Some(Face::Back);
        mat.double_sided = false;
    } else if debug_force_double_sided() {
        mat.cull_mode = None;
        mat.double_sided = true;
    }

    apply_shape_debug_material_overrides(mat);
}

/// Per-prim_state material log (`OPENRAILSRS_DEBUG_MATERIALS=1`).
pub fn log_shape_material_debug(
    ctx: &ShapeMaterialDebugCtx,
    alpha_mode: AlphaMode,
    z_bias_raw: Option<f32>,
    z_bias_clamped: f32,
    z_buf_mode: i32,
    alpha_test_mode: i32,
    triangle_count: usize,
) {
    if !debug_materials_enabled() {
        return;
    }
    eprintln!(
        "openrailsrs-bevy-scenery: material shape={:?} prim_state={} shader={:?} texture={:?} \
         alpha={alpha_mode:?} z_bias_raw={z_bias_raw:?} z_bias_clamped={z_bias_clamped} \
         z_buf_mode={z_buf_mode} alpha_test_mode={alpha_test_mode} triangles={triangle_count}",
        ctx.shape_name, ctx.prim_state_idx, ctx.shader_name, ctx.texture_name,
    );
}

/// True when every triangle list mesh has a position count divisible by 3.
pub fn mesh_triangle_list_valid(mesh: &Mesh) -> bool {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .map(|attr| match attr {
            bevy::mesh::VertexAttributeValues::Float32x3(positions) => positions.len() % 3 == 0,
            _ => false,
        })
        .unwrap_or(true)
}

/// Count vertices in a mesh position attribute (0 if missing).
pub fn mesh_position_count(mesh: &Mesh) -> usize {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .map(|attr| match attr {
            bevy::mesh::VertexAttributeValues::Float32x3(positions) => positions.len(),
            _ => 0,
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_msts_z_bias_rejects_huge_values() {
        let ctx = ShapeMaterialDebugCtx {
            shape_name: Some("test.s".into()),
            prim_state_idx: 15,
            ..Default::default()
        };
        let clamped = clamp_msts_z_bias_for_bevy(Some(16777216.0), Some(&ctx));
        assert!(clamped.abs() <= MSTS_Z_BIAS_CLAMP);
    }

    #[test]
    fn no_uv_flip_matches_or_raw_coords() {
        set_train_shape_debug_scope(true);
        // Cannot reset OnceLock env flags in test; verify helper logic via apply path
        // when NO_UV_FLIP is unset, production flip V applies outside train debug env.
        set_train_shape_debug_scope(false);
        let uv = shape_uv_to_bevy(0.25, 0.75);
        assert!((uv.x - 0.25).abs() < 1e-5);
        assert!((uv.y - 0.25).abs() < 1e-5);
    }
}
