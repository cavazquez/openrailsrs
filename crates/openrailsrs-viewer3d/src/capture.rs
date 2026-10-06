//! Screenshot capture for visual debugging / CI snapshots (#43).
//!
//! Gated by `OPENRAILSRS_SCREENSHOT=<path.png>`.
//!
//! Wait modes:
//! - Default: `OPENRAILSRS_SCREENSHOT_DELAY_S` wall-clock seconds (default 32).
//! - `OPENRAILSRS_SCREENSHOT_AFTER_READY=1`: wait until progressive WORLD spawn
//!   finishes (`WorldSpawnProgress` absent) and at least
//!   `OPENRAILSRS_SCREENSHOT_READY_FRAMES` frames in `Playing` (default 30),
//!   with an optional max wait via `OPENRAILSRS_SCREENSHOT_DELAY_S` (default 60).
//! - `OPENRAILSRS_SCREENSHOT_MIN_ODOMETER_M`: require actual travel before
//!   capture; a timeout fails instead of saving a misleading earlier frame.
//!   `OPENRAILSRS_SCREENSHOT_PAUSE_AT_TARGET=1` holds that checkpoint while
//!   progressive scenery catches up. The accompanying `.stream.json` records
//!   distance, tile coverage and any nearby objects still awaiting activation.
//! - `OPENRAILSRS_SCREENSHOT_AFTER_SERVICE=1`: also require service completion.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::viewer_log;
use crate::world::WorldSpawnProgress;

/// Wall-clock timing (not frame delta) so blocking load steps don't skew the delay.
#[derive(Resource)]
pub struct CaptureState {
    path: PathBuf,
    armed_at: Instant,
    /// Readiness timeout, or capture delay when readiness is not required.
    delay: Duration,
    after_ready: bool,
    ready_frames_needed: u32,
    ready_frames: u32,
    captured_at: Option<Instant>,
    target_odometer_m: Option<f64>,
    pause_at_target: bool,
    after_service: bool,
    during_lightning: bool,
}

pub fn capture_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_SCREENSHOT").is_some_and(|v| !v.is_empty())
}

/// Menu captures are opt-in QA for relocated binaries and confined packages.
pub fn capture_state_allowed(state: Res<State<crate::route_bootstrap::ViewerAppState>>) -> bool {
    *state.get() == crate::route_bootstrap::ViewerAppState::Playing
        || (*state.get() == crate::route_bootstrap::ViewerAppState::Menu
            && capture_enabled()
            && env_truthy("OPENRAILSRS_SCREENSHOT_MENU"))
}

fn env_truthy(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| {
        let v = v.to_string_lossy();
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

pub fn init_capture(mut commands: Commands) {
    let Some(path) = std::env::var_os("OPENRAILSRS_SCREENSHOT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let after_ready = env_truthy("OPENRAILSRS_SCREENSHOT_AFTER_READY");
    let default_delay = if after_ready { 60.0 } else { 32.0 };
    let delay_s = std::env::var("OPENRAILSRS_SCREENSHOT_DELAY_S")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(default_delay)
        .max(0.0);
    let ready_frames_needed = std::env::var("OPENRAILSRS_SCREENSHOT_READY_FRAMES")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(30)
        .max(1);
    let target_odometer_m = std::env::var("OPENRAILSRS_SCREENSHOT_MIN_ODOMETER_M")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0);
    viewer_log!(
        "openrailsrs-viewer3d: screenshot capture armed → {} (after_ready={after_ready}, delay={delay_s:.0}s, ready_frames={ready_frames_needed})",
        PathBuf::from(&path).display(),
    );
    commands.insert_resource(CaptureState {
        path: PathBuf::from(path),
        armed_at: Instant::now(),
        delay: Duration::from_secs_f32(delay_s),
        after_ready,
        ready_frames_needed,
        ready_frames: 0,
        captured_at: None,
        target_odometer_m,
        pause_at_target: env_truthy("OPENRAILSRS_SCREENSHOT_PAUSE_AT_TARGET"),
        after_service: env_truthy("OPENRAILSRS_SCREENSHOT_AFTER_SERVICE"),
        during_lightning: env_truthy("OPENRAILSRS_SCREENSHOT_DURING_LIGHTNING"),
    });
}

fn scenery_ready(progress: Option<Res<WorldSpawnProgress>>) -> bool {
    progress.is_none()
}

pub fn capture_system(
    state: Option<ResMut<CaptureState>>,
    progress: Option<Res<WorldSpawnProgress>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    mut live: Option<ResMut<crate::live::LiveDrive>>,
    scene: CaptureScene,
) {
    let Some(mut state) = state else {
        return;
    };
    match state.captured_at {
        None => {
            let lightning_ok =
                !state.during_lightning || scene.storm.as_ref().is_some_and(|s| s.flash > 0.3);
            if state.during_lightning
                && lightning_ok
                && let Some(live) = live.as_mut()
            {
                live.paused = true; // Freeze the actual event while its GPU upload completes.
            }
            let distance_ok = state.target_odometer_m.is_none_or(|target| {
                live.as_ref()
                    .is_some_and(|live| live.session.state.odometer_m >= target)
            }) && (!state.after_service
                || live.as_ref().is_some_and(|live| live.session.arrived));
            if distance_ok
                && state.target_odometer_m.is_some()
                && state.pause_at_target
                && let Some(live) = live.as_mut()
            {
                live.paused = true;
            }
            let timed_out = state.armed_at.elapsed() >= state.delay;
            let should_capture = distance_ok
                && lightning_ok
                && if state.after_ready {
                    if scenery_ready(progress)
                        && scene.terrain_progress.is_none()
                        && scene
                            .terrain_stream
                            .as_ref()
                            .is_none_or(|stream| stream.pending_work() == 0)
                        && scene.pipelines.as_ref().is_none_or(|p| p.ready())
                        && scene.lod_fades.is_empty()
                    {
                        state.ready_frames = state.ready_frames.saturating_add(1);
                    } else {
                        state.ready_frames = 0;
                    }
                    state.ready_frames >= state.ready_frames_needed
                } else {
                    state.armed_at.elapsed() >= state.delay
                };
            if timed_out
                && (state.after_ready
                    || state.target_odometer_m.is_some()
                    || state.after_service
                    || state.during_lightning)
                && !should_capture
            {
                viewer_log!(
                    "openrailsrs-viewer3d: screenshot checkpoint timed out before reaching distance/readiness"
                );
                exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                return;
            }
            if should_capture {
                let path = state.path.clone();
                {
                    let report = scene.report(live.as_deref());
                    let report_path = path.with_extension("stream.json");
                    if let Err(err) =
                        std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap())
                    {
                        viewer_log!(
                            "openrailsrs-viewer3d: cannot write streaming checkpoint: {err}"
                        );
                    }
                    viewer_log!("openrailsrs-viewer3d: scenery checkpoint {report}");
                }
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path.clone()));
                viewer_log!(
                    "openrailsrs-viewer3d: screenshot requested → {}",
                    path.display()
                );
                state.captured_at = Some(Instant::now());
            }
        }
        Some(t) => {
            // Give the render graph a few frames to flush the PNG to disk.
            if t.elapsed() >= Duration::from_secs_f32(2.5) {
                viewer_log!("openrailsrs-viewer3d: screenshot done, exiting");
                exit.write(AppExit::Success);
            }
        }
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct CaptureScene<'w, 's> {
    terrain_progress: Option<Res<'w, crate::terrain_spawn::TerrainSpawnProgress>>,
    terrain_stream: Option<Res<'w, crate::terrain_spawn::TerrainTileStream>>,
    world: Option<Res<'w, crate::world::WorldScene>>,
    activation: Option<Res<'w, crate::world::WorldSceneryStreamState>>,
    window: Option<Res<'w, crate::view_window::ViewWindow>>,
    origin: Option<Res<'w, crate::floating_origin::FloatingOrigin>>,
    entities: Option<Res<'w, crate::world_tile_index::WorldTileEntityIndex>>,
    focus: Option<Res<'w, crate::world::RouteFocus>>,
    sun: Option<Res<'w, crate::route_lighting::RouteSunState>>,
    environment: Option<Res<'w, crate::environment::LiveEnvironment>>,
    content: Option<Res<'w, crate::player_launch::ActivePlayerContent>>,
    storm: Option<Res<'w, crate::storm::StormState>>,
    performance: Res<'w, crate::performance::JourneyPerformance>,
    graphics_memory: Res<'w, crate::gpu_memory::GraphicsMemory>,
    weather_particles: Res<'w, crate::weather_particles::WeatherParticles>,
    renderer_selection: Res<'w, crate::weather_execution::RendererSelection>,
    pipelines: Option<Res<'w, crate::performance::ScenePipelineStatus>>,
    audio: Option<Res<'w, crate::native_audio::NativeAudio>>,
    effects: Option<Res<'w, crate::train_effects::TrainEffects>>,
    cab_diagnostic: Option<Res<'w, crate::cab_render::CabRenderDiagnostic>>,
    wet_surfaces: Option<Res<'w, crate::wet_surfaces::WetSurfaces>>,
    lod_fades: Query<'w, 's, &'static crate::world_lod_fade::LodFade>,
    menu_nodes: Query<
        'w,
        's,
        (
            &'static Name,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
    ui_text: Query<'w, 's, (&'static Text, &'static ComputedNode)>,
    camera: Query<
        'w,
        's,
        (
            &'static Transform,
            &'static Projection,
            Option<&'static crate::camera::OrbitState>,
            Has<bevy::render::view::NoIndirectDrawing>,
        ),
        With<Camera3d>,
    >,
    train_parts: Query<
        'w,
        's,
        &'static openrailsrs_bevy_scenery::shapes::ShapeAnimBinding,
        With<crate::rolling_stock_anim::TrainExteriorAnimPart>,
    >,
}

impl CaptureScene<'_, '_> {
    fn report(&self, live: Option<&crate::live::LiveDrive>) -> serde_json::Value {
        let deferred = self
            .world
            .as_ref()
            .zip(self.activation.as_ref())
            .zip(self.window.as_ref())
            .map(|((world, state), window)| {
                state.pending_shape_count(world, window.center_world, window.radius_m)
            });
        let camera =
            self.camera
                .single()
                .ok()
                .and_then(|(transform, projection, orbit, no_indirect)| {
                    let focus = self.focus.as_ref()?;
                    let shift = self
                        .origin
                        .as_ref()
                        .map_or(Vec3::ZERO, |origin| origin.shift);
                    let base =
                        Vec3::new(focus.center.x, focus.height_origin, focus.center.z) + shift;
                    let Projection::Perspective(projection) = projection else {
                        return None;
                    };
                    Some(serde_json::json!({
                        "position_world": (transform.translation + base).to_array(),
                        "rotation_xyzw": transform.rotation.to_array(),
                        "target_world": orbit.map(|orbit| (orbit.focus + base).to_array()),
                        "yaw_rad": orbit.map(|orbit| orbit.yaw),
                        "pitch_rad": orbit.map(|orbit| orbit.pitch),
                        "distance_m": orbit.map(|orbit| orbit.distance),
                        "fov_y_rad": projection.fov,
                        "aspect_ratio": projection.aspect_ratio,
                        "indirect_drawing": !no_indirect,
                    }))
                });
        let shared_shapes: std::collections::HashSet<_> = self
            .train_parts
            .iter()
            .map(|binding| std::sync::Arc::as_ptr(&binding.shape) as usize)
            .collect();
        let mut report = serde_json::json!({
            "odometer_m": live.map(|live| live.session.state.odometer_m),
            "service_complete": live.map(|live| live.session.arrived),
            "cpu_tiles": self.world.as_ref().map(|world| world.loaded_tiles.len()),
            "gpu_tiles": self.entities.as_ref().map(|index| index.tile_count()),
            "gpu_entities": self.entities.as_ref().map(|index| index.entity_count()),
            "unactivated_near_shapes": deferred,
            "pending_terrain_tiles": self.terrain_stream.as_ref().map_or(0,|stream| stream.pending_work()),
            "floating_origin_shift": self.origin.as_ref().map(|origin| origin.shift.to_array()),
            "clock_time_s": live.map(|live| live.clock_time_s()),
            "camera": camera,
            "cab_view": self.cab_diagnostic.as_ref().and_then(|diag| diag.report.as_ref()),
            "solar_direction": self.sun.as_ref().map(|sun| sun.direction.to_array()),
            "sunrise_s": self.sun.as_ref().and_then(|sun| sun.environment.map(|env| env.rise_time_s)),
            "sunset_s": self.sun.as_ref().and_then(|sun| sun.environment.map(|env| env.set_time_s)),
            "train_animated_parts": self.train_parts.iter().count(),
            "train_shared_shapes": shared_shapes.len(),
            "performance": self.performance.report(),
            "renderer": self.pipelines.as_ref().and_then(|p|p.device()),
            "native_audio": self.audio.as_ref().and_then(|a| a.engine.as_ref().map(|engine|engine.report())),
            "shader_pipelines": self.pipelines.as_ref().map(|p| {
                let (pending, failed) = p.counts();
                serde_json::json!({"pending":pending,"failed":failed})
            }),
            "pending_gpu_uploads": self.pipelines.as_ref().map(|p| p.pending_uploads()),
            "headlights": live.map(|live| live.session.headlights),
            "cab_light": live.map(|live| live.session.cab_light),
            "wiper": live.map(|live| live.session.wiper_active),
            "train_effects":self.effects.as_ref().map(|effects|effects.report()),
            "wetness":self.wet_surfaces.as_ref().map(|wet|wet.wetness),
            "snow_cover":self.wet_surfaces.as_ref().map(|wet|wet.snow_cover),
            "native_signal_programs":live.map(|l|l.session.native_signals.count()),
            "native_signal_errors":live.map(|l|&l.session.native_signals.errors),
            "active_lod_fades":self.lod_fades.iter().count(),
            "station_results":live.map(|live|&live.session.gameplay.stop_results),
            "quick_station_practice":live.map(|live|live.session.gameplay.quick_station_practice),
            "traffic": live.map(|live| live.traffic.services.iter().map(|s| serde_json::json!({"id":s.id,"departed":s.departed,"odometer_m":s.session.state.odometer_m,"edge":s.session.current_edge_id(),"velocity_kmh":s.session.velocity_mps()*3.6,"stops":s.session.gameplay.stop_results.len(),"arrived":s.session.arrived})).collect::<Vec<_>>()),
        });
        report["ui_layout"] = serde_json::Value::Array(self.menu_nodes.iter().filter(|(name, _, _)| {
                name.as_str().starts_with("ui-") || name.as_str().starts_with("home-")
                    || matches!(name.as_str(), "player-menu" | "launch-footer" | "launch-unavailable")
            }).map(|(name, node, transform)| {
                let center = transform.translation;
                let size = node.size();
                serde_json::json!({"name": name.as_str(), "min": (center - size * 0.5).to_array(), "max": (center + size * 0.5).to_array()})
            }).collect::<Vec<_>>());
        report["ui_text"] = serde_json::Value::Array(self.ui_text.iter().filter(|(text, _)| !text.0.trim().is_empty()).map(|(text, node)| {
            serde_json::json!({"text": text.0, "size": node.size().to_array()})
        }).collect());
        if let Some(environment) = self.environment.as_ref()
            && let Some(content) = self.content.as_ref()
        {
            report["environment"] = serde_json::json!({
                "selection": content.environment, "effective_weather": content.weather,
                "location": environment.location, "timezone": environment.timezone.map(|tz| tz.name()),
                "utc": environment.utc, "local_clock": environment.real_clock(content.environment).map(|t|t.to_rfc3339()),
                "sample": environment.current_sample(content.environment), "status":environment.network_status,
            });
        }
        report["storm"] = self.storm.as_ref().map_or(
            serde_json::Value::Null,
            |s| serde_json::json!({"strikes":s.strikes,"thunders":s.thunders,"flash":s.flash}),
        );
        report["graphics_memory"] =
            serde_json::to_value(&self.graphics_memory.0).unwrap_or_default();
        let (hits, misses) = openrailsrs_bevy_scenery::texture_cache::telemetry();
        report["texture_cache"] = serde_json::json!({"hits":hits,"misses":misses,"directory":openrailsrs_bevy_scenery::texture_cache::cache_dir()});
        report["texture_upload"] = serde_json::json!({
            "device_formats": format!("{:?}", openrailsrs_bevy_scenery::gpu_textures::device_texture_formats()),
            "policy": "authored DXT blocks when supported; lossless RGBA fallback",
        });
        report["weather_particles"] = self.weather_particles.report();
        report["renderer_selection"] =
            serde_json::to_value(*self.renderer_selection).unwrap_or_default();
        report
    }
}
