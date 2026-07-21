//! Open Rails `ForestMaterial` / `VSForest` — camera-facing tree billboards (#38).

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

/// Open Rails `ForestMaterial.ReferenceAlpha = 200` (0–255).
pub const FOREST_REFERENCE_ALPHA: f32 = 200.0 / 255.0;

pub const OR_FOREST_SHADER_PATH: &str = "shaders/or_forest.wgsl";

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct OrForestGpuParams {
    pub reference_alpha: f32,
    pub ambient: f32,
    pub _pad0: f32,
    pub _pad1: f32,
}

impl Default for OrForestGpuParams {
    fn default() -> Self {
        Self {
            reference_alpha: FOREST_REFERENCE_ALPHA,
            ambient: 0.92,
            _pad0: 0.0,
            _pad1: 0.0,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct OrForestMaterial {
    #[uniform(0)]
    pub params: OrForestGpuParams,
    #[texture(1)]
    #[sampler(2)]
    pub base_texture: Handle<Image>,
    pub alpha_mode: AlphaMode,
}

impl Material for OrForestMaterial {
    fn vertex_shader() -> ShaderRef {
        OR_FOREST_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        OR_FOREST_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Billboards are viewed from both sides.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub fn create_or_forest_material(
    materials: &mut Assets<OrForestMaterial>,
    texture: Handle<Image>,
) -> Handle<OrForestMaterial> {
    materials.add(OrForestMaterial {
        params: OrForestGpuParams::default(),
        base_texture: texture,
        // Mask matches OR `clip(a - ReferenceAlpha)`.
        alpha_mode: AlphaMode::Mask(FOREST_REFERENCE_ALPHA),
    })
}
