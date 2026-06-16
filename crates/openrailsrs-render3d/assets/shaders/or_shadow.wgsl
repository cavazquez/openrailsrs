// Open Rails SceneryShader.fx — _PSGetShadowEffect (VSM Chebyshev).
#define_import_path or_shadow

const OR_VSM_DEPTH_EPS: f32 = 0.00005;
const OR_VSM_VARIANCE_MIN: f32 = 0.00005;
const OR_VSM_CHEBYSHEV_POWER: f32 = 50.0;

/// Momentos OR: x = E[z], y = E[z^2], z = profundidad del receptor en espacio luz.
fn or_ps_chebyshev_from_moments(moments: vec3<f32>) -> f32 {
    let not_shadowed = moments.z - moments.x < OR_VSM_DEPTH_EPS;
    let ex = moments.x;
    let ex2 = moments.y;
    let variance = clamp(ex2 - ex * ex, OR_VSM_VARIANCE_MIN, 1.0);
    let m_d = moments.z - ex;
    let p = pow(variance / (variance + m_d * m_d), OR_VSM_CHEBYSHEV_POWER);
    return saturate(select(0.0, 1.0, not_shadowed) + p);
}

/// _PSGetShadowEffect(..., NormalLighting=true) de SceneryShader.fx.
fn or_ps_get_shadow_effect(normal_light: f32, moments: vec3<f32>) -> f32 {
    let vsm = or_ps_chebyshev_from_moments(moments);
    return vsm * saturate(normal_light * 5.0 - 2.0);
}

/// Fallback cuando solo hay PCF 0..1 (Bevy): tratar sombra filtrada como E[z] aproximado.
fn or_ps_get_shadow_effect_from_pcf(normal_light: f32, pcf_shadow: f32) -> f32 {
    let ex = pcf_shadow;
    let ex2 = ex * ex + OR_VSM_VARIANCE_MIN;
    let receiver = mix(0.0, 1.0, pcf_shadow);
    return or_ps_get_shadow_effect(normal_light, vec3(ex, ex2, receiver));
}
