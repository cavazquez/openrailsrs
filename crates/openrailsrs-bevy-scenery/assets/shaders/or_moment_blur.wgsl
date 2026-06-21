// Blur separable OR sobre textura de momentos Rg32 (ShadowMapBlur.fx).
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct OrMomentBlurParams {
    blur_dir: vec2<f32>,
    texel_size: vec2<f32>,
    cascade_layer: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var moment_texture: texture_2d_array<f32>;
@group(0) @binding(1) var moment_sampler: sampler;
@group(0) @binding(2) var<uniform> blur_params: OrMomentBlurParams;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let layer = i32(blur_params.cascade_layer);
    let step = blur_params.texel_size * blur_params.blur_dir;
    let centre = textureSample(moment_texture, moment_sampler, in.uv, layer).rg * 0.4430448;
    let tap01 = textureSample(moment_texture, moment_sampler, in.uv - step * 1.5, layer).rg * 0.2784776;
    let tap23 = textureSample(moment_texture, moment_sampler, in.uv + step * 1.5, layer).rg * 0.2784776;
    return vec4(tap01 + centre + tap23, 0.0, 0.0);
}
