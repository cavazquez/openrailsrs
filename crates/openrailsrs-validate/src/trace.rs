//! Normalized run traces and Open Rails `dump.csv` ingestion.

use std::path::Path;

use csv::ReaderBuilder;
use serde::{Deserialize, Serialize};

use crate::ValidateError;

/// One time sample from a run trace (openrailsrs or Open Rails).
#[derive(Debug, Clone, PartialEq)]
pub struct TraceSample {
    pub time_s: f64,
    pub velocity_mps: f64,
    pub distance_m: f64,
    pub energy_kwh: Option<f64>,
    pub throttle: Option<f64>,
    pub brake: Option<f64>,
}

/// Normalized time series from a CSV trace file.
#[derive(Debug, Clone, PartialEq)]
pub struct RunTrace {
    pub source: String,
    pub samples: Vec<TraceSample>,
}

/// How speed values in an Open Rails dump are expressed.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrSpeedUnit {
    #[default]
    Mph,
    Kmh,
    Mps,
}

/// How distance values in an Open Rails dump are expressed.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrDistanceUnit {
    #[default]
    Meters,
    Miles,
    Km,
}

/// Column names and unit conversion for Open Rails `dump.csv`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrColumnMap {
    #[serde(default = "default_time_column")]
    pub time_column: String,
    #[serde(default = "default_speed_column")]
    pub speed_column: String,
    #[serde(default = "default_distance_column")]
    pub distance_column: String,
    #[serde(default)]
    pub throttle_column: Option<String>,
    #[serde(default)]
    pub brake_column: Option<String>,
    #[serde(default)]
    pub speed_unit: OrSpeedUnit,
    #[serde(default)]
    pub distance_unit: OrDistanceUnit,
}

fn default_time_column() -> String {
    "Time".into()
}
fn default_speed_column() -> String {
    "Speed".into()
}
fn default_distance_column() -> String {
    "Distance".into()
}

impl Default for OrColumnMap {
    fn default() -> Self {
        Self {
            time_column: default_time_column(),
            speed_column: default_speed_column(),
            distance_column: default_distance_column(),
            throttle_column: None,
            brake_column: None,
            speed_unit: OrSpeedUnit::Mph,
            distance_unit: OrDistanceUnit::Meters,
        }
    }
}

impl OrColumnMap {
    fn speed_to_mps(&self, raw: f64) -> f64 {
        match self.speed_unit {
            OrSpeedUnit::Mph => raw * 0.447_04,
            OrSpeedUnit::Kmh => raw / 3.6,
            OrSpeedUnit::Mps => raw,
        }
    }

    fn distance_to_m(&self, raw: f64) -> f64 {
        match self.distance_unit {
            OrDistanceUnit::Meters => raw,
            OrDistanceUnit::Miles => raw * 1609.344,
            OrDistanceUnit::Km => raw * 1000.0,
        }
    }
}

/// Parse an openrailsrs `run.csv` into a normalized trace.
pub fn parse_openrailsrs_run_csv(path: &Path) -> Result<RunTrace, ValidateError> {
    let mut rdr = ReaderBuilder::new().has_headers(true).from_path(path)?;
    let headers = rdr.headers()?.clone();
    let idx = |name: &str| -> Result<Option<usize>, ValidateError> {
        Ok(headers.iter().position(|h| h.eq_ignore_ascii_case(name)))
    };
    let i_t = idx("time_s")?.ok_or_else(|| ValidateError::Msg("missing column time_s".into()))?;
    let i_v = idx("velocity_mps")?
        .ok_or_else(|| ValidateError::Msg("missing column velocity_mps".into()))?;
    let i_o =
        idx("odometer_m")?.ok_or_else(|| ValidateError::Msg("missing column odometer_m".into()))?;
    let i_e = idx("cumulative_energy_kwh")?;
    let i_th = idx("throttle")?;
    let i_br = idx("brake")?;

    let mut samples = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let time_s = match parse_f64(rec.get(i_t)) {
            Some(v) => v,
            None => continue,
        };
        let velocity_mps = parse_f64(rec.get(i_v)).unwrap_or(0.0);
        let distance_m = parse_f64(rec.get(i_o)).unwrap_or(0.0);
        let energy_kwh = i_e.and_then(|i| parse_f64(rec.get(i)));
        let throttle = i_th.and_then(|i| parse_f64(rec.get(i)));
        let brake = i_br.and_then(|i| parse_f64(rec.get(i)));
        samples.push(TraceSample {
            time_s,
            velocity_mps,
            distance_m,
            energy_kwh,
            throttle,
            brake,
        });
    }
    if samples.is_empty() {
        return Err(ValidateError::Msg(format!(
            "no numeric rows in {}",
            path.display()
        )));
    }
    Ok(RunTrace {
        source: path.display().to_string(),
        samples,
    })
}

/// Parse an Open Rails data-logger `dump.csv`.
pub fn parse_or_dump_csv(path: &Path, map: &OrColumnMap) -> Result<RunTrace, ValidateError> {
    let text = std::fs::read_to_string(path)?;
    let delimiter = detect_delimiter(&text)?;
    let body = skip_or_comment_preamble(&text);
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(delimiter)
        .from_reader(body.as_bytes());

    let headers = rdr.headers()?.clone();
    let col = |name: &str| -> Result<usize, ValidateError> {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
            .ok_or_else(|| ValidateError::Msg(format!("missing OR column {name}")))
    };
    let i_t = col(&map.time_column)?;
    let i_v = col(&map.speed_column)?;
    let i_d = col(&map.distance_column)?;
    let i_th = map.throttle_column.as_ref().map(|n| col(n)).transpose()?;
    let i_br = map.brake_column.as_ref().map(|n| col(n)).transpose()?;

    let mut samples = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let time_s = match parse_f64(rec.get(i_t)) {
            Some(v) => v,
            None => continue,
        };
        let velocity_mps = parse_f64(rec.get(i_v))
            .map(|v| map.speed_to_mps(v))
            .unwrap_or(0.0);
        let distance_m = parse_f64(rec.get(i_d))
            .map(|v| map.distance_to_m(v))
            .unwrap_or(0.0);
        let throttle = i_th.and_then(|i| parse_f64(rec.get(i)));
        let brake = i_br.and_then(|i| parse_f64(rec.get(i)));
        samples.push(TraceSample {
            time_s,
            velocity_mps,
            distance_m,
            energy_kwh: None,
            throttle,
            brake,
        });
    }
    if samples.is_empty() {
        return Err(ValidateError::Msg(format!(
            "no numeric rows in OR dump {}",
            path.display()
        )));
    }
    Ok(RunTrace {
        source: path.display().to_string(),
        samples,
    })
}

/// Resample two traces onto a common time grid with linear interpolation.
pub fn resample_traces(
    a: &RunTrace,
    b: &RunTrace,
    step_s: f64,
) -> Result<(Vec<TraceSample>, Vec<TraceSample>), ValidateError> {
    if step_s <= 0.0 {
        return Err(ValidateError::Msg("step_s must be positive".into()));
    }
    let t0 = a.samples[0].time_s.max(b.samples[0].time_s);
    let t_end = a
        .samples
        .last()
        .unwrap()
        .time_s
        .min(b.samples.last().unwrap().time_s);
    if t_end - t0 < 1.0 {
        return Err(ValidateError::Msg(format!(
            "temporal overlap too short: {:.3}s (need >= 1s)",
            t_end - t0
        )));
    }
    let mut grid = Vec::new();
    let mut t = t0;
    while t <= t_end + step_s * 0.5 {
        grid.push(t);
        t += step_s;
    }
    let ra: Vec<TraceSample> = grid
        .iter()
        .map(|&t| interpolate_sample(&a.samples, t))
        .collect();
    let rb: Vec<TraceSample> = grid
        .iter()
        .map(|&t| interpolate_sample(&b.samples, t))
        .collect();
    Ok((ra, rb))
}

fn detect_delimiter(text: &str) -> Result<u8, ValidateError> {
    let first = text
        .lines()
        .find(|line| {
            let t = line.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .ok_or_else(|| ValidateError::Msg("empty OR dump file".into()))?;
    for (d, _name) in [(b',', "comma"), (b';', "semicolon"), (b'\t', "tab")] {
        if first.contains(d as char) {
            return Ok(d);
        }
    }
    Err(ValidateError::Msg(format!(
        "could not detect CSV delimiter in header (expected comma, semicolon, or tab; got line starting with {first:?})"
    )))
}

fn skip_or_comment_preamble(text: &str) -> String {
    text.lines()
        .skip_while(|line| {
            let t = line.trim();
            t.is_empty() || t.starts_with('#')
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_f64(s: Option<&str>) -> Option<f64> {
    let s = s?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse().ok()
}

fn interpolate_sample(samples: &[TraceSample], t: f64) -> TraceSample {
    if samples.is_empty() {
        return TraceSample {
            time_s: t,
            velocity_mps: 0.0,
            distance_m: 0.0,
            energy_kwh: None,
            throttle: None,
            brake: None,
        };
    }
    if t <= samples[0].time_s {
        return sample_at_time(&samples[0], t);
    }
    if t >= samples.last().unwrap().time_s {
        return sample_at_time(samples.last().unwrap(), t);
    }
    let mut hi = 1;
    while hi < samples.len() && samples[hi].time_s < t {
        hi += 1;
    }
    let lo = hi - 1;
    let a = &samples[lo];
    let b = &samples[hi];
    let span = b.time_s - a.time_s;
    let f = if span.abs() < 1e-12 {
        0.0
    } else {
        (t - a.time_s) / span
    };
    TraceSample {
        time_s: t,
        velocity_mps: lerp(a.velocity_mps, b.velocity_mps, f),
        distance_m: lerp(a.distance_m, b.distance_m, f),
        energy_kwh: match (a.energy_kwh, b.energy_kwh) {
            (Some(x), Some(y)) => Some(lerp(x, y, f)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            (None, None) => None,
        },
        throttle: opt_lerp(a.throttle, b.throttle, f),
        brake: opt_lerp(a.brake, b.brake, f),
    }
}

fn sample_at_time(s: &TraceSample, t: f64) -> TraceSample {
    TraceSample {
        time_s: t,
        velocity_mps: s.velocity_mps,
        distance_m: s.distance_m,
        energy_kwh: s.energy_kwh,
        throttle: s.throttle,
        brake: s.brake,
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn opt_lerp(a: Option<f64>, b: Option<f64>, t: f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(lerp(x, y, t)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn parse_or_dump_minimal_fixture() {
        let path = fixtures_dir().join("or_dump_minimal.csv");
        let trace = parse_or_dump_csv(&path, &OrColumnMap::default()).expect("parse OR");
        assert_eq!(trace.samples.len(), 20);
        // 10 mph -> m/s
        assert!((trace.samples[0].velocity_mps - 4.4704).abs() < 0.01);
        assert!((trace.samples[0].distance_m - 0.0).abs() < 0.01);
        assert!((trace.samples[19].distance_m - 8.49376).abs() < 0.01);
    }

    #[test]
    fn parse_openrailsrs_aligned_fixture() {
        let path = fixtures_dir().join("ors_run_aligned.csv");
        let trace = parse_openrailsrs_run_csv(&path).expect("parse run");
        assert_eq!(trace.samples.len(), 20);
    }

    #[test]
    fn resample_overlap_too_short_errors() {
        let a = RunTrace {
            source: "a".into(),
            samples: vec![TraceSample {
                time_s: 0.0,
                velocity_mps: 0.0,
                distance_m: 0.0,
                energy_kwh: None,
                throttle: None,
                brake: None,
            }],
        };
        let b = RunTrace {
            source: "b".into(),
            samples: vec![TraceSample {
                time_s: 5.0,
                velocity_mps: 1.0,
                distance_m: 10.0,
                energy_kwh: None,
                throttle: None,
                brake: None,
            }],
        };
        assert!(resample_traces(&a, &b, 0.1).is_err());
    }
}
