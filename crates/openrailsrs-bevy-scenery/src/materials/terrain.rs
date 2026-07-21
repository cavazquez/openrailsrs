//! Dual-texture terrain material for viewer3d (fog #39 + shadows #42).
//!
//! Pipeline: [`crate::materials::TerrainPipelineFlags::VIEWER`] — lit, fog,
//! no night/VSM uniforms. GPU layout stays a single `overlay_scale` uniform so
//! existing viewer bind groups keep working. Shared CPU keys/UV/sanitize live
//! in [`crate::terrain`]; fragment lighting shares `terrain_common.wgsl` (#121).

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::render::render_resource::{RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

use super::TerrainPipelineFlags;

pub const TERRAIN_SHADER_PATH: &str = "shaders/terrain.wgsl";

/// OR-style terrain: base TERRTEX layer + micro-detail overlay (viewer pipeline).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerrainMaterial {
    #[uniform(0)]
    pub overlay_scale: f32,
    #[texture(1)]
    #[sampler(2)]
    pub base_texture: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub overlay_texture: Handle<Image>,
}

impl TerrainMaterial {
    /// Documented pipeline flags for this material type (not packed into GPU yet).
    pub const PIPELINE_FLAGS: TerrainPipelineFlags = TerrainPipelineFlags::VIEWER;
}

impl Material for TerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER_PATH.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}
