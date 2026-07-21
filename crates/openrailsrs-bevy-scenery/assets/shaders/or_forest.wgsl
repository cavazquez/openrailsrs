// Open Rails SceneryShader.fx — VSForest + PSVegetation (camera-facing billboards).
#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
    forward_io::{Vertex, VertexOutput},
    mesh_view_bindings as view_bindings,
    pbr_functions,
}

struct OrForestParams {
    reference_alpha: f32,
    ambient: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OrForestParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    // Tree base is stored in POSITION (same for all corners of a tree).
    var world_pos = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4(vertex.position, 1.0),
    ).xyz;

    // OR: EyeVector = normalize(View.M13,M23,M33); Side = normalize(cross(Eye, Down)).
    // Bevy camera looks down -Z of world_from_view.
    let eye = normalize(-view_bindings::view.world_from_view[2].xyz);
    var side = cross(eye, vec3(0.0, -1.0, 0.0));
    let side_len = length(side);
    if (side_len < 1e-4) {
        side = vec3(1.0, 0.0, 0.0);
    } else {
        side = side / side_len;
    }
    let up = vec3(0.0, 1.0, 0.0);

    // NORMAL.xy carries tree width/height (OR ForestPrimitive packing).
    let width = vertex.normal.x;
    let height = vertex.normal.y;
    world_pos += (vertex.uv.x - 0.5) * side * width;
    // OR: (uv.y - 1) * (0,-1,0) * height  ≡  (1 - uv.y) * up * height
    world_pos += (1.0 - vertex.uv.y) * up * height;

    out.world_position = vec4(world_pos, 1.0);
    out.position = position_world_to_clip(world_pos);
    out.world_normal = eye;
    out.uv = vertex.uv;
#ifdef VERTEX_COLORS
    out.color = vec4(1.0);
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif

    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(base_texture, base_sampler, in.uv);
    if (params.reference_alpha > 0.01 && color.a < params.reference_alpha) {
        discard;
    }
    // Soft vegetation lighting (OR Normal_Light carries Eye + N·L term; keep simple).
    let rgb = color.rgb * params.ambient;
    var out_color = vec4(rgb, color.a);
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
