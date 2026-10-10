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
            PostUpdate,
            sync_windshield
                .after(bevy::transform::TransformSystems::Propagate)
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
    pub snow: f32,
    pub _pad: Vec3,
    pub glass: Vec4,
    pub blade1: Vec4,
    pub blade2: Vec4,
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
    atmosphere: Option<Res<crate::weather_state::WeatherState>>,
    follow: Res<CameraFollowMode>,
    cvf: Res<crate::cab_cvf::CabCvfState>,
    overlay: Res<crate::cab_cvf_overlay::CabCvfOverlayState>,
    preferences: Res<crate::player_settings::PlayerSettings>,
    mut wipe: ResMut<WindshieldWipeState>,
    roots: Query<&GlobalTransform, With<crate::cab_view::CabInteriorRoot>>,
    mut cameras: Query<(
        Entity,
        &mut Camera3d,
        &Projection,
        &Camera,
        &GlobalTransform,
        Option<&mut WindshieldSettings>,
    )>,
) {
    let clock = live.session.time_s();
    if clock < wipe.last_clock_s {
        wipe.last_wipe_s = None;
    }
    wipe.last_clock_s = clock;
    if live.session.effective_wiper_active() {
        wipe.last_wipe_s = Some(clock);
    }
    let visible = (*follow == CameraFollowMode::DriverCam || follow.is_cab2d())
        && matches!(
            weather.weather,
            PlayerWeather::Rain | PlayerWeather::Snow | PlayerWeather::Storm
        );
    for (e, mut camera, projection, view, camera_transform, current) in &mut cameras {
        // Sample the resolved main-pass depth, without introducing a separate
        // prepass that would use the wrong alpha bindings for original materials.
        camera.depth_texture_usages =
            (TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING).into();
        let near = match projection {
            Projection::Perspective(p) => p.near,
            _ => 0.02,
        };
        let path = cvf.cvf_path.as_deref();
        let profile = path
            .and_then(|p| preferences.cab_profiles.get(p.to_string_lossy().as_ref()))
            .copied()
            .unwrap_or_default();
        let count = path
            .zip(cvf.runtime.as_ref())
            .map_or(2, |(p, r)| crate::cab_profile::blade_count(r, p));
        let front = overlay.view_index == 0 || !follow.is_cab2d();
        let glass = if follow.is_cab2d() {
            cvf.runtime
                .as_ref()
                .and_then(|r| r.cvf.views.get(overlay.view_index))
                .map_or(Vec4::new(0.0, 0.0, 1.0, 1.0), |v| {
                    crate::cab_profile::window_uv(
                        v,
                        view.logical_viewport_size()
                            .unwrap_or(Vec2::new(640.0, 480.0)),
                    )
                })
        } else {
            Vec4::new(0.0, 0.0, 1.0, 1.0)
        };
        let (left, right, radius) = if count == 1 {
            (0.5, 0.5, 0.82)
        } else {
            (0.25, 0.75, 0.60)
        };
        let mut blade1 = Vec4::new(left, 0.96, radius * profile.wipe_scale, f32::from(front));
        let mut blade2 = Vec4::new(
            right,
            0.96,
            radius * profile.wipe_scale,
            f32::from(front && count > 1),
        );
        if *follow == CameraFollowMode::DriverCam
            && let Some(runtime) = cvf.runtime.as_ref()
            && let Ok(root) = roots.single()
        {
            let screen = view
                .logical_viewport_size()
                .unwrap_or(Vec2::new(1280.0, 720.0));
            let mut pivots = Vec::new();
            for (index, matrix) in runtime.shape.matrices.iter().enumerate() {
                if !matrix
                    .name
                    .to_ascii_uppercase()
                    .starts_with("EXTERNALWIPERS")
                {
                    continue;
                }
                let local =
                    crate::cab_cvf::static_matrix_transform(&runtime.shape, index).translation;
                if let Ok(pixel) =
                    view.world_to_viewport(camera_transform, root.transform_point(local))
                {
                    let uv = pixel / screen;
                    if (0.0..=1.0).contains(&uv.x)
                        && (0.0..=1.2).contains(&uv.y)
                        && pivots.iter().all(|p: &Vec2| p.distance(uv) > 0.05)
                    {
                        pivots.push(uv);
                    }
                }
            }
            pivots.sort_by(|a, b| a.x.total_cmp(&b.x));
            if let Some(p) = pivots.first() {
                blade1.x = p.x;
                blade1.y = p.y;
            }
            if let Some(p) = pivots.get(1) {
                blade2.x = p.x;
                blade2.y = p.y;
            }
        }
        let settings = WindshieldSettings {
            time_s: clock as f32,
            rain: if *follow == CameraFollowMode::DriverCam || follow.is_cab2d() {
                atmosphere.as_ref().map_or(f32::from(visible), |s| {
                    s.atmosphere.rain.max(s.atmosphere.snow)
                })
            } else {
                0.0
            },
            snow: atmosphere
                .as_ref()
                .map_or(f32::from(weather.weather == PlayerWeather::Snow), |s| {
                    f32::from(s.atmosphere.snow > s.atmosphere.rain)
                }),
            near_clip: near,
            last_wipe_s: wipe.last_wipe_s.map_or(-1.0, |s| s as f32),
            wiper_on: f32::from(live.session.effective_wiper_active()),
            _pad: Vec3::new(
                live.session.velocity_mps().abs() as f32,
                atmosphere
                    .as_ref()
                    .map_or(0.0, |s| camera_transform.right().dot(s.atmosphere.wind_mps)),
                atmosphere.as_ref().map_or(0.0, |s| {
                    camera_transform.forward().dot(s.atmosphere.wind_mps)
                }),
            ),
            glass,
            blade1,
            blade2,
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
    if settings.rain < 0.002 {
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
    openrailsrs_audio::wiper::phase(time_s)
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
