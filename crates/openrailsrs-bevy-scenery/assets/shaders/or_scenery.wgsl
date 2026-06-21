// Open Rails SceneryShader.fx + VSM (atlas Rg32 blurreado o depth lineal Bevy).
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    pbr_functions,
}

struct OrSceneryParams {
    tint_r: f32,
    tint_g: f32,
    tint_b: f32,
    tint_a: f32,
    reference_alpha: f32,
    shadow_brightness: f32,
    full_brightness: f32,
    half_shadow_brightness: f32,
    specular_strength: f32,
    specular_power: f32,
    night_color_modifier: f32,
    image_texture_is_night: f32,
    shader_kind: f32,
    flags: f32,
    vsm_mode: f32,
    shadow_map_limit_x: f32,
    shadow_map_limit_y: f32,
    shadow_map_limit_z: f32,
    shadow_map_limit_w: f32,
    debug_flags: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OrSceneryParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;

#ifdef OR_VSM_MOMENT_ATLAS
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var or_moment_atlas: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var or_moment_sampler: sampler;
#endif

const OR_KIND_HALF_BRIGHT: f32 = 2.0;
const OR_KIND_DARK_SHADE: f32 = 3.0;
const OR_KIND_FULL_BRIGHT: f32 = 4.0;
const OR_FLAG_LIT: f32 = 1.0;
const OR_VSM_MODE_PCF: f32 = 0.0;
const OR_VSM_MODE_APPROX: f32 = 1.0;
const OR_VSM_MODE_EXACT: f32 = 2.0;
const OR_VSM_DEPTH_EPS: f32 = 0.00005;
const OR_VSM_VARIANCE_MIN: f32 = 0.00005;
const OR_VSM_CHEBYSHEV_POWER: f32 = 50.0;
const OR_SHADOW_CASCADE_COUNT: u32 = 4u;

fn or_half_lambert(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    return dot(normalize(normal), normalize(light_dir)) * 0.5 + 0.5;
}

fn or_ps_chebyshev_from_moments(moments: vec3<f32>) -> f32 {
    let not_shadowed = moments.z - moments.x < OR_VSM_DEPTH_EPS;
    let ex = moments.x;
    let ex2 = moments.y;
    let variance = clamp(ex2 - ex * ex, OR_VSM_VARIANCE_MIN, 1.0);
    let m_d = moments.z - ex;
    let p = pow(variance / (variance + m_d * m_d), OR_VSM_CHEBYSHEV_POWER);
    return saturate(select(0.0, 1.0, not_shadowed) + p);
}

fn or_ps_get_shadow_effect(normal_light: f32, moments: vec3<f32>) -> f32 {
    let vsm = or_ps_chebyshev_from_moments(moments);
    return vsm * saturate(normal_light * 5.0 - 2.0);
}

fn or_ps_get_shadow_effect_from_pcf(normal_light: f32, pcf_shadow: f32) -> f32 {
    let ex = pcf_shadow;
    let ex2 = ex * ex + OR_VSM_VARIANCE_MIN;
    let receiver = mix(0.0, 1.0, pcf_shadow);
    return or_ps_get_shadow_effect(normal_light, vec3(ex, ex2, receiver));
}

// Seleccion de cascada OR (SceneryShader.fx _Level9_3GetShadowEffect, ShadowMapLimit).
fn or_get_cascade_index_or(view_depth: f32) -> u32 {
    if (view_depth < params.shadow_map_limit_x) {
        return 0u;
    }
    if (view_depth < params.shadow_map_limit_y) {
        return 1u;
    }
    if (view_depth < params.shadow_map_limit_z) {
        return 2u;
    }
    if (view_depth < params.shadow_map_limit_w) {
        return 3u;
    }
    return OR_SHADOW_CASCADE_COUNT;
}

fn or_world_to_directional_light_local(
    light_id: u32,
    cascade_index: u32,
    offset_position: vec4<f32>,
) -> vec4<f32> {
    let light = view_bindings::lights.directional_lights[light_id];
    let cascade = light.cascades[cascade_index];
    let offset_position_clip = cascade.clip_from_world * offset_position;
    if (offset_position_clip.w <= 0.0) {
        return vec4(0.0);
    }
    let offset_position_ndc = offset_position_clip.xyz / offset_position_clip.w;
    if (any(offset_position_ndc.xy < vec2(-1.0)) || offset_position_ndc.z < 0.0
        || any(offset_position_ndc > vec3(1.0))) {
        return vec4(0.0);
    }
    let flip_correction = vec2(0.5, -0.5);
    let light_local = offset_position_ndc.xy * flip_correction + vec2(0.5, 0.5);
    return vec4(light_local, offset_position_ndc.z, 1.0);
}

#ifdef OR_VSM_MOMENT_ATLAS
fn or_sample_moments_from_atlas(
    light_id: u32,
    cascade_index: u32,
    light_local: vec4<f32>,
) -> vec3<f32> {
    let rg = textureSample(or_moment_atlas, or_moment_sampler, light_local.xy, i32(cascade_index)).rg;
    return vec3(rg.x, rg.y, light_local.z);
}
#endif


const OR_DEBUG_CASCADE_TINT: f32 = 1.0;

fn or_apply_cascade_debug_tint(rgb: vec3<f32>, view_depth: f32) -> vec3<f32> {
    if (view_depth < params.shadow_map_limit_x) {
        return rgb * 0.9 + vec3(0.1, 0.0, 0.0);
    }
    if (view_depth < params.shadow_map_limit_y) {
        return rgb * 0.9 + vec3(0.0, 0.1, 0.0);
    }
    if (view_depth < params.shadow_map_limit_z) {
        return rgb * 0.9 + vec3(0.0, 0.0, 0.1);
    }
    if (view_depth < params.shadow_map_limit_w) {
        return rgb * 0.9 + vec3(0.1, 0.1, 0.0);
    }
    return rgb;
}

fn or_shadow_modulation(normal_light: f32, pcf_shadow: f32) -> f32 {
    if (params.vsm_mode >= OR_VSM_MODE_APPROX) {
        return or_ps_get_shadow_effect_from_pcf(normal_light, pcf_shadow);
    }
    return pcf_shadow * saturate(normal_light * 5.0 - 2.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(base_texture, base_sampler, in.uv)
        * vec4(params.tint_r, params.tint_g, params.tint_b, params.tint_a);

    if (color.a < params.reference_alpha) {
        discard;
    }

    let kind = params.shader_kind;
    let lit = params.flags >= OR_FLAG_LIT;

    if (!lit) {
        return vec4(color.rgb * params.night_color_modifier, color.a);
    }

    if (kind == OR_KIND_FULL_BRIGHT) {
        return vec4(color.rgb, color.a);
    }

    let n = normalize(in.world_normal);
    let light = view_bindings::lights.directional_lights[0];
    let light_dir = light.direction_to_light;

    let ndotl = or_half_lambert(n, light_dir);
    let ambient = ndotl;

    var pcf_shadow = 1.0;
    var shadow_mod = 1.0;

    if ((light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
        let view_z = (view_bindings::view.view_from_world * in.world_position).z;
        let view_depth = -view_z;
        pcf_shadow = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
        let pcf_mod = or_shadow_modulation(ambient, pcf_shadow);

#ifdef OR_VSM_MOMENT_ATLAS
        if (params.vsm_mode >= OR_VSM_MODE_EXACT) {
            let cascade_index = or_get_cascade_index_or(view_depth);
            if (cascade_index < OR_SHADOW_CASCADE_COUNT) {
                let normal_offset = light.shadow_normal_bias
                    * light.cascades[cascade_index].texel_size
                    * n;
                let depth_offset = light.shadow_depth_bias * light.direction_to_light;
                let offset_position = vec4(in.world_position.xyz + normal_offset + depth_offset, in.world_position.w);
                let light_local = or_world_to_directional_light_local(0u, cascade_index, offset_position);
                if (light_local.w > 0.0) {
                    let moments = or_sample_moments_from_atlas(0u, cascade_index, light_local);
                    let moment_mod = or_ps_get_shadow_effect(ambient, moments);
                    // En espacios cerrados (tuneles) los momentos suelen discrepar del PCF;
                    // confiar en PCF cuando la diferencia es grande (pack/sample aun no alineado).
                    if (abs(moment_mod - pcf_mod) > 0.35) {
                        shadow_mod = pcf_mod;
                    } else {
                        shadow_mod = moment_mod;
                    }
                } else {
                    shadow_mod = pcf_mod;
                }
            } else {
                shadow_mod = pcf_mod;
            }
        } else {
            shadow_mod = pcf_mod;
        }
#else
        shadow_mod = or_shadow_modulation(ambient, pcf_shadow);
#endif
    }

    var lit_rgb = color.rgb;

    switch i32(kind) {
        case 2: {
            lit_rgb = color.rgb * params.half_shadow_brightness;
        }
        case 3: {
            lit_rgb = color.rgb * params.shadow_brightness;
        }
        default: {
            let night_cancel = params.image_texture_is_night;
            let t = saturate(ambient * shadow_mod + night_cancel);
            let brightness = mix(params.shadow_brightness, params.full_brightness, t);
            lit_rgb = color.rgb * brightness;

            if (params.specular_strength > 0.0) {
                let view_dir = pbr_functions::calculate_view(in.world_position, false);
                let spec = ndotl * params.specular_strength * pow(
                    saturate(dot(n, normalize(view_dir + light_dir))),
                    params.specular_power,
                ) * shadow_mod;
                lit_rgb += vec3(spec);
            }
        }
    }

    if (params.debug_flags >= OR_DEBUG_CASCADE_TINT) {
        let view_z_dbg = (view_bindings::view.view_from_world * in.world_position).z;
        lit_rgb = or_apply_cascade_debug_tint(lit_rgb, -view_z_dbg);
    }

    lit_rgb *= params.night_color_modifier;
    return vec4(lit_rgb, color.a);
}
