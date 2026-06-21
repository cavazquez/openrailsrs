//! MSTS shape mesh builders and scenery material helpers.

pub mod anim;
pub mod material;
pub mod mesh;

pub use anim::{
    ShapeAnimBinding, ShapeAnimState, animated_hierarchy_transform, animation_pose_matrices,
    lever_entity_transform_at_mesh_center, lever_entity_transform_rebased, update_world_shape_anim,
};

pub use material::{
    DARK_TEXTURE_LUMA_THRESHOLD, SCENERY_TEXTURE_ALBEDO_BOOST, SCENERY_TEXTURE_TARGET_LUMA,
    ace_mean_luma, alpha_mode_from_prim_state, apply_msts_vertex_tint, apply_z_buf_mode,
    brighten_cab_ace_rgba, brighten_dark_ace_rgba, cab_ace_brighten_enabled, cab_albedo_tint,
    cab_interior_albedo_boost, cab_or_scenery_material_with_texture, finalize_scenery_material,
    legacy_standard_scenery_enabled, or_lighting_enabled, resolve_or_lighting, scenery_albedo_tint,
    scenery_base_tint, scenery_material_tint_for_ace, scenery_materials_lit,
    scenery_uses_or_wgsl_shaders, shader_uses_vertex_color_multiply, shape_alpha_mode,
    shape_shader_requests_blending, texture_name_suggests_transparency,
};
pub use mesh::{
    LoadedShape, LoadedShapePart, MeshBuffers, MeshVertexColorMode, MeshVertexColorStats,
    append_primitive_mesh_buffers, build_mesh_from_shape, build_mesh_from_shape_at_distance,
    build_mesh_from_shape_lod, build_mesh_parts_from_shape,
    build_mesh_parts_from_shape_at_distance, build_mesh_parts_from_shape_lod, closest_lod_level,
    light_mat_idx_for_prim_state, lod_level_for_distance, lod_level_index_for_distance, mesh_aabb,
    mesh_buffers_bounds, mesh_has_uvs, mesh_uv_aabb, mesh_uv_degenerate, mesh_vertex_color_stats,
    msts_shape_to_train_rotation, primary_texture_filename, shader_name_for_prim_state,
    texture_for_prim_state,
};
pub use openrailsrs_or_shader::coordinates::shape_point_to_bevy;
