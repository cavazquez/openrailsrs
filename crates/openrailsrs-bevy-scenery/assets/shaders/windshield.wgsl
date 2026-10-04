#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
struct Settings { time_s: f32, rain: f32, near_clip: f32, last_wipe_s: f32, wiper_on: f32, snow: f32, _pad: vec3<f32>, glass: vec4<f32>, blade1: vec4<f32>, blade2: vec4<f32> }
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: Settings;
@group(0) @binding(3) var depth: texture_depth_2d;

fn hash(p: vec2<f32>) -> vec2<f32> { return fract(sin(vec2(dot(p, vec2(127.1, 311.7)), dot(p, vec2(269.5, 183.3)))) * 43758.5453); }
fn wet_after_wipe(uv: vec2<f32>, blade: vec4<f32>) -> f32 {
    if (settings.last_wipe_s < 0.0 || blade.w < 0.5) { return 1.0; }
    let vector = (uv - blade.xy) * vec2(1.0, 0.8);
    let angle = atan2(vector.x, -vector.y);
    if (abs(angle) > 0.95 || length(vector) > blade.z || length(vector) < 0.025) { return 1.0; }
    let a = (angle + 0.95) / 1.9;
    let phase = fract(settings.last_wipe_s / 1.8);
    let since = min(fract(phase - a * 0.5), fract(phase - (1.0 - a * 0.5))) * 1.8;
    return clamp((since + settings.time_s - settings.last_wipe_s) / 4.5, 0.04, 1.0);
}
@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let resolution = vec2<f32>(textureDimensions(depth));
    let d = textureLoad(depth, vec2<i32>(clamp(in.uv * resolution, vec2(0.0), resolution - 1.0)), 0);
    let dry = textureSample(scene, scene_sampler, in.uv);
    // Reverse-Z main depth: the desk/frames are close, scenery through a window
    // is distant. Rain must never appear over instruments or the opaque cab.
    if (d > settings.near_clip / 3.0) { return dry; }
    let pane = (in.uv - settings.glass.xy) / max(settings.glass.zw, vec2(0.001));
    if (any(pane < vec2(0.0)) || any(pane > vec2(1.0))) { return dry; }
    let wetness = min(wet_after_wipe(pane, settings.blade1), wet_after_wipe(pane, settings.blade2));
    let grid = in.uv * vec2(105.0, 70.0);
    let cell = floor(grid);
    let random = hash(cell);
    let fall = fract(settings.time_s * (0.035 + random.x * 0.04) + random.y);
    let local = fract(grid) - vec2(0.2 + random.x * 0.6, 0.12 + fall * 0.76);
    let radius = length(local * vec2(1.0, 0.7));
    let drop = (1.0 - smoothstep(0.055, 0.17, radius)) * wetness * step(0.5, random.x);
    let snow = settings.snow * (1.0 - smoothstep(0.09, 0.23, radius)) * wetness * step(0.6, random.x);
    let refracted = textureSample(scene, scene_sampler, in.uv + local * drop * 0.008);
    let highlight = (1.0 - smoothstep(0.01, 0.08, abs(radius - 0.12))) * drop * 0.16;
    let wet = mix(dry.rgb, refracted.rgb, drop * (1.0-settings.snow)) + vec3(highlight * (1.0-settings.snow));
    return vec4(mix(wet, vec3(0.80, 0.86, 0.91), snow * 0.65), dry.a);
}
