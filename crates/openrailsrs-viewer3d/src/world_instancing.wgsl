// WORLD GPU instancing (#58): albedo + alpha cutoff + scene light + fog (#76) +
// receive + cast directional shadows (#72).
#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_clip}
#import bevy_pbr::{
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    pbr_functions,
}
#import bevy_render::maths::PI

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

    let n = normalize(in.world_normal);
    var lit = color.rgb;
    let ambient = view_bindings::lights.ambient_color.rgb;
    let exposure = view_bindings::view.exposure;
    if (view_bindings::lights.n_directional_lights > 0u) {
        let light = view_bindings::lights.directional_lights[0];
        let light_dir = light.direction_to_light;
        // Physical directional lights are stored in lux. Match Bevy's diffuse BRDF
        // normalization and camera exposure; omitting both clips ordinary scenery white.
        let ndotl = max(dot(n, light_dir), 0.0);
        var shadow_mod = 1.0;
        if ((light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            let world_pos4 = vec4<f32>(in.world_position, 1.0);
            let view_z = (view_bindings::view.view_from_world * world_pos4).z;
            shadow_mod = fetch_directional_shadow(
                0u,
                world_pos4,
                n,
                view_z,
                in.clip_position.xy,
            );
        }
        let light_rgb = light.color.rgb;
        let diffuse_sun = light_rgb * (ndotl / PI) * shadow_mod;
        lit = color.rgb * exposure * (ambient + diffuse_sun);
    } else {
        lit = color.rgb * exposure * max(ambient, vec3<f32>(0.35));
    }

    var out_color = vec4<f32>(lit, color.a);
#ifdef DISTANCE_FOG
    out_color = pbr_functions::apply_fog(
        view_bindings::fog,
        out_color,
        in.world_position.xyz,
        view_bindings::view.world_position.xyz,
        in.clip_position.xy,
    );
#endif
    return out_color;
}

// ─── Shadow map cast (#72): depth-only with optional alpha discard ────────────

struct ShadowVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_shadow(vertex: Vertex) -> ShadowVertexOutput {
    let model = mat4x4<f32>(
        vertex.i_col0,
        vertex.i_col1,
        vertex.i_col2,
        vertex.i_col3,
    );
    let local_pos = model * vec4<f32>(vertex.position, 1.0);
    let world_from_local = get_world_from_local(0u);
    var out: ShadowVertexOutput;
    out.clip_position = mesh_position_local_to_clip(world_from_local, local_pos);
    out.uv = vertex.uv;
    return out;
}

@fragment
fn fragment_shadow(in: ShadowVertexOutput) {
    let cutoff = appearance.params.x;
    if cutoff > 0.0 {
        let color = appearance.base_color * textureSample(base_color_texture, base_color_sampler, in.uv);
        if color.a < cutoff {
            discard;
        }
    }
}
