//! Native sample playback. File I/O, WAV decoding and SMS interpretation run
//! outside Bevy; a bounded mailbox prevents audio updates accumulating at x64.
use crate::sms::{self, Command, Control, SmsProgram, TriggerKind};
use openrailsrs_formats::{
    ConsistEntry, ConsistFile, parse_named_stf, parse_vehicle_text, read_msts_file_to_string,
    read_msts_text_with_includes, resolve_path_case_insensitive,
};
use openrailsrs_train::{consist_asset_root, resolve_consist_entry_path};
use rodio::{Decoder, DeviceSinkBuilder, Player, Source, buffer::SamplesBuffer};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU32, Ordering},
        mpsc,
    },
};

#[derive(Clone, Debug)]
pub struct ConsistSoundSpec {
    pub id: usize,
    pub consist: PathBuf,
    pub route: PathBuf,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SoundState {
    pub speed: f32,
    pub distance: f32,
    pub variable1: f32,
    pub variable2: f32,
    pub variable3: f32,
    pub brake_cylinder: f32,
    pub brake_pipe: f32,
    pub throttle: f32,
    pub brake: f32,
    pub direction: f32,
    pub steam_phase: Option<f64>,
    pub engine_on: Option<bool>,
    pub injector1: bool,
    pub injector2: bool,
    pub blower: bool,
    pub damper: f32,
    pub cylinder_cocks: bool,
    pub air_leak: bool,
    pub horn: bool,
    pub wiper: bool,
    pub wiper_rate: f32,
    pub sander: bool,
    pub doors: bool,
    pub headlights: u8,
    pub curve_squeal: bool,
    pub coupler_contact: i8,
    pub coupler_force_n: f32,
    pub coupler_relative_speed: f32,
    pub coupling_event: u8,
}
impl SoundState {
    fn control(self, c: Control, distance: f32) -> f32 {
        match c {
            Control::Speed => self.speed.abs(),
            Control::Distance => distance * distance,
            Control::Variable1 => self.variable1,
            Control::Variable2 => self.variable2,
            Control::Variable3 => self.variable3,
            Control::BrakeCylinder => self.brake_cylinder,
        }
    }
    fn events(self, old: Self) -> Vec<u32> {
        let mut e = vec![];
        for (now, before, on, off) in [
            (self.horn, old.horn, 8, 9),
            (self.wiper, old.wiper, 6, 7),
            (self.sander, old.sander, 4, 5),
            (self.doors, old.doors, 105, 106),
            (self.injector1, old.injector1, 30, 31),
            (self.injector2, old.injector2, 27, 28),
        ] {
            if now != before {
                e.push(if now { on } else { off });
            }
        }
        if let Some(now) = self.engine_on
            && old.engine_on != Some(now)
        {
            e.push(if now { 23 } else { 24 });
        }
        if self.blower != old.blower {
            e.push(33);
        }
        if self.cylinder_cocks != old.cylinder_cocks {
            e.push(34);
            e.push(if self.cylinder_cocks { 137 } else { 138 });
        }
        for (now, before, event) in [
            (self.throttle, old.throttle, 16),
            (self.brake, old.brake, 17),
            (self.direction, old.direction, 15),
            (self.damper, old.damper, 32),
        ] {
            if (now - before).abs() > 0.005 {
                e.push(event);
            }
        }
        if self.coupling_event != 0
            && old.coupling_event != 0
            && self.coupling_event != old.coupling_event
        {
            e.push(u32::from(self.coupling_event));
        }
        if self.coupler_contact != 0
            && self.coupler_contact != old.coupler_contact
            && self.coupler_force_n.abs() > 1000.
            && self.coupler_relative_speed.abs() > 0.05
            && !e.contains(&58)
        {
            e.push(58);
        }
        if self.headlights != old.headlights {
            e.push(37);
        }
        if let (Some(now), Some(before)) = (self.steam_phase, old.steam_phase)
            && self.throttle > 0.0
            && now.is_finite()
            && before.is_finite()
        {
            let a = before.floor() as i64;
            let b = now.floor() as i64;
            let direction = if b >= a { 1 } else { -1 };
            for i in 1..=b.abs_diff(a).min(64) {
                let phase = a.saturating_add((i as i64) * direction);
                e.push(121 + phase.wrapping_sub(1).rem_euclid(16) as u32);
            }
        }
        e
    }
}

/// AirSinglePipe's sound transitions: sample every half simulation second,
/// report the beginning/end of a pressure change rather than restarting a
/// sound every frame. Cylinder pressure is PSI, pipe pressure enters in bar.
#[derive(Default)]
struct BrakeSound {
    last_check_s: Option<f64>,
    cylinder_psi: f32,
    pipe_psi: f32,
    cylinder_changing: bool,
    pipe_changing: bool,
}
impl BrakeSound {
    fn events(&mut self, time_s: f64, state: SoundState) -> Vec<u32> {
        let pipe_psi = state.brake_pipe * 14.503774;
        let Some(last) = self.last_check_s else {
            self.last_check_s = Some(time_s);
            self.cylinder_psi = state.brake_cylinder;
            self.pipe_psi = pipe_psi;
            return vec![];
        };
        if time_s - last + 1e-9 < 0.5 {
            return vec![];
        }
        self.last_check_s = Some(time_s);
        let mut events = vec![];
        for (now, before, changing, increase, decrease, stop) in [
            (
                state.brake_cylinder,
                &mut self.cylinder_psi,
                &mut self.cylinder_changing,
                14,
                54,
                139,
            ),
            (
                pipe_psi,
                &mut self.pipe_psi,
                &mut self.pipe_changing,
                141,
                142,
                143,
            ),
        ] {
            if (now - *before).abs() > 0.1 {
                if !*changing {
                    events.push(if now > *before { increase } else { decrease });
                }
                *changing = true;
            } else if *changing {
                *changing = false;
                events.push(stop);
            }
            *before = now;
        }
        events
    }
}
#[derive(Clone, Debug)]
pub struct TrainSoundFrame {
    pub id: usize,
    pub state: SoundState,
    pub distance_m: f32,
    pub vehicle_distances_m: Vec<f32>,
    /// RPM, pressure and events belong to each car, not the whole formation.
    /// A short or absent vector retains the train-level fallback for oracles.
    pub vehicle_states: Vec<SoundState>,
    /// Listener-relative placement, optional for old offline reference frames.
    pub vehicle_placement: Vec<SoundPlacement>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundPlacement {
    pub pan: f32,
    pub doppler: f32,
    pub horn_gain: f32,
    pub blocked: bool,
    pub horn_source: Option<EmitterPlacement>,
    pub leak_source: Option<EmitterPlacement>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmitterPlacement {
    pub pan: f32,
    pub distance_m: f32,
    pub doppler: f32,
    pub blocked: bool,
}
impl Default for SoundPlacement {
    fn default() -> Self {
        Self {
            pan: 0.0,
            doppler: 1.0,
            horn_gain: 1.0,
            blocked: false,
            horn_source: None,
            leak_source: None,
        }
    }
}
impl SoundPlacement {
    pub fn from_radial_velocity(pan: f32, receding_mps: f32) -> Self {
        Self {
            horn_gain: 1.0,
            blocked: false,
            horn_source: None,
            leak_source: None,
            pan: if pan.is_finite() {
                pan.clamp(-1.0, 1.0)
            } else {
                0.0
            },
            doppler: if receding_mps.is_finite() && receding_mps.abs() <= 100.0 {
                (343.0 / (343.0 + receding_mps)).clamp(0.75, 1.35)
            } else {
                1.0
            },
        }
    }
}
impl TrainSoundFrame {
    fn vehicle_state(&self, index: usize) -> SoundState {
        self.vehicle_states
            .get(index)
            .copied()
            .unwrap_or(self.state)
    }
    fn vehicle_distance(&self, index: usize) -> f32 {
        self.vehicle_distances_m
            .get(index)
            .copied()
            .unwrap_or(self.distance_m)
    }
}
#[derive(Clone, Debug, Default)]
pub struct SoundFrame {
    pub time_s: f64,
    pub cab: bool,
    pub passenger: bool,
    /// Index of the player's car to which the listener is attached.
    pub listener_vehicle: usize,
    pub paused: bool,
    pub volume: f32,
    pub mix: crate::mix::SoundMix,
    /// Presentation delta controls crossfades; zero preserves offline oracles.
    pub presentation_dt_s: f32,
    pub window_opening: f32,
    pub announcements: Vec<(String, String)>,
    pub cover: f32,
    pub rain_surface: crate::weather::RainSurface,
    pub trains: Vec<TrainSoundFrame>,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct SoundReport {
    pub programs: usize,
    pub streams: usize,
    pub samples: usize,
    pub decoded_mib: f32,
    pub active_voices: usize,
    pub warnings: Vec<String>,
    pub device: bool,
    pub wiper_fallback: bool,
    pub voices: Vec<VoiceReport>,
    pub update_us: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct VoiceReport {
    pub train: usize,
    pub vehicle: usize,
    pub program: String,
    pub stream: usize,
    pub resource: String,
    pub trigger: String,
    pub group: crate::mix::SoundGroup,
    pub gain: f32,
    pub paused: bool,
}

#[derive(Clone)]
struct Wave {
    samples: SamplesBuffer,
    rate: f32,
    loop_range_s: Option<(f64, f64)>,
}
#[derive(Clone, Copy, PartialEq)]
enum SoundLocation {
    Exterior,
    Cab,
    Passenger,
}
struct Bank {
    train: usize,
    vehicle: usize,
    location: SoundLocation,
    program: SmsProgram,
    program_name: String,
    curve_only: bool,
    samples: HashMap<String, Wave>,
}
impl Bank {
    fn audible(
        &self,
        listener: SoundLocation,
        listener_vehicle: usize,
        has_interior: bool,
    ) -> bool {
        if let Some(cameras) = &self.program.cameras {
            // Other services use the exterior viewpoint even when the player
            // is in a cab, as OR SoundSource.ConditionsMet does. The SMS can
            // enable exterior playback from an Engine (cab) sound reference.
            return match if self.train == 0 && self.vehicle == listener_vehicle {
                listener
            } else {
                SoundLocation::Exterior
            } {
                SoundLocation::Cab => cameras.cab,
                SoundLocation::Passenger => cameras.passenger,
                SoundLocation::Exterior => cameras.exterior,
            };
        }
        match self.location {
            SoundLocation::Cab | SoundLocation::Passenger => {
                self.train == 0 && self.vehicle == listener_vehicle && self.location == listener
            }
            SoundLocation::Exterior => {
                self.train != 0 || listener == SoundLocation::Exterior || !has_interior
            }
        }
    }
}
pub struct NativeSoundBank {
    banks: Vec<Bank>,
    external_pass_through: HashMap<(usize, usize), f32>,
    pub report: SoundReport,
}

/// OR 1.6.1 SoundSource.SetRolloffFactor + OpenAL inverse-distance-clamped.
/// Camera eligibility and SMS activation are applied separately, allowing
/// authored distance curves to retain their original meaning.
fn inverse_distance_gain(distance: f32, deactivation_distance: f32) -> f32 {
    const REFERENCE: f32 = 8.0;
    const MAX_DISTANCE: f32 = 2000.0;
    const GAIN_AT_MAX: f32 = 0.025;
    if !distance.is_finite() {
        return 0.0;
    }
    let maximum = if deactivation_distance > 0.0 {
        deactivation_distance.min(MAX_DISTANCE)
    } else {
        MAX_DISTANCE
    };
    // Invalid/very small ranges must not introduce infinite or negative gains.
    let rolloff = REFERENCE * (1.0 / GAIN_AT_MAX - 1.0) / (maximum - REFERENCE).max(0.001);
    REFERENCE / (REFERENCE + rolloff * (distance.clamp(REFERENCE, MAX_DISTANCE) - REFERENCE))
}

fn source_active(was_active: bool, distance: f32, program: &SmsProgram) -> bool {
    if !distance.is_finite() || distance > 2000.0 || (!was_active && distance == 2000.0) {
        return false;
    }
    if program.deactivation_distance_m > 0.0 && distance > program.deactivation_distance_m {
        return false;
    }
    was_active || program.distance_m <= 0.0 || distance < program.distance_m
}

fn external_pass_through(ast: &openrailsrs_formats::Ast) -> Option<f32> {
    let wagon = if sms::block(ast, "Wagon") {
        Some(ast)
    } else {
        sms::child(ast, "Wagon")
    }?;
    let block = sms::child(wagon, "ORTSExternalSoundPassedThroughPercent")?;
    let percent: f32 = sms::items(block).get(1).and_then(sms::text)?.parse().ok()?;
    (percent.is_finite() && percent >= 0.0).then(|| percent.min(100.0) / 100.0)
}

fn curve_sound_refs(route: &Path) -> Vec<(String, SoundLocation)> {
    let route = resolve_path_case_insensitive(route).unwrap_or_else(|| route.into());
    let entries = std::fs::read_dir(&route)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path());
    let mut index = None;
    fn find(a: &openrailsrs_formats::Ast) -> Option<usize> {
        if sms::block(a, "ORTSCurveSMSNumber") {
            return sms::items(a).get(1).and_then(sms::text)?.parse().ok();
        }
        sms::items(a)
            .iter()
            .filter(|c| matches!(c, openrailsrs_formats::Ast::List(_)))
            .find_map(find)
    }
    for file in entries.filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("trk"))) {
        index = read_msts_file_to_string(&file)
            .ok()
            .and_then(|s| parse_named_stf(&s).ok())
            .and_then(|a| find(&a));
        if index.is_some() {
            break;
        }
    }
    let Some(index) = index else {
        return vec![];
    };
    let Some(path) = resolve_path_case_insensitive(&route.join("ttype.dat")) else {
        return vec![];
    };
    let Ok(text) = read_msts_file_to_string(&path) else {
        return vec![];
    };
    let text = text
        .lines()
        .filter(|l| !l.starts_with("SIMISA"))
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(ast) = parse_named_stf(&format!("TrackTypes ( {text} )")) else {
        return vec![];
    };
    let types = sms::items(&ast)
        .iter()
        .filter(|a| sms::block(a, "TrackType"))
        .collect::<Vec<_>>();
    let Some(t) = types.get(index) else {
        return vec![];
    };
    [(2, SoundLocation::Passenger), (3, SoundLocation::Exterior)]
        .into_iter()
        .filter_map(|(i, location)| Some((sms::items(t).get(i).and_then(sms::text)?, location)))
        .collect()
}

fn resolve_sample(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let relative = name.replace('\\', "/");
    dirs.iter().find_map(|d| {
        // Windows GetFullPath collapses `Sound/../../common.sound` before
        // probing the filesystem. A shared reference is valid even when the
        // locomotive has no local Sound directory.
        let mut path = PathBuf::new();
        for component in d.join(&relative).components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if matches!(
                        path.components().next_back(),
                        Some(std::path::Component::Normal(_))
                    ) {
                        path.pop();
                    } else if !path.has_root() {
                        path.push("..");
                    }
                }
                other => path.push(other),
            }
        }
        resolve_path_case_insensitive(&path).filter(|p| p.is_file())
    })
}
fn sound_vehicle_path(root: &Path, name: &std::ffi::OsStr) -> PathBuf {
    // OR selects TRAINSET/stock/OpenRails/stock.eng before the MSTS version.
    // Its Sound references are still relative to the stock's Sound directory.
    resolve_path_case_insensitive(&root.join("OpenRails").join(name))
        .filter(|p| p.is_file())
        .or_else(|| resolve_path_case_insensitive(&root.join(name)).filter(|p| p.is_file()))
        .unwrap_or_else(|| root.join(name))
}
/// RIFF smpl/cue loops use frame indices, not interleaved sample indices. A WAV
/// without loop markers repeats in full, then stops at release.
fn loop_range(bytes: &[u8], rate: f32) -> Option<(f64, f64)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let u32at = |n| {
        bytes
            .get(n..n + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let mut pos = 12;
    let mut cues = vec![];
    while pos + 8 <= bytes.len() {
        let size = u32at(pos + 4)? as usize;
        let data = pos + 8;
        if data.checked_add(size)? > bytes.len() {
            break;
        }
        if &bytes[pos..pos + 4] == b"smpl" && size >= 60 && u32at(data + 28)? > 0 {
            let start = u32at(data + 44)? as f64 / rate as f64;
            let end = (u32at(data + 48)? as f64 + 1.0) / rate as f64;
            if end > start {
                return Some((start, end));
            }
        }
        if &bytes[pos..pos + 4] == b"cue " && size >= 4 {
            let count = u32at(data)? as usize;
            for i in 0..count.min((size - 4) / 24) {
                cues.push(u32at(data + 4 + i * 24 + 20)? as f64 / rate as f64);
            }
        }
        pos = data + size + (size % 2);
    }
    cues.sort_by(f64::total_cmp);
    cues.dedup();
    if cues.len() >= 2 && cues[1] > cues[0] {
        Some((cues[0], cues[1]))
    } else {
        None
    }
}
impl NativeSoundBank {
    pub fn load(specs: &[ConsistSoundSpec]) -> Self {
        let mut result = Self {
            banks: vec![],
            external_pass_through: HashMap::new(),
            report: SoundReport::default(),
        };
        let mut waves: HashMap<PathBuf, Wave> = HashMap::new();
        for spec in specs {
            let parsed = read_msts_file_to_string(&spec.consist)
                .and_then(|t| parse_vehicle_text(&t))
                .and_then(|a| ConsistFile::from_ast(&a));
            let entries = match parsed {
                Ok(c) => c.entries,
                Err(_e) => {
                    result.report.warnings.push(format!(
                        "{}: formación ilegible",
                        crate::mix::resource_name(&spec.consist.to_string_lossy())
                    ));
                    continue;
                }
            };
            let curve_refs = curve_sound_refs(&spec.route);
            // A program per vehicle is needed for distinct rolling sound. Share
            // decoded WAVs even when the same program serves all eight coaches.
            for (vehicle, entry) in entries.into_iter().enumerate() {
                let rel = match entry {
                    ConsistEntry::Engine { path, .. } | ConsistEntry::Wagon { path, .. } => path,
                };
                let path = resolve_consist_entry_path(consist_asset_root(&spec.consist), &rel);
                let authored = path.parent().unwrap_or(Path::new("."));
                let content = spec
                    .route
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(&spec.route);
                let root = if authored.ancestors().any(|p| {
                    p.file_name()
                        .is_some_and(|n| n.eq_ignore_ascii_case("TRAINSET"))
                }) {
                    authored.to_path_buf()
                } else {
                    authored
                        .file_name()
                        .and_then(|name| {
                            resolve_path_case_insensitive(
                                &content.join("TRAINS/TRAINSET").join(name),
                            )
                        })
                        .unwrap_or_else(|| authored.into())
                };
                let stock = sound_vehicle_path(&root, path.file_name().unwrap_or_default());
                let ast =
                    match read_msts_text_with_includes(&stock).and_then(|t| parse_named_stf(&t)) {
                        Ok(a) => a,
                        Err(_e) => {
                            result.report.warnings.push(format!(
                                "{}: definición ilegible",
                                crate::mix::resource_name(&stock.to_string_lossy())
                            ));
                            continue;
                        }
                    };
                let mut refs = vec![];
                if let Some(gain) = external_pass_through(&ast) {
                    result
                        .external_pass_through
                        .insert((spec.id, vehicle), gain);
                }
                fn visit(
                    a: &openrailsrs_formats::Ast,
                    inside: SoundLocation,
                    out: &mut Vec<(String, SoundLocation)>,
                ) {
                    let interior = if sms::block(a, "Wagon") {
                        SoundLocation::Exterior
                    } else if sms::block(a, "Engine") {
                        SoundLocation::Cab
                    } else if sms::block(a, "Inside") {
                        SoundLocation::Passenger
                    } else {
                        inside
                    };
                    if sms::block(a, "Sound") {
                        if let Some(name) = sms::items(a).get(1).and_then(sms::text) {
                            out.push((name, interior));
                        }
                        return;
                    }
                    for c in sms::items(a) {
                        if matches!(c, openrailsrs_formats::Ast::List(_)) {
                            visit(c, interior, out);
                        }
                    }
                }
                visit(&ast, SoundLocation::Exterior, &mut refs);
                refs.extend(curve_refs.iter().cloned());
                for (name, location) in refs {
                    if !curve_refs.iter().any(|r| r.0 == name && r.1 == location)
                        && location != SoundLocation::Exterior
                        && result.banks.iter().any(|b| {
                            b.train == spec.id && b.vehicle == vehicle && b.location == location
                        })
                    {
                        continue;
                    }
                    let dirs = vec![
                        root.join("Sound"),
                        root.clone(),
                        spec.route.join("SOUND"),
                        content.join("SOUND"),
                    ];
                    let Some(path) = resolve_sample(&dirs, &name) else {
                        result.report.warnings.push(format!(
                            "Falta SMS: {} ({})",
                            crate::mix::resource_name(&name),
                            crate::mix::resource_name(&stock.to_string_lossy())
                        ));
                        continue;
                    };
                    let program = match SmsProgram::from_path(&path) {
                        Ok(p) => p,
                        Err(_e) => {
                            result.report.warnings.push(format!(
                                "{}: programa SMS inválido",
                                crate::mix::resource_name(&path.to_string_lossy())
                            ));
                            continue;
                        }
                    };
                    result
                        .report
                        .warnings
                        .extend(program.warnings.iter().map(|w| {
                            format!(
                                "{}: {w}",
                                crate::mix::resource_name(&path.to_string_lossy())
                            )
                        }));
                    let mut samples = HashMap::new();
                    for stream in &program.streams {
                        for trigger in &stream.triggers {
                            for command in &trigger.commands {
                                let Command::Play { files, .. } = command else {
                                    continue;
                                };
                                for name in files {
                                    if samples.contains_key(name) {
                                        continue;
                                    }
                                    let search = vec![
                                        path.parent().unwrap().into(),
                                        root.join("Sound"),
                                        spec.route.join("SOUND"),
                                        content.join("SOUND"),
                                    ];
                                    let Some(file) = resolve_sample(&search, name) else {
                                        result.report.warnings.push(format!(
                                            "Missing WAV: {} ({})",
                                            crate::mix::resource_name(name),
                                            crate::mix::resource_name(&path.to_string_lossy())
                                        ));
                                        continue;
                                    };
                                    if !waves.contains_key(&file) {
                                        let loaded = (|| -> Result<Wave, String> {
                                            let bytes =
                                                std::fs::read(&file).map_err(|e| e.to_string())?;
                                            let decoder = Decoder::try_from(std::io::Cursor::new(
                                                bytes.clone(),
                                            ))
                                            .map_err(|e| e.to_string())?;
                                            let channels = decoder.channels();
                                            let rate = decoder.sample_rate();
                                            // Bound each WAV and the whole bank. A corrupt/custom content
                                            // file must not exhaust RAM before the train starts.
                                            let data: Vec<_> = decoder.take(16_000_001).collect();
                                            if data.len() > 16_000_000
                                                || result.report.decoded_mib
                                                    + data.len() as f32 * 4.0 / 1048576.0
                                                    > 128.0
                                            {
                                                return Err(
                                                    "WAV decode budget exceeded (128 MiB bank)"
                                                        .into(),
                                                );
                                            }
                                            result.report.decoded_mib +=
                                                data.len() as f32 * 4.0 / 1048576.0;
                                            Ok(Wave {
                                                samples: SamplesBuffer::new(channels, rate, data),
                                                rate: rate.get() as f32,
                                                loop_range_s: loop_range(&bytes, rate.get() as f32),
                                            })
                                        })();
                                        match loaded {
                                            Ok(w) => {
                                                waves.insert(file.clone(), w);
                                            }
                                            Err(e) => {
                                                result.report.warnings.push(format!(
                                                    "{}: {e}",
                                                    crate::mix::resource_name(
                                                        &file.to_string_lossy()
                                                    )
                                                ));
                                                continue;
                                            }
                                        }
                                    }
                                    samples.insert(name.clone(), waves[&file].clone());
                                }
                            }
                        }
                    }
                    result.report.streams += program.streams.len();
                    result.banks.push(Bank {
                        train: spec.id,
                        vehicle,
                        location,
                        program,
                        program_name: crate::mix::resource_name(&path.to_string_lossy()),
                        curve_only: curve_refs.iter().any(|r| r.0 == name && r.1 == location),
                        samples,
                    });
                }
            }
        }
        result.report.programs = result.banks.len();
        result.report.samples = waves.len();
        result.report.warnings.sort();
        result.report.warnings.dedup();
        result
    }
}

struct LoopSource {
    source: SamplesBuffer,
    wave: Wave,
    elapsed_samples: usize,
    release: Arc<AtomicU8>,
    looping: bool,
}
impl Iterator for LoopSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let rate = self.wave.rate as f64;
        let channels = self.source.channels().get() as f64;
        let duration = self.wave.samples.total_duration()?.as_secs_f64();
        let (start, end) = self.wave.loop_range_s.unwrap_or((0.0, duration));
        let released = self.release.load(Ordering::Relaxed);
        if released == 2 {
            self.source
                .try_seek(std::time::Duration::from_secs_f64(end))
                .ok()?;
            self.elapsed_samples = (end * rate * channels) as usize;
            self.release.store(1, Ordering::Relaxed);
        }
        if self.looping && released == 0 && self.elapsed_samples as f64 >= end * rate * channels {
            self.source = self.wave.samples.clone();
            self.source
                .try_seek(std::time::Duration::from_secs_f64(start))
                .ok()?;
            self.elapsed_samples = (start * rate * channels) as usize;
        }
        let sample = self.source.next()?;
        self.elapsed_samples += 1;
        Some(sample)
    }
}
impl Source for LoopSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.source.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
struct StreamRuntime {
    player: Player,
    release: Option<Arc<AtomicU8>>,
    rate: f32,
    volume: f32,
    stereo_speed: f32,
    enabled: Vec<bool>,
    deadlines: Vec<f64>,
    choices: Vec<usize>,
    pan: Arc<AtomicU32>,
    group: crate::mix::SoundGroup,
    resource: String,
    trigger: String,
    mix_gain: f32,
    gain: f32,
}

/// Mono positional SMS sources become stereo without moving authored stereo
/// or Ignore3D programs. The shared pan changes without reloading a sample.
struct PannedSource {
    source: LoopSource,
    pan: Arc<AtomicU32>,
    right: Option<f32>,
}
impl Iterator for PannedSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        let mut mono = self.source.next()?;
        let channels = self.source.channels().get();
        for _ in 1..channels {
            mono += self.source.next()?;
        }
        mono /= f32::from(channels);
        let pan = f32::from_bits(self.pan.load(Ordering::Relaxed)).clamp(-1.0, 1.0);
        // Centre retains the native mono gain; extremes eliminate the far ear.
        self.right = Some(mono * (1.0 + pan).min(1.0));
        Some(mono * (1.0 - pan).min(1.0))
    }
}
impl Source for PannedSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        rodio::ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
struct Playback {
    bank: NativeSoundBank,
    voices: Vec<Vec<StreamRuntime>>,
    previous: HashMap<(usize, usize), (SoundState, f32)>,
    brake_sounds: HashMap<(usize, usize), BrakeSound>,
    last_coupler_impact: HashMap<(usize, usize), f64>,
    active_sources: Vec<bool>,
    initial: bool,
    last_time_s: f64,
    mixer: rodio::mixer::Mixer,
    wiper: Player,
    native_wiper: bool,
    native_wiper_vehicles: std::collections::HashSet<usize>,
    wiper_vehicle: usize,
    update_us: u64,
}
impl Playback {
    fn new(bank: NativeSoundBank, mixer: &rodio::mixer::Mixer) -> Self {
        let native_wiper_vehicles: std::collections::HashSet<_> = bank.banks.iter().filter(|b| {
            b.train == 0 && b.audible(SoundLocation::Cab,b.vehicle,true)
                && b.program.streams.iter().flat_map(|s| &s.triggers).any(|trigger| {
                    matches!(trigger.kind, TriggerKind::Discrete(6))
                        && trigger.commands.iter().any(|command| {
                            matches!(command, Command::Play { files, .. } if files.iter().any(|f| b.samples.contains_key(f)))
                        })
                })
        }).map(|b|b.vehicle).collect();
        let native_wiper = native_wiper_vehicles.contains(&0);
        let voices = bank
            .banks
            .iter()
            .map(|b| {
                b.program
                    .streams
                    .iter()
                    .map(|s| StreamRuntime {
                        player: Player::connect_new(mixer),
                        release: None,
                        rate: 1.0,
                        volume: 1.0,
                        stereo_speed: 1.0,
                        enabled: vec![true; s.triggers.len()],
                        deadlines: vec![0.0; s.triggers.len()],
                        choices: vec![0; s.triggers.len()],
                        pan: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
                        group: if b.curve_only {
                            crate::mix::SoundGroup::Rolling
                        } else {
                            crate::mix::classify(s, b.location != SoundLocation::Exterior)
                        },
                        resource: String::new(),
                        trigger: String::new(),
                        mix_gain: 0.0,
                        gain: 0.0,
                    })
                    .collect()
            })
            .collect();
        Self {
            active_sources: vec![false; bank.banks.len()],
            bank,
            voices,
            previous: HashMap::new(),
            brake_sounds: HashMap::new(),
            last_coupler_impact: HashMap::new(),
            initial: true,
            last_time_s: 0.0,
            mixer: mixer.clone(),
            wiper: Player::connect_new(mixer),
            native_wiper,
            native_wiper_vehicles,
            wiper_vehicle: 0,
            update_us: 0,
        }
    }
    fn update(&mut self, frame: &SoundFrame) {
        let started = std::time::Instant::now();
        if frame.time_s < self.last_time_s {
            // Restart and saved-game restore rewind the simulation clock. Keep
            // decoded samples but reset triggers, deadlines and loop releases.
            for voice in self.voices.iter_mut().flatten() {
                voice.player.stop();
                voice.player = Player::connect_new(&self.mixer);
                voice.release = None;
                voice.enabled.fill(true);
                voice.deadlines.fill(0.0);
                voice.choices.fill(0);
                voice.volume = 1.0;
            }
            self.previous.clear();
            self.brake_sounds.clear();
            self.last_coupler_impact.clear();
            self.active_sources.fill(false);
            self.initial = true;
            self.wiper.stop();
            self.wiper = Player::connect_new(&self.mixer);
        }
        self.last_time_s = frame.time_s;
        self.wiper_vehicle = frame.listener_vehicle;
        self.native_wiper = self.native_wiper_vehicles.contains(&frame.listener_vehicle);
        self.update_wiper(frame);
        let interior_gain = self
            .bank
            .external_pass_through
            .get(&(0, frame.listener_vehicle))
            .copied()
            // Open Rails UserSettings.ExternalSoundPassThruPercent defaults to 50.
            .unwrap_or(0.5);
        let interior_gain = crate::acoustics::window_gain(interior_gain, frame.window_opening);
        let mut frame_events = HashMap::new();
        for ((bank, voices), active) in self
            .bank
            .banks
            .iter()
            .zip(&mut self.voices)
            .zip(&mut self.active_sources)
        {
            let Some(train) = frame.trains.iter().find(|t| t.id == bank.train) else {
                for v in voices {
                    v.player.pause();
                }
                continue;
            };
            let state = train.vehicle_state(bank.vehicle);
            let history = self.previous.get(&(train.id, bank.vehicle)).copied();
            let previous = history.map_or(SoundState::default(), |(state, _)| state);
            let events = frame_events
                .entry((train.id, bank.vehicle))
                .or_insert_with(|| {
                    let mut previous = previous;
                    if history.is_none() {
                        previous.coupler_contact = state.coupler_contact;
                        previous.coupling_event = state.coupling_event;
                    }
                    let mut events = state.events(previous);
                    if events.contains(&58) && state.coupling_event == previous.coupling_event {
                        let key = (train.id, bank.vehicle);
                        if self
                            .last_coupler_impact
                            .get(&key)
                            .is_some_and(|t| frame.time_s - *t < 0.25)
                        {
                            events.retain(|e| *e != 58);
                        } else {
                            self.last_coupler_impact.insert(key, frame.time_s);
                        }
                    }
                    events.extend(
                        self.brake_sounds
                            .entry((train.id, bank.vehicle))
                            .or_default()
                            .events(frame.time_s, state),
                    );
                    events
                });
            let source_distance = train.vehicle_distance(bank.vehicle);
            *active = source_active(*active, source_distance, &bank.program);
            let listener = if frame.cab {
                SoundLocation::Cab
            } else if frame.passenger {
                SoundLocation::Passenger
            } else {
                SoundLocation::Exterior
            };
            let has_interior = self.bank.banks.iter().any(|b| {
                b.train == 0
                    && b.vehicle == frame.listener_vehicle
                    && b.location == listener
                    && !b.curve_only
            });
            let audible = *active
                && (!bank.curve_only || state.curve_squeal)
                && bank.audible(listener, frame.listener_vehicle, has_interior);
            let external = bank
                .program
                .cameras
                .as_ref()
                .map_or(bank.location == SoundLocation::Exterior, |c| c.exterior);
            let spatial_gain = if !external || bank.program.ignore_3d || bank.program.stereo {
                1.0
            } else {
                inverse_distance_gain(source_distance, bank.program.deactivation_distance_m)
            };
            let distance_gain = spatial_gain
                * if external && (frame.cab || frame.passenger) {
                    interior_gain
                } else {
                    1.0
                };
            for (stream, voice) in bank.program.streams.iter().zip(voices) {
                let mut placement = if external && !bank.program.ignore_3d && !bank.program.stereo {
                    train
                        .vehicle_placement
                        .get(bank.vehicle)
                        .copied()
                        .unwrap_or_default()
                } else {
                    SoundPlacement::default()
                };
                let horn_stream = stream
                    .triggers
                    .iter()
                    .any(|t| matches!(t.kind, TriggerKind::Discrete(8 | 9)));
                let leak_stream=stream.triggers.iter().flat_map(|t|&t.commands).any(|c| matches!(c,Command::Play{files,..} if files.iter().any(|f|f.to_ascii_lowercase().contains("leak"))));
                let emitter = if horn_stream {
                    placement.horn_source
                } else if leak_stream {
                    placement.leak_source
                } else {
                    None
                };
                let source_distance = emitter.map_or(source_distance, |e| e.distance_m);
                let distance_gain = emitter.map_or(distance_gain, |e| {
                    placement.pan = e.pan;
                    placement.doppler = e.doppler;
                    placement.blocked = e.blocked;
                    inverse_distance_gain(e.distance_m, bank.program.deactivation_distance_m)
                        * if frame.cab || frame.passenger {
                            interior_gain
                        } else {
                            1.
                        }
                });
                voice.pan.store(placement.pan.to_bits(), Ordering::Relaxed);
                for (i, trigger) in stream.triggers.iter().enumerate() {
                    if !voice.enabled[i] {
                        continue;
                    }
                    let fire = match trigger.kind {
                        TriggerKind::Initial => self.initial,
                        TriggerKind::Discrete(e) => events.contains(&e),
                        TriggerKind::Variable {
                            control,
                            increasing,
                            threshold,
                        } => {
                            // Native distance-decrease triggers start at MAX;
                            // all other variable triggers start at zero.
                            let old = history.map_or_else(
                                || {
                                    if control == Control::Distance && !increasing {
                                        f32::MAX
                                    } else {
                                        0.0
                                    }
                                },
                                |(state, distance)| state.control(control, distance),
                            );
                            let now = state.control(control, source_distance);
                            if increasing {
                                old <= threshold && now > threshold
                            } else {
                                old >= threshold && now < threshold
                            }
                        }
                        TriggerKind::Random { min_s, max_s } => {
                            if voice.deadlines[i] == 0.0 {
                                voice.deadlines[i] = frame.time_s + min_s.max(0.1) as f64;
                                false
                            } else if frame.time_s >= voice.deadlines[i] {
                                voice.deadlines[i] =
                                    frame.time_s + ((min_s + max_s) * 0.5).max(0.1) as f64;
                                true
                            } else {
                                false
                            }
                        }
                        TriggerKind::Distance { min_m, max_m } => {
                            if voice.deadlines[i] == 0.0 {
                                voice.deadlines[i] = state.distance as f64 + min_m.max(0.1) as f64;
                                false
                            } else if state.distance as f64 >= voice.deadlines[i] {
                                voice.deadlines[i] =
                                    state.distance as f64 + ((min_m + max_m) * 0.5).max(0.1) as f64;
                                true
                            } else {
                                false
                            }
                        }
                    };
                    if !fire {
                        continue;
                    }
                    for command in &trigger.commands {
                        match command {
                            Command::Play {
                                files,
                                looping,
                                release: _,
                                random,
                            } => {
                                if files.is_empty() {
                                    continue;
                                }
                                let choice = voice.choices[i];
                                voice.choices[i] = choice.wrapping_add(1);
                                let index = if *random {
                                    choice.wrapping_mul(1664525).wrapping_add(1013904223)
                                        % files.len()
                                } else {
                                    choice % files.len()
                                };
                                let Some(wave) = bank.samples.get(&files[index]) else {
                                    continue;
                                };
                                voice.resource = crate::mix::resource_name(&files[index]);
                                voice.trigger = format!("{:?}", trigger.kind);
                                voice.player.stop();
                                voice.player = Player::connect_new(&self.mixer);
                                let release = Arc::new(AtomicU8::new(0));
                                let source = LoopSource {
                                    source: wave.samples.clone(),
                                    wave: wave.clone(),
                                    elapsed_samples: 0,
                                    release: release.clone(),
                                    looping: *looping,
                                };
                                if external && !bank.program.ignore_3d && !bank.program.stereo {
                                    voice.player.append(PannedSource {
                                        source,
                                        pan: voice.pan.clone(),
                                        right: None,
                                    });
                                } else {
                                    voice.player.append(source);
                                }
                                voice.rate = wave.rate;
                                voice.stereo_speed =
                                    if bank.program.stereo && wave.samples.channels().get() == 1 {
                                        2.0
                                    } else {
                                        1.0
                                    };
                                voice.release = Some(release);
                            }
                            Command::Release { jump } => {
                                if let Some(flag) = &voice.release {
                                    flag.store(if *jump { 2 } else { 1 }, Ordering::Relaxed);
                                }
                            }
                            Command::Volume(v) => voice.volume = *v,
                            Command::Enable(i) => {
                                if let Some(e) = voice.enabled.get_mut(*i) {
                                    *e = true;
                                }
                            }
                            Command::Disable(i) => {
                                if let Some(e) = voice.enabled.get_mut(*i) {
                                    *e = false;
                                }
                            }
                        }
                    }
                }
                let curves = stream
                    .volumes
                    .iter()
                    .map(|c| c.value(state.control(c.control, source_distance)))
                    .product::<f32>();
                let spatial = if horn_stream {
                    placement.horn_gain
                } else if placement.blocked {
                    0.55
                } else {
                    1.
                };
                let target = if audible && (!leak_stream || state.air_leak) {
                    distance_gain * spatial
                } else {
                    0.0
                };
                voice.mix_gain = if self.initial || frame.presentation_dt_s <= 0.0 {
                    target
                } else {
                    crate::mix::approach(
                        voice.mix_gain,
                        target,
                        frame.presentation_dt_s.min(0.2),
                        0.18,
                    )
                };
                let gain = bank.program.volume
                    * stream.volume
                    * voice.volume
                    * curves
                    * voice.mix_gain
                    * frame.mix.gain(voice.group)
                    * frame.volume;
                voice.gain = gain.clamp(0.0, 1.0);
                voice.player.set_volume(voice.gain);
                let pitch = stream.frequency.as_ref().map_or(1.0, |c| {
                    c.value(state.control(c.control, source_distance)) / voice.rate
                });
                voice.player.set_speed(
                    (pitch
                        * voice.stereo_speed
                        * placement.doppler
                        * if voice.group == crate::mix::SoundGroup::Cab
                            && stream
                                .triggers
                                .iter()
                                .any(|t| matches!(t.kind, TriggerKind::Discrete(6 | 7)))
                        {
                            state.wiper_rate.max(1.0)
                        } else {
                            1.0
                        })
                    .clamp(0.1, 4.0),
                );
                if frame.paused {
                    voice.player.pause();
                } else {
                    voice.player.play();
                }
            }
        }
        self.previous = self
            .bank
            .banks
            .iter()
            .filter_map(|bank| {
                let train = frame.trains.iter().find(|t| t.id == bank.train)?;
                Some((
                    (bank.train, bank.vehicle),
                    (
                        train.vehicle_state(bank.vehicle),
                        train.vehicle_distance(bank.vehicle),
                    ),
                ))
            })
            .collect();
        self.initial = false;
        self.update_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    }

    fn update_wiper(&mut self, frame: &SoundFrame) {
        if self.native_wiper {
            if !self.wiper.empty() {
                self.wiper.stop();
                self.wiper = Player::connect_new(&self.mixer);
            }
            return;
        }
        let train = frame.trains.iter().find(|t| t.id == 0);
        let on = train.is_some_and(|t| t.vehicle_state(frame.listener_vehicle).wiper);
        if !on {
            if !self.wiper.empty() {
                self.wiper.stop();
                self.wiper = Player::connect_new(&self.mixer);
            }
            return;
        }
        if self.wiper.empty() {
            self.wiper.append(
                SamplesBuffer::new(
                    rodio::ChannelCount::new(1).unwrap(),
                    rodio::SampleRate::new(crate::wiper::SAMPLE_RATE).unwrap(),
                    crate::wiper::samples(frame.time_s),
                )
                .repeat_infinite(),
            );
        }
        let gain = if frame.cab {
            1.0
        } else if !frame.cab && !frame.passenger {
            train.map_or(0.0, |t| {
                0.15 * inverse_distance_gain(t.vehicle_distance(frame.listener_vehicle), 100.0)
            })
        } else {
            0.0
        };
        self.wiper.set_speed(train.map_or(1., |t| {
            t.vehicle_state(frame.listener_vehicle).wiper_rate.max(1.)
        }));
        self.wiper
            .set_volume(frame.volume * frame.mix.gain(crate::mix::SoundGroup::Cab) * gain);
        if frame.paused {
            self.wiper.pause();
        } else {
            self.wiper.play();
        }
    }

    fn report(&self) -> SoundReport {
        let mut report = self.bank.report.clone();
        report.active_voices = self
            .voices
            .iter()
            .flatten()
            .filter(|v| !v.player.empty())
            .count();
        report.active_voices += usize::from(!self.wiper.empty());
        report.wiper_fallback = !self.native_wiper;
        report.update_us = self.update_us;
        report.voices = self
            .bank
            .banks
            .iter()
            .zip(&self.voices)
            .flat_map(|(b, voices)| {
                voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| !v.player.empty())
                    .map(|(stream, v)| VoiceReport {
                        train: b.train,
                        vehicle: b.vehicle,
                        program: b.program_name.clone(),
                        stream,
                        resource: v.resource.clone(),
                        trigger: v.trigger.clone(),
                        group: v.group,
                        gain: v.gain,
                        paused: v.player.is_paused(),
                    })
            })
            .take(256)
            .collect();
        if !self.wiper.empty() {
            report.voices.push(VoiceReport {
                train: 0,
                vehicle: self.wiper_vehicle,
                program: "limpiaparabrisas · respaldo Rust".into(),
                stream: 0,
                resource: "wiper sintetizado".into(),
                trigger: "orden y batería disponibles".into(),
                group: crate::mix::SoundGroup::Cab,
                gain: self.wiper.volume(),
                paused: self.wiper.is_paused(),
            });
        }
        report
    }
}

pub struct NativeAudioEngine {
    tx: mpsc::SyncSender<SoundFrame>,
    thunder_tx: mpsc::SyncSender<crate::thunder::ThunderEvent>,
    weather_tx: mpsc::SyncSender<(f32, f32)>,
    report: Arc<Mutex<SoundReport>>,
}
fn mix_limit() -> rodio::source::LimitSettings {
    // Limit the combined formation rather than clipping each PCM output sample.
    // Immediate attack protects horn/brake transients; release preserves stereo
    // balance and the quiet parts of the native recordings.
    rodio::source::LimitSettings::default()
        .with_threshold(-1.0)
        .with_knee_width(2.0)
        .with_attack(std::time::Duration::ZERO)
        .with_release(std::time::Duration::from_millis(50))
}
impl NativeAudioEngine {
    pub fn start(specs: Vec<ConsistSoundSpec>) -> Option<Self> {
        Self::start_with_announcements(specs, vec![])
    }
    pub fn start_with_announcements(
        specs: Vec<ConsistSoundSpec>,
        announcements: Vec<crate::announcements::LicensedSample>,
    ) -> Option<Self> {
        if std::env::var_os("OPENRAILSRS_DISABLE_AUDIO").is_some() {
            return None;
        }
        let (tx, rx) = mpsc::sync_channel::<SoundFrame>(2);
        let (thunder_tx, thunder_rx) = mpsc::sync_channel::<crate::thunder::ThunderEvent>(2);
        let (weather_tx, weather_rx) = mpsc::sync_channel::<(f32, f32)>(2);
        let report = Arc::new(Mutex::new(SoundReport::default()));
        let shared = report.clone();
        std::thread::spawn(move || {
            let bank = NativeSoundBank::load(&specs);
            let Ok(output) = DeviceSinkBuilder::open_default_sink() else {
                let mut r = bank.report;
                r.warnings.push("No audio output device".into());
                *shared.lock().unwrap() = r;
                return;
            };
            let (mixer, source) = rodio::mixer::mixer(
                output.config().channel_count(),
                output.config().sample_rate(),
            );
            let covered = Arc::new(AtomicU32::new(0.0f32.to_bits()));
            output.mixer().add(
                crate::acoustics::CoveredSource::new(source, covered.clone()).limit(mix_limit()),
            );
            let mut playback = Playback::new(bank, &mixer);
            let mut announcements =
                crate::announcements::AnnouncementPlayer::new(announcements, &mixer);
            let mut thunder_voices: Vec<(Player, f32)> = vec![];
            let rain: [Player; 4] = std::array::from_fn(|_| Player::connect_new(&mixer));
            let wind = Player::connect_new(&mixer);
            for (player, samples) in rain
                .iter()
                .zip(crate::weather::RainSurface::ALL.map(|s| s.samples()))
                .chain(std::iter::once((&wind, crate::weather::samples(true))))
            {
                player.append(
                    SamplesBuffer::new(
                        rodio::ChannelCount::new(1).unwrap(),
                        rodio::SampleRate::new(crate::weather::SAMPLE_RATE).unwrap(),
                        samples,
                    )
                    .repeat_infinite(),
                );
                player.set_volume(0.);
            }
            let mut atmosphere = (0., 0.);
            let mut surface_gains = [0.0; 4];
            for frame in rx {
                covered.store(
                    (frame.cover.clamp(0., 1.)
                        * if frame.cab || frame.passenger {
                            0.035
                        } else {
                            0.22
                        })
                    .to_bits(),
                    Ordering::Relaxed,
                );
                playback.update(&frame);
                announcements.update(
                    frame.time_s,
                    frame.paused,
                    frame.volume * frame.mix.gain(crate::mix::SoundGroup::Announcements),
                    &frame.announcements,
                );
                for latest in weather_rx.try_iter() {
                    atmosphere = latest;
                }
                let gains = crate::weather::gains(
                    atmosphere.0,
                    atmosphere.1,
                    frame.cab || frame.passenger,
                    frame.volume * frame.mix.gain(crate::mix::SoundGroup::Ambient),
                    frame.paused,
                );
                for (i, player) in rain.iter().enumerate() {
                    let target = if i == frame.rain_surface.index() {
                        frame.rain_surface.gain()
                    } else {
                        0.
                    };
                    surface_gains[i] = crate::mix::approach(
                        surface_gains[i],
                        target,
                        frame.presentation_dt_s.min(0.2),
                        0.3,
                    );
                    player.set_volume(gains.0 * surface_gains[i]);
                }
                wind.set_volume(gains.1);
                for player in rain.iter().chain(std::iter::once(&wind)) {
                    if frame.paused {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
                thunder_voices.retain(|(player, _)| !player.empty());
                for event in thunder_rx.try_iter() {
                    if thunder_voices.len() >= 2 {
                        thunder_voices.remove(0).0.stop();
                    }
                    let player = Player::connect_new(&mixer);
                    player.append(SamplesBuffer::new(
                        rodio::ChannelCount::new(1).unwrap(),
                        rodio::SampleRate::new(crate::thunder::SAMPLE_RATE).unwrap(),
                        crate::thunder::samples(event.seed, event.distance_m),
                    ));
                    thunder_voices.push((player, event.distance_m));
                }
                for (player, distance) in &thunder_voices {
                    player.set_volume(
                        frame.volume
                            * frame.mix.gain(crate::mix::SoundGroup::Ambient)
                            * crate::thunder::gain(*distance, frame.cab),
                    );
                    if frame.paused {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
                let mut r = playback.report();
                r.warnings.extend(announcements.warnings.iter().cloned());
                r.decoded_mib += announcements.decoded_mib;
                r.device = true;
                *shared.lock().unwrap() = r;
            }
        });
        Some(Self {
            tx,
            thunder_tx,
            weather_tx,
            report,
        })
    }
    pub fn send(&self, frame: SoundFrame) {
        let _ = self.tx.try_send(frame);
    }
    pub fn thunder(&self, event: crate::thunder::ThunderEvent) {
        let _ = self.thunder_tx.try_send(event);
    }
    pub fn weather(&self, rain: f32, wind_mps: f32) {
        let _ = self.weather_tx.try_send((rain, wind_mps));
    }
    pub fn report(&self) -> SoundReport {
        self.report.lock().unwrap().clone()
    }
}

/// Device-independent oracle using the same SMS runtime, sources and rodio mixer
/// as live playback. WAV output permits listening/review without opening ALSA.
pub fn render_oracle(
    specs: &[ConsistSoundSpec],
    frames: &[SoundFrame],
    output: &Path,
) -> Result<SoundReport, String> {
    use std::io::Write;
    let (mixer, source) = rodio::mixer::mixer(
        std::num::NonZeroU16::new(2).unwrap(),
        std::num::NonZeroU32::new(44100).unwrap(),
    );
    let wet = Arc::new(AtomicU32::new(0.0f32.to_bits()));
    let mut source = crate::acoustics::CoveredSource::new(source, wet.clone()).limit(mix_limit());
    let mut playback = Playback::new(NativeSoundBank::load(specs), &mixer);
    let mut pcm = vec![];
    for pair in frames.windows(2) {
        wet.store(pair[0].cover.clamp(0., 0.28).to_bits(), Ordering::Relaxed);
        playback.update(&pair[0]);
        let samples = ((pair[1].time_s - pair[0].time_s).max(0.0) * 44100.0) as usize;
        for _ in 0..samples * 2 {
            pcm.extend_from_slice(
                &((source.next().unwrap_or(0.0).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes(),
            );
        }
    }
    let mut file = std::fs::File::create(output).map_err(|e| e.to_string())?;
    file.write_all(b"RIFF")
        .and_then(|_| file.write_all(&(pcm.len() as u32 + 36).to_le_bytes()))
        .and_then(|_| file.write_all(b"WAVEfmt "))
        .map_err(|e| e.to_string())?;
    for bytes in [
        16u32.to_le_bytes().to_vec(),
        1u16.to_le_bytes().to_vec(),
        2u16.to_le_bytes().to_vec(),
        44100u32.to_le_bytes().to_vec(),
        176400u32.to_le_bytes().to_vec(),
        4u16.to_le_bytes().to_vec(),
        16u16.to_le_bytes().to_vec(),
    ] {
        file.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    file.write_all(b"data")
        .and_then(|_| file.write_all(&(pcm.len() as u32).to_le_bytes()))
        .and_then(|_| file.write_all(&pcm))
        .map_err(|e| e.to_string())?;
    Ok(playback.report())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bank(vehicle: usize, program: SmsProgram) -> Bank {
        Bank {
            train: 0,
            vehicle,
            location: SoundLocation::Exterior,
            program,
            program_name: "test.sms".into(),
            curve_only: false,
            samples: HashMap::from([("tone.wav".into(), test_wave())]),
        }
    }

    fn test_playback(banks: Vec<Bank>) -> (Playback, rodio::mixer::MixerSource) {
        let (mixer, source) = rodio::mixer::mixer(
            std::num::NonZeroU16::new(2).unwrap(),
            std::num::NonZeroU32::new(44100).unwrap(),
        );
        (
            Playback::new(
                NativeSoundBank {
                    banks,
                    external_pass_through: HashMap::new(),
                    report: SoundReport::default(),
                },
                &mixer,
            ),
            source,
        )
    }

    fn test_frame() -> SoundFrame {
        SoundFrame {
            volume: 1.0,
            trains: vec![TrainSoundFrame {
                id: 0,
                state: SoundState::default(),
                distance_m: 0.0,
                vehicle_distances_m: vec![],
                vehicle_states: vec![],
                vehicle_placement: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn positional_sound_pans_without_restarting_and_doppler_reverses_on_passing() {
        let wave = test_wave();
        let pan = Arc::new(AtomicU32::new((-1.0f32).to_bits()));
        let mut source = PannedSource {
            source: LoopSource {
                source: wave.samples.clone(),
                wave,
                elapsed_samples: 0,
                release: Arc::new(AtomicU8::new(0)),
                looping: true,
            },
            pan: pan.clone(),
            right: None,
        };
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(0.1));
        assert_eq!(source.next(), Some(0.0));
        pan.store(1.0f32.to_bits(), Ordering::Relaxed);
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(0.2));
        pan.store(0.0f32.to_bits(), Ordering::Relaxed);
        assert_eq!(source.next(), Some(0.3));
        assert_eq!(source.next(), Some(0.3));
        assert!(SoundPlacement::from_radial_velocity(0.0, -30.0).doppler > 1.0);
        assert!(SoundPlacement::from_radial_velocity(0.0, 30.0).doppler < 1.0);
        assert_eq!(
            SoundPlacement::from_radial_velocity(f32::NAN, 500.0),
            SoundPlacement::default()
        );
    }

    #[test]
    fn ignore3d_and_authored_stereo_keep_native_pitch_and_balance() {
        for option in ["Ignore3D ()", "Stereo ()"] {
            let program = SmsProgram::parse(&format!("Tr_SMS ( ScalabiltyGroup ( 5 {option} Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )")).unwrap();
            let (mut playback, _) = test_playback(vec![test_bank(0, program)]);
            let mut frame = test_frame();
            playback.update(&frame);
            let authored_speed = playback.voices[0][0].player.speed();
            frame.trains[0].vehicle_placement =
                vec![SoundPlacement::from_radial_velocity(-1.0, -30.0)];
            playback.update(&frame);
            let voice = &playback.voices[0][0];
            assert_eq!(f32::from_bits(voice.pan.load(Ordering::Relaxed)), 0.0);
            assert_eq!(voice.player.speed(), authored_speed);
        }
    }

    #[test]
    fn wiper_fallback_is_audible_in_cab_and_obeys_pause_mute_and_off() {
        let (mut playback, mut source) = test_playback(vec![]);
        let mut frame = test_frame();
        frame.cab = true;
        frame.trains[0].state.wiper = true;
        playback.update(&frame);
        let energy = |source: &mut rodio::mixer::MixerSource| {
            source.take(22050).map(|s| s * s).sum::<f32>() / 22050.0
        };
        assert!(energy(&mut source) > 0.0001);
        assert_eq!(playback.report().active_voices, 1);
        frame.paused = true;
        playback.update(&frame);
        for _ in 0..1000 {
            source.next();
        }
        assert_eq!(energy(&mut source), 0.0);
        frame.paused = false;
        frame.volume = 0.0;
        playback.update(&frame);
        for _ in 0..1000 {
            source.next();
        }
        assert_eq!(energy(&mut source), 0.0);
        frame.volume = 1.0;
        playback.update(&frame);
        assert!(energy(&mut source) > 0.0001);
        frame.trains[0].state.wiper = false;
        playback.update(&frame);
        for _ in 0..1000 {
            source.next();
        }
        assert_eq!(energy(&mut source), 0.0);
        assert_eq!(playback.report().active_voices, 0);
    }

    #[test]
    fn playable_native_wiper_is_used_once_and_missing_wav_gets_fallback() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Activation ( CabCam () ) Streams ( 1 Stream ( Triggers ( 2 Discrete_Trigger ( 6 StartLoop ( 1 File ( tone.wav -1 ) ) ) Discrete_Trigger ( 7 ReleaseLoopRelease () ) ) ) ) ) )").unwrap();
        let mut bank = test_bank(0, program);
        bank.location = SoundLocation::Cab;
        let (mut native, _) = test_playback(vec![bank]);
        assert!(!native.report().wiper_fallback);
        let mut frame = test_frame();
        frame.cab = true;
        frame.trains[0].state.wiper = true;
        native.update(&frame);
        assert!(native.wiper.empty());
        let mut bank = test_bank(0, native.bank.banks[0].program.clone());
        bank.location = SoundLocation::Cab;
        bank.samples.clear();
        let (mut fallback, _) = test_playback(vec![bank]);
        fallback.update(&frame);
        assert!(!fallback.wiper.empty());
    }

    #[test]
    fn included_interior_sound_and_pass_through_load_for_each_car() {
        let dir = tempfile::tempdir().unwrap();
        let route = dir.path().join("ROUTES/Test");
        let stock = dir.path().join("TRAINS/TRAINSET/Stock");
        let consist = dir.path().join("TRAINS/CONSISTS/two.con");
        std::fs::create_dir_all(&route).unwrap();
        std::fs::create_dir_all(stock.join("Sound")).unwrap();
        std::fs::create_dir_all(consist.parent().unwrap()).unwrap();
        std::fs::write(&consist, "Train ( TrainCfg ( Two Wagon ( WagonData (car0 Stock) ) Wagon ( WagonData (car1 Stock) ) ) )").unwrap();
        for car in ["car0", "car1"] {
            std::fs::write(
                stock.join(format!("{car}.wag")),
                format!("Wagon ( {car} Include (interior.inc) )"),
            )
            .unwrap();
        }
        std::fs::write(
            stock.join("interior.inc"),
            "Inside ( Sound (interior.sms) ) ORTSExternalSoundPassedThroughPercent (25)",
        )
        .unwrap();
        std::fs::write(
            stock.join("Sound/interior.sms"),
            "Tr_SMS ( ScalabiltyGroup (5 Activation (PassengerCam ()) Streams (0)) )",
        )
        .unwrap();
        let bank = NativeSoundBank::load(&[ConsistSoundSpec {
            id: 0,
            consist,
            route,
        }]);
        assert!(
            bank.report.warnings.is_empty(),
            "{:?}",
            bank.report.warnings
        );
        assert_eq!(bank.banks.len(), 2);
        assert_eq!(bank.external_pass_through.get(&(0, 0)), Some(&0.25));
        assert_eq!(bank.external_pass_through.get(&(0, 1)), Some(&0.25));
        for b in bank.banks {
            assert!(b.audible(SoundLocation::Passenger, b.vehicle, true));
            assert!(!b.audible(SoundLocation::Passenger, 1 - b.vehicle, true));
        }
    }

    #[test]
    fn distance_attenuation_matches_the_original_binary_reference() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-audio.json")).unwrap();
        for row in reference["attenuation"].as_array().unwrap() {
            let distance = row["distance_m"].as_f64().unwrap() as f32;
            let maximum = row["deactivation_m"].as_f64().unwrap() as f32;
            let expected = row["gain"].as_f64().unwrap() as f32;
            let actual = inverse_distance_gain(distance, maximum);
            assert!(
                (actual - expected).abs() < 1e-6,
                "{distance}, {maximum}: {actual} vs {expected}"
            );
        }
        assert_eq!(inverse_distance_gain(f32::INFINITY, 0.0), 0.0);
        assert!(inverse_distance_gain(50.0, 8.0).is_finite());
    }

    #[test]
    fn distance_trigger_commands_match_the_original_binary_boundaries() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-audio.json")).unwrap();
        let program =
            SmsProgram::parse(include_str!("../../../oracles/fixtures/audio-distance.sms"))
                .unwrap();
        let (mut playback, _) = test_playback(vec![test_bank(0, program.clone())]);
        let mut frame = test_frame();
        for (i, row) in reference["distance_checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            frame.time_s = i as f64;
            frame.trains[0].distance_m = row["distance_m"].as_f64().unwrap() as f32;
            playback.update(&frame);
            let actual = playback.voices[0][0].volume;
            let expected = row["volume"].as_f64().unwrap() as f32;
            assert_eq!(actual, expected, "distance={}m", frame.trains[0].distance_m);
        }
        // A first frame outside the threshold must fire Distance_Inc_Past,
        // whose native initial value is zero rather than the decrease sentinel.
        let (mut playback, _) = test_playback(vec![test_bank(0, program)]);
        frame = test_frame();
        frame.trains[0].distance_m = 101.0;
        playback.update(&frame);
        assert_eq!(playback.voices[0][0].volume, 0.8);
    }

    #[test]
    fn nearby_distance_loop_starts_once_and_restarts_after_reentry_or_rewind() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( Triggers ( 1 Variable_Trigger ( Distance_Dec_Past 100 StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let (mut playback, _) = test_playback(vec![test_bank(0, program)]);
        let mut frame = test_frame();
        for (i, distance, starts) in [(0, 50.0, 1), (1, 49.0, 1), (2, 101.0, 1), (3, 99.0, 2)] {
            frame.time_s = i as f64;
            frame.trains[0].distance_m = distance;
            playback.update(&frame);
            assert_eq!(playback.voices[0][0].choices[0], starts);
            assert_eq!(playback.voices[0][0].player.len(), 1);
        }
        frame.time_s = 0.0;
        playback.update(&frame);
        assert_eq!(playback.voices[0][0].choices[0], 1);
    }

    #[test]
    fn sms_distance_hysteresis_and_unattenuated_flags_preserve_native_scope() {
        let mut program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Activation ( ExternalCam () Distance (100) ) Deactivation ( Distance (150) ) Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let mut active = false;
        for (distance, expected) in [
            (120.0, false),
            (100.0, false),
            (99.0, true),
            (125.0, true),
            (150.0, true),
            (151.0, false),
            (120.0, false),
            (99.0, true),
            (f32::INFINITY, false),
        ] {
            active = source_active(active, distance, &program);
            assert_eq!(active, expected, "distance={distance}");
        }
        program.distance_m = 500.0;
        program.deactivation_distance_m = 1000.0;
        let normal = test_bank(0, program.clone());
        let mut unattenuated = test_bank(0, program.clone());
        unattenuated.program.ignore_3d = true;
        let mut stereo = test_bank(0, program);
        stereo.program.stereo = true;
        let (mut playback, _) = test_playback(vec![normal, unattenuated, stereo]);
        let mut frame = test_frame();
        frame.trains[0].distance_m = 100.0;
        playback.update(&frame);
        assert!(playback.voices[0][0].player.volume() < 0.5);
        assert_eq!(playback.voices[1][0].player.volume(), 1.0);
        assert_eq!(playback.voices[2][0].player.volume(), 1.0);
        let mut program = playback.bank.banks[0].program.clone();
        program.distance_m = 0.0;
        program.deactivation_distance_m = 0.0;
        assert!(!source_active(false, 2000.0, &program));
        assert!(source_active(true, 2000.0, &program));
        assert!(!source_active(true, 2001.0, &program));
    }

    #[test]
    fn window_opening_changes_native_pass_through_without_duplicate_voices() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Activation ( ExternalCam () PassengerCam () CabCam () Distance (100) ) Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let (mut playback, _) = test_playback(vec![test_bank(0, program)]);
        playback.bank.external_pass_through.insert((0, 0), 0.2);
        let mut frame = test_frame();
        frame.cab = true;
        for (opening, gain) in [(0., 0.2), (0.5, 0.6), (1., 1.), (0., 0.2)] {
            frame.window_opening = opening;
            playback.update(&frame);
            assert!((playback.voices[0][0].player.volume() - gain).abs() < 1e-6);
            assert_eq!(playback.voices[0][0].player.len(), 1);
        }
        frame.cab = false;
        playback.update(&frame);
        assert_eq!(playback.voices[0][0].player.volume(), 1.);
        frame.passenger = true;
        frame.window_opening = 0.5;
        playback.update(&frame);
        assert!((playback.voices[0][0].player.volume() - 0.6).abs() < 1e-6);
    }
    #[test]
    fn leak_emitter_does_not_move_engine_and_repair_silences_only_leak() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 2 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( air_leak.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let mut bank = test_bank(0, program);
        bank.samples.insert("air_leak.wav".into(), test_wave());
        let (mut playback, _) = test_playback(vec![bank]);
        let mut frame = test_frame();
        frame.trains[0].state.air_leak = true;
        frame.trains[0].vehicle_placement = vec![SoundPlacement {
            pan: -0.75,
            leak_source: Some(EmitterPlacement {
                pan: 0.8,
                distance_m: 0.,
                doppler: 1.,
                blocked: false,
            }),
            ..Default::default()
        }];
        playback.update(&frame);
        assert_eq!(
            f32::from_bits(playback.voices[0][0].pan.load(Ordering::Relaxed)),
            -0.75
        );
        assert_eq!(
            f32::from_bits(playback.voices[0][1].pan.load(Ordering::Relaxed)),
            0.8
        );
        assert!(playback.voices[0][1].player.volume() > 0.);
        frame.trains[0].state.air_leak = false;
        playback.update(&frame);
        assert_eq!(playback.voices[0][1].player.volume(), 0.);
        assert!(playback.voices[0][0].player.volume() > 0.);
    }
    #[test]
    fn external_sound_uses_the_listening_car_override_including_other_services() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Activation ( ExternalCam () PassengerCam () CabCam () Distance (100) ) Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let mut bank = test_bank(0, program);
        bank.train = 1;
        let (mut playback, _) = test_playback(vec![bank]);
        let mut frame = test_frame();
        frame.cab = true;
        frame.listener_vehicle = 2;
        frame.trains[0].id = 1;
        playback.update(&frame);
        assert_eq!(playback.voices[0][0].player.volume(), 0.5);
        playback.bank.external_pass_through.insert((1, 0), 0.9);
        playback.bank.external_pass_through.insert((0, 2), 0.2);
        playback.update(&frame);
        assert_eq!(playback.voices[0][0].player.volume(), 0.2);
        assert_eq!(playback.voices[0][0].player.len(), 1);
        for (value, expected) in [(25, Some(0.25)), (100, Some(1.0)), (-1, None)] {
            let ast = parse_named_stf(&format!(
                "Wagon ( unit ORTSExternalSoundPassedThroughPercent ( {value} ) )"
            ))
            .unwrap();
            assert_eq!(external_pass_through(&ast), expected);
        }
    }

    #[test]
    fn each_vehicle_controls_its_own_engine_curve_and_brake_trigger() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( VolumeCurve ( Variable2Controlled CurvePoints ( 2 0 0.2 1 1 ) ) Triggers ( 2 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) Variable_Trigger ( BrakeCyl_Inc_Past 15 SetStreamVolume (0.3) ) ) ) ) ) )").unwrap();
        let (mut playback, _) =
            test_playback(vec![test_bank(0, program.clone()), test_bank(1, program)]);
        let mut frame = test_frame();
        frame.trains[0].state.brake_cylinder = 30.0;
        frame.trains[0].vehicle_states = vec![
            SoundState {
                variable2: 0.2,
                ..Default::default()
            },
            SoundState {
                variable2: 0.8,
                brake_cylinder: 30.0,
                ..Default::default()
            },
        ];
        playback.update(&frame);
        assert_eq!(playback.voices[0][0].volume, 1.0);
        assert_eq!(playback.voices[1][0].volume, 0.3);
        assert!((playback.voices[0][0].player.volume() - 0.36).abs() < 1e-6);
        assert!((playback.voices[1][0].player.volume() - 0.252).abs() < 1e-6);
    }

    #[test]
    fn train_and_pipe_pressure_events_use_native_ids_cadence_and_stop_edges() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-audio.json")).unwrap();
        let ids: HashMap<_, _> = reference["brake_events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["native_event"].as_str().unwrap(),
                    r["id"].as_u64().unwrap() as u32,
                )
            })
            .collect();
        let mut brakes = BrakeSound::default();
        let released = SoundState {
            brake_pipe: 5.0,
            ..Default::default()
        };
        let applied = SoundState {
            brake_cylinder: 30.0,
            brake_pipe: 4.0,
            ..released
        };
        assert!(brakes.events(0.0, released).is_empty());
        assert!(brakes.events(0.2, applied).is_empty());
        assert_eq!(
            brakes.events(0.5, applied),
            vec![
                ids["TrainBrakePressureIncrease"],
                ids["BrakePipePressureDecrease"]
            ]
        );
        let ramping = SoundState {
            brake_cylinder: 35.0,
            brake_pipe: 3.9,
            ..applied
        };
        assert!(
            brakes.events(1.0, ramping).is_empty(),
            "continuing pressure change must not restart the loop"
        );
        assert_eq!(
            brakes.events(1.5, ramping),
            vec![
                ids["TrainBrakePressureStoppedChanging"],
                ids["BrakePipePressureStoppedChanging"]
            ]
        );
        assert_eq!(
            brakes.events(2.0, released),
            vec![
                ids["TrainBrakePressureDecrease"],
                ids["BrakePipePressureIncrease"]
            ]
        );
        assert_eq!(
            brakes.events(2.5, released),
            vec![
                ids["TrainBrakePressureStoppedChanging"],
                ids["BrakePipePressureStoppedChanging"]
            ]
        );
        let noise = SoundState {
            brake_cylinder: 0.05,
            brake_pipe: 4.999,
            ..released
        };
        assert!(
            brakes.events(3.0, noise).is_empty(),
            "sub-0.1 PSI changes are not new sounds"
        );
    }

    #[test]
    fn pressure_events_reach_every_sms_on_the_car_and_release_the_hiss() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( Triggers ( 2 Discrete_Trigger (14 StartLoopRelease ( 1 File (tone.wav -1) ) ) Discrete_Trigger (139 ReleaseLoopReleaseWithJump ()) ) ) ) ) )").unwrap();
        let (mut playback, _) =
            test_playback(vec![test_bank(0, program.clone()), test_bank(0, program)]);
        let mut frame = test_frame();
        frame.trains[0].state.brake_pipe = 5.0;
        playback.update(&frame);
        frame.time_s = 0.5;
        frame.trains[0].state.brake_cylinder = 30.0;
        playback.update(&frame);
        for voices in &playback.voices {
            assert_eq!(voices[0].choices[0], 1);
            assert_eq!(
                voices[0].release.as_ref().unwrap().load(Ordering::Relaxed),
                0
            );
        }
        frame.time_s = 1.0;
        playback.update(&frame);
        for voices in &playback.voices {
            assert_eq!(voices[0].choices[0], 1);
            assert_eq!(
                voices[0].release.as_ref().unwrap().load(Ordering::Relaxed),
                2
            );
        }
        frame.time_s = 0.0;
        playback.update(&frame);
        assert!(playback.voices.iter().all(|v| v[0].choices[0] == 0));
    }
    #[test]
    fn openrails_vehicle_sound_override_keeps_the_stock_sound_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Stock");
        std::fs::create_dir_all(root.join("OPENRAILS")).unwrap();
        let base = root.join("motor.eng");
        let native = root.join("OPENRAILS/MOTOR.eng");
        std::fs::write(&base, b"base").unwrap();
        std::fs::write(&native, b"native").unwrap();
        assert_eq!(
            sound_vehicle_path(&root, std::ffi::OsStr::new("motor.eng")),
            native
        );
        std::fs::remove_file(&native).unwrap();
        assert_eq!(
            sound_vehicle_path(&root, std::ffi::OsStr::new("MOTOR.eng")),
            base
        );
    }
    #[test]
    fn declared_sms_cameras_override_the_eng_sound_location() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 1 Activation ( ExternalCam () PassengerCam () CabCam () Distance (750) ) Streams (0) ) )").unwrap();
        let mut bank = Bank {
            train: 0,
            vehicle: 0,
            location: SoundLocation::Cab,
            program,
            program_name: "test.sms".into(),
            curve_only: false,
            samples: HashMap::new(),
        };
        assert!(bank.audible(SoundLocation::Exterior, 0, true));
        assert!(bank.audible(SoundLocation::Passenger, 0, true));
        assert!(bank.audible(SoundLocation::Cab, 0, true));
        bank.program = SmsProgram::parse(
            "Tr_SMS ( ScalabiltyGroup ( 1 Activation ( CabCam () ExternalCam (0) ) Streams (0) ) )",
        )
        .unwrap();
        assert!(bank.audible(SoundLocation::Cab, 0, true));
        assert!(!bank.audible(SoundLocation::Cab, 1, true));
        assert!(!bank.audible(SoundLocation::Exterior, 0, true));
        bank.train = 1;
        assert!(!bank.audible(SoundLocation::Cab, 0, true));
    }
    #[test]
    fn formation_mix_preserves_quiet_samples_and_limits_sudden_peaks() {
        let quiet = [0.1_f32, -0.2].repeat(100);
        let input = quiet.iter().copied().chain([2.5, -3.0, 7.0, -7.0]);
        let source = SamplesBuffer::new(
            std::num::NonZeroU16::new(2).unwrap(),
            std::num::NonZeroU32::new(44100).unwrap(),
            input.collect::<Vec<_>>(),
        );
        let limited: Vec<_> = source.limit(mix_limit()).collect();
        assert_eq!(&limited[..quiet.len()], &quiet);
        assert!(limited.iter().all(|v| v.is_finite() && v.abs() <= 0.9));
    }
    #[test]
    fn shared_sound_resolves_without_a_local_sound_folder() {
        let dir = tempfile::tempdir().unwrap();
        let stock = dir.path().join("Class121");
        let shared = dir.path().join("KIHA31/SOUND");
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        let sms = shared.join("k31cab.sms");
        std::fs::write(&sms, b"").unwrap();
        assert_eq!(
            resolve_sample(&[stock.join("Sound")], r"..\..\Kiha31\Sound\K31Cab.sms"),
            Some(sms)
        );
    }
    fn test_wave() -> Wave {
        Wave {
            samples: SamplesBuffer::new(
                std::num::NonZeroU16::new(1).unwrap(),
                std::num::NonZeroU32::new(8).unwrap(),
                vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7],
            ),
            rate: 8.0,
            loop_range_s: Some((0.25, 0.625)),
        }
    }
    #[test]
    fn authored_loop_preserves_intro_and_releases_into_the_tail() {
        let wave = test_wave();
        let release = Arc::new(AtomicU8::new(0));
        let mut source = LoopSource {
            source: wave.samples.clone(),
            wave,
            elapsed_samples: 0,
            release: release.clone(),
            looping: true,
        };
        assert_eq!(
            source.by_ref().take(8).collect::<Vec<_>>(),
            vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.2, 0.3, 0.4]
        );
        release.store(1, Ordering::Relaxed);
        assert_eq!(source.collect::<Vec<_>>(), vec![0.5, 0.6, 0.7]);
        let wave = test_wave();
        let mut jump = LoopSource {
            source: wave.samples.clone(),
            wave,
            elapsed_samples: 0,
            release: Arc::new(AtomicU8::new(2)),
            looping: true,
        };
        assert_eq!(jump.by_ref().collect::<Vec<_>>(), vec![0.5, 0.6, 0.7]);
    }

    #[test]
    fn cab_passenger_and_exterior_switch_without_restarting_loops() {
        let program = SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let banks = [
            (SoundLocation::Exterior, 0.1),
            (SoundLocation::Cab, 0.25),
            (SoundLocation::Passenger, 0.4),
        ]
        .into_iter()
        .map(|(location, amplitude)| {
            let samples = SamplesBuffer::new(
                std::num::NonZeroU16::new(1).unwrap(),
                std::num::NonZeroU32::new(44100).unwrap(),
                vec![amplitude; 4410],
            );
            Bank {
                train: 0,
                vehicle: 0,
                location,
                program: program.clone(),
                program_name: "test.sms".into(),
                curve_only: false,
                samples: HashMap::from([(
                    "tone.wav".into(),
                    Wave {
                        samples,
                        rate: 44100.0,
                        loop_range_s: None,
                    },
                )]),
            }
        })
        .collect();
        let (mixer, mut source) = rodio::mixer::mixer(
            std::num::NonZeroU16::new(2).unwrap(),
            std::num::NonZeroU32::new(44100).unwrap(),
        );
        let mut playback = Playback::new(
            NativeSoundBank {
                banks,
                external_pass_through: HashMap::new(),
                report: SoundReport::default(),
            },
            &mixer,
        );
        let mut frame = SoundFrame {
            volume: 1.0,
            trains: vec![TrainSoundFrame {
                id: 0,
                state: SoundState::default(),
                distance_m: 0.0,
                vehicle_distances_m: vec![],
                vehicle_states: vec![],
                vehicle_placement: vec![],
            }],
            ..Default::default()
        };
        for (cab, passenger, volume, expected) in [
            (false, false, 1.0, 0.1),
            (true, false, 1.0, 0.25),
            (false, true, 1.0, 0.4),
            (true, false, 0.0, 0.0),
        ] {
            frame.cab = cab;
            frame.passenger = passenger;
            frame.volume = volume;
            playback.update(&frame);
            let signal = source.by_ref().take(8820).last().unwrap();
            assert!(
                (signal - expected).abs() < 0.001,
                "{cab} {passenger}: {signal} vs {expected}"
            );
        }
        assert!(playback.voices.iter().all(|v| v[0].player.len() == 1));
    }

    #[test]
    fn steam_chuff_events_follow_wheel_phase_including_large_steps() {
        let before = SoundState {
            steam_phase: Some(0.0),
            throttle: 1.0,
            ..Default::default()
        };
        let now = SoundState {
            steam_phase: Some(3.2),
            ..before
        };
        assert_eq!(now.events(before), vec![121, 122, 123]);
        let reverse = SoundState {
            steam_phase: Some(-3.2),
            ..before
        };
        let events = reverse.events(before);
        assert_eq!(events.len(), 4);
        assert!(events.iter().all(|e| (121..=136).contains(e)));
        let coast = SoundState {
            throttle: 0.0,
            ..now
        };
        assert!(!coast.events(before).iter().any(|e| (121..=136).contains(e)));
    }
    #[test]
    fn event_edges_do_not_repeat_while_horn_is_held() {
        let old = SoundState::default();
        let now = SoundState {
            horn: true,
            brake_pipe: 4.0,
            ..old
        };
        assert!(now.events(old).contains(&8));
        assert!(!now.events(now).contains(&8));
        assert!(old.events(now).contains(&9));
    }

    #[test]
    fn sand_delivery_emits_original_sms_edges_without_repeating() {
        let off = SoundState::default();
        let on = SoundState {
            sander: true,
            ..off
        };
        assert_eq!(on.events(off), vec![4]);
        assert!(on.events(on).is_empty());
        assert_eq!(off.events(on), vec![5]);
    }

    #[test]
    fn traction_control_edges_match_original_dll_sound_events() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../oracles/openrails-traction-operation.json"
        ))
        .unwrap();
        let old = SoundState {
            engine_on: Some(false),
            ..Default::default()
        };
        let on = SoundState {
            engine_on: Some(true),
            injector1: true,
            injector2: true,
            blower: true,
            damper: 0.5,
            cylinder_cocks: true,
            ..old
        };
        let mut emitted = on.events(old);
        emitted.extend(old.events(on));
        emitted.sort_unstable();
        emitted.dedup();
        let mut native: Vec<_> = reference["sound_events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_u64().unwrap() as u32)
            .collect();
        native.sort_unstable();
        assert_eq!(emitted, native);
        assert!(
            on.events(on).is_empty(),
            "held switches must not restart native loops"
        );
    }

    #[test]
    fn plan30_sms_mix_camera_fades_preserve_loop_and_trigger_state() {
        let program=SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Activation ( CabCam () ) Streams ( 1 Stream ( VolumeCurve ( Variable2Controlled CurvePoints ( 2 0 0.2 1 1 ) ) Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let mut bank = test_bank(0, program);
        bank.location = SoundLocation::Cab;
        let (mut playback, mut source) = test_playback(vec![bank]);
        let mut frame = test_frame();
        frame.cab = true;
        frame.presentation_dt_s = 1. / 60.;
        playback.update(&frame);
        let release = playback.voices[0][0].release.as_ref().unwrap().clone();
        for tick in 1..=60 {
            frame.time_s = tick as f64 / 60.;
            frame.trains[0].state.variable2 = tick as f32 / 60.;
            playback.update(&frame);
            for _ in 0..100 {
                source.next();
            }
        }
        assert_eq!(playback.report().active_voices, 1);
        assert!(Arc::ptr_eq(
            &release,
            playback.voices[0][0].release.as_ref().unwrap()
        ));
        assert_eq!(
            playback.report().voices[0].group,
            crate::mix::SoundGroup::Motor
        );
        frame.cab = false;
        for tick in 61..=120 {
            frame.time_s = tick as f64 / 60.;
            playback.update(&frame);
        }
        assert!(playback.report().voices[0].gain < 0.01);
        frame.cab = true;
        frame.mix.gains[crate::mix::SoundGroup::Motor.index()] = 0.;
        playback.update(&frame);
        assert_eq!(playback.report().voices[0].gain, 0.);
        frame.mix = Default::default();
        frame.paused = true;
        playback.update(&frame);
        assert!(playback.report().voices[0].paused);
        assert!(Arc::ptr_eq(
            &release,
            playback.voices[0][0].release.as_ref().unwrap()
        ));
    }
    #[test]
    fn plan30_coupler_contacts_generate_one_native_event_per_physical_change() {
        let old = SoundState {
            coupling_event: 58,
            ..Default::default()
        };
        let impact = SoundState {
            coupler_contact: 1,
            coupler_force_n: 12000.,
            coupler_relative_speed: 0.2,
            ..old
        };
        assert_eq!(impact.events(old), vec![58]);
        assert!(impact.events(impact).is_empty());
        assert!(
            SoundState {
                coupler_relative_speed: 0.,
                ..impact
            }
            .events(old)
            .is_empty()
        );
        assert_eq!(
            SoundState {
                coupling_event: 61,
                ..old
            }
            .events(old),
            vec![61]
        );
        assert_eq!(
            old.events(SoundState {
                coupling_event: 61,
                ..old
            }),
            vec![58]
        );
        assert!(old.events(SoundState::default()).is_empty());
    }
    #[test]
    fn plan30_optional_curve_programs_do_not_mute_normal_sounds_or_play_on_straight_track() {
        let program=SmsProgram::parse("Tr_SMS ( ScalabiltyGroup ( 5 Streams ( 1 Stream ( Triggers ( 1 Initial_Trigger ( StartLoop ( 1 File ( tone.wav -1 ) ) ) ) ) ) ) )").unwrap();
        let normal = test_bank(0, program.clone());
        let mut curve = test_bank(0, program);
        curve.curve_only = true;
        curve.location = SoundLocation::Passenger;
        let (mut playback, _source) = test_playback(vec![normal, curve]);
        let mut frame = test_frame();
        frame.passenger = true;
        playback.update(&frame);
        assert!(playback.voices[0][0].gain > 0.);
        assert_eq!(playback.voices[1][0].gain, 0.);
        frame.trains[0].state.curve_squeal = true;
        frame.time_s = 1.;
        playback.update(&frame);
        assert!(playback.voices[0][0].gain > 0.);
        assert!(playback.voices[1][0].gain > 0.);
        assert_eq!(playback.voices[1][0].group, crate::mix::SoundGroup::Rolling);
        frame.trains[0].state.curve_squeal = false;
        frame.time_s = 2.;
        playback.update(&frame);
        assert!(playback.voices[0][0].gain > 0.);
        assert_eq!(playback.voices[1][0].gain, 0.);
    }
}
