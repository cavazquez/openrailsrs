// Dual-texture terrain for viewer3d (#42 shadows + #39 fog).
// Pipeline flags: lit=true, night=false, vsm=false, fog=true (see TerrainPipelineFlags::VIEWER).
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    pbr_functions,
}
#import "shaders/terrain_common.wgsl"::terrain_half_lambert
#import "shaders/railway_lighting.wgsl" as railway_lighting

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> overlay_scale: f32;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var overlay_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var overlay_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<uniform> surface_weather: vec2<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<uniform> enhancement: vec4<f32>;

const SHADOW_BRIGHTNESS: f32 = 0.5;
#import "shaders/snow_surface.wgsl"::snow_surface
const FULL_BRIGHTNESS: f32 = 1.0;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(base_texture, base_sampler, in.uv);
    let overlay_uv = in.uv * overlay_scale;
    let overlay = textureSample(overlay_texture, overlay_sampler, overlay_uv);
    // Open Rails SceneryShader.fx PSTerrain: detail multiplies by RGB * 2.
    // Overlay alpha does not change the terrain lighting or the blend weight.
    var rgb = base.rgb * overlay.rgb * 2.0;

    let n = normalize(in.world_normal);
    let wet = clamp(surface_weather.x, 0.0, 1.0);
    var puddle = 0.0;
    if enhancement.x > 0.0 {
        // The origin offset keeps the pattern fixed while tiles are rebased.
        let p = in.world_position.xz + enhancement.zw;
        let macro_variation = sin(p.x * 0.034) * sin(p.y * 0.047);
        let detail = sin(p.x * 4.3 + p.y * 2.7) * 0.008;
        rgb *= 1.0 + macro_variation * 0.055 + detail;
        puddle = smoothstep(0.6, 0.95, wet) * smoothstep(0.7, 0.95, -macro_variation)
            * smoothstep(0.93, 0.99, n.y);
        rgb *= 1.0 - puddle * 0.12;
    }
    let snow = snow_surface(in.uv, n, surface_weather.y);
    rgb = mix(rgb * (1.0 - 0.16 * wet), vec3(0.78, 0.84, 0.89) * snow.y, snow.x * 0.86);
    let light = view_bindings::lights.directional_lights[0];
    let light_dir = light.direction_to_light;
    let ambient = terrain_half_lambert(n, light_dir);
    var shadow_mod = 1.0;
    if ((light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
        let view_z = (view_bindings::view.view_from_world * in.world_position).z;
        shadow_mod = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
        shadow_mod = shadow_mod * saturate(ambient * 5.0 - 2.0);
    }
    let t = saturate(ambient * shadow_mod);
    rgb = rgb * (mix(SHADOW_BRIGHTNESS, FULL_BRIGHTNESS, t) * railway_lighting::railway_daylight()
        + railway_lighting::railway_spot_lighting(in.world_position, n, in.position.xy));
    if (wet > 0.0) {
        let eye = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);
        let h = normalize(eye + light_dir);
        rgb += vec3((wet * 0.18 + puddle * 0.22) * pow(max(dot(n, h), 0.0), 40.0) * t * railway_lighting::railway_daylight());
    }

    var out_color = vec4<f32>(rgb, 1.0);
#ifdef DISTANCE_FOG
    out_color = pbr_functions::apply_fog(
        view_bindings::fog,
        out_color,
        in.world_position.xyz,
        view_bindings::view.world_position.xyz,
        in.position.xy,
    );
#endif
    return out_color;
}
