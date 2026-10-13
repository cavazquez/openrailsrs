#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import "shaders/cloud_field.wgsl"::cloud_coverage
struct SkyParameters { horizon: vec4<f32>, zenith: vec4<f32>, sun: vec4<f32>, clouds: vec4<f32>, cloud_world:vec4<f32> };
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkyParameters;
@fragment fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray=normalize(in.world_position.xyz-view.world_position.xyz);
    let elevation=clamp(ray.y,0.0,1.0);
    var colour=mix(sky.horizon.rgb,sky.zenith.rgb,pow(elevation,0.55));
    let toward_sun=max(dot(ray,sky.sun.xyz),0.0);
    colour+=vec3(0.5,0.28,0.11)*pow(toward_sun,18.0)*sky.clouds.z*(1.0-sky.clouds.w);
    // Continuous directional 3D noise avoids the clamped planar projection's
    // vertical bands at low elevation. Wind remains tied to the simulation clock.
    let distance=(1200.-view.world_position.y)/max(ray.y,0.08);
    let cloud_position=view.world_position.xz+ray.xz*distance+sky.cloud_world.xy;
    let cloud=cloud_coverage(cloud_position,sky.clouds.x)*smoothstep(0.0,0.18,elevation);
    let cloud_colour=mix(sky.horizon.rgb*0.55,vec3(0.75,0.78,0.81),sky.sun.w);
    colour=mix(colour,cloud_colour,cloud*(0.7+0.3*sky.clouds.w));
    return vec4(colour,1.0);
}
