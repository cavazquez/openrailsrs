// WORLD GPU instancing (#58): albedo + alpha cutoff + scene light + fog (#76) +
// receive + cast directional shadows (#72).
#import bevy_pbr::view_transformations::position_world_to_clip
#import bevy_pbr::{
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    pbr_functions,
}
#import bevy_render::maths::PI
#import "shaders/railway_lighting.wgsl"::railway_spot_lighting

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
    // x = alpha_cutoff (0 = disabled), y = double_sided, zw unused
    params: vec4<f32>,
    world_from_local: mat4x4<f32>,
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
    // Each custom draw starts its own instance buffer at zero; mesh[0] belongs
    // to an unrelated scene entity when Bevy uses storage mesh uniforms.
    let world_from_local = appearance.world_from_local;
    var out: VertexOutput;
    let world_pos4 = world_from_local * local_pos;
    out.clip_position = position_world_to_clip(world_pos4.xyz);
    out.world_position = world_pos4.xyz;
    let n = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    out.world_normal = normalize((world_from_local * vec4<f32>(n, 0.0)).xyz);
    out.uv = vertex.uv;
    return out;
}

// A fixed Bayer matrix has complementary coverage for old/new meshes and
// does not shimmer across frames. Apply the same policy in shadow depth.
fn discard_lod(position: vec2<f32>) -> bool {
    let fade=appearance.params.z;
    if fade==0.0 {return false;}
    let x=u32(position.x)%4u;
    let y=u32(position.y)%4u;
    let bayer=array<f32,16>(0.,8.,2.,10.,12.,4.,14.,6.,3.,11.,1.,9.,15.,7.,13.,5.);
    let threshold=(bayer[y*4u+x]+0.5)/16.0;
    return select(threshold >= fade,threshold < -fade,fade<0.0);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if discard_lod(in.clip_position.xy) {discard;}
    var color = appearance.base_color * textureSample(base_color_texture, base_color_sampler, in.uv);
    let cutoff = appearance.params.x;
    if cutoff > 0.0 && color.a < cutoff {
        discard;
    }

    var n = normalize(in.world_normal);
    if appearance.params.y > 0.0 && !front_facing {
        n = -n;
    }
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

    lit += color.rgb * railway_spot_lighting(vec4(in.world_position, 1.0), n, in.clip_position.xy);
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
    let world_from_local = appearance.world_from_local;
    var out: ShadowVertexOutput;
    out.clip_position = position_world_to_clip((world_from_local * local_pos).xyz);
    out.uv = vertex.uv;
    return out;
}

@fragment
fn fragment_shadow(in: ShadowVertexOutput) {
    if discard_lod(in.clip_position.xy) {discard;}
    let cutoff = appearance.params.x;
    if cutoff > 0.0 {
        let color = appearance.base_color * textureSample(base_color_texture, base_color_sampler, in.uv);
        if color.a < cutoff {
            discard;
        }
    }
}
