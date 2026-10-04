//! Rain on cab windows. Main-pass depth masks the opaque desk and window frames;
//! the effect runs before UI, so the original 2D panel also masks its windows.
//! All timing uses the simulation clock: pause and saved games stay coherent.
use crate::{
    camera::CameraFollowMode,
    live::LiveDrive,
    player_launch::{ActivePlayerContent, PlayerWeather},
};
use bevy::{
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{ViewDepthTexture, ViewTarget},
    },
};

pub struct WindshieldPlugin;
impl Plugin for WindshieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<WindshieldSettings>::default(),
            UniformComponentPlugin::<WindshieldSettings>::default(),
        ))
        .init_resource::<WindshieldWipeState>()
        .add_systems(
            Update,
            sync_windshield
                .run_if(crate::live::live_mode_active)
                .run_if(in_state(crate::ViewerAppState::Playing)),
        );
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core3d, draw_windshield.in_set(Core3dSystems::PostProcess));
    }
}

#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct WindshieldSettings {
    pub time_s: f32,
    pub rain: f32,
    pub near_clip: f32,
    pub last_wipe_s: f32,
    pub wiper_on: f32,
    pub _pad: Vec3,
}
#[derive(Resource, Default)]
struct WindshieldWipeState {
    last_wipe_s: Option<f64>,
    last_clock_s: f64,
}

fn sync_windshield(
    mut commands: Commands,
    live: Res<LiveDrive>,
    weather: Res<ActivePlayerContent>,
    follow: Res<CameraFollowMode>,
    mut wipe: ResMut<WindshieldWipeState>,
    mut cameras: Query<(
        Entity,
        &mut Camera3d,
        &Projection,
        Option<&mut WindshieldSettings>,
    )>,
) {
    let clock = live.session.time_s();
    if clock < wipe.last_clock_s {
        wipe.last_wipe_s = None;
    }
    wipe.last_clock_s = clock;
    if live.session.wiper_active {
        wipe.last_wipe_s = Some(clock);
    }
    let visible = (*follow == CameraFollowMode::DriverCam || follow.is_cab2d())
        && weather.weather == PlayerWeather::Rain;
    for (e, mut camera, projection, current) in &mut cameras {
        // Sample the resolved main-pass depth, without introducing a separate
        // prepass that would use the wrong alpha bindings for original materials.
        camera.depth_texture_usages =
            (TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING).into();
        let near = match projection {
            Projection::Perspective(p) => p.near,
            _ => 0.02,
        };
        let settings = WindshieldSettings {
            time_s: clock as f32,
            rain: f32::from(visible),
            near_clip: near,
            last_wipe_s: wipe.last_wipe_s.map_or(-1.0, |s| s as f32),
            wiper_on: f32::from(live.session.wiper_active),
            ..default()
        };
        if let Some(mut current) = current {
            *current = settings;
        } else {
            commands.entity(e).insert(settings);
        }
    }
}

#[derive(Resource)]
struct WindshieldPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline: CachedRenderPipelineId,
}
fn init_pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "windshield-layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<WindshieldSettings>(true),
                texture_depth_2d(),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor::default());
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("windshield-rain".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load("shaders/windshield.wgsl"),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8UnormSrgb,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(WindshieldPipeline {
        layout,
        sampler,
        pipeline,
    });
}

fn draw_windshield(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthTexture,
        &WindshieldSettings,
        &DynamicUniformIndex<WindshieldSettings>,
    )>,
    pipeline: Option<Res<WindshieldPipeline>>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<WindshieldSettings>>,
    mut ctx: RenderContext,
) {
    let (target, depth, settings, index) = view.into_inner();
    if settings.rain < 0.5 {
        return;
    }
    let Some(pipeline) = pipeline else { return };
    let Some(render_pipeline) = cache.get_render_pipeline(pipeline.pipeline) else {
        return;
    };
    let Some(binding) = uniforms.uniforms().binding() else {
        return;
    };
    let output = target.post_process_write();
    let group = ctx.render_device().create_bind_group(
        "windshield-bindings",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((output.source, &pipeline.sampler, binding, depth.view())),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("windshield-rain"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: output.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, &group, &[index.index()]);
    pass.draw(0..3, 0..1);
}

/// Repeating back-and-forth blade motion, shared with original cab bones.
pub fn wiper_phase(time_s: f64) -> f64 {
    1.0 - ((time_s / 1.8).rem_euclid(1.0) * 2.0 - 1.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wipe_has_two_passes_and_returns_to_park() {
        assert_eq!(wiper_phase(0.0), 0.0);
        assert!((wiper_phase(0.9) - 1.0).abs() < 1e-9);
        assert!(wiper_phase(1.8).abs() < 1e-9);
    }
}
