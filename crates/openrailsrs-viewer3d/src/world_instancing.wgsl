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
#import "shaders/snow_surface.wgsl"::snow_surface

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
    @location(3) @interpolate(flat) vegetation_seed: f32,
};

struct AppearanceUniform {
    surface_weather: vec4<f32>,
    base_color: vec4<f32>,
    // x = alpha_cutoff (0 = disabled), y = double_sided, zw unused
    params: vec4<f32>,
    world_from_local: mat4x4<f32>,
    vegetation: vec4<f32>,
};

@group(3) @binding(0)
var<uniform> appearance: AppearanceUniform;
@group(3) @binding(1)
var base_color_texture: texture_2d<f32>;
@group(3) @binding(2)
var base_color_sampler: sampler;

fn phase_seed(model:mat4x4<f32>)->f32 {return model[3].x*0.31+model[3].z*0.47;}

fn vegetation_position(position: vec3<f32>, model: mat4x4<f32>) -> vec4<f32> {
    var p = position;
    if appearance.vegetation.x > 0.0 {
        let snow = clamp(appearance.surface_weather.y, 0.0, 1.0);
        p.y *= mix(1.0, 0.35, snow);
        let root = (appearance.world_from_local * model[3]).xyz;
        let distance = length(root.xz - appearance.surface_weather.zw);
        let seed=fract(sin(phase_seed(model)) * 43758.5453);
        let density=mix(1.0,0.28,smoothstep(appearance.vegetation.x*0.28,appearance.vegetation.x*0.55,distance));
        p *= smoothstep(seed-0.08,seed,density);
        p.y *= mix(1.0, 0.65, smoothstep(appearance.vegetation.x * 0.3, appearance.vegetation.x * 0.65, distance));
        let phase = model[3].x * 0.31 + model[3].z * 0.47;
        let wind = appearance.vegetation.zw;
        let bend=wind * p.y * p.y * 0.04 * sin(appearance.vegetation.y * 1.4 + phase) * (1.0 - snow * 0.7);
        p += vec3(bend.x,0.0,bend.y);
    }
    return model * vec4(p, 1.0);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    let model = mat4x4<f32>(
        vertex.i_col0,
        vertex.i_col1,
        vertex.i_col2,
        vertex.i_col3,
    );
    let local_pos = vegetation_position(vertex.position, model);
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
    out.vegetation_seed = fract(sin(phase_seed(model)) * 43758.5453);
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
    if appearance.vegetation.x > 0.0 {
        let distance = length(in.world_position.xz - view_bindings::view.world_position.xz);
        let far = appearance.vegetation.x;
        let coverage = 1.0 - smoothstep(far * 0.7, far, distance);
        let threshold = fract(sin(dot(floor(in.clip_position.xy), vec2(12.9898, 78.233))) * 43758.5453);
        if threshold > coverage { discard; }
        let variation = 0.88 + 0.24 * in.vegetation_seed;
        color = vec4(color.rgb * mix(0.68, 1.12, in.uv.y) * variation, color.a);
    }
    let cutoff = appearance.params.x;
    if cutoff > 0.0 && color.a < cutoff {
        discard;
    }

    var n = normalize(in.world_normal);
    if appearance.params.y > 0.0 && !front_facing {
        n = -n;
    }
    let wet=appearance.surface_weather.x;
    let snow_detail=snow_surface(in.uv,n,appearance.surface_weather.y);
    let snow=max(snow_detail.x, select(0.0, appearance.surface_weather.y * 0.65, appearance.vegetation.x > 0.0));
    // Preserve cutout alpha: snow on a tree must not turn its quad opaque.
    color=vec4(mix(color.rgb * (1.0-0.16*wet),vec3(0.78,0.84,0.90)*snow_detail.y,snow),color.a);
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
    if wet>0.0 && view_bindings::lights.n_directional_lights>0u {
        let light=view_bindings::lights.directional_lights[0];
        let eye=normalize(view_bindings::view.world_position.xyz-in.world_position);
        let h=normalize(eye+light.direction_to_light);
        let daylight=clamp(light.direction_to_light.y*2.0,0.0,1.0);
        lit+=vec3(pow(max(dot(n,h),0.0),48.0)*wet*0.04*daylight);
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
    let local_pos = vegetation_position(vertex.position, model);
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
