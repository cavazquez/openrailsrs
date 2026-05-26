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

/// Parse an Open Rails data-logger `dump.csv` or evaluation `*Speed.csv`.
pub fn parse_or_dump_csv(path: &Path, map: &OrColumnMap) -> Result<RunTrace, ValidateError> {
    let text = std::fs::read_to_string(path)?;
    let delimiter = detect_delimiter(&text)?;
    let body = skip_or_comment_preamble(&text);
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(delimiter)
        .from_reader(body.as_bytes());

    let headers = rdr.headers()?.clone();
    if is_or_evaluation_header(&headers) {
        let fields = or_eval_fields_from_header(&headers);
        if fields.is_empty() {
            return Err(ValidateError::Msg(format!(
                "unrecognized OR evaluation header in {}",
                path.display()
            )));
        }
        return parse_or_evaluation_speed_csv(path, map, &fields, &body);
    }
    if is_or_performance_header(&headers) {
        return parse_or_performance_dump_csv(path, map, &body);
    }

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

/// Column order for OR evaluation `*Speed.csv` (Options → Evaluation → train speed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrEvalField {
    Time,
    TrainSpeed,
    MaxSpeed,
    SignalAspect,
    Elevation,
    Direction,
    ControlMode,
    DistanceTravelled,
    ThrottlePerc,
    BrakePressure,
    DynBrakePerc,
    GearIndex,
}

fn is_or_evaluation_header(headers: &csv::StringRecord) -> bool {
    let mut has_time = false;
    let mut has_speed = false;
    for h in headers.iter() {
        let h = h.trim();
        if h.eq_ignore_ascii_case("TIME") {
            has_time = true;
        }
        if h.eq_ignore_ascii_case("TRAINSPEED") {
            has_speed = true;
        }
    }
    has_time && has_speed
}

fn or_eval_fields_from_header(headers: &csv::StringRecord) -> Vec<OrEvalField> {
    headers
        .iter()
        .filter_map(|h| {
            let h = h.trim();
            if h.is_empty() {
                return None;
            }
            Some(match h.to_ascii_uppercase().as_str() {
                "TIME" => OrEvalField::Time,
                "TRAINSPEED" => OrEvalField::TrainSpeed,
                "MAXSPEED" => OrEvalField::MaxSpeed,
                "SIGNALASPECT" => OrEvalField::SignalAspect,
                "ELEVATION" => OrEvalField::Elevation,
                "DIRECTION" => OrEvalField::Direction,
                "CONTROLMODE" => OrEvalField::ControlMode,
                "DISTANCETRAVELLED" => OrEvalField::DistanceTravelled,
                "THROTTLEPERC" => OrEvalField::ThrottlePerc,
                "BRAKEPRESSURE" => OrEvalField::BrakePressure,
                "DYNBRAKEPERC" => OrEvalField::DynBrakePerc,
                "GEARINDEX" => OrEvalField::GearIndex,
                _ => return None,
            })
        })
        .collect()
}

fn parse_or_evaluation_speed_csv(
    path: &Path,
    map: &OrColumnMap,
    fields: &[OrEvalField],
    body: &str,
) -> Result<RunTrace, ValidateError> {
    let mut samples = Vec::new();
    let mut t0: Option<f64> = None;
    let mut header_seen = false;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.is_empty() || parts[0].trim().is_empty() {
            continue;
        }
        let row = parse_or_eval_row(fields, &parts, map)?;
        let Some(abs_t) = row.time_abs_s else {
            continue;
        };
        let base = *t0.get_or_insert(abs_t);
        samples.push(TraceSample {
            time_s: abs_t - base,
            velocity_mps: row.velocity_mps,
            distance_m: row.distance_m,
            energy_kwh: None,
            throttle: row.throttle,
            brake: row.brake,
        });
    }
    if samples.is_empty() {
        return Err(ValidateError::Msg(format!(
            "no numeric rows in OR evaluation speed log {}",
            path.display()
        )));
    }
    Ok(RunTrace {
        source: path.display().to_string(),
        samples,
    })
}

struct OrEvalRow {
    time_abs_s: Option<f64>,
    velocity_mps: f64,
    distance_m: f64,
    throttle: Option<f64>,
    brake: Option<f64>,
}

fn parse_or_eval_row(
    fields: &[OrEvalField],
    parts: &[&str],
    map: &OrColumnMap,
) -> Result<OrEvalRow, ValidateError> {
    let mut row = parse_or_eval_row_positional(fields, parts, map)?;
    if let Some(tail) = parse_or_eval_tail(parts) {
        row.throttle = Some(tail.throttle);
        row.brake = Some(tail.brake);
        if tail.distance_m.is_some() {
            row.distance_m = tail.distance_m.unwrap_or(row.distance_m);
        }
    }
    Ok(row)
}

fn parse_or_eval_row_positional(
    fields: &[OrEvalField],
    parts: &[&str],
    map: &OrColumnMap,
) -> Result<OrEvalRow, ValidateError> {
    let mut idx = 0;
    let mut row = OrEvalRow {
        time_abs_s: None,
        velocity_mps: 0.0,
        distance_m: 0.0,
        throttle: None,
        brake: None,
    };
    for field in fields {
        if idx >= parts.len() {
            break;
        }
        match field {
            OrEvalField::Time => {
                row.time_abs_s = parse_hms_to_seconds(parts[idx]);
                idx += 1;
            }
            OrEvalField::TrainSpeed => {
                let (raw, consumed) = consume_or_train_speed(parts, idx);
                row.velocity_mps = map.speed_to_mps(raw);
                idx += consumed;
            }
            OrEvalField::MaxSpeed | OrEvalField::Elevation => {
                let (_, consumed) = consume_or_formatted_decimal(parts, idx);
                idx += consumed;
            }
            OrEvalField::SignalAspect | OrEvalField::ControlMode | OrEvalField::Direction => {
                idx += 1;
            }
            OrEvalField::DistanceTravelled => {
                let (raw, consumed) = consume_or_distance(parts, idx);
                row.distance_m = map.distance_to_m(raw);
                idx += consumed;
            }
            OrEvalField::ThrottlePerc => {
                row.throttle = Some(parse_or_int_field(parts[idx]) as f64 / 100.0);
                idx += 1;
            }
            OrEvalField::BrakePressure => {
                row.brake = Some(parse_or_int_field(parts[idx]) as f64);
                idx += 1;
            }
            OrEvalField::DynBrakePerc | OrEvalField::GearIndex => {
                idx += 1;
            }
        }
    }
    Ok(row)
}

/// OR evaluation rows often gain extra comma-separated tokens (e.g. `CLEAR_2`, `AUTO_SIGNAL`)
/// so fixed column indices drift. Throttle, brake, and distance are anchored at the tail:
/// `..., THROTTLE, BRAKE, -001, GEAR`.
struct OrEvalTail {
    distance_m: Option<f64>,
    throttle: f64,
    brake: f64,
}

fn parse_or_eval_tail(parts: &[&str]) -> Option<OrEvalTail> {
    let anchor = parts.iter().position(|p| p.trim() == "-001")?;
    if anchor < 2 {
        return None;
    }
    // OR uses `-001` in two different positions depending on context:
    //
    // Activity / AUTO_SIGNAL mode:
    //   ..., DISTANCETRAVELLED (single large token), THROTTLEPERC, BRAKEPRESSURE, -001 (DYNBRAKE), ...
    //   anchor-2 = THROTTLEPERC, anchor-1 = BRAKEPRESSURE (positive PSI).
    //
    // Explorer mode (no service brake applied):
    //   ..., DIST_INT, DIST_DEC, THROTTLEPERC, -001 (BRAKEPRESSURE = sentinel), DYNBRAKE, ...
    //   anchor-1 = THROTTLEPERC, anchor itself is the brake sentinel (0 PSI).
    //
    // We detect the explorer layout by checking whether anchor-1 looks like a valid
    // brake-cylinder pressure (>= 0 PSI integer) *and* there is an "EXPLORER" token,
    // or more robustly: anchor-1 is a short 0-padded integer ≤ 100 that would be a
    // reasonable throttle, while anchor-2 is also small (distance decimal).
    //
    // Explorer mode can still emit THROTTLEPERC + BRAKEPRESSURE before `-001` (dyn brake).
    // Only when brake is the sentinel itself (`..., THROTTLE, -001, DYN`) do we use the
    // short explorer layout.
    let is_explorer = parts
        .iter()
        .any(|p| p.trim().eq_ignore_ascii_case("EXPLORER"));
    let brake_is_sentinel = parts[anchor - 1].trim() == "-001";
    let (throttle, brake, dist_before) = if is_explorer && brake_is_sentinel && anchor >= 2 {
        let throttle = parse_or_int_field(parts[anchor - 2]) as f64 / 100.0;
        let distance_m = parse_or_eval_distance_before(parts, anchor - 2);
        (throttle, 0.0_f64, distance_m)
    } else {
        let throttle = parse_or_int_field(parts[anchor - 2]) as f64 / 100.0;
        let brake = parse_or_int_field(parts[anchor - 1]) as f64;
        let distance_m = parse_or_eval_distance_before(parts, anchor - 2);
        (throttle, brake, distance_m)
    };
    Some(OrEvalTail {
        distance_m: dist_before,
        throttle,
        brake,
    })
}

fn parse_or_eval_distance_before(parts: &[&str], before_idx: usize) -> Option<f64> {
    if before_idx == 0 {
        return Some(0.0);
    }
    // Skip preamble: time + train speed (up to 2 tokens) + max speed (up to 2) + signal + elevation.
    let mut idx = 1usize;
    let (_, speed_used) = consume_or_train_speed(parts, idx);
    idx += speed_used;
    let (_, max_used) = consume_or_formatted_decimal(parts, idx);
    idx += max_used;
    // signal aspect (single token, may contain underscore)
    if idx < before_idx {
        idx += 1;
    }
    // elevation
    if idx < before_idx {
        let (_, elev_used) = consume_or_formatted_decimal(parts, idx);
        idx += elev_used;
    }
    // direction / control mode tokens until the numeric distance cluster right before throttle.
    let mut best: Option<f64> = None;
    while idx < before_idx {
        let (raw, consumed) = consume_or_distance(parts, idx);
        if idx + consumed <= before_idx {
            best = Some(raw);
        }
        idx += consumed.max(1);
    }
    best
}

/// OR evaluation train speed uses `ToString("0000.0")`; with comma CSV separator the decimal
/// point becomes a second token (e.g. `0016,4` → **16.4** mph, not 1.64).
fn consume_or_train_speed(parts: &[&str], idx: usize) -> (f64, usize) {
    let t1 = parts.get(idx).copied().unwrap_or("").trim();
    if t1.is_empty() {
        return (0.0, 1);
    }
    if let Some(t2) = parts.get(idx + 1) {
        let t2 = t2.trim();
        if is_or_decimal_fragment(t2) {
            let whole = parse_or_int_field(t1) as f64;
            let frac_digits = t2.len() as i32;
            let frac = parse_or_int_field(t2) as f64 / 10_f64.powi(frac_digits);
            return (whole + frac, 2);
        }
    }
    consume_or_formatted_decimal(parts, idx)
}

/// OR writes `ToString("0000.0")` / `ToString("00.0")`; with comma CSV separator the decimal
/// comma splits one logical field into two tokens (see `Train.LogTrainSpeed` in Open Rails).
fn consume_or_formatted_decimal(parts: &[&str], idx: usize) -> (f64, usize) {
    let t1 = parts.get(idx).copied().unwrap_or("").trim();
    if t1.is_empty() {
        return (0.0, 1);
    }
    if let Some(v) = parse_f64(Some(t1)) {
        if t1.contains('.') || t1.contains('e') || t1.contains('E') {
            return (v, 1);
        }
    }
    if let Some(t2) = parts.get(idx + 1) {
        let t2 = t2.trim();
        if is_or_decimal_fragment(t2) {
            if let Some(v) = parse_f64(Some(&format!("{t1}.{t2}"))) {
                if v <= 9999.9 {
                    return (v, 2);
                }
            }
            let v = parse_or_int_field(t1) as f64 / 10.0 + parse_or_int_field(t2) as f64 / 100.0;
            return (v, 2);
        }
    }
    (parse_f64(Some(t1)).unwrap_or(0.0), 1)
}

fn consume_or_distance(parts: &[&str], idx: usize) -> (f64, usize) {
    let t1 = parts.get(idx).copied().unwrap_or("").trim();
    if t1.is_empty() {
        return (0.0, 1);
    }
    if let Some(v) = parse_f64(Some(t1)) {
        if t1.contains('.') || t1.contains('e') || t1.contains('E') {
            return (v, 1);
        }
    }
    if let Some(t2) = parts.get(idx + 1) {
        let t2 = t2.trim();
        if is_or_distance_continuation(t2) {
            if let Some(v) = parse_f64(Some(&format!("{t1}.{t2}"))) {
                return (v, 2);
            }
            if t2.contains('E') || t2.contains('e') {
                if let Some(v) = parse_f64(Some(&format!("{t1}.{t2}"))) {
                    return (v, 2);
                }
            }
        }
    }
    (parse_f64(Some(t1)).unwrap_or(0.0), 1)
}

fn is_or_decimal_fragment(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.len() <= 4 && s.chars().all(|c| c.is_ascii_digit())
}

fn is_or_distance_continuation(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.contains('E') || s.contains('e') {
        return s.chars().all(|c| c.is_ascii_digit() || ".Ee+-".contains(c));
    }
    s.chars().all(|c| c.is_ascii_digit())
}

fn parse_or_int_field(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    s.parse().unwrap_or(0)
}

fn parse_hms_to_seconds(hms: &str) -> Option<f64> {
    let mut it = hms.split(':');
    let h: f64 = it.next()?.parse().ok()?;
    let m: f64 = it.next()?.parse().ok()?;
    let s: f64 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some(h * 3600.0 + m * 60.0 + s)
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

fn is_or_performance_header(headers: &csv::StringRecord) -> bool {
    headers
        .get(0)
        .map(|h| h.trim().eq_ignore_ascii_case("Speed (mph)"))
        .unwrap_or(false)
}

struct PerfRow {
    time_abs_s: f64,
    velocity_mps: f64,
    throttle: Option<f64>,
}

fn parse_perf_row_line(line: &str, map: &OrColumnMap) -> Option<PerfRow> {
    let parts: Vec<&str> = line.split(',').collect();
    let mut time_idx = None;
    let mut speed_mph = None;
    let mut throttle = None;

    for (i, part) in parts.iter().enumerate() {
        let p = part.trim();
        if time_idx.is_none() && parse_hms_to_seconds(p).is_some() {
            time_idx = Some(i);
            if let Some(next) = parts.get(i + 1) {
                let n = next.trim();
                if let Ok(v) = n.parse::<f64>() {
                    if (0.0..=100.0).contains(&v) {
                        throttle = Some(v / 100.0);
                    }
                }
            }
        }
        let lower = p.to_ascii_lowercase();
        if lower.ends_with("mph") {
            let num: String = lower.chars().take(lower.len().saturating_sub(3)).collect();
            if let Ok(v) = num.parse::<f64>() {
                speed_mph = Some(v);
            }
        }
    }

    let idx = time_idx?;
    let time_abs_s = parse_hms_to_seconds(parts[idx].trim())?;
    Some(PerfRow {
        time_abs_s,
        velocity_mps: map.speed_to_mps(speed_mph.unwrap_or(0.0)),
        throttle,
    })
}

fn parse_or_performance_dump_csv(
    path: &Path,
    map: &OrColumnMap,
    body: &str,
) -> Result<RunTrace, ValidateError> {
    let mut samples: Vec<TraceSample> = Vec::new();
    let mut t0: Option<f64> = None;
    let mut prev_t = 0.0;
    let mut distance_m = 0.0;
    let mut header_seen = false;

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }
        let Some(row) = parse_perf_row_line(line, map) else {
            continue;
        };
        let base = *t0.get_or_insert(row.time_abs_s);
        let time_s = row.time_abs_s - base;
        if !samples.is_empty() {
            let dt = time_s - prev_t;
            if dt > 0.0 {
                distance_m += samples.last().unwrap().velocity_mps * dt;
            }
        }
        prev_t = time_s;
        samples.push(TraceSample {
            time_s,
            velocity_mps: row.velocity_mps,
            distance_m,
            energy_kwh: None,
            throttle: row.throttle,
            brake: None,
        });
    }

    if samples.is_empty() {
        return Err(ValidateError::Msg(format!(
            "no numeric rows in OR performance dump {}",
            path.display()
        )));
    }
    Ok(RunTrace {
        source: path.display().to_string(),
        samples,
    })
}

/// Peak brake value in an OR evaluation trace (pipe pressure, often 0–44 PSI).
pub fn infer_brake_full_scale(trace: &RunTrace) -> f64 {
    let max = trace
        .samples
        .iter()
        .filter_map(|s| s.brake)
        .fold(0.0_f64, f64::max);
    if max <= 1.0 + 1e-6 { 1.0 } else { max.max(1.0) }
}

/// Scale OR brake pressure samples to openrailsrs `[0, 1]` for comparison.
pub fn normalize_trace_brake_to_fraction(trace: &mut RunTrace, full_scale: Option<f64>) {
    let scale = full_scale.unwrap_or_else(|| infer_brake_full_scale(trace));
    if scale <= 1.0 + 1e-6 {
        return;
    }
    for sample in &mut trace.samples {
        if let Some(b) = sample.brake {
            sample.brake = Some((b / scale).clamp(0.0, 1.0));
        }
    }
}

/// Convert an OR evaluation `*Speed.csv` into a `ScriptedDriver` CSV (`time_s,throttle,brake`).
pub fn write_or_eval_driver_csv(
    or_eval_path: &Path,
    out_path: &Path,
    brake_full_scale: Option<f64>,
) -> Result<usize, ValidateError> {
    let mut trace = parse_or_dump_csv(or_eval_path, &OrColumnMap::default())?;
    let brake_scale = brake_full_scale.unwrap_or_else(|| infer_brake_full_scale(&trace));
    normalize_trace_brake_to_fraction(&mut trace, Some(brake_scale));

    let mut wtr = csv::WriterBuilder::new()
        .has_headers(true)
        .from_path(out_path)
        .map_err(|e| ValidateError::Msg(format!("write driver CSV: {e}")))?;
    wtr.write_record(["time_s", "throttle", "brake"])
        .map_err(|e| ValidateError::Msg(format!("write driver header: {e}")))?;

    let mut deduped: Vec<&TraceSample> = Vec::new();
    for s in &trace.samples {
        if deduped
            .last()
            .is_some_and(|p| (p.time_s - s.time_s).abs() < 1e-9)
        {
            deduped.pop();
        }
        deduped.push(s);
    }
    let mut rows = 0usize;
    for s in deduped {
        let throttle = s.throttle.unwrap_or(0.0);
        let brake = (s.brake.unwrap_or(0.0) / brake_scale).clamp(0.0, 1.0);
        wtr.write_record([
            format!("{:.3}", s.time_s),
            format!("{throttle:.4}"),
            format!("{brake:.4}"),
        ])
        .map_err(|e| ValidateError::Msg(format!("write driver row: {e}")))?;
        rows += 1;
    }
    wtr.flush()
        .map_err(|e| ValidateError::Msg(format!("flush driver CSV: {e}")))?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn parse_or_performance_dump_subset_fixture() {
        let path = fixtures_dir().join("or_perf_subset.csv");
        let trace = parse_or_dump_csv(&path, &OrColumnMap::default()).expect("parse perf dump");
        assert!(trace.samples.len() >= 10);
        assert!(trace.samples[0].time_s.abs() < 0.01);
        assert!(
            trace.samples.iter().any(|s| s.throttle.is_some()),
            "expected throttle column in perf dump"
        );
    }

    #[test]
    fn write_or_eval_driver_csv_fixture() {
        let eval = fixtures_dir().join("or_eval_speed_minimal.csv");
        let out = std::env::temp_dir().join("openrailsrs_driver_test.csv");
        let rows = write_or_eval_driver_csv(&eval, &out, None).expect("write driver");
        assert!(rows >= 10);
        let text = std::fs::read_to_string(&out).expect("read driver");
        assert!(text.contains("time_s,throttle,brake"));
        assert!(text.contains("0.8000") || text.contains("0.8"));
        let _ = std::fs::remove_file(out);
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
    fn parse_or_eval_speed_minimal_fixture() {
        let path = fixtures_dir().join("or_eval_speed_minimal.csv");
        let trace = parse_or_dump_csv(&path, &OrColumnMap::default()).expect("parse eval speed");
        assert!(trace.samples.len() >= 10);
        assert!((trace.samples[0].velocity_mps).abs() < 0.01);
        assert!((trace.samples[0].distance_m).abs() < 0.01);
        // 10:00:47 → 0016,4 = 16.4 mph (comma replaces decimal in OR Speed.csv)
        let late = trace
            .samples
            .iter()
            .find(|s| (s.time_s - 47.0).abs() < 0.5)
            .expect("sample near t=47s");
        let mph = late.velocity_mps / 0.447_04;
        assert!((mph - 16.4).abs() < 0.5, "expected ~16.4 mph, got {mph}");
        assert!(late.distance_m > 50.0);
        assert!(late.throttle.unwrap_or(0.0) > 0.5);
        // monotonic distance after movement starts
        let mut prev = 0.0;
        for s in &trace.samples {
            if s.distance_m > 1.0 {
                assert!(
                    s.distance_m + 0.01 >= prev,
                    "distance should be non-decreasing: {} then {}",
                    prev,
                    s.distance_m
                );
            }
            prev = s.distance_m;
        }
    }

    #[test]
    fn parse_or_train_speed_comma_decimal_is_whole_plus_fraction() {
        assert!((consume_or_train_speed(&["0016", "4"], 0).0 - 16.4).abs() < 1e-6);
        assert!((consume_or_train_speed(&["0011", "2"], 0).0 - 11.2).abs() < 1e-6);
        assert!((consume_or_train_speed(&["0001", "6"], 0).0 - 1.6).abs() < 1e-6);
    }

    #[test]
    fn parse_or_evaluation_header_detection() {
        let path = fixtures_dir().join("or_eval_speed_minimal.csv");
        let text = std::fs::read_to_string(&path).unwrap();
        let body = skip_or_comment_preamble(&text);
        let mut rdr = ReaderBuilder::new()
            .has_headers(true)
            .from_reader(body.as_bytes());
        let headers = rdr.headers().unwrap().clone();
        assert!(is_or_evaluation_header(&headers));
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
