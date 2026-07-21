//! MSTS shape mesh builders and scenery material helpers.

pub mod anim;
pub mod debug;
pub mod material;
pub mod mesh;

pub use anim::{
    ShapeAnimBinding, ShapeAnimState, animated_hierarchy_transform, animation_playback_speed,
    animation_pose_matrices, lever_entity_transform_at_mesh_center, lever_entity_transform_rebased,
    shape_has_loop_animation, update_world_shape_anim, world_baked_anim_transform,
};
pub use debug::{
    DebugFaceColorMode, MSTS_Z_BIAS_CLAMP, MSTS_Z_BIAS_WARN_ABS, ShapeMaterialDebugCtx,
    apply_shape_debug_material_overrides, apply_train_debug_material_overrides,
    clamp_msts_z_bias_for_bevy, debug_consist_enabled, debug_cull_front, debug_cull_normal,
    debug_face_color_mode, debug_flip_u, debug_flip_uv, debug_flip_v, debug_force_double_sided,
    debug_force_opaque, debug_force_single_sided, debug_force_unlit, debug_materials_enabled,
    debug_no_uv_flip, debug_shape_stats_enabled, debug_vehicle_transforms_enabled,
    log_shape_material_debug, mesh_position_count, mesh_triangle_list_valid,
    set_train_shape_debug_scope, shape_uv_to_bevy, train_debug_flip_winding_active,
    train_shape_debug_active, train_shape_debug_scope,
};

pub use material::{
    DARK_TEXTURE_LUMA_THRESHOLD, SCENERY_TEXTURE_ALBEDO_BOOST, SCENERY_TEXTURE_TARGET_LUMA,
    ace_mean_luma, alpha_mode_from_prim_state, apply_msts_vertex_tint,
    apply_train_exterior_culling, apply_z_buf_mode, brighten_cab_ace_rgba, brighten_dark_ace_rgba,
    cab_ace_brighten_enabled, cab_albedo_tint, cab_interior_albedo_boost,
    cab_or_scenery_material_with_texture, finalize_scenery_material,
    legacy_standard_scenery_enabled, or_lighting_enabled, resolve_or_lighting, scenery_albedo_tint,
    scenery_base_tint, scenery_material_tint_for_ace, scenery_materials_lit,
    scenery_uses_or_wgsl_shaders, shader_uses_vertex_color_multiply, shape_alpha_mode,
    shape_shader_requests_blending, texture_name_suggests_transparency,
    train_exterior_material_with_texture,
};
pub use mesh::{
    LoadedShape, LoadedShapePart, MeshBuffers, MeshVertexColorMode, MeshVertexColorStats,
    append_primitive_mesh_buffers, build_mesh_from_shape, build_mesh_from_shape_at_distance,
    build_mesh_from_shape_lod, build_mesh_parts_from_shape,
    build_mesh_parts_from_shape_at_distance, build_mesh_parts_from_shape_lod, closest_lod_level,
    light_mat_idx_for_prim_state, lod_level_for_distance, lod_level_index_for_distance, mesh_aabb,
    mesh_buffers_bounds, mesh_has_uvs, mesh_uv_aabb, mesh_uv_degenerate, mesh_vertex_color_stats,
    msts_shape_to_train_rotation, primary_texture_filename, shader_name_for_prim_state,
    texture_for_prim_state, write_mesh_wavefront, write_shape_wavefront_from_path,
};
pub use openrailsrs_or_shader::coordinates::shape_point_to_bevy;
