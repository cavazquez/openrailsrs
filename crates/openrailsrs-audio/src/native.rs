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
        atomic::{AtomicU8, Ordering},
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
    pub horn: bool,
    pub wiper: bool,
    pub doors: bool,
    pub headlights: u8,
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
            (self.doors, old.doors, 105, 106),
        ] {
            if now != before {
                e.push(if now { on } else { off });
            }
        }
        for (now, before, event) in [
            (self.throttle, old.throttle, 16),
            (self.brake, old.brake, 17),
            (self.direction, old.direction, 15),
        ] {
            if (now - before).abs() > 0.005 {
                e.push(event);
            }
        }
        if self.headlights != old.headlights {
            e.push(37);
        }
        if let (Some(now), Some(before)) = (self.steam_phase, old.steam_phase)
            && self.throttle > 0.0
            && now >= before
        {
            let start = (before.floor() as u64).saturating_add(1);
            let end = now.floor() as u64;
            for phase in start.max(end.saturating_sub(63))..=end {
                e.push(121 + ((phase - 1) % 16) as u32);
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
                Err(e) => {
                    result
                        .report
                        .warnings
                        .push(format!("{}: {e}", spec.consist.display()));
                    continue;
                }
            };
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
                let root = authored
                    .file_name()
                    .and_then(|name| {
                        resolve_path_case_insensitive(&content.join("TRAINS/TRAINSET").join(name))
                    })
                    .unwrap_or_else(|| authored.into());
                let stock = sound_vehicle_path(&root, path.file_name().unwrap_or_default());
                let ast =
                    match read_msts_text_with_includes(&stock).and_then(|t| parse_named_stf(&t)) {
                        Ok(a) => a,
                        Err(e) => {
                            result.report.warnings.push(e.to_string());
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
                for (name, location) in refs {
                    if location != SoundLocation::Exterior
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
                        result
                            .report
                            .warnings
                            .push(format!("Missing SMS: {name} ({})", stock.display()));
                        continue;
                    };
                    let program = match SmsProgram::from_path(&path) {
                        Ok(p) => p,
                        Err(e) => {
                            result
                                .report
                                .warnings
                                .push(format!("{}: {e}", path.display()));
                            continue;
                        }
                    };
                    result.report.warnings.extend(
                        program
                            .warnings
                            .iter()
                            .map(|w| format!("{}: {w}", path.display())),
                    );
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
                                            "Missing WAV: {name} ({})",
                                            path.display()
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
                                                result
                                                    .report
                                                    .warnings
                                                    .push(format!("{}: {e}", file.display()));
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
}
struct Playback {
    bank: NativeSoundBank,
    voices: Vec<Vec<StreamRuntime>>,
    previous: HashMap<(usize, usize), (SoundState, f32)>,
    brake_sounds: HashMap<(usize, usize), BrakeSound>,
    active_sources: Vec<bool>,
    initial: bool,
    last_time_s: f64,
    mixer: rodio::mixer::Mixer,
}
impl Playback {
    fn new(bank: NativeSoundBank, mixer: &rodio::mixer::Mixer) -> Self {
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
            initial: true,
            last_time_s: 0.0,
            mixer: mixer.clone(),
        }
    }
    fn update(&mut self, frame: &SoundFrame) {
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
            self.active_sources.fill(false);
            self.initial = true;
        }
        self.last_time_s = frame.time_s;
        let interior_gain = self
            .bank
            .external_pass_through
            .get(&(0, frame.listener_vehicle))
            .copied()
            // Open Rails UserSettings.ExternalSoundPassThruPercent defaults to 50.
            .unwrap_or(0.5);
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
                    let mut events = state.events(previous);
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
                b.train == 0 && b.vehicle == frame.listener_vehicle && b.location == listener
            });
            let audible = *active && bank.audible(listener, frame.listener_vehicle, has_interior);
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
                                voice.player.stop();
                                voice.player = Player::connect_new(&self.mixer);
                                let release = Arc::new(AtomicU8::new(0));
                                voice.player.append(LoopSource {
                                    source: wave.samples.clone(),
                                    wave: wave.clone(),
                                    elapsed_samples: 0,
                                    release: release.clone(),
                                    looping: *looping,
                                });
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
                let gain = bank.program.volume
                    * stream.volume
                    * voice.volume
                    * curves
                    * distance_gain
                    * frame.volume;
                voice
                    .player
                    .set_volume(if audible { gain.clamp(0.0, 1.0) } else { 0.0 });
                let pitch = stream.frequency.as_ref().map_or(1.0, |c| {
                    c.value(state.control(c.control, source_distance)) / voice.rate
                });
                voice
                    .player
                    .set_speed((pitch * voice.stereo_speed).clamp(0.1, 4.0));
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
    }
    fn report(&self) -> SoundReport {
        let mut report = self.bank.report.clone();
        report.active_voices = self
            .voices
            .iter()
            .flatten()
            .filter(|v| !v.player.empty())
            .count();
        report
    }
}

pub struct NativeAudioEngine {
    tx: mpsc::SyncSender<SoundFrame>,
    thunder_tx: mpsc::SyncSender<crate::thunder::ThunderEvent>,
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
        if std::env::var_os("OPENRAILSRS_DISABLE_AUDIO").is_some() {
            return None;
        }
        let (tx, rx) = mpsc::sync_channel(2);
        let (thunder_tx, thunder_rx) = mpsc::sync_channel::<crate::thunder::ThunderEvent>(2);
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
            output.mixer().add(source.limit(mix_limit()));
            let mut playback = Playback::new(bank, &mixer);
            let mut thunder_voices: Vec<(Player, f32)> = vec![];
            for frame in rx {
                playback.update(&frame);
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
                    player.set_volume(frame.volume * crate::thunder::gain(*distance, frame.cab));
                    if frame.paused {
                        player.pause();
                    } else {
                        player.play();
                    }
                }
                let mut r = playback.report();
                r.device = true;
                *shared.lock().unwrap() = r;
            }
        });
        Some(Self {
            tx,
            thunder_tx,
            report,
        })
    }
    pub fn send(&self, frame: SoundFrame) {
        let _ = self.tx.try_send(frame);
    }
    pub fn thunder(&self, event: crate::thunder::ThunderEvent) {
        let _ = self.thunder_tx.try_send(event);
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
    let mut source = source.limit(mix_limit());
    let mut playback = Playback::new(NativeSoundBank::load(specs), &mixer);
    let mut pcm = vec![];
    for pair in frames.windows(2) {
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
            }],
            ..Default::default()
        }
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
}
