//! Shaders MSTS / Open Rails -> parametros PBR aproximados (Bevy StandardMaterial).
//!
//! OR usa HLSL en `SceneryShader.fx` (HalfBright con sombra minima ~75%, Specular*, TexDiff...).
//! Bevy no tiene esos shaders; aproximamos con metallic/roughness/reflectance y un fill ambiente.

use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrShaderKind {
    Unknown,
    Tex,
    TexDiff,
    HalfBright,
    FullBright,
    Bright,
    Dark,
    DarkShade,
    Specular25,
    Specular750,
    Specular,
    AddATex,
    BlendATex,
}

#[derive(Debug, Clone, Copy)]
pub struct OrShaderPbr {
    pub metallic: f32,
    pub roughness: f32,
    pub reflectance: f32,
    pub albedo_scale: f32,
    /// Relleno de sombras (HalfBright OR: `HalfShadowBrightness = 0.75`).
    pub ambient_fill: LinearRgba,
    /// FullBright / Bright OR: sin sombreado direccional.
    pub force_unlit: bool,
}

impl OrShaderPbr {
    fn base(lit: bool, default_roughness: f32) -> Self {
        Self {
            metallic: 0.05,
            roughness: default_roughness,
            reflectance: 0.5,
            albedo_scale: 1.0,
            ambient_fill: LinearRgba::new(0.0, 0.0, 0.0, 1.0),
            force_unlit: !lit,
        }
    }
}

pub fn classify_or_shader(shader_name: Option<&str>) -> OrShaderKind {
    let Some(shader) = shader_name else {
        return OrShaderKind::Unknown;
    };
    let n = shader.to_ascii_lowercase();
    if n.contains("halfbright") {
        return OrShaderKind::HalfBright;
    }
    if n.contains("fullbright") {
        return OrShaderKind::FullBright;
    }
    if n == "bright" || (n.ends_with("bright") && !n.contains("half")) {
        return OrShaderKind::Bright;
    }
    if n.contains("darkshade") {
        return OrShaderKind::DarkShade;
    }
    if n.contains("dark") {
        return OrShaderKind::Dark;
    }
    if n.contains("specular750") || n.contains("specular_750") {
        return OrShaderKind::Specular750;
    }
    if n.contains("specular25") || n.contains("specular_25") {
        return OrShaderKind::Specular25;
    }
    if n.contains("specular") {
        return OrShaderKind::Specular;
    }
    if n.contains("addatex") {
        return OrShaderKind::AddATex;
    }
    if n.contains("blendatex") {
        return OrShaderKind::BlendATex;
    }
    if n.contains("texdiff") || n == "texdiff" {
        return OrShaderKind::TexDiff;
    }
    if n == "tex" {
        return OrShaderKind::Tex;
    }
    OrShaderKind::Unknown
}

fn texture_name_suggests_rail(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    lower.contains("rail")
        || lower.contains("ukfs_r")
        || (lower.starts_with("ukfs_") && lower.contains("head"))
}

fn texture_name_suggests_sleeper(texture_name: &str) -> bool {
    let lower = texture_name.to_ascii_lowercase();
    lower.contains("tie") || lower.contains("sleeper") || lower.contains("ukfs_t")
}

/// PBR + flags segun shader OR y nombre de textura (rieles UKFS, etc.).
pub fn resolve_or_material_pbr(
    texture_name: &str,
    shader_name: Option<&str>,
    lit: bool,
    default_roughness: f32,
) -> OrShaderPbr {
    let kind = classify_or_shader(shader_name);
    let mut hints = OrShaderPbr::base(lit, default_roughness);

    if !lit {
        return hints;
    }

    hints.force_unlit = false;

    // Rieles / metal UKFS (prioridad sobre shader generico).
    if texture_name_suggests_rail(texture_name) {
        hints.metallic = 0.82;
        hints.roughness = match kind {
            OrShaderKind::Specular750 | OrShaderKind::Specular => 0.22,
            OrShaderKind::Specular25 => 0.38,
            _ => 0.32,
        };
        hints.reflectance = 0.58;
        return hints;
    }
    if texture_name_suggests_sleeper(texture_name) {
        hints.metallic = 0.0;
        hints.roughness = 0.94;
        hints.reflectance = 0.35;
        return hints;
    }

    match kind {
        OrShaderKind::Tex | OrShaderKind::TexDiff | OrShaderKind::Unknown => {}
        OrShaderKind::HalfBright => {
            hints.roughness = 0.68;
            hints.reflectance = 0.62;
            hints.albedo_scale = 1.03;
            // Sombra no baja del ~75% (OR PSHalfBright).
            hints.ambient_fill = LinearRgba::new(0.11, 0.12, 0.14, 1.0);
        }
        OrShaderKind::FullBright | OrShaderKind::Bright => {
            hints.force_unlit = true;
            hints.albedo_scale = 1.08;
        }
        OrShaderKind::Dark | OrShaderKind::DarkShade => {
            hints.roughness = 0.92;
            hints.reflectance = 0.35;
            hints.albedo_scale = 0.92;
        }
        OrShaderKind::Specular25 => {
            hints.metallic = 0.35;
            hints.roughness = 0.48;
            hints.reflectance = 0.72;
        }
        OrShaderKind::Specular750 => {
            hints.metallic = 0.78;
            hints.roughness = 0.24;
            hints.reflectance = 0.78;
        }
        OrShaderKind::Specular => {
            hints.metallic = 0.55;
            hints.roughness = 0.35;
            hints.reflectance = 0.70;
        }
        OrShaderKind::AddATex | OrShaderKind::BlendATex => {}
    }

    let lower = texture_name.to_ascii_lowercase();
    if lower.starts_with("ukfs_") && kind == OrShaderKind::Unknown {
        hints.metallic = 0.12;
        hints.roughness = 0.78;
        hints.reflectance = 0.45;
    }

    hints
}

pub fn apply_albedo_scale(tint: Color, scale: f32) -> Color {
    if (scale - 1.0).abs() < 0.001 {
        return tint;
    }
    let c = tint.to_linear();
    Color::linear_rgba(
        (c.red * scale).min(1.5),
        (c.green * scale).min(1.5),
        (c.blue * scale).min(1.5),
        c.alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_halfbright_and_specular() {
        assert_eq!(
            classify_or_shader(Some("HalfBright")),
            OrShaderKind::HalfBright
        );
        assert_eq!(
            classify_or_shader(Some("Specular25")),
            OrShaderKind::Specular25
        );
        assert_eq!(classify_or_shader(Some("TexDiff")), OrShaderKind::TexDiff);
    }

    #[test]
    fn halfbright_adds_ambient_fill_when_lit() {
        let p = resolve_or_material_pbr("brick.ace", Some("HalfBright"), true, 0.85);
        assert!(p.ambient_fill.red > 0.0);
        assert!(!p.force_unlit);
    }

    #[test]
    fn fullbright_forces_unlit_in_day() {
        let p = resolve_or_material_pbr("signal.ace", Some("FullBright"), true, 0.85);
        assert!(p.force_unlit);
    }

    #[test]
    fn ukfs_rail_stays_metallic_with_texdiff() {
        let p = resolve_or_material_pbr("ukfs_rail.ace", Some("TexDiff"), true, 0.85);
        assert!(p.metallic > 0.7);
    }
}
