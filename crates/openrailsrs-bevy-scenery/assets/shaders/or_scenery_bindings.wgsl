// Common authored material layout for colour, shadow and depth passes.
struct OrSceneryParams {
    tint_r: f32,
    tint_g: f32,
    tint_b: f32,
    tint_a: f32,
    reference_alpha: f32,
    shadow_brightness: f32,
    full_brightness: f32,
    half_shadow_brightness: f32,
    specular_strength: f32,
    specular_power: f32,
    night_color_modifier: f32,
    image_texture_is_night: f32,
    shader_kind: f32,
    flags: f32,
    vsm_mode: f32,
    shadow_map_limit_x: f32,
    shadow_map_limit_y: f32,
    shadow_map_limit_z: f32,
    shadow_map_limit_w: f32,
    debug_flags: f32,
    wetness: f32,
    snow_cover: f32,
    weather_pad0: f32,
    weather_pad1: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OrSceneryParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
