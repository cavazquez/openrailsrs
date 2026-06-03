//! Headless screenshot capture for visual debugging / CI snapshots.
//!
//! Gated by `OPENRAILSRS_SCREENSHOT=<path.png>`. Waits
//! `OPENRAILSRS_SCREENSHOT_DELAY_S` seconds (default 32, enough for progressive
//! world + terrain spawn), grabs the primary window, writes the PNG, then exits.
//! This lets the agent iterate on rendering without a human in the loop.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::viewer_log;

/// Wall-clock timing (not frame delta) so blocking load steps don't skew the delay.
#[derive(Resource)]
pub struct CaptureState {
    path: PathBuf,
    armed_at: Instant,
    delay: Duration,
    captured_at: Option<Instant>,
}

pub fn capture_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_SCREENSHOT").is_some_and(|v| !v.is_empty())
}

pub fn init_capture(mut commands: Commands) {
    let Some(path) = std::env::var_os("OPENRAILSRS_SCREENSHOT") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let delay_s = std::env::var("OPENRAILSRS_SCREENSHOT_DELAY_S")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(32.0)
        .max(0.0);
    viewer_log!(
        "openrailsrs-viewer3d: screenshot capture armed → {} in {:.0}s",
        PathBuf::from(&path).display(),
        delay_s
    );
    commands.insert_resource(CaptureState {
        path: PathBuf::from(path),
        armed_at: Instant::now(),
        delay: Duration::from_secs_f32(delay_s),
        captured_at: None,
    });
}

pub fn capture_system(
    state: Option<ResMut<CaptureState>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut state) = state else {
        return;
    };
    match state.captured_at {
        None => {
            if state.armed_at.elapsed() >= state.delay {
                let path = state.path.clone();
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
