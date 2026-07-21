//! Stable terrain material cache keys and overlay UV scale (#121 / #122).

use openrailsrs_formats::TerrainShader;

use crate::materials::or_terrain::{DEFAULT_MICROTEX, overlay_scale_from_uvcalc};

/// Overlay UV scale from `terrain_uvcalcs[1].d` (OR default 32).
pub fn terrain_shader_overlay_scale(shader: &TerrainShader) -> f32 {
    shader
        .uvcalcs
        .get(1)
        .map(|c| overlay_scale_from_uvcalc(c.d))
        .unwrap_or(32.0)
}

/// Stable key for sharing terrain materials across patches/tiles (texture + UV only).
///
/// Pipeline flags (`lit` / `night` / VSM) are appended by apps via
/// [`terrain_material_cache_key`] when they affect the GPU material.
pub fn terrain_shader_material_key(shader: &TerrainShader) -> String {
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
    terrain_material_cache_key(
        base_name,
        overlay_name,
        terrain_shader_overlay_scale(shader),
        None,
    )
}

/// Build a material cache key from texture names + overlay scale (+ optional pipeline flags).
pub fn terrain_material_cache_key(
    base_name: &str,
    overlay_name: &str,
    overlay_scale: f32,
    pipeline_flags: Option<&str>,
) -> String {
    match pipeline_flags {
        Some(flags) if !flags.is_empty() => {
            format!("{base_name}|{overlay_name}|{overlay_scale:.6}|{flags}")
        }
        _ => format!("{base_name}|{overlay_name}|{overlay_scale:.6}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{TerrainShader, TerrainUvCalc};

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
        assert!((terrain_shader_overlay_scale(&shader) - 32.0).abs() < 1e-3);
    }

    #[test]
    fn material_key_includes_optional_pipeline_flags() {
        let base = terrain_material_cache_key("a.ace", "b.ace", 32.0, None);
        assert_eq!(base, "a.ace|b.ace|32.000000");
        let flagged = terrain_material_cache_key(
            "a.ace",
            "b.ace",
            32.0,
            Some("lit=true|night=false|or=true"),
        );
        assert!(flagged.ends_with("|lit=true|night=false|or=true"));
    }
}
