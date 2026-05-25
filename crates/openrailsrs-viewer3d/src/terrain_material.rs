//! Dual-texture terrain material (OR-style base + microtex overlay).

use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub const TERRAIN_SHADER_PATH: &str = "shaders/terrain.wgsl";

/// OR-style terrain: base TERRTEX layer + micro-detail overlay.
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

impl Material for TerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER_PATH.into()
    }
}
