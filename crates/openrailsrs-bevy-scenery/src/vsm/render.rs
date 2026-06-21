//! Pass de momentos VSM estilo Open Rails: depth Bevy -> Rg32 (z, z²) + blur separable.

use std::mem::size_of;

use bevy::asset::{AssetServer, Handle};
use bevy::core_pipeline::{
    FullscreenShader,
    core_3d::main_opaque_pass_3d,
    schedule::{Core3d, Core3dSystems},
};
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{LATE_SHADOW_PASS, ViewShadowBindings, per_view_shadow_pass};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    binding_types::{sampler, texture_2d_array, uniform_buffer},
    *,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;

use crate::vsm::debug_settings::OrVsmDebugSettings;

const MOMENT_ATLAS_SIZE: u32 = 2048;
const MOMENT_CASCADE_COUNT: u32 = 4;
const PACK_SHADER: &str = "shaders/or_moment_pack.wgsl";
const BLUR_SHADER: &str = "shaders/or_moment_blur.wgsl";
const PREVIEW_SHADER: &str = "shaders/or_moment_preview.wgsl";
const PREVIEW_SIZE: u32 = 320;

fn extent_pixel_count(extent: Extent3d) -> usize {
    (extent.width * extent.height * extent.depth_or_array_layers) as usize
}

/// Handle compartido del atlas Rg32 de momentos (main + render world).
#[derive(Clone, Resource, ExtractResource, Default)]
pub struct OrMomentAtlasImage(pub Handle<Image>);

/// Textura ping-pong para blur separable.
#[derive(Clone, Resource, ExtractResource, Default)]
pub struct OrMomentBlurTempImage(pub Handle<Image>);

/// RGBA8 para UI de depuracion (capa del atlas de momentos).
#[derive(Clone, Resource, ExtractResource, Default)]
pub struct OrMomentPreviewImage(pub Handle<Image>);

/// Marca camaras que deben ejecutar el pass VSM (modo `exact`).
#[derive(Component, Clone, Copy, ExtractComponent, Debug)]
pub struct OrVsmRenderSettings {
    pub enabled: bool,
}

#[derive(Clone, Copy, ShaderType, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct OrMomentPackParams {
    cascade_layer: u32,
    _pad: [u32; 3],
}

#[derive(Clone, Copy, ShaderType, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct OrMomentPreviewParams {
    cascade_layer: u32,
    _pad: u32,
}

#[derive(Clone, Copy, ShaderType, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct OrMomentBlurParams {
    blur_dir: Vec2,
    texel_size: Vec2,
    cascade_layer: u32,
    _pad: [u32; 3],
}

#[derive(Resource)]
struct OrVsmPipelines {
    pack_layout: BindGroupLayoutDescriptor,
    blur_layout: BindGroupLayoutDescriptor,
    preview_layout: BindGroupLayoutDescriptor,
    pack_pipeline: CachedRenderPipelineId,
    blur_pipeline: CachedRenderPipelineId,
    preview_pipeline: CachedRenderPipelineId,
    linear_sampler: Sampler,
}

#[derive(Component)]
struct OrVsmViewBindGroups {
    pack_bind_group: BindGroup,
    blur_src_to_temp: BindGroup,
    blur_temp_to_src: BindGroup,
    pack_params_buffer: Buffer,
    blur_params_buffer: Buffer,
}

pub struct OrVsmRenderPlugin;

impl Plugin for OrVsmRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrMomentAtlasImage>()
            .init_resource::<OrMomentBlurTempImage>()
            .init_resource::<OrMomentPreviewImage>()
            .add_plugins(ExtractResourcePlugin::<OrMomentAtlasImage>::default())
            .add_plugins(ExtractResourcePlugin::<OrMomentBlurTempImage>::default())
            .add_plugins(ExtractResourcePlugin::<OrMomentPreviewImage>::default())
            .add_plugins(ExtractResourcePlugin::<OrVsmDebugSettings>::default())
            .add_plugins(ExtractComponentPlugin::<OrVsmRenderSettings>::default())
            .add_systems(Startup, init_or_moment_textures);
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .add_systems(RenderStartup, init_or_vsm_pipelines)
            .add_systems(
                Render,
                prepare_or_vsm_view_bind_groups.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                Core3d,
                or_vsm_moment_pass
                    .after(per_view_shadow_pass::<LATE_SHADOW_PASS>)
                    .before(main_opaque_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// Crea textura Rg32 array para momentos OR.
pub fn create_moment_atlas_image(size: u32, layers: u32) -> Image {
    let format = TextureFormat::Rg32Float;
    let extent = Extent3d {
        width: size,
        height: size,
        depth_or_array_layers: layers,
    };
    let pixel_size = 8usize; // Rg32Float
    let data = vec![0u8; pixel_size * extent_pixel_count(extent)];
    Image {
        data: Some(data),
        data_order: bevy::render::render_resource::TextureDataOrder::default(),
        texture_descriptor: TextureDescriptor {
            label: Some("or_moment_atlas"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            label: Some("or_moment_atlas_sampler".into()),
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            address_mode_w: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Nearest,
            ..default()
        }),
        texture_view_descriptor: Some(TextureViewDescriptor {
            label: Some("or_moment_atlas_view"),
            dimension: Some(TextureViewDimension::D2Array),
            ..Default::default()
        }),
        ..default()
    }
}

pub fn create_moment_preview_image(size: u32) -> Image {
    let format = TextureFormat::Rgba8UnormSrgb;
    let extent = Extent3d {
        width: size,
        height: size,
        depth_or_array_layers: 1,
    };
    let data = vec![0u8; 4 * extent_pixel_count(extent)];
    Image {
        data: Some(data),
        data_order: bevy::render::render_resource::TextureDataOrder::default(),
        texture_descriptor: TextureDescriptor {
            label: Some("or_moment_preview"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            label: Some("or_moment_preview_sampler".into()),
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        }),
        ..default()
    }
}

fn init_or_moment_textures(
    mut atlas: ResMut<OrMomentAtlasImage>,
    mut blur_temp: ResMut<OrMomentBlurTempImage>,
    mut preview: ResMut<OrMomentPreviewImage>,
    mut images: ResMut<Assets<Image>>,
) {
    if atlas.0 == Handle::default() {
        atlas.0 = images.add(create_moment_atlas_image(
            MOMENT_ATLAS_SIZE,
            MOMENT_CASCADE_COUNT,
        ));
    }
    if blur_temp.0 == Handle::default() {
        blur_temp.0 = images.add(create_moment_atlas_image(
            MOMENT_ATLAS_SIZE,
            MOMENT_CASCADE_COUNT,
        ));
    }
    if preview.0 == Handle::default() {
        preview.0 = images.add(create_moment_preview_image(PREVIEW_SIZE));
    }
}

fn init_or_vsm_pipelines(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let depth_array_entry = BindingType::Texture {
        sample_type: TextureSampleType::Depth,
        view_dimension: TextureViewDimension::D2Array,
        multisampled: false,
    }
    .into_bind_group_layout_entry_builder();

    let pack_layout = BindGroupLayoutDescriptor::new(
        "or_moment_pack_layout",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (0, depth_array_entry),
                (1, sampler(SamplerBindingType::Filtering)),
                (2, uniform_buffer::<OrMomentPackParams>(false)),
            ),
        ),
    );

    let blur_layout = BindGroupLayoutDescriptor::new(
        "or_moment_blur_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<OrMomentBlurParams>(false),
            ),
        ),
    );

    let pack_shader: Handle<Shader> = asset_server.load(PACK_SHADER);
    let blur_shader: Handle<Shader> = asset_server.load(BLUR_SHADER);
    let vertex_state = fullscreen_shader.to_vertex_state();

    let pack_pipeline = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("or_moment_pack_pipeline".into()),
        layout: vec![pack_layout.clone()],
        vertex: vertex_state.clone(),
        fragment: Some(FragmentState {
            shader: pack_shader,
            entry_point: Some("fragment".into()),
            shader_defs: vec![],
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rg32Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        ..default()
    });

    let blur_pipeline = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("or_moment_blur_pipeline".into()),
        layout: vec![blur_layout.clone()],
        vertex: vertex_state,
        fragment: Some(FragmentState {
            shader: blur_shader,
            entry_point: Some("fragment".into()),
            shader_defs: vec![],
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rg32Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        ..default()
    });

    let linear_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("or_vsm_linear_depth_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });

    let preview_layout = BindGroupLayoutDescriptor::new(
        "or_moment_preview_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d_array(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<OrMomentPreviewParams>(false),
            ),
        ),
    );

    let preview_shader: Handle<Shader> = asset_server.load(PREVIEW_SHADER);
    let preview_pipeline = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("or_moment_preview_pipeline".into()),
        layout: vec![preview_layout.clone()],
        vertex: fullscreen_shader.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: preview_shader,
            entry_point: Some("fragment".into()),
            shader_defs: vec![],
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        ..default()
    });

    commands.insert_resource(OrVsmPipelines {
        pack_layout,
        blur_layout,
        preview_layout,
        pack_pipeline,
        blur_pipeline,
        preview_pipeline,
        linear_sampler,
    });
}

fn moment_layer_view(texture: &Texture, layer: u32, label: &'static str) -> TextureView {
    texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

fn write_uniform<T: ShaderType + bytemuck::Pod>(queue: &RenderQueue, buffer: &Buffer, value: &T) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(value));
}

#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::needless_borrow
)]
fn prepare_or_vsm_view_bind_groups(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    pipelines: Res<OrVsmPipelines>,
    atlas: Res<OrMomentAtlasImage>,
    blur_temp: Res<OrMomentBlurTempImage>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    views: Query<
        (Entity, &ViewShadowBindings),
        (
            With<OrVsmRenderSettings>,
            With<ExtractedView>,
            Without<OrVsmViewBindGroups>,
        ),
    >,
) {
    let Some(atlas_gpu) = gpu_images.get(atlas.0.id()) else {
        return;
    };
    let Some(blur_gpu) = gpu_images.get(blur_temp.0.id()) else {
        return;
    };
    let pack_layout = pipeline_cache.get_bind_group_layout(&pipelines.pack_layout);
    let blur_layout = pipeline_cache.get_bind_group_layout(&pipelines.blur_layout);

    let texel = 1.0 / MOMENT_ATLAS_SIZE as f32;

    for (entity, shadow_bindings) in &views {
        let pack_params_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("or_moment_pack_params"),
            size: size_of::<OrMomentPackParams>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_params_buffer = render_device.create_buffer(&BufferDescriptor {
            label: Some("or_moment_blur_params"),
            size: size_of::<OrMomentBlurParams>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pack_bind_group = render_device.create_bind_group(
            "or_moment_pack_bind_group",
            &pack_layout,
            &BindGroupEntries::sequential((
                &shadow_bindings.directional_light_depth_texture_view,
                &pipelines.linear_sampler,
                pack_params_buffer.as_entire_binding(),
            )),
        );

        let blur_src_to_temp = render_device.create_bind_group(
            "or_moment_blur_src_to_temp",
            &blur_layout,
            &BindGroupEntries::sequential((
                &atlas_gpu.texture_view,
                &atlas_gpu.sampler,
                blur_params_buffer.as_entire_binding(),
            )),
        );

        let blur_temp_to_src = render_device.create_bind_group(
            "or_moment_blur_temp_to_src",
            &blur_layout,
            &BindGroupEntries::sequential((
                &blur_gpu.texture_view,
                &blur_gpu.sampler,
                blur_params_buffer.as_entire_binding(),
            )),
        );

        write_uniform(
            &render_queue,
            &blur_params_buffer,
            &OrMomentBlurParams {
                texel_size: Vec2::new(texel, texel),
                ..default()
            },
        );

        commands.entity(entity).insert(OrVsmViewBindGroups {
            pack_bind_group,
            blur_src_to_temp,
            blur_temp_to_src,
            pack_params_buffer,
            blur_params_buffer,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn or_vsm_moment_pass(
    view: ViewQuery<(&OrVsmRenderSettings, &OrVsmViewBindGroups)>,
    mut render_context: RenderContext,
    pipelines: Res<OrVsmPipelines>,
    pipeline_cache: Res<PipelineCache>,
    atlas: Res<OrMomentAtlasImage>,
    blur_temp: Res<OrMomentBlurTempImage>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_queue: Res<RenderQueue>,
    render_device: Res<RenderDevice>,
    preview_image: Res<OrMomentPreviewImage>,
    debug: Option<Res<OrVsmDebugSettings>>,
) {
    let (settings, bind_groups) = view.into_inner();

    if !settings.enabled {
        return;
    }

    let Some(pack_pipeline) = pipeline_cache.get_render_pipeline(pipelines.pack_pipeline) else {
        return;
    };
    let Some(blur_pipeline) = pipeline_cache.get_render_pipeline(pipelines.blur_pipeline) else {
        return;
    };
    let Some(atlas_gpu) = gpu_images.get(atlas.0.id()) else {
        return;
    };
    let Some(blur_gpu) = gpu_images.get(blur_temp.0.id()) else {
        return;
    };

    let texel = 1.0 / MOMENT_ATLAS_SIZE as f32;

    for cascade in 0..MOMENT_CASCADE_COUNT {
        write_uniform(
            &render_queue,
            &bind_groups.pack_params_buffer,
            &OrMomentPackParams {
                cascade_layer: cascade,
                _pad: [0; 3],
            },
        );

        let atlas_layer_view =
            moment_layer_view(&atlas_gpu.texture, cascade, "or_moment_atlas_layer");
        let blur_layer_view = moment_layer_view(&blur_gpu.texture, cascade, "or_moment_blur_layer");

        {
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("or_moment_pack"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &atlas_layer_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_render_pipeline(pack_pipeline);
            pass.set_bind_group(0, &bind_groups.pack_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        write_uniform(
            &render_queue,
            &bind_groups.blur_params_buffer,
            &OrMomentBlurParams {
                blur_dir: Vec2::new(1.0, 0.0),
                texel_size: Vec2::new(texel, texel),
                cascade_layer: cascade,
                _pad: [0; 3],
            },
        );

        {
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("or_moment_blur_h"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &blur_layer_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_render_pipeline(blur_pipeline);
            pass.set_bind_group(0, &bind_groups.blur_src_to_temp, &[]);
            pass.draw(0..3, 0..1);
        }

        write_uniform(
            &render_queue,
            &bind_groups.blur_params_buffer,
            &OrMomentBlurParams {
                blur_dir: Vec2::new(0.0, 1.0),
                texel_size: Vec2::new(texel, texel),
                cascade_layer: cascade,
                _pad: [0; 3],
            },
        );

        {
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("or_moment_blur_v"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &atlas_layer_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_render_pipeline(blur_pipeline);
            pass.set_bind_group(0, &bind_groups.blur_temp_to_src, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    if debug
        .as_ref()
        .is_some_and(|d| d.atlas_preview && d.mode == crate::vsm::OrVsmMode::Exact)
    {
        let layer = debug
            .as_ref()
            .map(|d| d.atlas_layer)
            .unwrap_or(0)
            .min(MOMENT_CASCADE_COUNT - 1);
        if let (Some(preview_pipeline), Some(preview_gpu)) = (
            pipeline_cache.get_render_pipeline(pipelines.preview_pipeline),
            gpu_images.get(preview_image.0.id()),
        ) {
            let preview_layout = pipeline_cache.get_bind_group_layout(&pipelines.preview_layout);
            let preview_params_buffer = render_device.create_buffer(&BufferDescriptor {
                label: Some("or_moment_preview_params"),
                size: size_of::<OrMomentPreviewParams>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            write_uniform(
                &render_queue,
                &preview_params_buffer,
                &OrMomentPreviewParams {
                    cascade_layer: layer,
                    _pad: 0,
                },
            );
            let preview_bind_group = render_device.create_bind_group(
                "or_moment_preview_bind_group",
                &preview_layout,
                &BindGroupEntries::sequential((
                    &atlas_gpu.texture_view,
                    &atlas_gpu.sampler,
                    preview_params_buffer.as_entire_binding(),
                )),
            );
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("or_moment_preview"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &preview_gpu.texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_render_pipeline(preview_pipeline);
            pass.set_bind_group(0, &preview_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moment_atlas_is_rg32_array() {
        let img = create_moment_atlas_image(512, 3);
        assert_eq!(img.texture_descriptor.format, TextureFormat::Rg32Float);
        assert_eq!(img.texture_descriptor.size.depth_or_array_layers, 3);
    }

    #[test]
    fn uniform_buffer_sizes_match_wgsl() {
        assert_eq!(size_of::<OrMomentPackParams>(), 16);
        assert_eq!(size_of::<OrMomentPreviewParams>(), 8);
        assert_eq!(size_of::<OrMomentBlurParams>(), 32);
    }
}
