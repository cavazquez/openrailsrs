// Dual-texture terrain for viewer3d (#42 shadows + #39 fog).
// Lighting/shadows follow OpenRails PSTerrain / OrTerrainMaterial half-Lambert × cascade.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    pbr_functions,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> overlay_scale: f32;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var overlay_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var overlay_sampler: sampler;

const SHADOW_BRIGHTNESS: f32 = 0.5;
const FULL_BRIGHTNESS: f32 = 1.0;

fn half_lambert(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    return dot(normalize(normal), normalize(light_dir)) * 0.5 + 0.5;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(base_texture, base_sampler, in.uv);
    let overlay_uv = in.uv * overlay_scale;
    let overlay = textureSample(overlay_texture, overlay_sampler, overlay_uv);
    let detail = overlay.rgb;
    let mix_strength = clamp(overlay.a * 0.65 + 0.2, 0.0, 1.0);
    var rgb = mix(base.rgb, base.rgb * detail, mix_strength);

    let n = normalize(in.world_normal);
    let light = view_bindings::lights.directional_lights[0];
    let light_dir = light.direction_to_light;
    let ambient = half_lambert(n, light_dir);
    var shadow_mod = 1.0;
    if ((light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
        let view_z = (view_bindings::view.view_from_world * in.world_position).z;
        shadow_mod = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
        shadow_mod = shadow_mod * saturate(ambient * 5.0 - 2.0);
    }
    let t = saturate(ambient * shadow_mod);
    rgb = rgb * mix(SHADOW_BRIGHTNESS, FULL_BRIGHTNESS, t);

    var out_color = vec4<f32>(rgb, 1.0);
#ifdef DISTANCE_FOG
    out_color = pbr_functions::apply_fog(
        view_bindings::fog,
        out_color,
        in.world_position.xyz,
        view_bindings::view.world_position.xyz,
        in.position.xy,
    );
#endif
    return out_color;
}
