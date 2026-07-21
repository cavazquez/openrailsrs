#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    pbr_functions,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> overlay_scale: f32;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var overlay_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var overlay_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(base_texture, base_sampler, in.uv);
    let overlay_uv = in.uv * overlay_scale;
    let overlay = textureSample(overlay_texture, overlay_sampler, overlay_uv);
    let detail = overlay.rgb;
    let mix_strength = clamp(overlay.a * 0.65 + 0.2, 0.0, 1.0);
    let rgb = mix(base.rgb, base.rgb * detail, mix_strength);
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
