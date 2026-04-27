use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use csv::ReaderBuilder;

use crate::ExportError;

/// Human-readable textual replay from a `run.csv` (first N rows).
pub fn textual_replay_from_csv(path: &Path, max_lines: usize) -> Result<String, ExportError> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(buf.as_bytes());
    let mut out = String::from("textual replay\n");
    for (i, rec) in rdr.records().enumerate() {
        if i >= max_lines {
            break;
        }
        let rec = rec?;
        out.push_str(&format!("{i}: {rec:?}\n"));
    }
    Ok(out)
}

/// Animated terminal replay from a `run.csv`.
///
/// Reads the CSV row by row and refreshes a single line in the terminal using ANSI escape codes,
/// showing a progress bar plus key telemetry (time, speed, position).  Sleeps `dt / speed_factor`
/// between rows so the replay feels live.
///
/// Columns expected: `time_s`, `velocity_mps`, `position_m`.  Extra columns are ignored.
pub fn animated_replay_from_csv(path: &Path, speed_factor: f64) -> Result<(), ExportError> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(buf.as_bytes());

    let headers = rdr.headers()?.clone();
    let idx_time = headers.iter().position(|h| h == "time_s");
    let idx_vel = headers.iter().position(|h| h == "velocity_mps");
    // Accept both "odometer_m" (run.csv native) and "position_m" (generic).
    let idx_pos = headers
        .iter()
        .position(|h| h == "odometer_m" || h == "position_m");

    // Collect all rows first so we can know the total distance for the progress bar.
    let records: Vec<csv::StringRecord> = rdr.records().collect::<Result<_, _>>()?;

    let total_pos: f64 = records
        .last()
        .and_then(|r| idx_pos.and_then(|i| r.get(i)))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .max(1.0);

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Hide cursor while animating.
    write!(out, "\x1b[?25l")?;

    let mut prev_time_s: f64 = 0.0;
    for rec in &records {
        let time_s: f64 = idx_time
            .and_then(|i| rec.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let vel_mps: f64 = idx_vel
            .and_then(|i| rec.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let pos_m: f64 = idx_pos
            .and_then(|i| rec.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        let frac = (pos_m / total_pos).clamp(0.0, 1.0);
        let bar_width: usize = 30;
        let filled = (frac * bar_width as f64).round() as usize;
        let bar: String = std::iter::repeat_n('#', filled)
            .chain(std::iter::repeat_n('-', bar_width - filled))
            .collect();

        // Erase line + carriage return; no newline so next frame overwrites.
        write!(
            out,
            "\x1b[2K\r[{bar}] t={time_s:>7.1}s  v={:>5.1}km/h  pos={pos_m:>8.0}m",
            vel_mps * 3.6,
        )?;
        out.flush()?;

        let dt = (time_s - prev_time_s).max(0.0);
        prev_time_s = time_s;
        if dt > 0.0 && speed_factor > 0.0 {
            let sleep_ms = ((dt / speed_factor) * 1000.0) as u64;
            std::thread::sleep(Duration::from_millis(sleep_ms));
        }
    }

    // Final newline + restore cursor.
    writeln!(out)?;
    write!(out, "\x1b[?25h")?;
    out.flush()?;

    Ok(())
}
