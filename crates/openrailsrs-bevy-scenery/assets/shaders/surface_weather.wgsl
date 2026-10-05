// Preserve Bevy's standard material alpha, lighting and fog; cover upward surfaces.
#import "shaders/snow_surface.wgsl"::snow_surface
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::{prepass_io::{VertexOutput, FragmentOutput}, pbr_deferred_functions::deferred_output}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> snow_cover: vec4<f32>;
#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither
#endif

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif
    var input = pbr_input_from_standard_material(in, is_front);
    input.material.base_color = alpha_discard(input.material, input.material.base_color);
    let snow = snow_surface(in.uv, in.world_normal, snow_cover.x);
    let cover = snow.x;
    input.material.base_color = vec4(mix(input.material.base_color.rgb,
        vec3(0.78, 0.84, 0.89) * snow.y, cover * 0.86), input.material.base_color.a);
    input.material.perceptual_roughness = mix(input.material.perceptual_roughness, 0.85, cover);
#ifdef PREPASS_PIPELINE
    return deferred_output(in, input);
#else
    var out: FragmentOutput;
    if (input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(input);
    } else {
        out.color = input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
#endif
}
