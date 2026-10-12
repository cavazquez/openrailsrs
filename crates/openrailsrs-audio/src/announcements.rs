//! Decoded licensed announcements, bounded independently of the SMS bank.
use rodio::{Decoder, Player, Source, buffer::SamplesBuffer};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
};
#[derive(Clone, Debug)]
pub struct LicensedSample {
    pub id: String,
    pub path: PathBuf,
    pub scope: PathBuf,
}
pub struct AnnouncementPlayer {
    waves: HashMap<String, SamplesBuffer>,
    player: Player,
    played: HashSet<String>,
    queue: VecDeque<String>,
    last_clock: f64,
    pub decoded_mib: f32,
    pub warnings: Vec<String>,
}
impl AnnouncementPlayer {
    pub fn new(specs: Vec<LicensedSample>, mixer: &rodio::mixer::Mixer) -> Self {
        let mut this = Self {
            waves: HashMap::new(),
            player: Player::connect_new(mixer),
            played: HashSet::new(),
            queue: VecDeque::new(),
            last_clock: 0.,
            decoded_mib: 0.,
            warnings: vec![],
        };
        for spec in specs.into_iter().take(256) {
            let load = (|| -> Result<SamplesBuffer, String> {
                let root = spec.scope.canonicalize().map_err(|e| e.to_string())?;
                let path = spec.path.canonicalize().map_err(|e| e.to_string())?;
                if !path.starts_with(&root)
                    || std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024
                {
                    return Err("Recurso fuera del paquete o demasiado grande".into());
                }
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let decode =
                    Decoder::try_from(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
                let channels = decode.channels();
                let rate = decode.sample_rate();
                if channels.get() > 2 || rate.get() > 48000 {
                    return Err(
                        "Anuncio fuera del formato admitido (mono/estéreo, hasta 48 kHz)".into(),
                    );
                }
                let max = (rate.get() as usize * channels.get() as usize * 60).min(8_000_000);
                let data: Vec<_> = decode.take(max + 1).collect();
                let mib = data.len() as f32 * 4. / 1048576.;
                if data.len() > max || this.decoded_mib + mib > 32. {
                    return Err("Presupuesto de anuncios superado (60 s, banco de 32 MiB)".into());
                }
                this.decoded_mib += mib;
                Ok(SamplesBuffer::new(channels, rate, data))
            })();
            match load {
                Ok(wave) => {
                    this.waves.insert(spec.id, wave);
                }
                Err(e) => this.warnings.push(format!("Anuncio {}: {e}", spec.id)),
            }
        }
        this
    }
    pub fn update(&mut self, clock: f64, paused: bool, gain: f32, events: &[(String, String)]) {
        if clock < self.last_clock {
            self.queue.clear();
            self.player.clear();
            self.played.clear();
        }
        self.last_clock = clock;
        if paused {
            self.player.pause();
            return;
        }
        self.player.play();
        for (key, id) in events {
            if self.waves.contains_key(id) && !self.played.contains(key) && self.queue.len() < 8 {
                self.played.insert(key.clone());
                self.queue.push_back(id.clone());
            }
        }
        if self.player.empty()
            && let Some(id) = self.queue.pop_front()
            && let Some(wave) = self.waves.get(&id)
        {
            self.player.append(wave.clone());
        }
        self.player.set_volume(gain.clamp(0., 1.) * 0.45);
    }
}
