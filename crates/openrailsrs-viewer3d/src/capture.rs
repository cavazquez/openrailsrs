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
    /// Max wait / delay before forcing capture (or the only wait when not after-ready).
    delay: Duration,
    after_ready: bool,
    ready_frames_needed: u32,
    ready_frames: u32,
    captured_at: Option<Instant>,
    target_odometer_m: Option<f64>,
    pause_at_target: bool,
    after_service: bool,
}

pub fn capture_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_SCREENSHOT").is_some_and(|v| !v.is_empty())
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
                && if state.after_ready {
                    if scenery_ready(progress) {
                        state.ready_frames = state.ready_frames.saturating_add(1);
                    } else {
                        state.ready_frames = 0;
                    }
                    let ready_ok = state.ready_frames >= state.ready_frames_needed;
                    ready_ok
                        || (timed_out && state.target_odometer_m.is_none() && !state.after_service)
                } else {
                    state.armed_at.elapsed() >= state.delay
                };
            if timed_out
                && (state.target_odometer_m.is_some() || state.after_service)
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
                if state.target_odometer_m.is_some() || state.after_service {
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
                if state.after_ready && state.ready_frames < state.ready_frames_needed {
                    viewer_log!(
                        "openrailsrs-viewer3d: screenshot forced after delay (scenery not ready)"
                    );
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
pub struct CaptureScene<'w> {
    world: Option<Res<'w, crate::world::WorldScene>>,
    activation: Option<Res<'w, crate::world::WorldSceneryStreamState>>,
    window: Option<Res<'w, crate::view_window::ViewWindow>>,
    origin: Option<Res<'w, crate::floating_origin::FloatingOrigin>>,
    entities: Option<Res<'w, crate::world_tile_index::WorldTileEntityIndex>>,
}

impl CaptureScene<'_> {
    fn report(&self, live: Option<&crate::live::LiveDrive>) -> serde_json::Value {
        let deferred = self
            .world
            .as_ref()
            .zip(self.activation.as_ref())
            .zip(self.window.as_ref())
            .map(|((world, state), window)| {
                state.pending_shape_count(world, window.center_world, window.radius_m)
            });
        serde_json::json!({
            "odometer_m": live.map(|live| live.session.state.odometer_m),
            "service_complete": live.map(|live| live.session.arrived),
            "cpu_tiles": self.world.as_ref().map(|world| world.loaded_tiles.len()),
            "gpu_tiles": self.entities.as_ref().map(|index| index.tile_count()),
            "gpu_entities": self.entities.as_ref().map(|index| index.entity_count()),
            "unactivated_near_shapes": deferred,
            "floating_origin_shift": self.origin.as_ref().map(|origin| origin.shift.to_array()),
        })
    }
}
