//! Opt-in real-time endurance checks. Never enabled by a saved player setting.
use bevy::prelude::*;
use std::{path::PathBuf, time::Instant};
#[derive(Resource)]
pub struct Soak {
    routes: Vec<SoakRoute>,
    next_route: f64,
    route_index: usize,
    route_changes: u32,
    route_interval: f64,
    start: Instant,
    duration: f64,
    next_sample: f64,
    next_cycle: f64,
    rows: u32,
    cycles: u32,
    snapshots: u32,
    path: PathBuf,
}
#[derive(Clone, serde::Deserialize)]
struct SoakRoute {
    path: PathBuf,
    route_root: Option<PathBuf>,
}
fn next_deadline(age: f64, period: f64) -> f64 {
    // An absolute cadence prevents loading delays from accumulating on every
    // round trip. Missed slots are skipped, never counted as completed work.
    (age / period).floor().mul_add(period, period)
}
impl Soak {
    pub fn finished(&self) -> bool {
        self.start.elapsed().as_secs_f64() >= self.duration
    }
}
pub fn install(app: &mut App) {
    let Some(seconds) = std::env::var("OPENRAILSRS_SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && (30. ..=21600.).contains(s))
    else {
        return;
    };
    let Some(path) = std::env::var_os("OPENRAILSRS_SOAK_OUT").map(PathBuf::from) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&path, "").is_err() {
        return;
    }
    let route_interval = std::env::var("OPENRAILSRS_SOAK_ROUTE_INTERVAL_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && (30. ..=600.).contains(s))
        .unwrap_or(180.);
    app.insert_resource(Soak {
        routes: std::env::var("OPENRAILSRS_SOAK_ROUTES")
            .ok()
            .filter(|s| s.len() <= 8192)
            .and_then(|s| serde_json::from_str::<Vec<SoakRoute>>(&s).ok())
            .filter(|routes| {
                routes.len() <= 8
                    && routes.iter().all(|r| {
                        r.path.is_absolute()
                            && r.path.is_file()
                            && r.route_root
                                .as_ref()
                                .is_none_or(|p| p.is_absolute() && p.is_dir())
                    })
            })
            .unwrap_or_default(),
        route_interval,
        next_route: route_interval,
        route_index: 0,
        route_changes: 0,
        start: Instant::now(),
        duration: seconds,
        next_sample: 30.,
        next_cycle: 60.,
        rows: 0,
        cycles: 0,
        snapshots: 0,
        path,
    })
    .add_systems(
        Last,
        sample.run_if(in_state(crate::ViewerAppState::Playing)),
    );
}
fn sample(world: &mut World) {
    let age = world.resource::<Soak>().start.elapsed().as_secs_f64();
    // Freeze travel at the requested wall time, letting streaming and GPU
    // publication finish before the final readiness-gated capture.
    if world.resource::<Soak>().finished()
        && let Some(mut live) = world.get_resource_mut::<crate::live::LiveDrive>()
    {
        live.paused = true;
    }
    let cycle = age >= world.resource::<Soak>().next_cycle;
    if cycle {
        let count = world.resource::<Soak>().cycles;
        world
            .resource_mut::<crate::player_settings::PlayerSettings>()
            .weather_profile = match count % 4 {
            0 => crate::weather_state::WeatherProfile::Downpour,
            1 => crate::weather_state::WeatherProfile::HeavySnow,
            2 => crate::weather_state::WeatherProfile::StormCycle,
            _ => crate::weather_state::WeatherProfile::Automatic,
        };
        *world.resource_mut::<crate::camera::CameraFollowMode>() = if count.is_multiple_of(2) {
            crate::camera::CameraFollowMode::ChaseCam
        } else {
            crate::camera::CameraFollowMode::DriverCam
        };
        if let Some(mut live) = world.get_resource_mut::<crate::live::LiveDrive>() {
            let saved = live.session.snapshot();
            let traffic = live.traffic.snapshot();
            let encoded = serde_json::to_vec(&(saved, traffic)).expect("serializable session");
            let (saved, traffic) =
                serde_json::from_slice(&encoded).expect("own serialized session");
            if live.session.restore_snapshot(saved).is_ok()
                && live.traffic.restore_snapshot(traffic).is_ok()
            {
                world.resource_mut::<Soak>().snapshots += 1;
            }
        }
        let mut state = world.resource_mut::<Soak>();
        state.cycles += 1;
        state.next_cycle = next_deadline(age, 60.);
    }
    if age < world.resource::<Soak>().next_sample {
        return;
    }
    let state = world.resource::<Soak>();
    let path = state.path.clone();
    let row = serde_json::json!({"wall_s":age,"cycles":state.cycles,"save_restore_cycles":state.snapshots,"route_changes":state.route_changes,"scenario":world.resource::<crate::live::LiveDrive>().session.scenario_name,
        "schema":2,"entities":world.entities().count_spawned(),"entity_index_capacity":world.entities().len(),"meshes":world.resource::<Assets<Mesh>>().len(),"images":world.resource::<Assets<Image>>().len(),
        "fonts":world.resource::<Assets<Font>>().len(),
        "font_atlases":world.resource::<bevy::text::FontAtlasSet>().values().flatten().count(),
        "shaders":world.resource::<Assets<bevy::shader::Shader>>().len(),
        "effect_assets":world.resource::<Assets<bevy_hanabi::EffectAsset>>().len(),
        "render_resources":world.resource::<crate::performance::ScenePipelineStatus>().render_resources(),
        "native_audio":world.resource::<crate::native_audio::NativeAudio>().engine.as_ref().map(|e|e.report()),
        "graphics_memory":world.resource::<crate::gpu_memory::GraphicsMemory>().0,
        "performance":world.resource::<crate::performance::JourneyPerformance>().report(),
        "heap_allocator":crate::performance::heap_allocator_report(),
        "geometry_cache":crate::prepared_geometry::report(),
        "shape_stages":crate::performance::shape_stages_report(),
        "scene_cleanup":world.resource::<crate::scene_cleanup::SceneCleanup>().report(),
        "render_budget":world.resource::<crate::render_budget::RenderBudget>(),
        "residency":{ "pressured":world.resource::<crate::gpu_memory::ResidencyPolicy>().pressured,"transitions":world.resource::<crate::gpu_memory::ResidencyPolicy>().transitions,"budget_mib":world.resource::<crate::gpu_memory::ResidencyPolicy>().budget_mib,"basis":world.resource::<crate::gpu_memory::ResidencyPolicy>().basis},
        "renderer":world.resource::<crate::performance::ScenePipelineStatus>().device(),
        "pending_uploads":world.resource::<crate::performance::ScenePipelineStatus>().pending_uploads()});
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) {
        let _ = writeln!(f, "{row}");
    }
    let mut state = world.resource_mut::<Soak>();
    state.rows += 1;
    state.next_sample = next_deadline(age, 30.);
    if age >= state.next_route && !state.routes.is_empty() && !state.finished() {
        let route = state.routes[state.route_index % state.routes.len()].clone();
        state.route_index += 1;
        state.route_changes += 1;
        state.next_route = next_deadline(age, state.route_interval);
        let environment = world
            .resource::<crate::player_settings::PlayerSettings>()
            .environment;
        world
            .resource_mut::<crate::player_launch::PlayerLaunchQueue>()
            .0 = Some(crate::player_launch::QueuedPlayerLaunch {
            path: route.path,
            route_root: route.route_root,
            weather: environment.manual_weather,
            environment,
            resume: None,
            weather_checkpoint: None,
            player_options: None,
        });
    }
}
