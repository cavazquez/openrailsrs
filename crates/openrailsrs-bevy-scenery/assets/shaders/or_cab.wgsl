// Open Rails SceneryShader.fx — CABVIEW3D interior (no VSM atlas).
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
}

struct OrCabParams {
    tint_r: f32,
    tint_g: f32,
    tint_b: f32,
    tint_a: f32,
    reference_alpha: f32,
    shadow_brightness: f32,
    full_brightness: f32,
    half_shadow_brightness: f32,
    shader_kind: f32,
    cab_min_brightness: f32,
    flags: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OrCabParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;

const OR_KIND_TEX_DIFF: f32 = 1.0;
const OR_KIND_HALF_BRIGHT: f32 = 2.0;
const OR_KIND_DARK_SHADE: f32 = 3.0;
const OR_KIND_FULL_BRIGHT: f32 = 4.0;
const OR_FLAG_LIT: f32 = 1.0;
const OR_FLAG_BLEND: f32 = 2.0;
const OR_FLAG_OR_LIKE: f32 = 4.0;

const OR_LIKE_AMBIENT: f32 = 0.78;

fn or_half_lambert(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    return dot(normalize(normal), normalize(light_dir)) * 0.5 + 0.5;
}

fn or_apply_fixed_brightness(
    rgb: vec3<f32>,
    kind: f32,
    shadow_b: f32,
    full_b: f32,
    half_b: f32,
) -> vec3<f32> {
    if (kind >= OR_KIND_FULL_BRIGHT) {
        return rgb;
    }
    if (kind >= OR_KIND_HALF_BRIGHT && kind < OR_KIND_DARK_SHADE) {
        return rgb * half_b;
    }
    if (kind >= OR_KIND_DARK_SHADE && kind < OR_KIND_FULL_BRIGHT) {
        return rgb * shadow_b;
    }
    return rgb * mix(shadow_b, full_b, OR_LIKE_AMBIENT);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(base_texture, base_sampler, in.uv)
        * vec4(params.tint_r, params.tint_g, params.tint_b, params.tint_a);
#ifdef VERTEX_COLORS
    color *= in.color;
#endif

#ifdef OR_CAB_DEBUG_UV
    return vec4(fract(in.uv), 0.0, 1.0);
#endif

#ifdef OR_CAB_DEBUG_VCOLOR
    return vec4(in.color.rgb, 1.0);
#endif

#ifdef OR_CAB_DEBUG_ALBEDO
    if (params.reference_alpha >= 0.0 && color.a < params.reference_alpha) {
        discard;
    }
    return vec4(color.rgb, color.a);
#endif

    // Blend / Add cab glass: output texture alpha as-is (no cutout discard).
    if (params.flags >= OR_FLAG_BLEND) {
        return vec4(color.rgb, color.a);
    }

    // OR solid opaque: ReferenceAlpha < 0 forces alpha = 1 (SceneryShader.fx _PSSceneryFade).
    if (params.reference_alpha < 0.0) {
        color.a = 1.0;
    } else if (color.a < params.reference_alpha) {
        discard;
    }

    let kind = params.shader_kind;
    let lit = params.flags >= OR_FLAG_LIT && params.flags < OR_FLAG_BLEND;
    let or_like = params.flags >= OR_FLAG_OR_LIKE;

    if (or_like && !lit) {
        var rgb = or_apply_fixed_brightness(
            color.rgb,
            kind,
            params.shadow_brightness,
            params.full_brightness,
            params.half_shadow_brightness,
        );
        rgb = max(rgb, color.rgb * params.cab_min_brightness);
        return vec4(rgb, color.a);
    }

    if (!lit) {
        return vec4(color.rgb, color.a);
    }

    if (kind >= OR_KIND_FULL_BRIGHT) {
        return vec4(color.rgb, color.a);
    }

    let n = normalize(in.world_normal);
    let light = view_bindings::lights.directional_lights[0];
    let light_dir = light.direction_to_light;
    let ndotl = or_half_lambert(n, light_dir);
    let ambient = ndotl;

    var shadow_mod = 1.0;
    if ((light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
        let view_z = (view_bindings::view.view_from_world * in.world_position).z;
        shadow_mod = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
        shadow_mod = shadow_mod * saturate(ambient * 5.0 - 2.0);
    }

    var lit_rgb = color.rgb;

    if (kind >= OR_KIND_HALF_BRIGHT && kind < OR_KIND_DARK_SHADE) {
        lit_rgb = color.rgb * params.half_shadow_brightness;
    } else if (kind >= OR_KIND_DARK_SHADE && kind < OR_KIND_FULL_BRIGHT) {
        lit_rgb = color.rgb * params.shadow_brightness;
    } else {
        let t = saturate(ambient * shadow_mod);
        let brightness = mix(params.shadow_brightness, params.full_brightness, t);
        lit_rgb = color.rgb * brightness;
    }

    lit_rgb = max(lit_rgb, color.rgb * params.cab_min_brightness);
    return vec4(lit_rgb, color.a);
}
