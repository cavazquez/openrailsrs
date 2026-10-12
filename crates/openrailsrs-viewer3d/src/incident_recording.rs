//! Local incident recordings; verification runs in a separate simulation worker.
use bevy::prelude::*;
use openrailsrs_sim::replay::{
    IncidentReplay, ReplayFrame, ReplayHeader, ReplayInput, ReplayOutput, ReplayReport,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
};
#[derive(Serialize, Deserialize)]
struct RecordingFile {
    schema: u32,
    scenario_toml: String,
    replay: IncidentReplay,
}
#[derive(Resource, Default)]
pub struct IncidentRecording {
    recording: Option<RecordingFile>,
    pending_frame: Option<(f64, f64, Option<f64>, ReplayInput)>,
    pub latest: Option<PathBuf>,
    pub status: String,
    worker: Option<Mutex<mpsc::Receiver<Result<WorkerReply, String>>>>,
    bytes: usize,
}
enum WorkerReply {
    Saved(PathBuf),
    Verified(ReplayReport, PathBuf),
}
fn resources(
    scenario: &openrailsrs_scenarios::ScenarioFile,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let root = Path::new(&scenario.route.path);
    let track = if root.is_file() {
        root.to_owned()
    } else {
        root.join("track.toml")
    };
    result.insert(
        "track.toml".into(),
        openrailsrs_train::consist_audit::content_file_sha256(&track).map_err(|e| e.to_string())?,
    );
    let files = std::iter::once(&scenario.train.consist)
        .chain(scenario.extra_trains.iter().map(|t| &t.consist));
    for (i, con) in files.enumerate() {
        let con = Path::new(con);
        result.insert(
            format!("formation-{i}"),
            openrailsrs_train::consist_audit::content_file_sha256(con)
                .map_err(|e| e.to_string())?,
        );
        let ast = openrailsrs_formats::parse_vehicle_text(
            &openrailsrs_formats::read_msts_file_to_string(con).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let consist =
            openrailsrs_formats::ConsistFile::from_ast(&ast).map_err(|e| e.to_string())?;
        if consist.entries.len() > 64 {
            return Err("Demasiados vehículos para grabar".into());
        }
        for (j, e) in consist.entries.iter().enumerate() {
            let p = openrailsrs_train::resolve_consist_entry_path(
                openrailsrs_train::consist_asset_root(con),
                e.path(),
            );
            result.insert(
                format!("formation-{i}-vehicle-{j}"),
                openrailsrs_train::consist_audit::source_fingerprint(&p)
                    .map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(result)
}
impl IncidentRecording {
    pub fn active(&self) -> bool {
        self.recording.is_some()
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn start(&mut self, live: &crate::live::LiveDrive) -> Result<(), String> {
        if self.active() || self.busy() {
            return Err("Ya hay una grabación u operación en curso".into());
        }
        let scenario_toml = crate::saved_game::canonical_scenario(live.scenario_path())?;
        let scenario: openrailsrs_scenarios::ScenarioFile =
            toml::from_str(&scenario_toml).map_err(|e| e.to_string())?;
        let header=ReplayHeader{schema:1,engine_contract:"native-rust-replay-v1".into(),application_version:env!("CARGO_PKG_VERSION").into(),open_rails_reference:"1.6.1".into(),
            binary_sha256:std::env::current_exe().ok().and_then(|p|openrailsrs_train::consist_audit::content_file_sha256(&p).ok()).unwrap_or_default(),resources:resources(&scenario)?,seed:scenario.simulation.seed,
            scope:"Física, controles, equipo, puertas, clima/adherencia, señales, tráfico y decisiones Rust. No audio/GPU ni abastecimiento activo o host C#. Archivo local: contiene referencias a tu contenido; no se adjunta a issues automáticamente.".into()};
        let replay = IncidentReplay::start(header, &live.session, &live.traffic)?;
        let recording = RecordingFile {
            schema: 1,
            scenario_toml,
            replay,
        };
        self.bytes = serde_json::to_vec(&recording)
            .map_err(|e| e.to_string())?
            .len();
        if self.bytes > 16 * 1024 * 1024 {
            return Err("El estado inicial excede 16 MiB".into());
        }
        self.recording = Some(recording);
        self.status =
            "Grabando comandos y resultados; máximo 12000 pasos o 64 MiB. F7 detiene y guarda."
                .into();
        Ok(())
    }
    pub fn before(&mut self, live: &crate::live::LiveDrive, dt: f64, automatic: Option<f64>) {
        if self.recording.is_none() {
            return;
        }
        match ReplayInput::capture(&live.session) {
            Ok(input) => self.pending_frame = Some((live.session.time_s(), dt, automatic, input)),
            Err(e) => {
                self.status = e;
                let _ = self.stop();
            }
        }
    }
    pub fn after(&mut self, live: &crate::live::LiveDrive) {
        let Some((clock_s, dt, automatic, input)) = self.pending_frame.take() else {
            return;
        };
        let Some(file) = self.recording.as_mut() else {
            return;
        };
        match ReplayOutput::capture(&live.session, &live.traffic) {
            Ok(expected) => {
                let frame = ReplayFrame {
                    clock_s,
                    dt,
                    automatic,
                    input,
                    expected,
                };
                let bytes = serde_json::to_vec(&frame).map_or(64 * 1024 * 1024, |v| v.len());
                if self.bytes + bytes + 1 > 60 * 1024 * 1024 {
                    let _ = self.stop();
                    return;
                }
                self.bytes += bytes + 1;
                file.replay.frames.push(frame);
            }
            Err(e) => {
                self.status = e;
                let _ = self.stop();
                return;
            }
        }
        if file.replay.frames.len() >= 12000 {
            let _ = self.stop();
        }
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err("Esperá la verificación anterior".into());
        }
        let file = self.recording.take().ok_or("No hay una grabación activa")?;
        self.pending_frame = None;
        if file.replay.frames.is_empty() {
            return Err(
                "La grabación no contiene pasos: continuá la partida antes de detenerla".into(),
            );
        }
        let dir = crate::player_settings::player_data_dir().join("incident-recordings");
        let path = dir.join(format!(
            "incident-{}.json",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let (tx, rx) = mpsc::channel();
        self.worker = Some(Mutex::new(rx));
        self.status = "Guardando grabación en segundo plano…".into();
        std::thread::spawn(move || {
            let result = (|| {
                let bytes = serde_json::to_vec(&file).map_err(|e| e.to_string())?;
                if bytes.len() > 64 * 1024 * 1024 {
                    return Err("Grabación supera 64 MiB".into());
                }
                crate::player_settings::atomic_write(&path, &bytes)?;
                Ok(WorkerReply::Saved(path))
            })();
            let _ = tx.send(result);
        });
        Ok(())
    }
    pub fn verify(&mut self) -> Result<(), String> {
        if self.active() || self.busy() {
            return Err("Detené y guardá la grabación antes de reproducirla".into());
        }
        let path = self
            .latest
            .clone()
            .or_else(|| {
                latest_recording(
                    &crate::player_settings::player_data_dir().join("incident-recordings"),
                )
            })
            .ok_or("No hay un registro local para reproducir")?;
        let (tx, rx) = mpsc::channel();
        self.worker = Some(Mutex::new(rx));
        self.status =
            "Recalculando la sesión en segundo plano; tu partida permanece en pausa…".into();
        std::thread::spawn(move || {
            let result = verify_file(&path).map(|r| WorkerReply::Verified(r, path));
            let _ = tx.send(result);
        });
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.worker.as_ref() else {
            return false;
        };
        let Ok(rx) = rx.try_lock() else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(_) => Err("La verificación terminó sin respuesta".into()),
        };
        drop(rx);
        self.worker = None;
        match result {
            Ok(WorkerReply::Saved(path)) => {
                self.status = format!(
                    "Grabación guardada: {}. Reproducir recalcula la física sin mover tu partida.",
                    path.display()
                );
                self.latest = Some(path);
            }
            Ok(WorkerReply::Verified(r, path)) => {
                self.status = if r.matched {
                    format!(
                        "Coincidencia: {} pasos hasta {:.3} s. Informe: {}",
                        r.frames,
                        r.clock_s,
                        path.with_extension("report.json").display()
                    )
                } else {
                    r.first_difference
                        .unwrap_or_else(|| "Reproducción diferente".into())
                };
            }
            Err(e) => self.status = e,
        }
        true
    }
}
fn verify_file(path: &Path) -> Result<ReplayReport, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
        return Err("Registro demasiado grande".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Registro demasiado grande".into());
    }
    let file: RecordingFile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if file.schema != 1 {
        return Err("Formato de grabación incompatible".into());
    }
    let scenario: openrailsrs_scenarios::ScenarioFile =
        toml::from_str(&file.scenario_toml).map_err(|e| e.to_string())?;
    let resources = resources(&scenario)?;
    if resources != file.replay.header.resources {
        return Err("El contenido cambió desde la grabación; revisá las versiones y hashes".into());
    }
    let mut live = crate::live::LiveDrive::from_scenario(Path::new("."), &scenario)?;
    let report = file
        .replay
        .replay(&mut live.session, &mut live.traffic, &resources)?;
    crate::player_settings::atomic_write(
        &path.with_extension("report.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )?;
    Ok(report)
}
fn latest_recording(directory: &Path) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .take(4096)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if !name.starts_with("incident-")
                || !name.ends_with(".json")
                || name.ends_with(".report.json")
            {
                return None;
            }
            let kind = entry.file_type().ok()?;
            if !kind.is_file() {
                return None;
            }
            let meta = entry.metadata().ok()?;
            (meta.len() <= 64 * 1024 * 1024).then(|| (meta.modified().ok(), entry.path()))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, path)| path)
}
pub fn poll(
    mut recorder: ResMut<IncidentRecording>,
    mut ui: ResMut<crate::player_ui::PlayerUiState>,
) {
    if recorder.poll() {
        ui.rebuild = true;
    }
}
