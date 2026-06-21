// Visualiza una capa del atlas Rg32 de momentos (E[z], E[z^2]).
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct OrMomentPreviewParams {
    cascade_layer: u32,
    _pad: u32,
}

@group(0) @binding(0) var moment_atlas: texture_2d_array<f32>;
@group(0) @binding(1) var moment_sampler: sampler;
@group(0) @binding(2) var<uniform> params: OrMomentPreviewParams;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let rg = textureSample(moment_atlas, moment_sampler, in.uv, i32(params.cascade_layer)).rg;
    let z = saturate(rg.x);
    let z2 = saturate(rg.y);
    return vec4(z, z2, abs(z2 - z * z) * 8.0, 1.0);
}
