// Open Rails SceneryShader.fx PSTerrain (TerrainLevel9_3).
// Pipeline flags: lit/night via uniforms; vsm=true in render3d app; fog via DISTANCE_FOG.
// Shares the viewer's OR detail multiplication: lit_rgb * overlay * 2.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    pbr_functions,
}
#import "shaders/terrain_common.wgsl"::terrain_half_lambert
#import "shaders/railway_lighting.wgsl" as railway_lighting
#import "shaders/snow_surface.wgsl"::snow_surface

struct OrTerrainParams {
    shadow_brightness: f32,
    full_brightness: f32,
    night_color_modifier: f32,
    image_texture_is_night: f32,
    overlay_scale: f32,
    lit: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OrTerrainParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var overlay_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var overlay_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(base_texture, base_sampler, in.uv);

    var t = params.image_texture_is_night;
    if (params.lit >= 0.5) {
        let n = normalize(in.world_normal);
        let light = view_bindings::lights.directional_lights[0];
        let light_dir = light.direction_to_light;
        let ambient = terrain_half_lambert(n, light_dir);
        var shadow_mod = 1.0;
        if ((light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            let view_z = (view_bindings::view.view_from_world * in.world_position).z;
            shadow_mod = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
            shadow_mod = shadow_mod * saturate(ambient * 5.0 - 2.0);
        }
        t = saturate(ambient * shadow_mod + params.image_texture_is_night);
    }

    let snow = snow_surface(in.uv, in.world_normal, params._pad1);
    let surface = mix(color.rgb,vec3(0.78,0.84,0.90)*snow.y,snow.x*0.86);
    var lit_rgb = surface * mix(params.shadow_brightness, params.full_brightness, t);
    let overlay = textureSample(overlay_texture, overlay_sampler, in.uv * params.overlay_scale).rgb * 2.0;
    lit_rgb = lit_rgb * overlay;
    lit_rgb = lit_rgb * min(params.night_color_modifier, railway_lighting::railway_daylight());
    if (params.lit >= 0.5) {
        lit_rgb += surface * overlay * railway_lighting::railway_spot_lighting(in.world_position, normalize(in.world_normal), in.position.xy);
    }
    // Rain darkens the ground and adds a restrained sun reflection at grazing angles.
    let wet = clamp(params._pad0, 0.0, 1.0);
    lit_rgb *= 1.0 - wet * 0.18;
    if (params.lit >= 0.5 && wet > 0.0) {
        let n = normalize(in.world_normal);
        let eye = normalize(view_bindings::view.world_position.xyz - in.world_position.xyz);
        let light = view_bindings::lights.directional_lights[0].direction_to_light;
        let h = normalize(eye + light);
        lit_rgb += vec3(pow(max(dot(n, h), 0.0), 32.0) * wet * 0.04 * railway_lighting::railway_daylight());
    }
    var out_color = vec4(lit_rgb, color.a);
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
