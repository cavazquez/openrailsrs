// Injected into Hanabi's fragment context, pinned to 0.19.0. Persistent seed
// and shelter coordinates are particle attributes, without GPU readback.
// Hanabi 0.19 exposes effect properties to vertices, not fragments.
if color.a <= 0.0 { discard; }
let grid = particle.f32x4_1;
let world = particle.position;
let grid_uv = (world.xz - grid.xy) / (grid.z * 2.0) + vec2(0.5);
let dim = textureDimensions(material_texture_0);
let pixel = clamp(vec2<i32>(grid_uv * vec2<f32>(dim)), vec2(0), vec2<i32>(dim) - vec2(1));
let packed = textureLoad(material_texture_0, pixel, 0).rg;
let roof = grid.w + (packed.r * 65280.0 + packed.g * 255.0) / 65535.0 * 512.0;
let sheltered = smoothstep(roof + 0.03, roof + 0.7, world.y);
let variation = particle.f32x4_0.w;
let p = uv * 2.0 - vec2(1.0);
let radius = length(p);
let angle = atan2(p.y, p.x);
let lobes = select(3.0, 6.0, variation > 0.5);
let edge = 0.64 + 0.17 * sin(angle * lobes + variation * 19.0) + 0.08 * sin(angle * 11.0 + variation * 37.0);
let aa = max(fwidth(radius), 0.055);
let grain = 0.78 + 0.22 * sin(p.x * 23.0 + variation * 31.0) * cos(p.y * 27.0);
var alpha = (1.0 - smoothstep(edge - aa, edge + aa, radius)) * grain;
if !snow {
    alpha = (1.0 - smoothstep(0.18, 0.5, abs(uv.x - 0.5)))
        * (1.0 - smoothstep(0.3, 0.5, abs(uv.y - 0.5))) * 0.48;
}
alpha *= sheltered * color.a * 0.82;
if alpha < 0.015 { discard; }
color = vec4(color.rgb, alpha);
