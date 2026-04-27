/// Synthesized audio engine for cab mode.
///
/// Produces three sounds entirely from generated sine waves — no external
/// audio files required.  The engine runs in a dedicated OS thread and
/// receives commands via an `mpsc` channel.  If no audio output device is
/// available (CI, headless servers) `try_start()` returns `None` and the
/// cab continues silently.
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use rodio::{OutputStreamBuilder, Sink, Source, source::SineWave};

/// Commands sent to the audio thread.
pub enum AudioCmd {
    /// Update the engine sound to reflect current speed (m/s).
    SetVelocity(f64),
    /// Update brake squeal intensity (0.0 = silent, 1.0 = full).
    SetBraking(f64),
    /// Play a 500 ms horn one-shot.
    Horn,
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
        let stream_handle = OutputStreamBuilder::open_default_stream().ok()?;

        thread::spawn(move || {
            // Keep the stream alive for the duration of the thread.
            let _stream = stream_handle;

            let engine_sink = Sink::connect_new(_stream.mixer());
            let brake_sink = Sink::connect_new(_stream.mixer());

            // Seed engine with a quiet idle tone.
            engine_sink.append(SineWave::new(60.0).amplify(0.15).repeat_infinite());
            engine_sink.set_volume(0.15);
            engine_sink.play();
            brake_sink.set_volume(0.0);
            brake_sink.append(SineWave::new(800.0).amplify(1.0).repeat_infinite());
            brake_sink.play();

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
                        let horn_sink = Sink::connect_new(_stream.mixer());
                        horn_sink.append(
                            SineWave::new(440.0)
                                .amplify(0.6)
                                .take_duration(Duration::from_millis(500)),
                        );
                        horn_sink.detach();
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
