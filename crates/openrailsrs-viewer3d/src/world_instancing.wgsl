#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}
#import bevy_pbr::mesh_view_bindings::view

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Instance affine columns (Mat4 column-major via 4×vec4).
    @location(3) i_col0: vec4<f32>,
    @location(4) i_col1: vec4<f32>,
    @location(5) i_col2: vec4<f32>,
    @location(6) i_col3: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_position: vec3<f32>,
};

struct AppearanceUniform {
    base_color: vec4<f32>,
    // x = alpha_cutoff (0 = disabled), yzw unused
    params: vec4<f32>,
};

@group(3) @binding(0)
var<uniform> appearance: AppearanceUniform;
@group(3) @binding(1)
var base_color_texture: texture_2d<f32>;
@group(3) @binding(2)
var base_color_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    let model = mat4x4<f32>(
        vertex.i_col0,
        vertex.i_col1,
        vertex.i_col2,
        vertex.i_col3,
    );
    let local_pos = model * vec4<f32>(vertex.position, 1.0);
    // Entity Transform carries floating-origin; instances are in that local frame.
    let world_from_local = get_world_from_local(0u);
    var out: VertexOutput;
    out.clip_position = mesh_position_local_to_clip(world_from_local, local_pos);
    let world_pos4 = world_from_local * local_pos;
    out.world_position = world_pos4.xyz;
    let n = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    out.world_normal = normalize((world_from_local * vec4<f32>(n, 0.0)).xyz);
    out.uv = vertex.uv;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = appearance.base_color * textureSample(base_color_texture, base_color_sampler, in.uv);
    let cutoff = appearance.params.x;
    if cutoff > 0.0 && color.a < cutoff {
        discard;
    }
    // Simple Lambert using view forward as a stand-in light (v1; full PBR later).
    let n = normalize(in.world_normal);
    let light_dir = normalize(vec3<f32>(0.35, 0.9, 0.25));
    let ndotl = clamp(dot(n, light_dir), 0.25, 1.0);
    color = vec4<f32>(color.rgb * ndotl, color.a);
    return color;
}
