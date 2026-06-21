//! MSTS / Open Rails shader names → OR scenery kinds (cab + shapes).
//!
//! Paridad con `Shapes.cs` (`ShaderNames`, `VertexLightModeMap`) en Open Rails.

/// Open Rails `SceneryMaterial.SetState` alpha test when `alphatestmode == 1` (`ReferenceAlpha = 200`).
pub const OR_MSTS_ALPHA_TEST_CUTOFF: f32 = 200.0 / 255.0;

/// OR solid opaque materials force full alpha (`ReferenceAlpha = -1` in `SceneryShader.fx`).
pub const OR_OPAQUE_REFERENCE_ALPHA: f32 = -1.0;

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

/// OR `VertexLightModeMap[12 + LightMatIdx]` (`Shapes.cs`).
pub fn or_light_mat_kind(light_mat_idx: i32) -> Option<OrShaderKind> {
    match 12 + light_mat_idx {
        0 => Some(OrShaderKind::DarkShade),
        1 => Some(OrShaderKind::HalfBright),
        4 => Some(OrShaderKind::FullBright),
        5 => Some(OrShaderKind::Specular750),
        6 => Some(OrShaderKind::Specular25),
        _ => None,
    }
}

/// Effective OR material kind: shader name + vertex light model (Open Rails `SceneryMaterialOptions`).
pub fn resolve_or_material_kind(
    shader_name: Option<&str>,
    light_mat_idx: Option<i32>,
) -> OrShaderKind {
    // OR `ShaderNames` table — `Tex` / additive / blend-atlas use FullBright stage.
    let from_shader = match shader_name.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("tex") => OrShaderKind::FullBright,
        Some(s) if s.starts_with("blendatex") || s.starts_with("addatex") => {
            OrShaderKind::FullBright
        }
        _ => classify_or_shader(shader_name),
    };

    let Some(lm) = light_mat_idx else {
        return from_shader;
    };
    let Some(lit) = or_light_mat_kind(lm) else {
        return from_shader;
    };

    match lit {
        OrShaderKind::FullBright | OrShaderKind::HalfBright | OrShaderKind::DarkShade => lit,
        OrShaderKind::Specular750 | OrShaderKind::Specular25 => lit,
        _ => from_shader,
    }
}

pub fn or_shader_kind_gpu_id(kind: OrShaderKind) -> f32 {
    match kind {
        OrShaderKind::Tex
        | OrShaderKind::Unknown
        | OrShaderKind::AddATex
        | OrShaderKind::BlendATex => 0.0,
        OrShaderKind::TexDiff => 1.0,
        OrShaderKind::HalfBright => 2.0,
        OrShaderKind::DarkShade | OrShaderKind::Dark => 3.0,
        OrShaderKind::FullBright | OrShaderKind::Bright => 4.0,
        OrShaderKind::Specular25 => 5.0,
        OrShaderKind::Specular750 => 6.0,
        OrShaderKind::Specular => 7.0,
    }
}

/// GPU kind for cab interior — `Tex` is FullBright in OR (`ShaderNames`).
pub fn or_cab_shader_kind_gpu_id(kind: OrShaderKind) -> f32 {
    match kind {
        OrShaderKind::Tex | OrShaderKind::FullBright | OrShaderKind::Bright => 4.0,
        other => or_shader_kind_gpu_id(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cab_shaders() {
        assert_eq!(
            classify_or_shader(Some("HalfBright")),
            OrShaderKind::HalfBright
        );
        assert_eq!(classify_or_shader(Some("TexDiff")), OrShaderKind::TexDiff);
        assert_eq!(
            classify_or_shader(Some("FullBright")),
            OrShaderKind::FullBright
        );
    }

    #[test]
    fn blendatexdiff_resolves_fullbright_for_cab() {
        assert_eq!(
            resolve_or_material_kind(Some("BlendATexDiff"), None),
            OrShaderKind::FullBright
        );
    }

    #[test]
    fn or_tex_shader_resolves_fullbright() {
        assert_eq!(
            resolve_or_material_kind(Some("Tex"), None),
            OrShaderKind::FullBright
        );
        assert_eq!(
            or_cab_shader_kind_gpu_id(resolve_or_material_kind(Some("Tex"), None)),
            4.0
        );
    }

    #[test]
    fn light_mat_fullbright_overrides_texdiff() {
        assert_eq!(
            resolve_or_material_kind(Some("TexDiff"), Some(-8)),
            OrShaderKind::FullBright
        );
    }
}
