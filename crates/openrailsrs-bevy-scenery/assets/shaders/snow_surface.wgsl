// UV anchored detail survives floating-origin rebases and streaming unchanged.
fn snow_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
fn snow_noise(p: vec2<f32>) -> f32 {
    let i = floor(p); let f = fract(p); let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(snow_hash(i), snow_hash(i+vec2(1.0,0.0)), u.x),
        mix(snow_hash(i+vec2(0.0,1.0)), snow_hash(i+vec2(1.0)), u.x), u.y);
}
fn snow_surface(uv: vec2<f32>, normal: vec3<f32>, amount: f32) -> vec2<f32> {
    if amount <= 0.001 { return vec2(0.0, 1.0); }
    let coverage_noise = snow_noise(uv * 8.0) * 0.66 + snow_noise(uv * 23.0) * 0.34;
    let upward = smoothstep(0.30, 0.85, normalize(normal).y);
    let coverage = clamp(amount,0.0,1.0) * smoothstep(0.05, 0.62, clamp(amount,0.0,1.0) + coverage_noise * 0.44 - 0.22) * upward;
    let grain = 0.88 + snow_noise(uv * 170.0) * 0.12;
    return vec2(coverage, grain);
}
