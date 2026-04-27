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
/// Renders a rich multi-line panel refreshed in-place using ANSI escape codes.
/// Each frame shows:
///   - Route progress bar (odometer)
///   - Time, velocity (current + peak)
///   - Throttle and brake bars
///   - Current edge and position on edge
///   - Cumulative energy
///
/// Sleeps `dt / speed_factor` between rows so the replay feels live.
pub fn animated_replay_from_csv(path: &Path, speed_factor: f64) -> Result<(), ExportError> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(buf.as_bytes());

    let headers = rdr.headers()?.clone();
    let col = |name: &str| headers.iter().position(|h| h == name);

    let idx_time = col("time_s");
    let idx_vel = col("velocity_mps");
    let idx_pos = headers
        .iter()
        .position(|h| h == "odometer_m" || h == "position_m");
    let idx_edge = col("edge_id");
    let idx_pos_on_edge = col("pos_on_edge_m");
    let idx_throttle = col("throttle");
    let idx_brake = col("brake");
    let idx_energy = col("cumulative_energy_kwh");

    let records: Vec<csv::StringRecord> = rdr.records().collect::<Result<_, _>>()?;

    let total_pos: f64 = records
        .last()
        .and_then(|r| idx_pos.and_then(|i| r.get(i)))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0)
        .max(1.0);

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("run.csv");

    const W: usize = 62; // inner panel width (between the border chars)
    const PANEL_LINES: usize = 11; // total lines printed per frame

    let hline: String = std::iter::repeat_n('─', W).collect();
    let border_top = format!("┌{hline}┐");
    let border_mid = format!("├{hline}┤");
    let border_bot = format!("└{hline}┘");

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    write!(out, "\x1b[?25l")?; // hide cursor

    let mut prev_time_s: f64 = 0.0;
    let mut peak_vel_kmh: f64 = 0.0;

    let fcol = |rec: &csv::StringRecord, idx: Option<usize>| -> f64 {
        idx.and_then(|i| rec.get(i))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    };

    for (frame, rec) in records.iter().enumerate() {
        let time_s = fcol(rec, idx_time);
        let vel_mps = fcol(rec, idx_vel);
        let pos_m = fcol(rec, idx_pos);
        let throttle = fcol(rec, idx_throttle).clamp(0.0, 1.0);
        let brake = fcol(rec, idx_brake).clamp(0.0, 1.0);
        let energy_kwh = fcol(rec, idx_energy);
        let edge_id: String = idx_edge.and_then(|i| rec.get(i)).unwrap_or("").to_string();
        let pos_on_edge = fcol(rec, idx_pos_on_edge);

        let vel_kmh = vel_mps * 3.6;
        if vel_kmh > peak_vel_kmh {
            peak_vel_kmh = vel_kmh;
        }

        // ── helpers ──────────────────────────────────────────────────
        let bar = |frac: f64, width: usize, fill: char, empty: char| -> String {
            let filled = (frac.clamp(0.0, 1.0) * width as f64).round() as usize;
            std::iter::repeat_n(fill, filled)
                .chain(std::iter::repeat_n(empty, width - filled))
                .collect()
        };
        let row = |content: &str| -> String {
            // Pad/truncate to W chars, wrap in border.
            let visible_len = content.chars().count();
            let pad = W.saturating_sub(visible_len);
            format!("│{}{}│", content, " ".repeat(pad))
        };

        // ── build panel lines ────────────────────────────────────────
        let pct = (pos_m / total_pos * 100.0) as u32;
        let route_bar = bar(pos_m / total_pos, 36, '█', '░');
        let throttle_bar = bar(throttle, 20, '█', '▒');
        let brake_bar = bar(brake, 20, '█', '▒');

        let l_header = format!("  openrailsrs  ·  replay  ·  {:<31}", filename);
        let l_sep1 = String::new();
        let l_route = format!("  Recorrido  {route_bar}  {:>5.0}m  {:>3}%", pos_m, pct);
        let l_time = format!("  Tiempo      {:>8.1} s", time_s);
        let l_vel = format!(
            "  Velocidad   {:>5.1} km/h       ↑ pico {:>5.1} km/h",
            vel_kmh, peak_vel_kmh
        );
        let l_throttle = format!("  Tracción    [{throttle_bar}]  {:>3.0}%", throttle * 100.0);
        let l_brake = format!("  Freno       [{brake_bar}]  {:>3.0}%", brake * 100.0);
        let l_edge = format!(
            "  Arista      {edge_id:<10}  pos en arista {:>7.0} m",
            pos_on_edge
        );
        let l_energy = format!("  Energía     {energy_kwh:.3} kWh");

        // ── draw ─────────────────────────────────────────────────────
        if frame > 0 {
            // Move cursor up PANEL_LINES to overwrite previous frame.
            write!(out, "\x1b[{}A", PANEL_LINES)?;
        }
        for line in [
            border_top.as_str(),
            &row(&l_header),
            &row(&l_sep1),
            &row(&l_route),
            &row(&l_time),
            &row(&l_vel),
            &row(&l_throttle),
            &row(&l_brake),
            &row(&l_edge),
            &row(&l_energy),
            border_bot.as_str(),
        ] {
            writeln!(out, "\x1b[2K{line}")?;
        }
        out.flush()?;

        let dt = (time_s - prev_time_s).max(0.0);
        prev_time_s = time_s;
        if dt > 0.0 && speed_factor > 0.0 {
            let sleep_ms = ((dt / speed_factor) * 1000.0) as u64;
            std::thread::sleep(Duration::from_millis(sleep_ms));
        }

        let _ = border_mid; // unused but kept for future section separators
    }

    write!(out, "\x1b[?25h")?; // restore cursor
    out.flush()?;
    Ok(())
}
