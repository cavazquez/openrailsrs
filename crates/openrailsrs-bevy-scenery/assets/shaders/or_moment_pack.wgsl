// Empaqueta profundidad de sombra Bevy en momentos OR (z, z²) por cascada.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// 16 bytes: alinear con OrMomentPackParams en Rust (evitar vec3<u32> -> 32 B en WGSL).
struct OrMomentPackParams {
    cascade_layer: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var shadow_depth: texture_depth_2d_array;
@group(0) @binding(1) var shadow_sampler: sampler;
@group(0) @binding(2) var<uniform> params: OrMomentPackParams;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let z = textureSample(
        shadow_depth,
        shadow_sampler,
        in.uv,
        i32(params.cascade_layer),
    );
    return vec4(z, z * z, 0.0, 0.0);
}
