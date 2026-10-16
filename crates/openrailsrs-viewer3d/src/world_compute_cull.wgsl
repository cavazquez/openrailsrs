// One invocation per placement, rather than one visibility test per mesh vertex.
struct Params {
    clip_from_group: mat4x4<f32>,
    center: vec4<f32>,
    half: vec4<f32>,
    counts: vec4<u32>,
};
@group(0) @binding(0) var<storage, read> original: array<mat4x4<f32>>;
@group(0) @binding(1) var<storage, read_write> visible: array<mat4x4<f32>>;
@group(0) @binding(2) var<storage, read_write> indirect: array<atomic<u32>>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let index=invocation.x;
    if index>=params.counts.x { return; }
    let clip=params.clip_from_group*original[index];
    let center=clip*vec4(params.center.xyz,1.0);
    let half=params.half.xyz;
    let envelope=abs(clip[0])*half.x+abs(clip[1])*half.y+abs(clip[2])*half.z;
    // Two pixels also cover a jittered view; shared bounds must not drop edge
    // placements when the vertex pass applies temporal antialiasing jitter.
    let pixels=4.0*(abs(center.w)+envelope.w)/max(vec2<f32>(params.counts.yz),vec2(1.0));
    let extent=envelope+vec4(pixels,0.0,0.0)+vec4(0.01);
    let outside=center.x-extent.x>center.w+extent.w
        || center.x+extent.x< -center.w-extent.w
        || center.y-extent.y>center.w+extent.w
        || center.y+extent.y< -center.w-extent.w
        || center.z-extent.z>center.w+extent.w
        || center.z+extent.z<0.0;
    if !outside {
        let destination=atomicAdd(&indirect[1],1u);
        visible[destination]=original[index];
    }
}
