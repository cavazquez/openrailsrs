#import bevy_pbr::forward_io::VertexOutput
struct MoonParameters { light: vec4<f32> };
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> moon: MoonParameters;
@fragment fn fragment(in: VertexOutput)->@location(0) vec4<f32> {
    let p=vec2(in.uv.x*2.-1.,1.-in.uv.y*2.); let r=dot(p,p);
    if r>1. { discard; }
    let normal=vec3(p,sqrt(max(0.,1.-r)));
    let illumination=max(dot(normal,normalize(moon.light.xyz)),0.);
    let texture=0.8+0.1*sin(p.x*32.+sin(p.y*25.))+0.08*sin(p.y*43.+p.x*17.);
    return vec4(vec3(0.92,0.93,0.87)*texture*(0.012+illumination*1.3),moon.light.w);
}
