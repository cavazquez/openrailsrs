use core::num::NonZero;

use super::OitBuffers;
use crate::{FullscreenShader, oit::OrderIndependentTransparencySettings, prepass::DepthPrepass};
use bevy_app::Plugin;
use bevy_asset::{AssetServer, embedded_asset, load_embedded_asset};
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_log::warn;
use bevy_platform::collections::HashMap;
use bevy_render::{
    Render, RenderApp, RenderSystems,
    camera::ExtractedCamera,
    render_resource::{
        BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
        BlendComponent, BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites,
        DownlevelFlags, FragmentState, PipelineCache, RenderPipelineDescriptor, ShaderStages,
        TextureFormat,
        binding_types::{
            storage_buffer_read_only_sized, storage_buffer_sized, texture_depth_2d, uniform_buffer,
        },
    },
    renderer::{RenderAdapter, RenderDevice},
    view::{ExtractedView, ViewUniform, ViewUniforms},
};
use bevy_shader::ShaderDefVal;
use bevy_utils::default;

/// Contains the render node used to run the resolve pass.
pub mod node;

/// Minimum required value of `wgpu::Limits::max_storage_buffers_per_shader_stage`.
pub const OIT_REQUIRED_STORAGE_BUFFERS: u32 = 3;

/// Plugin needed to resolve the Order Independent Transparency (OIT) buffer to the screen.
pub struct OitResolvePlugin;
impl Plugin for OitResolvePlugin {
    fn build(&self, app: &mut bevy_app::App) {
        embedded_asset!(app, "oit_resolve.wgsl");
    }

    fn finish(&self, app: &mut bevy_app::App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        if !is_oit_supported(
            render_app.world().resource::<RenderAdapter>(),
            render_app.world().resource::<RenderDevice>(),
            true,
        ) {
            return;
        }

        render_app
            .add_systems(
                Render,
                (
                    queue_oit_resolve_pipeline.in_set(RenderSystems::Queue),
                    prepare_oit_resolve_bind_group.in_set(RenderSystems::PrepareBindGroups),
                ),
            )
            .insert_resource(OitResolvePipeline::new());
    }
}

pub fn is_oit_supported(adapter: &RenderAdapter, device: &RenderDevice, warn: bool) -> bool {
    if !adapter
        .get_downlevel_capabilities()
        .flags
        .contains(DownlevelFlags::FRAGMENT_WRITABLE_STORAGE)
    {
        if warn {
            warn!(
                "OrderIndependentTransparencyPlugin not loaded. GPU lacks support: DownlevelFlags::FRAGMENT_WRITABLE_STORAGE."
            );
        }
        return false;
    }

    let max_storage_buffers_per_shader_stage = device.limits().max_storage_buffers_per_shader_stage;

    if max_storage_buffers_per_shader_stage < OIT_REQUIRED_STORAGE_BUFFERS {
        if warn {
            warn!(
                max_storage_buffers_per_shader_stage,
                OIT_REQUIRED_STORAGE_BUFFERS,
                "OrderIndependentTransparencyPlugin not loaded. RenderDevice lacks support: max_storage_buffers_per_shader_stage < OIT_REQUIRED_STORAGE_BUFFERS."
            );
        }
        return false;
    }

    true
}

/// Bind group for the OIT resolve pass.
#[derive(Resource, Deref)]
pub struct OitResolveBindGroup(pub BindGroup);

/// Bind group layouts used for the OIT resolve pass.
#[derive(Resource)]
pub struct OitResolvePipeline {
    /// View bind group layout.
    pub view_bind_group_layout: BindGroupLayoutDescriptor,
    /// Depth bind group layout.
    pub oit_depth_bind_group_layout: BindGroupLayoutDescriptor,
}

impl OitResolvePipeline {
    fn new() -> Self {
        let view_bind_group_layout = BindGroupLayoutDescriptor::new(
            "oit_resolve_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<ViewUniform>(true),
                    // nodes
                    storage_buffer_read_only_sized(false, None),
                    // heads
                    storage_buffer_sized(false, None),
                    // atomic_counter
                    storage_buffer_sized(false, NonZero::<u64>::new(size_of::<u32>() as u64)),
                ),
            ),
        );

        let oit_depth_bind_group_layout = BindGroupLayoutDescriptor::new(
            "oit_depth_bind_group_layout",
            &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, texture_depth_2d()),
        );
        OitResolvePipeline {
            view_bind_group_layout,
            oit_depth_bind_group_layout,
        }
    }
}

#[derive(Component, Deref, Clone, Copy)]
pub struct OitResolvePipelineId(pub CachedRenderPipelineId);

/// This key is used to cache the pipeline id and to specialize the render pipeline descriptor.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct OitResolvePipelineKey {
    target_format: TextureFormat,
    sorted_fragment_max_count: u32,
    depth_prepass: bool,
}

// PipelineCache retains queued variants for the lifetime of the render app. Cache
// the complete descriptor key independently of camera entities so replacing a
// camera, or returning A -> B -> A, reuses the existing variant.
fn cached_oit_resolve_pipeline(
    pipelines: &mut HashMap<OitResolvePipelineKey, CachedRenderPipelineId>,
    key: OitResolvePipelineKey,
    queue: impl FnOnce() -> CachedRenderPipelineId,
) -> CachedRenderPipelineId {
    *pipelines.entry(key).or_insert_with(queue)
}

pub fn queue_oit_resolve_pipeline(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    resolve_pipeline: Res<OitResolvePipeline>,
    cameras: Query<
        (
            Entity,
            &ExtractedView,
            &OrderIndependentTransparencySettings,
            Has<DepthPrepass>,
        ),
        (
            With<OrderIndependentTransparencySettings>,
            With<ExtractedCamera>,
        ),
    >,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
    mut cached_pipeline_ids: Local<HashMap<OitResolvePipelineKey, CachedRenderPipelineId>>,
) {
    for (e, view, oit_settings, depth_prepass) in &cameras {
        let key = OitResolvePipelineKey {
            target_format: view.target_format,
            sorted_fragment_max_count: oit_settings.sorted_fragment_max_count,
            depth_prepass,
        };

        let pipeline_id = cached_oit_resolve_pipeline(&mut cached_pipeline_ids, key, || {
            let desc = specialize_oit_resolve_pipeline(
                key,
                &resolve_pipeline,
                &fullscreen_shader,
                &asset_server,
            );
            pipeline_cache.queue_render_pipeline(desc)
        });
        commands.entity(e).insert(OitResolvePipelineId(pipeline_id));
    }
}

fn specialize_oit_resolve_pipeline(
    key: OitResolvePipelineKey,
    resolve_pipeline: &OitResolvePipeline,
    fullscreen_shader: &FullscreenShader,
    asset_server: &AssetServer,
) -> RenderPipelineDescriptor {
    let mut layout = vec![resolve_pipeline.view_bind_group_layout.clone()];
    let mut shader_defs = vec![ShaderDefVal::UInt(
        "SORTED_FRAGMENT_MAX_COUNT".into(),
        key.sorted_fragment_max_count,
    )];
    if key.depth_prepass {
        shader_defs.push(ShaderDefVal::Bool("DEPTH_PREPASS".into(), true));
    } else {
        layout.push(resolve_pipeline.oit_depth_bind_group_layout.clone());
    }

    RenderPipelineDescriptor {
        label: Some("oit_resolve_pipeline".into()),
        layout,
        fragment: Some(FragmentState {
            shader: load_embedded_asset!(asset_server, "oit_resolve.wgsl"),
            shader_defs,
            targets: vec![Some(ColorTargetState {
                format: key.target_format,
                blend: Some(BlendState {
                    color: BlendComponent::OVER,
                    alpha: BlendComponent::OVER,
                }),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        vertex: fullscreen_shader.to_vertex_state(),
        ..default()
    }
}

pub fn prepare_oit_resolve_bind_group(
    mut commands: Commands,
    resolve_pipeline: Res<OitResolvePipeline>,
    render_device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<OitBuffers>,
) {
    if let (
        Some(view_binding),
        Some(nodes_binding),
        Some(heads_binding),
        Some(atomic_counter_binding),
    ) = (
        view_uniforms.uniforms.binding(),
        buffers.nodes.binding(),
        buffers.heads.binding(),
        buffers.atomic_counter.binding(),
    ) {
        let bind_group = render_device.create_bind_group(
            "oit_resolve_bind_group",
            &pipeline_cache.get_bind_group_layout(&resolve_pipeline.view_bind_group_layout),
            &BindGroupEntries::sequential((
                view_binding,
                nodes_binding,
                heads_binding,
                atomic_counter_binding,
            )),
        );
        commands.insert_resource(OitResolveBindGroup(bind_group));
    }
}

#[cfg(test)]
mod oit_cache_regressions {
    use super::*;

    fn key(
        target_format: TextureFormat,
        sorted_fragment_max_count: u32,
        depth_prepass: bool,
    ) -> OitResolvePipelineKey {
        OitResolvePipelineKey {
            target_format,
            sorted_fragment_max_count,
            depth_prepass,
        }
    }

    // The callback stands in for PipelineCache::queue_render_pipeline. These
    // tests check the production cache's decision to queue, without a GPU.
    fn variant(
        cache: &mut HashMap<OitResolvePipelineKey, CachedRenderPipelineId>,
        queued: &mut usize,
        key: OitResolvePipelineKey,
    ) -> CachedRenderPipelineId {
        cached_oit_resolve_pipeline(cache, key, || {
            let id = CachedRenderPipelineId::new(*queued);
            *queued += 1;
            id
        })
    }

    #[test]
    fn returning_to_previous_oit_configuration_reuses_the_pipeline() {
        let mut cache = HashMap::default();
        let mut queued = 0;
        let a = key(TextureFormat::Rgba8UnormSrgb, 8, false);
        let b = key(TextureFormat::Rgba8UnormSrgb, 8, true);

        let first_a = variant(&mut cache, &mut queued, a);
        let first_b = variant(&mut cache, &mut queued, b);
        let returned_a = variant(&mut cache, &mut queued, a);
        let returned_b = variant(&mut cache, &mut queued, b);

        assert_eq!(returned_a, first_a);
        assert_eq!(returned_b, first_b);
        assert_ne!(first_a, first_b);
        assert_eq!(queued, 2);
    }

    #[test]
    fn replacing_camera_entities_reuses_the_same_oit_configuration() {
        let mut cache = HashMap::default();
        let mut queued = 0;
        let key = key(TextureFormat::Rgba8UnormSrgb, 8, false);
        let mut world = World::new();
        let first_camera = world.spawn_empty().id();
        let first_pipeline = variant(&mut cache, &mut queued, key);

        assert!(world.despawn(first_camera));
        let replacement_camera = world.spawn_empty().id();
        assert_ne!(first_camera, replacement_camera);
        let replacement_pipeline = variant(&mut cache, &mut queued, key);

        assert_eq!(replacement_pipeline, first_pipeline);
        assert_eq!(queued, 1);
    }

    #[test]
    fn all_oit_descriptor_key_parameters_keep_variants_separate() {
        let mut cache = HashMap::default();
        let mut queued = 0;
        let mut keys = Vec::new();
        let mut ids = Vec::new();
        for format in [TextureFormat::Rgba8UnormSrgb, TextureFormat::Rgba16Float] {
            for max_count in [8, 16] {
                for depth_prepass in [false, true] {
                    let key = key(format, max_count, depth_prepass);
                    keys.push(key);
                    ids.push(variant(&mut cache, &mut queued, key));
                }
            }
        }

        let unique_ids = ids.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique_ids.len(), 8);
        assert_eq!(queued, 8);
        for (key, original) in keys.into_iter().zip(ids) {
            assert_eq!(variant(&mut cache, &mut queued, key), original);
        }
        assert_eq!(queued, 8);
    }
}
