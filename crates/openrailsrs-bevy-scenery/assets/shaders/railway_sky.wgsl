#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
struct SkyParameters { horizon: vec4<f32>, zenith: vec4<f32>, sun: vec4<f32>, clouds: vec4<f32> };
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkyParameters;
fn hash(p: vec2<f32>) -> f32 {return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
fn noise(p: vec2<f32>) -> f32 {
    let i=floor(p);let f=fract(p);let u=f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1.,0.)),u.x),mix(hash(i+vec2(0.,1.)),hash(i+vec2(1.,1.)),u.x),u.y);
}
fn clouds(p: vec2<f32>) -> f32 {return noise(p)*0.55+noise(p*2.1)*0.27+noise(p*4.3)*0.13+noise(p*8.5)*0.05;}
@fragment fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray=normalize(in.world_position.xyz-view.world_position.xyz);
    let elevation=clamp(ray.y,0.0,1.0);
    var colour=mix(sky.horizon.rgb,sky.zenith.rgb,pow(elevation,0.55));
    let toward_sun=max(dot(ray,sky.sun.xyz),0.0);
    colour+=vec3(0.5,0.28,0.11)*pow(toward_sun,18.0)*sky.clouds.z*(1.0-sky.clouds.w);
    // Project a high cloud layer, gently fading near the horizon to avoid a
    // stretched/infinite texture. Wind uses the simulation clock, including pause.
    let plane=ray.xz/max(ray.y,0.10)*2.0+vec2(sky.clouds.y,sky.clouds.y*0.37);
    let cloud=smoothstep(1.0-sky.clouds.x,1.14-sky.clouds.x,clouds(plane))
        *smoothstep(0.0,0.18,elevation);
    let cloud_colour=mix(sky.horizon.rgb*0.55,vec3(0.75,0.78,0.81),sky.sun.w);
    colour=mix(colour,cloud_colour,cloud*(0.7+0.3*sky.clouds.w));
    return vec4(colour,1.0);
}
