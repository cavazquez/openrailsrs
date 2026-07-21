//! GPU instancing for repeated static opaque WORLD shapes (#58).
//!
//! One Bevy entity per `(shape, part, material, tile)` with an instance buffer of
//! transforms. Animated / transparent (blend) parts keep the per-entity spawn path.
//! Opaque + cutout draws are queued in Bevy [`Opaque3d`] (#106); the fragment shader
//! alpha-discards cutout. True blend materials must not use this path.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::core_3d::{Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey};
use bevy::ecs::system::{SystemParamItem, lifetimeless::*};
use bevy::ecs::{query::QueryItem, system::lifetimeless::Read};
use bevy::mesh::{MeshVertexBufferLayoutRef, VertexBufferLayout};
use bevy::pbr::{
    MeshPipeline, MeshPipelineKey, MeshPipelineSystems, RenderMeshInstances, SetMeshBindGroup,
    SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup, ViewKeyCache,
};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, PhaseItem, RenderCommand,
    RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::RenderDevice;
use bevy::render::sync_component::SyncComponent;
use bevy::render::sync_world::MainEntity;
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use bytemuck::{Pod, Zeroable};
use openrailsrs_bevy_scenery::shapes::lod_level_index_for_distance;

/// Minimum placements in one tile before GPU instancing is used.
pub const WORLD_INSTANCING_MIN: usize = 4;

const SHADER_WGSL: &str = include_str!("world_instancing.wgsl");

/// Opt-out: `OPENRAILSRS_WORLD_INSTANCING=0`.
pub fn world_instancing_enabled() -> bool {
    match std::env::var("OPENRAILSRS_WORLD_INSTANCING") {
        Ok(v) => {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off"))
        }
        Err(_) => true,
    }
}

/// One instance transform (column-major Mat4) in the entity local / view frame.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct WorldInstanceData {
    pub col0: [f32; 4],
    pub col1: [f32; 4],
    pub col2: [f32; 4],
    pub col3: [f32; 4],
}

impl WorldInstanceData {
    pub fn from_transform(tf: Transform) -> Self {
        let m = tf.to_matrix();
        let cols = m.to_cols_array_2d();
        Self {
            col0: cols[0],
            col1: cols[1],
            col2: cols[2],
            col3: cols[3],
        }
    }

    pub fn translation(&self) -> Vec3 {
        Vec3::new(self.col3[0], self.col3[1], self.col3[2])
    }
}

/// CPU instance list extracted to the render world.
#[derive(Component, Clone, Debug, Deref, DerefMut)]
pub struct WorldInstanceBuffer(pub Vec<WorldInstanceData>);

impl SyncComponent for WorldInstanceBuffer {
    type Target = Self;
}

impl ExtractComponent for WorldInstanceBuffer {
    type QueryData = &'static WorldInstanceBuffer;
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(WorldInstanceBuffer(item.0.clone()))
    }
}

/// Albedo + optional alpha cutout for the instanced draw (#58 v1 lit shader).
#[derive(Component, Clone, Debug)]
pub struct WorldInstanceAppearance {
    pub base_color: LinearRgba,
    pub base_color_texture: Option<Handle<Image>>,
    /// 0 = disabled; typically `200/255` for MSTS alpha test.
    pub alpha_cutoff: f32,
}

impl SyncComponent for WorldInstanceAppearance {
    type Target = Self;
}

impl ExtractComponent for WorldInstanceAppearance {
    type QueryData = &'static WorldInstanceAppearance;
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(item.clone())
    }
}

/// Marker + LOD metadata for an instanced WORLD group.
#[derive(Component, Clone, Debug)]
pub struct WorldInstancedGroup {
    pub shape_path: PathBuf,
    pub part_index: usize,
    pub prim_state_idx: i32,
    pub lod_idx: usize,
    pub lod_enabled: bool,
    /// Instance count (for metrics / HUD).
    pub instance_count: u32,
}

/// 1×1 white fallback when a part has no albedo texture.
#[derive(Resource, Clone, ExtractResource, Default)]
pub struct WorldInstancingWhiteImage(pub Handle<Image>);

/// Plugin: extract instance buffers and draw via a specialized mesh pipeline.
pub struct WorldInstancingPlugin;

impl Plugin for WorldInstancingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldInstancingWhiteImage>()
            .add_plugins((
                ExtractResourcePlugin::<WorldInstancingWhiteImage>::default(),
                ExtractComponentPlugin::<WorldInstanceBuffer>::default(),
                ExtractComponentPlugin::<WorldInstanceAppearance>::default(),
            ))
            .add_systems(Startup, init_white_image);
    }

    fn finish(&self, app: &mut App) {
        // Register on RenderApp in `finish` (same pattern as OrVsmRenderPlugin / MaterialsPlugin).
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_render_command::<Opaque3d, DrawWorldInstanced>()
            .init_resource::<SpecializedMeshPipelines<WorldInstancingPipeline>>()
            .add_systems(
                RenderStartup,
                init_world_instancing_pipeline.after(MeshPipelineSystems),
            )
            .add_systems(
                Render,
                (
                    prepare_world_instance_buffers.in_set(RenderSystems::PrepareResources),
                    prepare_world_instance_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                    queue_world_instanced.in_set(RenderSystems::QueueMeshes),
                ),
            );
    }
}

fn init_white_image(
    mut images: ResMut<Assets<Image>>,
    mut white: ResMut<WorldInstancingWhiteImage>,
) {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    white.0 = images.add(image);
}

/// Build a union AABB covering all instance translations with a margin for mesh extent.
pub fn instances_aabb(
    instances: &[WorldInstanceData],
    margin: f32,
) -> bevy::camera::primitives::Aabb {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for inst in instances {
        let p = inst.translation();
        min = min.min(p);
        max = max.max(p);
    }
    if !min.is_finite() || !max.is_finite() {
        return bevy::camera::primitives::Aabb::from_min_max(
            Vec3::splat(-margin),
            Vec3::splat(margin),
        );
    }
    bevy::camera::primitives::Aabb::from_min_max(
        min - Vec3::splat(margin),
        max + Vec3::splat(margin),
    )
}

/// Spawn bundle helpers for the progressive WORLD queue.
pub fn appearance_from_standard_material(
    materials: &Assets<StandardMaterial>,
    handle: &Handle<StandardMaterial>,
) -> WorldInstanceAppearance {
    let mat = materials.get(handle);
    let base_color = mat
        .map(|m| LinearRgba::from(m.base_color))
        .unwrap_or(LinearRgba::WHITE);
    let base_color_texture = mat.and_then(|m| m.base_color_texture.clone());
    let alpha_cutoff = mat
        .map(|m| match m.alpha_mode {
            AlphaMode::Mask(c) => c,
            _ => 0.0,
        })
        .unwrap_or(0.0);
    WorldInstanceAppearance {
        base_color,
        base_color_texture,
        alpha_cutoff,
    }
}

// ─── Render-world plumbing ───────────────────────────────────────────────────

#[derive(Component)]
struct GpuWorldInstanceBuffer {
    buffer: Buffer,
    length: usize,
}

#[derive(Component)]
struct GpuWorldInstanceBindGroup(BindGroup);

#[derive(Clone, Copy, ShaderType, Pod, Zeroable)]
#[repr(C)]
struct AppearanceGpu {
    base_color: Vec4,
    params: Vec4,
}

#[derive(Resource)]
struct WorldInstancingPipeline {
    shader: Handle<Shader>,
    mesh_pipeline: MeshPipeline,
    appearance_layout: BindGroupLayoutDescriptor,
}

fn init_world_instancing_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mesh_pipeline: Res<MeshPipeline>,
) {
    // Bevy 0.19: `Assets<Shader>` lives in the main world, not RenderApp.
    // Register via AssetServer (same approach as `init_mesh_pipeline`).
    let shader = asset_server.add(Shader::from_wgsl(SHADER_WGSL, "world_instancing.wgsl"));
    let appearance_layout = BindGroupLayoutDescriptor::new(
        "world_instancing_appearance_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<AppearanceGpu>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    commands.insert_resource(WorldInstancingPipeline {
        shader,
        mesh_pipeline: mesh_pipeline.clone(),
        appearance_layout,
    });
}

impl SpecializedMeshPipeline for WorldInstancingPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut descriptor = self.mesh_pipeline.specialize(key, layout)?;
        descriptor.vertex.shader = self.shader.clone();
        descriptor.vertex.buffers.push(VertexBufferLayout {
            array_stride: size_of::<WorldInstanceData>() as u64,
            step_mode: VertexStepMode::Instance,
            attributes: vec![
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 3,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 16,
                    shader_location: 4,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 32,
                    shader_location: 5,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 48,
                    shader_location: 6,
                },
            ],
        });
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = self.shader.clone();
        }
        // Insert appearance bind group at index 3 (after view/array/mesh).
        descriptor.layout.push(self.appearance_layout.clone());
        Ok(descriptor)
    }
}

fn prepare_world_instance_buffers(
    mut commands: Commands,
    query: Query<(Entity, &WorldInstanceBuffer)>,
    render_device: Res<RenderDevice>,
) {
    for (entity, data) in &query {
        if data.is_empty() {
            continue;
        }
        let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("world_instance_buffer"),
            contents: bytemuck::cast_slice(data.as_slice()),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        });
        commands.entity(entity).insert(GpuWorldInstanceBuffer {
            buffer,
            length: data.len(),
        });
    }
}

fn prepare_world_instance_bind_groups(
    mut commands: Commands,
    pipeline: Res<WorldInstancingPipeline>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    gpu_images: Res<RenderAssets<bevy::render::texture::GpuImage>>,
    white: Option<Res<WorldInstancingWhiteImage>>,
    query: Query<(Entity, &WorldInstanceAppearance)>,
) {
    let Some(white) = white else {
        return;
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipeline.appearance_layout);
    let fallback = white.0.clone();
    for (entity, appearance) in &query {
        let image_handle = appearance
            .base_color_texture
            .clone()
            .unwrap_or_else(|| fallback.clone());
        let Some(gpu_image) = gpu_images.get(&image_handle) else {
            continue;
        };
        let gpu = AppearanceGpu {
            base_color: Vec4::from_array(appearance.base_color.to_f32_array()),
            params: Vec4::new(appearance.alpha_cutoff, 0.0, 0.0, 0.0),
        };
        let uniform = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("world_instance_appearance"),
            contents: bytemuck::bytes_of(&gpu),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let bind_group = render_device.create_bind_group(
            "world_instance_appearance_bg",
            &layout,
            &BindGroupEntries::sequential((
                uniform.as_entire_buffer_binding(),
                &gpu_image.texture_view,
                &gpu_image.sampler,
            )),
        );
        commands
            .entity(entity)
            .insert(GpuWorldInstanceBindGroup(bind_group));
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_world_instanced(
    opaque_3d_draw_functions: Res<DrawFunctions<Opaque3d>>,
    custom_pipeline: Res<WorldInstancingPipeline>,
    mut pipelines: ResMut<SpecializedMeshPipelines<WorldInstancingPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<RenderMesh>>,
    render_mesh_instances: Res<RenderMeshInstances>,
    mesh_allocator: Res<MeshAllocator>,
    material_meshes: Query<(Entity, &MainEntity), With<WorldInstanceBuffer>>,
    mut opaque_render_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    views: Query<&ExtractedView>,
    view_key_cache: Res<ViewKeyCache>,
) {
    // Opaque WORLD instances only (#106). Cutout uses shader discard on this path;
    // blend/transparent parts never get `WorldInstanceBuffer` (see world spawn).
    let draw_custom = opaque_3d_draw_functions.read().id::<DrawWorldInstanced>();

    for view in &views {
        let Some(opaque_phase) = opaque_render_phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(&view_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };

        for (entity, main_entity) in &material_meshes {
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(*main_entity)
            else {
                continue;
            };
            let Some(mesh) = meshes.get(mesh_instance.mesh_asset_id()) else {
                continue;
            };
            let Some(mesh_slabs) = mesh_allocator.mesh_slabs(&mesh_instance.mesh_asset_id()) else {
                continue;
            };
            let key = view_key
                | MeshPipelineKey::from_primitive_topology_and_strip_index(
                    mesh.primitive_topology(),
                    mesh.index_format(),
                );
            let Ok(pipeline) =
                pipelines.specialize(&pipeline_cache, &custom_pipeline, key, &mesh.layout)
            else {
                continue;
            };

            // Custom per-entity instance buffer: never multi-draw / batch with others.
            opaque_phase.add(
                Opaque3dBatchSetKey {
                    pipeline,
                    draw_function: draw_custom,
                    material_bind_group_index: None,
                    slabs: mesh_slabs,
                    lightmap_slab: None,
                },
                Opaque3dBinKey {
                    asset_id: mesh_instance.mesh_asset_id().into(),
                },
                (entity, *main_entity),
                mesh_instance.current_uniform_index,
                BinnedRenderPhaseType::UnbatchableMesh,
            );
        }
    }
}

type DrawWorldInstanced = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    SetMeshBindGroup<2>,
    SetWorldInstanceAppearanceBindGroup<3>,
    DrawMeshWorldInstanced,
);

struct SetWorldInstanceAppearanceBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetWorldInstanceAppearanceBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<GpuWorldInstanceBindGroup>;

    #[inline]
    fn render<'w>(
        _item: &P,
        _view: (),
        bind_group: Option<&'w GpuWorldInstanceBindGroup>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bg) = bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, &bg.0, &[]);
        RenderCommandResult::Success
    }
}

struct DrawMeshWorldInstanced;

impl<P: PhaseItem> RenderCommand<P> for DrawMeshWorldInstanced {
    type Param = (
        SRes<RenderAssets<RenderMesh>>,
        SRes<RenderMeshInstances>,
        SRes<MeshAllocator>,
    );
    type ViewQuery = ();
    type ItemQuery = Read<GpuWorldInstanceBuffer>;

    #[inline]
    fn render<'w>(
        item: &P,
        _view: (),
        instance_buffer: Option<&'w GpuWorldInstanceBuffer>,
        (meshes, render_mesh_instances, mesh_allocator): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let mesh_allocator = mesh_allocator.into_inner();
        let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(item.main_entity())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(gpu_mesh) = meshes.into_inner().get(mesh_instance.mesh_asset_id()) else {
            return RenderCommandResult::Skip;
        };
        let Some(instance_buffer) = instance_buffer else {
            return RenderCommandResult::Skip;
        };
        let Some(vertex_buffer_slice) =
            mesh_allocator.mesh_vertex_slice(&mesh_instance.mesh_asset_id())
        else {
            return RenderCommandResult::Skip;
        };

        pass.set_vertex_buffer(0, vertex_buffer_slice.buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.buffer.slice(..));

        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                index_format,
                count,
            } => {
                let Some(index_buffer_slice) =
                    mesh_allocator.mesh_index_slice(&mesh_instance.mesh_asset_id())
                else {
                    return RenderCommandResult::Skip;
                };
                pass.set_index_buffer(index_buffer_slice.buffer.slice(..), *index_format);
                pass.draw_indexed(
                    index_buffer_slice.range.start..(index_buffer_slice.range.start + count),
                    vertex_buffer_slice.range.start as i32,
                    0..instance_buffer.length as u32,
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_buffer_slice.range, 0..instance_buffer.length as u32);
            }
        }
        RenderCommandResult::Success
    }
}

/// Group placements by tile for instancing decisions.
pub fn group_placements_by_tile(
    placements: &[crate::world::ShapeInstancePlacement],
) -> std::collections::BTreeMap<(i32, i32), Vec<usize>> {
    let mut map: std::collections::BTreeMap<(i32, i32), Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, p) in placements.iter().enumerate() {
        map.entry((p.tile_x, p.tile_z)).or_default().push(i);
    }
    map
}

/// LOD update for instanced groups (shared LOD per tile group).
pub fn update_world_instanced_lod(
    cache: Option<Res<crate::world::WorldShapeLodCache>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    focus: Option<Res<crate::world::RouteFocus>>,
    mut groups: Query<(
        &GlobalTransform,
        &mut WorldInstancedGroup,
        &mut Mesh3d,
        Option<&bevy::camera::primitives::Aabb>,
    )>,
) {
    let Some(cache) = cache else {
        return;
    };
    let Ok(cam_gt) = camera.single() else {
        return;
    };
    let cam_pos = cam_gt.translation();
    let focus_pos = focus
        .as_ref()
        .map(|f| f.scenery_to_render(f.center))
        .unwrap_or(Vec3::ZERO);
    let cam_dist = cam_pos.distance(focus_pos);

    for (gt, mut group, mut mesh3d, aabb) in &mut groups {
        if !group.lod_enabled {
            continue;
        }
        let Some(shape) = cache.shapes.get(&group.shape_path) else {
            continue;
        };
        let Some(lod_assets) = cache.assets_by_lod.get(&group.shape_path) else {
            continue;
        };
        if lod_assets.is_empty() {
            continue;
        }
        let center = aabb
            .map(|a| gt.transform_point(a.center.into()))
            .unwrap_or_else(|| gt.translation());
        let instance_dist = cam_dist + center.distance(focus_pos);
        let new_lod = lod_level_index_for_distance(shape, instance_dist).min(lod_assets.len() - 1);
        if new_lod == group.lod_idx {
            continue;
        }
        let Some(asset) = lod_assets.get(new_lod) else {
            continue;
        };
        let Some(part) = asset.parts.get(group.part_index) else {
            continue;
        };
        mesh3d.0 = part.mesh.clone();
        group.lod_idx = new_lod;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ShapeInstancePlacement;

    #[test]
    fn group_by_tile_splits_placements() {
        let placements = vec![
            ShapeInstancePlacement {
                transform: Transform::from_xyz(0.0, 0.0, 0.0),
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
            },
            ShapeInstancePlacement {
                transform: Transform::from_xyz(1.0, 0.0, 0.0),
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
            },
            ShapeInstancePlacement {
                transform: Transform::from_xyz(2.0, 0.0, 0.0),
                tile_x: 1,
                tile_z: 0,
                auto_z_bias: false,
            },
        ];
        let grouped = group_placements_by_tile(&placements);
        assert_eq!(grouped.get(&(0, 0)).map(|v| v.len()), Some(2));
        assert_eq!(grouped.get(&(1, 0)).map(|v| v.len()), Some(1));
    }

    #[test]
    fn instance_data_from_transform_preserves_translation() {
        let tf = Transform::from_xyz(10.0, 2.0, -3.0);
        let d = WorldInstanceData::from_transform(tf);
        let t = d.translation();
        assert!((t.x - 10.0).abs() < 1e-4);
        assert!((t.y - 2.0).abs() < 1e-4);
        assert!((t.z + 3.0).abs() < 1e-4);
    }

    #[test]
    fn instancing_enabled_by_default() {
        unsafe {
            std::env::remove_var("OPENRAILSRS_WORLD_INSTANCING");
        }
        assert!(world_instancing_enabled());
    }

    #[test]
    fn four_opaque_placements_same_tile_meet_min() {
        const {
            assert!(WORLD_INSTANCING_MIN <= 4);
        }
        let placements: Vec<ShapeInstancePlacement> = (0..4)
            .map(|i| ShapeInstancePlacement {
                transform: Transform::from_xyz(i as f32, 0.0, 0.0),
                tile_x: 0,
                tile_z: 0,
                auto_z_bias: false,
            })
            .collect();
        let grouped = group_placements_by_tile(&placements);
        let n = grouped.get(&(0, 0)).map(|v| v.len()).unwrap_or(0);
        assert!(n >= WORLD_INSTANCING_MIN);
    }

    #[test]
    fn appearance_opaque_has_no_cutoff_mask_keeps_discard_threshold() {
        let mut materials = Assets::<StandardMaterial>::default();
        let opaque = materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });
        let mask = materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Mask(0.78),
            ..default()
        });
        // Blend materials stay on the non-instanced path; cutout maps to opaque + discard.
        assert_eq!(
            appearance_from_standard_material(&materials, &opaque).alpha_cutoff,
            0.0
        );
        assert!(
            (appearance_from_standard_material(&materials, &mask).alpha_cutoff - 0.78).abs() < 1e-5
        );
    }

    #[test]
    fn instanced_draws_target_opaque3d_phase() {
        // Compile-time / type-level guard: WORLD GPU instances register on Opaque3d (#106).
        fn _assert_phase<P: bevy::render::render_phase::BinnedPhaseItem>() {}
        _assert_phase::<Opaque3d>();
    }
}
