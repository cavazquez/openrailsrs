//! Save browsing and compatibility use the same validator as session restore.
use crate::saved_game::SavedGame;
use bevy::prelude::*;
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
};

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub route: String,
    pub train: String,
    pub timestamp: i64,
    pub distance_m: f64,
    pub time_s: f64,
    pub progress: usize,
    pub compatible: Result<(), String>,
}
impl Entry {
    pub fn label(&self) -> String {
        let date = chrono::DateTime::from_timestamp(self.timestamp, 0)
            .map_or("Fecha desconocida".into(), |t| {
                t.format("%d/%m/%Y %H:%M UTC").to_string()
            });
        format!(
            "{} · {}\n{} · {:.2} km · {:.0} min · {} paradas",
            self.route,
            self.train,
            date,
            self.distance_m / 1000.0,
            self.time_s / 60.0,
            self.progress
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Compatible,
    Incompatible,
    Progressed,
}
impl Filter {
    pub const ALL: [Self; 4] = [
        Self::All,
        Self::Compatible,
        Self::Incompatible,
        Self::Progressed,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Todas",
            Self::Compatible => "Compatibles",
            Self::Incompatible => "Incompatibles",
            Self::Progressed => "Con avance",
        }
    }
    pub fn matches(self, e: &Entry, query: &str) -> bool {
        let text = format!("{} {} {}", e.route, e.train, e.label()).to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|w| text.contains(w))
            && match self {
                Self::All => true,
                Self::Compatible => e.compatible.is_ok(),
                Self::Incompatible => e.compatible.is_err(),
                Self::Progressed => e.distance_m > 0.0 || e.progress > 0,
            }
    }
}
enum Reply {
    Entry(Entry),
    Finished,
}
#[derive(Resource, Default)]
pub struct SaveHistory {
    pub entries: Vec<Entry>,
    pub loading: bool,
    reply: Option<Mutex<mpsc::Receiver<Reply>>>,
}
type ValidationCache = Option<(
    String,
    openrailsrs_sim::LiveDriveSession,
    openrailsrs_sim::LiveTraffic,
)>;
fn compatibility(game: &SavedGame, cache: &mut ValidationCache) -> Result<(), String> {
    game.validate_route_available()?;
    if let Some((source, session, traffic)) = cache
        && source == &game.scenario_toml
    {
        session.validate_snapshot(&game.session)?;
        traffic.validate_snapshot(&game.traffic)?;
        return openrailsrs_sim::SessionSnapshot::validate_shared_dispatcher(
            std::iter::once(&game.session).chain(game.traffic.iter().map(|s| &s.session)),
        );
    }
    // Retain at most one route, and never start optional script hosts while browsing.
    *cache = None;
    let scenario: openrailsrs_scenarios::ScenarioFile = toml::from_str(&game.scenario_toml)
        .map_err(|e| format!("Escenario guardado inválido: {e}"))?;
    let session = openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
        .map_err(|e| format!("Contenido no disponible: {e}"))?;
    let traffic = openrailsrs_sim::LiveTraffic::from_scenario(Path::new("."), &scenario)?;
    *cache = Some((game.scenario_toml.clone(), session, traffic));
    compatibility(game, cache)
}
pub fn scan(directory: &Path, mut publish: impl FnMut(Entry)) {
    let mut cache = None;
    let mut files = (0..3)
        .map(|i| directory.join(format!("save-{}.json", i + 1)))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    if let Ok(history) = std::fs::read_dir(directory.join("history")) {
        let mut archives = history
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
            .collect::<Vec<_>>();
        archives.sort();
        archives.reverse();
        files.extend(archives.into_iter().take(256));
    }
    for path in files {
        match SavedGame::read(&path) {
            Ok(game) => {
                let timestamp = game.created_utc.unwrap_or_else(|| {
                    std::fs::metadata(&path)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs() as i64)
                });
                let compatible = compatibility(&game, &mut cache);
                publish(Entry {
                    path,
                    route: game.description,
                    train: game.train_name,
                    timestamp,
                    distance_m: game.session.state.odometer_m,
                    time_s: game.session.state.time.0,
                    progress: game.session.gameplay.passed_stops.len(),
                    compatible,
                });
            }
            Err(e) => publish(Entry {
                route: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
                train: String::new(),
                timestamp: 0,
                distance_m: 0.0,
                time_s: 0.0,
                progress: 0,
                compatible: Err(e),
            }),
        }
    }
}
impl SaveHistory {
    pub fn request(&mut self, directory: PathBuf) {
        if self.loading {
            return;
        }
        self.entries.clear();
        self.loading = true;
        let (tx, rx) = mpsc::channel();
        self.reply = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            scan(&directory, |e| {
                let _ = tx.send(Reply::Entry(e));
            });
            let _ = tx.send(Reply::Finished);
        });
    }
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.reply else {
            return false;
        };
        let Ok(rx) = rx.try_lock() else {
            return false;
        };
        let mut changed = false;
        for _ in 0..8 {
            let reply = match rx.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading = false;
                    break;
                }
            };
            changed = true;
            match reply {
                Reply::Entry(e) => self.entries.push(e),
                Reply::Finished => self.loading = false,
            }
        }
        changed
    }
}
pub fn poll_history(
    mut history: ResMut<SaveHistory>,
    mut ui: ResMut<crate::player_ui::PlayerUiState>,
) {
    if history.poll() {
        ui.rebuild = true;
    }
}
pub fn history_keys(
    mut input: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut ui: ResMut<crate::player_ui::PlayerUiState>,
) {
    use bevy::input::{ButtonState, keyboard::Key};
    for event in input.read() {
        if !ui.history_focus || event.state != ButtonState::Pressed {
            continue;
        }
        match &event.logical_key {
            Key::Backspace => {
                ui.history_query.pop();
            }
            Key::Escape | Key::Enter => ui.history_focus = false,
            Key::Character(c) if ui.history_query.len() < 160 => {
                for ch in c.chars().filter(|c| !c.is_control()) {
                    if ui.history_query.len() + ch.len_utf8() <= 160 {
                        ui.history_query.push(ch);
                    }
                }
            }
            _ => {}
        }
        ui.rebuild = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan30_history_filters_metadata_and_uses_the_real_restore_validator() {
        let scenario =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&scenario).unwrap();
        live.session.driver_direction = 1.;
        live.session.driver_throttle = 0.5;
        live.session.step_realtime(2., |_| {});
        let dir = tempfile::tempdir().unwrap();
        let slot = dir.path().join("save-1.json");
        let camera = crate::saved_game::SavedCamera::capture(
            crate::camera::CameraFollowMode::ChaseCam,
            &default(),
            &default(),
            &default(),
        );
        SavedGame::save(
            &mut live,
            &crate::player_launch::ActivePlayerContent {
                description: "Smoke · viaje de prueba".into(),
                ..default()
            },
            camera,
            &slot,
            None,
            None,
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("history")).unwrap();
        std::fs::copy(&slot, dir.path().join("history/1.json")).unwrap();
        let mut corrupt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&slot).unwrap()).unwrap();
        corrupt["version"] = serde_json::json!(999);
        std::fs::write(
            dir.path().join("save-2.json"),
            serde_json::to_vec(&corrupt).unwrap(),
        )
        .unwrap();
        corrupt["version"] = serde_json::json!(1);
        corrupt["route_root"] = serde_json::json!(dir.path().join("missing-original-route"));
        let missing = dir.path().join("save-3.json");
        std::fs::write(&missing, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(
            SavedGame::prepare_resume(&missing)
                .unwrap_err()
                .contains("carpeta de la ruta")
        );
        let mut entries = vec![];
        scan(dir.path(), |e| entries.push(e));
        assert_eq!(entries.len(), 4);
        assert_eq!(entries.iter().filter(|e| e.compatible.is_ok()).count(), 2);
        let unavailable = entries.iter().find(|e| e.path == missing).unwrap();
        assert!(!unavailable.train.is_empty());
        assert!(unavailable.timestamp > 0);
        assert!(Filter::Incompatible.matches(unavailable, "smoke prueba"));
        let good = entries.iter().find(|e| e.compatible.is_ok()).unwrap();
        assert!(!good.train.is_empty());
        assert!(good.timestamp > 0);
        assert!(good.label().contains("UTC"));
        assert!(Filter::Compatible.matches(good, "smoke prueba"));
        assert!(!Filter::Incompatible.matches(good, ""));
        assert!(Filter::Progressed.matches(good, ""));
        let year = chrono::DateTime::from_timestamp(good.timestamp, 0)
            .unwrap()
            .format("%Y")
            .to_string();
        assert!(Filter::All.matches(good, &year));
        assert!(!Filter::All.matches(good, "otra-ruta"));
        assert!(SavedGame::read(&good.path).is_ok());
    }
}
