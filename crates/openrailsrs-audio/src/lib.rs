//! Synthesized audio engine for openrailsrs.
//!
//! Produces cab sounds (motor / brake / horn) and ambient sound regions
//! entirely from generated sine waves — no external audio files required.
//! The engine runs in a dedicated OS thread and receives commands via an
//! `mpsc` channel.  If no audio output device is available (CI, headless
//! servers) [`AudioEngine::try_start`] returns `None` and callers continue
//! silently.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use rodio::{DeviceSinkBuilder, Player, Source, source::SineWave};

/// Commands sent to the audio thread.
pub enum AudioCmd {
    /// Update the engine sound to reflect current speed (m/s).
    SetVelocity(f64),
    /// Update brake squeal intensity (0.0 = silent, 1.0 = full).
    SetBraking(f64),
    /// Play a 500 ms horn one-shot.
    Horn,
    /// Start playing an ambient loop for a sound region.
    ///
    /// `id` uniquely identifies the region (further `EnterRegion` for the same
    /// id is a no-op until a `LeaveRegion` happens).
    EnterRegion {
        id: String,
        kind: String,
        base_volume: f32,
    },
    /// Stop the ambient loop for a previously entered region.
    LeaveRegion { id: String },
    /// Shut down the audio thread cleanly.
    Stop,
}

/// Handle to the background audio thread.
pub struct AudioEngine {
    tx: Sender<AudioCmd>,
}

impl AudioEngine {
    /// Try to initialise an audio output stream and start the audio thread.
    ///
    /// Returns `None` when no audio device is available (CI-safe).
    pub fn try_start() -> Option<Self> {
        let (tx, rx) = mpsc::channel::<AudioCmd>();

        // Try opening the default audio device on this thread first;
        // bail without launching the thread if unavailable.
        let stream_handle = DeviceSinkBuilder::open_default_sink().ok()?;

        thread::spawn(move || {
            // Keep the stream alive for the duration of the thread.
            let _stream = stream_handle;

            let engine_sink = Player::connect_new(_stream.mixer());
            let brake_sink = Player::connect_new(_stream.mixer());

            // Seed engine with a quiet idle tone.
            engine_sink.append(SineWave::new(60.0).amplify(0.15).repeat_infinite());
            engine_sink.set_volume(0.15);
            engine_sink.play();
            brake_sink.set_volume(0.0);
            brake_sink.append(SineWave::new(800.0).amplify(1.0).repeat_infinite());
            brake_sink.play();

            // Active ambient sinks indexed by region id.
            let mut region_sinks: HashMap<String, Player> = HashMap::new();

            for cmd in rx {
                match cmd {
                    AudioCmd::SetVelocity(v_mps) => {
                        // Volume scales with speed: quiet idle, louder at speed.
                        let vol = (0.1 + (v_mps / 40.0).clamp(0.0, 0.9)) as f32;
                        engine_sink.set_volume(vol);
                    }
                    AudioCmd::SetBraking(brake) => {
                        let vol = (brake.clamp(0.0, 1.0) * 0.4) as f32;
                        brake_sink.set_volume(vol);
                    }
                    AudioCmd::Horn => {
                        let horn_sink = Player::connect_new(_stream.mixer());
                        horn_sink.append(
                            SineWave::new(440.0)
                                .amplify(0.6)
                                .take_duration(Duration::from_millis(500)),
                        );
                        horn_sink.detach();
                    }
                    AudioCmd::EnterRegion {
                        id,
                        kind,
                        base_volume,
                    } => {
                        if region_sinks.contains_key(&id) {
                            continue;
                        }
                        let sink = Player::connect_new(_stream.mixer());
                        let freq = frequency_for_kind(&kind);
                        sink.append(SineWave::new(freq).amplify(0.3).repeat_infinite());
                        sink.set_volume(base_volume.clamp(0.0, 1.0));
                        sink.play();
                        region_sinks.insert(id, sink);
                    }
                    AudioCmd::LeaveRegion { id } => {
                        if let Some(sink) = region_sinks.remove(&id) {
                            sink.stop();
                        }
                    }
                    AudioCmd::Stop => break,
                }
            }
        });

        Some(AudioEngine { tx })
    }

    /// Send a command to the audio thread (fire-and-forget; ignores send errors).
    pub fn send(&self, cmd: AudioCmd) {
        let _ = self.tx.send(cmd);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCmd::Stop);
    }
}

/// Map an ambient region `kind` string to a base sine-wave frequency.
///
/// Pure synthesis stand-in for real `.sms` samples — keeps the engine
/// dependency-free while still differentiating common MSTS region types.
fn frequency_for_kind(kind: &str) -> f32 {
    match kind.to_ascii_lowercase().as_str() {
        "tunnel" => 90.0,
        "depot" | "yard" => 150.0,
        "forest" | "rural" => 250.0,
        "urban" | "city" => 320.0,
        _ => 200.0,
    }
}
