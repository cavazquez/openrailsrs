#import bevy_pbr::{
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}
struct ParticleUniforms {
    center: vec4<f32>, phase: vec4<f32>, right: vec4<f32>,
    up: vec4<f32>, wind_time: vec4<f32>, grid: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> weather: ParticleUniforms;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var shelter: texture_2d<f32>;

fn wrap(v: f32, period: f32) -> f32 {
    return v - floor((v + period * 0.5) / period) * period;
}
fn particle_position(seed: vec4<f32>) -> vec3<f32> {
    let t = weather.wind_time.w;
    let snow = weather.up.w > 0.5;
    let fall = select(20.0 + seed.w * 16.0, 0.7 + seed.w * 1.2, snow);
    let flutter = select(vec2(0.0), vec2(sin(t * 0.73 + seed.x * 31.0), cos(t * 0.51 + seed.z * 27.0)) * 0.65, snow);
    let span = weather.center.w * 2.0;
    let x = seed.x * span + weather.wind_time.x * t + flutter.x;
    let z = seed.z * span + weather.wind_time.z * t + flutter.y;
    let y = seed.y * weather.phase.w - fall * t;
    return weather.center.xyz + vec3(wrap(x - weather.phase.x, span), wrap(y - weather.phase.y, weather.phase.w), wrap(z - weather.phase.z, span));
}
fn particle_corner(seed: vec4<f32>, uv: vec2<f32>) -> vec3<f32> {
    if weather.up.w < 0.5 {
        return weather.right.xyz * ((uv.x - 0.5) * 0.025) + vec3(0.0, (uv.y - 0.5) * (0.8 + seed.w * 0.65), 0.0);
    }
    let angle = seed.w * 6.283185307 + weather.wind_time.w * (0.35 + seed.x);
    let v = (uv * 2.0 - vec2(1.0)) * (0.018 + seed.w * 0.040);
    let r = vec2(v.x * cos(angle) - v.y * sin(angle), v.x * sin(angle) + v.y * cos(angle));
    return weather.right.xyz * r.x + weather.up.xyz * r.y;
}

@vertex fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let seed = vec4(in.normal, in.color.x);
    var world = in.position;
    if weather.right.w > 0.5 { world = particle_position(seed) + particle_corner(seed, in.uv); }
    out.world_position = vec4(world, 1.0);
    out.position = position_world_to_clip(world);
    out.world_normal = normalize(cross(weather.right.xyz, weather.up.xyz));
    out.uv = in.uv;
    let grid_uv = (world.xz - weather.grid.xy) / (weather.grid.z * 2.0) + vec2(0.5);
    let dim = textureDimensions(shelter);
    let pixel = clamp(vec2<i32>(grid_uv * vec2<f32>(dim)), vec2(0), vec2<i32>(dim) - vec2(1));
    let roof = textureLoad(shelter, pixel, 0).x;
    let horizontal = length(world.xz - weather.center.xz);
    let opacity = smoothstep(roof + 0.03, roof + 0.7, world.y)
        * (1.0 - smoothstep(weather.center.w * 0.72, weather.center.w, horizontal));
    out.uv_b = vec2(seed.w, opacity);
    out.color = vec4(1.0);
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = in.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = 0;
#endif
    return out;
}

// Irregular clumps and crystalline edges at close range; distant flakes resolve
// to soft specks. Rotation, silhouette and physical size vary per seed.
fn flake_mask(uv: vec2<f32>, variation: f32) -> f32 {
    let p = uv * 2.0 - vec2(1.0);
    let radius = length(p);
    let angle = atan2(p.y, p.x);
    let lobes = select(3.0, 6.0, variation > 0.5);
    let edge = 0.64 + 0.17 * sin(angle * lobes + variation * 19.0)
        + 0.08 * sin(angle * 11.0 + variation * 37.0);
    let aa = max(fwidth(radius), 0.055);
    let outline = 1.0 - smoothstep(edge - aa, edge + aa, radius);
    let grain = 0.78 + 0.22 * sin(p.x * 23.0 + variation * 31.0) * cos(p.y * 27.0);
    return outline * grain;
}
@fragment fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
    var alpha = flake_mask(in.uv, in.uv_b.x);
    if weather.up.w < 0.5 {
        alpha = (1.0 - smoothstep(0.18, 0.5, abs(in.uv.x - 0.5)))
            * (1.0 - smoothstep(0.3, 0.5, abs(in.uv.y - 0.5))) * 0.48;
    }
    alpha *= in.uv_b.y;
    if alpha < 0.015 { discard; }
    var input = pbr_input_from_standard_material(in, front);
    input.material.base_color.a *= alpha;
    input.material.base_color = alpha_discard(input.material, input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(input);
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
}
