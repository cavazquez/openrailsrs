use std::path::Path;

use csv::ReaderBuilder;
use serde::Serialize;

use crate::ValidateError;

#[derive(Debug, Default, Serialize)]
pub struct SeriesStats {
    pub max_abs_diff: f64,
    pub mean_abs_diff: f64,
    pub rms_diff: f64,
    pub samples: u64,
}

#[derive(Debug, Serialize)]
pub struct ComparisonReport {
    pub file_a: String,
    pub file_b: String,
    pub time_alignment: String,
    pub velocity: SeriesStats,
    pub position: SeriesStats,
    pub energy: SeriesStats,
}

/// Compare two run CSVs row-by-row on aligned `time_s` (must match within epsilon).
pub fn compare_csv_files(a: &Path, b: &Path) -> Result<ComparisonReport, ValidateError> {
    let rows_a = load_numeric_rows(a)?;
    let rows_b = load_numeric_rows(b)?;
    if rows_a.len() != rows_b.len() {
        return Err(ValidateError::Msg(format!(
            "row count mismatch: {} vs {}",
            rows_a.len(),
            rows_b.len()
        )));
    }
    let eps = 1e-4;
    let mut vel = SeriesStats::default();
    let mut pos = SeriesStats::default();
    let mut ene = SeriesStats::default();
    for (ra, rb) in rows_a.iter().zip(rows_b.iter()) {
        if (ra.time - rb.time).abs() > eps {
            return Err(ValidateError::Msg(format!(
                "time mismatch at row: {} vs {}",
                ra.time, rb.time
            )));
        }
        accumulate(&mut vel, ra.velocity - rb.velocity);
        accumulate(&mut pos, ra.odometer - rb.odometer);
        accumulate(&mut ene, ra.energy - rb.energy);
    }
    finalize_stats(&mut vel);
    finalize_stats(&mut pos);
    finalize_stats(&mut ene);

    Ok(ComparisonReport {
        file_a: a.display().to_string(),
        file_b: b.display().to_string(),
        time_alignment: "by_row_same_time_s".into(),
        velocity: vel,
        position: pos,
        energy: ene,
    })
}

struct Row {
    time: f64,
    velocity: f64,
    odometer: f64,
    energy: f64,
}

fn load_numeric_rows(path: &Path) -> Result<Vec<Row>, ValidateError> {
    let mut rdr = ReaderBuilder::new().has_headers(true).from_path(path)?;
    let headers = rdr.headers()?.clone();
    let idx = |name: &str| {
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .ok_or_else(|| ValidateError::Msg(format!("missing column {name}")))
    };
    let i_t = idx("time_s")?;
    let i_v = idx("velocity_mps")?;
    let i_o = idx("odometer_m")?;
    let i_e = idx("cumulative_energy_kwh")?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        out.push(Row {
            time: rec.get(i_t).unwrap_or("0").parse().unwrap_or(0.0),
            velocity: rec.get(i_v).unwrap_or("0").parse().unwrap_or(0.0),
            odometer: rec.get(i_o).unwrap_or("0").parse().unwrap_or(0.0),
            energy: rec.get(i_e).unwrap_or("0").parse().unwrap_or(0.0),
        });
    }
    Ok(out)
}

fn accumulate(s: &mut SeriesStats, diff: f64) {
    let ad = diff.abs();
    s.max_abs_diff = s.max_abs_diff.max(ad);
    s.mean_abs_diff += ad;
    s.rms_diff += diff * diff;
    s.samples += 1;
}

fn finalize_stats(s: &mut SeriesStats) {
    if s.samples == 0 {
        return;
    }
    s.mean_abs_diff /= s.samples as f64;
    s.rms_diff = (s.rms_diff / s.samples as f64).sqrt();
}
