use std::fmt::Arguments;
use std::io::{IsTerminal, Write};
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
    let now = SystemTime::now();
    let duration = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let ms = duration.subsec_millis();
    let secs = duration.as_secs() as i64;

    #[repr(C)]
    #[derive(Default)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: i64,
        tm_zone: *const i8,
    }

    unsafe extern "C" {
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }

    let mut tm = Tm::default();
    let ptr = unsafe { localtime_r(&secs, &mut tm) };
    if !ptr.is_null() {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            tm.tm_hour, tm.tm_min, tm.tm_sec, ms
        )
    } else {
        let total_s = duration.as_secs();
        let s = total_s % 60;
        let total_m = total_s / 60;
        let m = total_m % 60;
        let h = (total_m / 60) % 24;
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    }
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

fn write_line(writer: &mut impl Write, args: Arguments<'_>) {
    // A redirected log can fill its filesystem or lose its reader. Diagnostic
    // output must not abort the simulation in either case.
    let _ = writeln!(writer, "{args}");
}

pub fn message(args: Arguments<'_>) {
    let mut stderr = std::io::stderr().lock();
    if use_color() {
        write_line(
            &mut stderr,
            format_args!("{} \x1b[1;32mopenrailsrs-viewer3d:\x1b[0m {args}", prefix()),
        );
    } else {
        write_line(
            &mut stderr,
            format_args!("{} openrailsrs-viewer3d: {args}", prefix()),
        );
    }
}

/// Log a completed step with its own duration (since `since`).
pub fn log_step(label: &str, since: Instant) {
    let step_ms = since.elapsed().as_secs_f64() * 1000.0;
    if use_color() {
        message(format_args!("{label} \x1b[33m({step_ms:.1}ms step)\x1b[0m"));
    } else {
        message(format_args!("{label} ({step_ms:.1}ms step)"));
    }
}

#[macro_export]
macro_rules! viewer_log {
    ($($arg:tt)*) => {{
        $crate::log::message(format_args!($($arg)*));
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenWriter(i32);
    impl Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(self.0))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::from_raw_os_error(self.0))
        }
    }

    #[test]
    fn full_log_and_closed_reader_do_not_abort_simulation() {
        for error in [122, 32] {
            write_line(&mut BrokenWriter(error), format_args!("driving {} m", 2500));
        }
        let mut bytes = Vec::new();
        write_line(&mut bytes, format_args!("driving {} m", 2500));
        assert_eq!(bytes, b"driving 2500 m\n");
    }
}
