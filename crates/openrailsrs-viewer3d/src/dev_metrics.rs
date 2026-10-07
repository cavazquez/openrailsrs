//! Opt-in viewer diagnostics. Bounded samples; startup is reported separately.
//! The input probe measures queue-to-next-ECS-frame latency, not display photons.
use bevy::{diagnostic::DiagnosticsStore, prelude::*};
use serde::Serialize;
use std::{collections::VecDeque, time::Instant};

#[derive(Clone, Debug, Serialize)]
pub struct FrameSample {
    pub frame_ms: f64,
    pub cpu_main_ms: f64,
    pub gpu_ms: Option<f64>,
    pub render_cpu_ms: Option<f64>,
    pub input_queue_ms: Option<f64>,
    pub pending_assets: usize,
    pub pending_pipelines: usize,
    pub streaming: bool,
    pub category: &'static str,
}
#[derive(Resource, Default)]
pub struct DevMetrics {
    samples: VecDeque<FrameSample>,
    start: Option<Instant>,
    previous: Option<Instant>,
    input_probe: Option<Instant>,
    input_ms: Option<f64>,
    warmup_frames: u32,
    sampled: bool,
    probe_cadence: u8,
    renderer: Vec<(String, f64)>,
    exports: u64,
}
pub fn percentile(values: impl IntoIterator<Item = f64>, p: f64) -> Option<f64> {
    let mut values: Vec<_> = values
        .into_iter()
        .filter(|v| v.is_finite() && *v >= 0.)
        .collect();
    values.sort_by(f64::total_cmp);
    (!values.is_empty()).then(|| {
        values[((values.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1)]
    })
}
impl DevMetrics {
    pub fn report(&self) -> serde_json::Value {
        let summary = |f: fn(&FrameSample) -> Option<f64>| {
            serde_json::json!({
                "p50": percentile(self.samples.iter().filter_map(f), 0.5),
                "p95": percentile(self.samples.iter().filter_map(f), 0.95),
                "p99": percentile(self.samples.iter().filter_map(f), 0.99),
            })
        };
        let mut categories = std::collections::BTreeMap::new();
        for sample in &self.samples {
            *categories.entry(sample.category).or_insert(0_u64) += 1;
        }
        serde_json::json!({"samples":self.samples.len(),"warmup_frames":self.warmup_frames,
            "frame_ms":summary(|s|Some(s.frame_ms)), "cpu_main_ms":summary(|s|Some(s.cpu_main_ms)),
            "gpu_ms":summary(|s|s.gpu_ms),
            "gpu_time_scope":"longest instrumented render span; nested passes are not summed", "input_queue_ms":summary(|s|s.input_queue_ms),
            "input_latency_scope":"synthetic queue to next ECS frame; excludes hardware and display",
            "hitches_over_100_ms": self.samples.iter().filter(|s|s.frame_ms>100.).count(),
            "categories":categories, "renderer_diagnostics":self.renderer,
            "hitches":self.samples.iter().filter(|s|s.frame_ms>50.).take(256).collect::<Vec<_>>(),
            "exports":self.exports})
    }
}
pub fn begin_frame(mut metrics: ResMut<DevMetrics>) {
    metrics.start = Some(Instant::now());
    metrics.input_ms = metrics
        .input_probe
        .take()
        .map(|at| at.elapsed().as_secs_f64() * 1000.);
}
fn render_times(store: &DiagnosticsStore) -> (Option<f64>, Option<f64>) {
    // The longest measured render span includes nested passes: summing them
    // would double-count GPU work. Timestamp queries can be unavailable.
    let longest = |suffix| {
        store
            .iter()
            .filter(|d| d.path().as_str().ends_with(suffix))
            .filter_map(|d| d.value())
            .filter(|v| v.is_finite())
            .max_by(f64::total_cmp)
    };
    (longest("elapsed_gpu"), longest("elapsed_cpu"))
}
pub fn category(
    pipelines: usize,
    assets: usize,
    streaming: bool,
    cpu: f64,
    gpu: Option<f64>,
    frame: f64,
) -> &'static str {
    if pipelines > 0 {
        "shader_pipeline"
    } else if assets > 0 {
        "assets_upload"
    } else if streaming {
        "streaming"
    } else if gpu.is_some_and(|g| g > frame * 0.6) {
        "renderer_gpu"
    } else if cpu > frame * 0.6 {
        "cpu_main"
    } else {
        "presentation_or_idle"
    }
}
#[allow(clippy::too_many_arguments)]
pub fn end_frame(
    mut metrics: ResMut<DevMetrics>,
    state: Res<State<crate::ViewerAppState>>,
    pipeline: Res<crate::performance::ScenePipelineStatus>,
    store: Res<DiagnosticsStore>,
    startup: Option<Res<crate::route_bootstrap::ViewerLoadingScreen>>,
    world: Option<Res<crate::world::WorldSpawnProgress>>,
    terrain: Option<Res<crate::terrain_spawn::TerrainSpawnProgress>>,
    tiles: Option<Res<crate::terrain_spawn::TerrainTileStream>>,
) {
    let now = Instant::now();
    let previous = metrics.previous.replace(now);
    if *state.get() != crate::ViewerAppState::Playing || startup.is_some() {
        return;
    }
    if !metrics.sampled {
        if !pipeline.ready() || world.is_some() || terrain.is_some() {
            return;
        }
        metrics.warmup_frames += 1;
        if metrics.warmup_frames < 60 {
            return;
        }
        metrics.sampled = true;
    }
    let Some(previous) = previous else { return };
    let cpu = metrics
        .start
        .map_or(0., |s| s.elapsed().as_secs_f64() * 1000.);
    let frame = now.duration_since(previous).as_secs_f64() * 1000.;
    let (gpu, render_cpu) = render_times(&store);
    let pipelines = pipeline.counts().0;
    let assets = pipeline.pending_uploads();
    let streaming =
        world.is_some() || terrain.is_some() || tiles.is_some_and(|t| t.pending_work() > 0);
    let input_queue_ms = metrics.input_ms;
    if metrics.samples.len() == 12_000 {
        metrics.samples.pop_front();
    }
    metrics.samples.push_back(FrameSample {
        frame_ms: frame,
        cpu_main_ms: cpu,
        gpu_ms: gpu,
        render_cpu_ms: render_cpu,
        input_queue_ms,
        pending_assets: assets,
        pending_pipelines: pipelines,
        streaming,
        category: category(pipelines, assets, streaming, cpu, gpu, frame),
    });
    metrics.renderer = store
        .iter()
        .filter(|d| d.path().as_str().starts_with("render/"))
        .filter_map(|d| Some((d.path().as_str().to_string(), d.value()?)))
        .collect();
    // One cheap, independent input dispatch probe every 20 sampled frames.
    metrics.probe_cadence = (metrics.probe_cadence + 1) % 20;
    if metrics.probe_cadence == 0 {
        metrics.input_probe = Some(Instant::now());
    }
}
pub fn export(world: &mut World) {
    let mut report = world.resource::<DevMetrics>().report();
    report["graphics_memory"] =
        serde_json::to_value(&world.resource::<crate::gpu_memory::GraphicsMemory>().0).unwrap();
    report["renderer"] = serde_json::to_value(
        world
            .resource::<crate::performance::ScenePipelineStatus>()
            .device(),
    )
    .unwrap();
    let path = std::env::var_os("OPENRAILSRS_DIAGNOSTICS_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::player_settings::player_data_dir().join("diagnostics/viewer.json")
        });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec_pretty(&report)
        .ok()
        .and_then(|bytes| std::fs::write(&path, bytes).ok())
    {
        Some(()) => {
            world.resource_mut::<DevMetrics>().exports += 1;
            crate::viewer_log!("Viewer diagnostics: {}", path.display());
        }
        None => {
            crate::viewer_log!("Could not write viewer diagnostics: {}", path.display());
        }
    }
}
pub fn install(app: &mut App) {
    app.init_resource::<DevMetrics>()
        .add_systems(
            OnEnter(crate::ViewerAppState::Playing),
            |mut metrics: ResMut<DevMetrics>| *metrics = default(),
        )
        .add_systems(First, begin_frame)
        .add_systems(Last, end_frame);
    #[cfg(feature = "experimental-framepace")]
    {
        use bevy_framepace::{FramepacePlugin, FramepaceSettings, Limiter};
        let value = std::env::var("OPENRAILSRS_FRAMEPACE").unwrap_or_else(|_| "off".into());
        let limiter = match value.as_str() {
            "30" => Limiter::from_framerate(30.),
            "60" => Limiter::from_framerate(60.),
            "off" | "unlimited" => Limiter::Off,
            _ => {
                crate::viewer_log!("Unknown frame pacing option {value}; disabled");
                Limiter::Off
            }
        };
        app.add_plugins(FramepacePlugin)
            .insert_resource(FramepaceSettings { limiter });
    }
    #[cfg(feature = "dev-tools")]
    install_tools(app);
}
#[cfg(feature = "dev-tools")]
fn install_tools(app: &mut App) {
    crate::dev_camera::install(app);
    use bevy::dev_tools::{
        fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig},
        picking_debug::DebugPickingPlugin,
    };
    let enabled = std::env::var("OPENRAILSRS_DEV_TOOLS").is_ok_and(|v| v == "1");
    app.add_plugins((
        FpsOverlayPlugin {
            config: FpsOverlayConfig {
                enabled,
                text_config: TextFont::from_font_size(16.),
                frame_time_graph_config: FrameTimeGraphConfig {
                    enabled,
                    ..default()
                },
                ..default()
            },
        },
        DebugPickingPlugin,
        bevy::render::diagnostic::RenderDiagnosticsPlugin,
    ))
    .insert_resource(bevy::picking::mesh_picking::MeshPickingSettings {
        require_markers: true,
        ..default()
    })
    .add_systems(Update, tools_keys)
    .add_systems(
        StateTransition,
        bevy::dev_tools::states::log_transitions::<crate::ViewerAppState>
            .run_if(|| std::env::var("OPENRAILSRS_DEV_TOOLS").is_ok_and(|v| v == "1")),
    );
    #[cfg(not(feature = "dev-inspector"))]
    app.add_systems(Update, debug_picking_targets);
}
#[cfg(all(feature = "dev-tools", not(feature = "dev-inspector")))]
fn debug_picking_targets(
    mut commands: Commands,
    mode: Res<bevy::dev_tools::picking_debug::DebugPickingMode>,
    cameras: Query<(Entity, Has<bevy::picking::mesh_picking::MeshPickingCamera>), With<Camera3d>>,
    targets: Query<Entity, (With<Mesh3d>, Without<bevy::picking::Pickable>)>,
) {
    let enabled = *mode != bevy::dev_tools::picking_debug::DebugPickingMode::Disabled;
    for (camera, marked) in &cameras {
        if enabled && !marked {
            commands
                .entity(camera)
                .insert(bevy::picking::mesh_picking::MeshPickingCamera);
        } else if !enabled && marked {
            commands
                .entity(camera)
                .remove::<bevy::picking::mesh_picking::MeshPickingCamera>();
        }
    }
    if enabled {
        for target in &targets {
            commands
                .entity(target)
                .insert(bevy::picking::Pickable::default());
        }
    }
}
#[cfg(feature = "dev-tools")]
fn tools_keys(world: &mut World) {
    let keys = world.resource::<ButtonInput<KeyCode>>();
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    if world
        .resource::<crate::player_settings::PlayerSettings>()
        .keys
        .values()
        .any(|k| k == "F11")
    {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if control && shift {
        export(world);
        return;
    }
    if control {
        use bevy::dev_tools::picking_debug::DebugPickingMode;
        let mut mode = world.resource_mut::<DebugPickingMode>();
        *mode = if *mode == DebugPickingMode::Disabled {
            DebugPickingMode::Normal
        } else {
            DebugPickingMode::Disabled
        };
    } else {
        let mut config = world.resource_mut::<bevy::dev_tools::fps_overlay::FpsOverlayConfig>();
        config.enabled = !config.enabled;
        config.frame_time_graph_config.enabled = config.enabled;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentiles_and_hitch_evidence_are_bounded_and_unambiguous() {
        assert_eq!(percentile([1., 2., 3., 100., f64::NAN], 0.5), Some(2.));
        assert_eq!(percentile([1., 2., 3., 100.], 0.99), Some(100.));
        assert_eq!(percentile([], 0.99), None);
        assert_eq!(category(1, 8, true, 90., Some(4.), 100.), "shader_pipeline");
        assert_eq!(category(0, 8, true, 90., Some(4.), 100.), "assets_upload");
        assert_eq!(category(0, 0, true, 90., Some(4.), 100.), "streaming");
        assert_eq!(category(0, 0, false, 2., Some(90.), 100.), "renderer_gpu");
    }
}
