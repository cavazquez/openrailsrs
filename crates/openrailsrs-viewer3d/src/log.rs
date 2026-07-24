use std::io::IsTerminal;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime};

static START: OnceLock<Instant> = OnceLock::new();

/// Call once at process entry before any heavy loading.
pub fn init() {
    let _ = START.set(Instant::now());
}

fn elapsed_ms() -> f64 {
    START
        .get()
        .map(|t| t.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn wall_hms_ms() -> String {
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let total_ms = duration.as_millis();
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = (total_m / 60) % 24;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

fn use_color() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub fn prefix() -> String {
    if use_color() {
        format!("\x1b[36m[{:>8.1}ms {}]\x1b[0m", elapsed_ms(), wall_hms_ms())
    } else {
        format!("[{:>8.1}ms {}]", elapsed_ms(), wall_hms_ms())
    }
}

/// Log a completed step with its own duration (since `since`).
pub fn log_step(label: &str, since: Instant) {
    let step_ms = since.elapsed().as_secs_f64() * 1000.0;
    if use_color() {
        eprintln!(
            "{} \x1b[1;32mopenrailsrs-viewer3d:\x1b[0m {label} \x1b[33m({step_ms:.1}ms step)\x1b[0m",
            prefix()
        );
    } else {
        eprintln!(
            "{} openrailsrs-viewer3d: {label} ({step_ms:.1}ms step)",
            prefix()
        );
    }
}

#[macro_export]
macro_rules! viewer_log {
    ($($arg:tt)*) => {{
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) && std::env::var_os("NO_COLOR").is_none() {
            eprintln!("{} \x1b[1;32mopenrailsrs-viewer3d:\x1b[0m {}", $crate::log::prefix(), format!($($arg)*));
        } else {
            eprintln!("{} openrailsrs-viewer3d: {}", $crate::log::prefix(), format!($($arg)*));
        }
    }};
}
