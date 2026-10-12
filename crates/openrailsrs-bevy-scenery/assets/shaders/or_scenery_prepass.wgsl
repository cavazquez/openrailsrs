#import bevy_pbr::{prepass_io, pbr_functions, pbr_prepass_functions}
#import "shaders/or_scenery_bindings.wgsl"::{params, base_texture, base_sampler}

#ifdef PREPASS_FRAGMENT
struct Output {
    @builtin(frag_depth) depth:f32,
#ifdef NORMAL_PREPASS
    @location(0) normal:vec4<f32>,
#endif
#ifdef MOTION_VECTOR_PREPASS
    @location(1) motion_vector:vec2<f32>,
#endif
}
@fragment
fn fragment(in: prepass_io::VertexOutput) -> Output {
#ifdef VERTEX_UVS_A
    let alpha = textureSample(base_texture,base_sampler,in.uv).a * params.tint_a;
    if params.reference_alpha > 0.01 && alpha < params.reference_alpha { discard; }
#endif
#ifdef VISIBILITY_RANGE_DITHER
    pbr_functions::visibility_range_dither(in.position,in.visibility_range_dither);
#endif
    var out:Output;
#ifdef NORMAL_PREPASS
    out.normal=vec4(normalize(in.world_normal)*0.5+0.5,1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector=pbr_prepass_functions::calculate_motion_vector(in.world_position,in.previous_world_position);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.depth=in.unclipped_depth;
#else
    out.depth=in.position.z;
#endif
    return out;
}
#endif
