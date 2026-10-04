//! Route/service catalog and validated launch requests for the Bevy start menu.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_scenarios::{ScenarioFile, load_scenario};
use serde::{Deserialize, Serialize};

use crate::player_settings::{atomic_write, player_data_dir};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerWeather {
    #[default]
    Clear,
    Rain,
    Fog,
}
impl PlayerWeather {
    pub const ALL: [Self; 3] = [Self::Clear, Self::Rain, Self::Fog];
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Despejado",
            Self::Rain => "Lluvia",
            Self::Fog => "Niebla",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServiceChoice {
    pub name: String,
    pub source: PathBuf,
    pub route_dir: PathBuf,
    pub scenery_root: Option<PathBuf>,
    pub native_activity: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct PlayerLaunchMenu {
    pub routes: Vec<String>,
    pub services: Vec<ServiceChoice>,
    pub route: usize,
    pub service: usize,
    pub consists: Vec<PathBuf>,
    pub consist: usize,
    pub paths: Vec<PathBuf>,
    pub path: usize,
    pub start_time_s: f64,
    pub season: usize,
    pub weather: PlayerWeather,
    pub status: String,
    pub consist_audits: HashMap<PathBuf, openrailsrs_train::ConsistAudit>,
    auditor: openrailsrs_train::ConsistAuditor,
}

impl Default for PlayerLaunchMenu {
    fn default() -> Self {
        Self::discover(Path::new("."), None)
    }
}

impl PlayerLaunchMenu {
    pub fn discover(project: &Path, scenery_root: Option<PathBuf>) -> Self {
        let mut services = vec![];
        let native = scenery_root.or_else(default_chiltern_root);
        for (folder, label) in [
            ("chiltern_traffic", "Chiltern"),
            ("chiltern_local", "Chiltern"),
            ("chiltern", "Chiltern"),
            ("smoke", "Ruta de práctica"),
            ("steam", "Vapor"),
            ("sce", "SCE"),
        ] {
            let path = project.join("examples").join(folder).join("scenario.toml");
            if let Ok(scenario) = load_scenario(&path) {
                let Some(dir) = path.parent() else { continue };
                let route_dir = absolute(&dir.join(&scenario.route.path));
                let source = absolute(&path);
                if !route_dir.join("track.toml").is_file() {
                    continue;
                }
                services.push(ServiceChoice {
                    name: scenario.scenario.name,
                    source,
                    route_dir,
                    scenery_root: if label == "Chiltern" {
                        native.clone()
                    } else {
                        None
                    },
                    native_activity: false,
                });
            }
        }
        // Native activities use the existing importer and the pinned route graph.
        if let Some(root) = native.as_ref()
            && let Some(route_dir) = services.first().map(|s| s.route_dir.clone())
        {
            for source in files_with_extension(&root.join("ACTIVITIES"), "act") {
                let name = source
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                services.push(ServiceChoice {
                    name: format!("Actividad · {name}"),
                    source,
                    route_dir: route_dir.clone(),
                    scenery_root: Some(root.clone()),
                    native_activity: true,
                });
            }
        }
        let routes = services
            .iter()
            .map(route_label)
            .fold(Vec::new(), |mut v, s| {
                if !v.contains(&s) {
                    v.push(s);
                }
                v
            });
        let mut menu = Self {
            routes,
            services,
            route: 0,
            service: 0,
            consists: vec![],
            consist: 0,
            paths: vec![],
            path: 0,
            start_time_s: 35700.0,
            season: 1,
            weather: PlayerWeather::Clear,
            status: String::new(),
            consist_audits: HashMap::new(),
            auditor: openrailsrs_train::ConsistAuditor::new(
                native
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .map(|p| vec![p.join("TRAINS/TRAINSET")])
                    .unwrap_or_default(),
            ),
        };
        menu.refresh_choices();
        menu
    }
    pub fn current(&self) -> Option<&ServiceChoice> {
        self.services.get(self.service)
    }
    pub fn service_indices(&self) -> Vec<usize> {
        self.services
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                self.routes
                    .get(self.route)
                    .is_some_and(|r| r == &route_label(s))
            })
            .map(|(i, _)| i)
            .collect()
    }
    pub fn cycle_route(&mut self, delta: i32) {
        self.route = cycle(self.route, self.routes.len(), delta);
        self.service = self.service_indices().first().copied().unwrap_or(0);
        self.refresh_choices();
    }
    pub fn cycle_service(&mut self, delta: i32) {
        let indices = self.service_indices();
        let i = indices.iter().position(|i| *i == self.service).unwrap_or(0);
        self.service = indices
            .get(cycle(i, indices.len(), delta))
            .copied()
            .unwrap_or(0);
        self.refresh_choices();
    }
    pub fn refresh_choices(&mut self) {
        self.consists.clear();
        self.paths.clear();
        self.consist = 0;
        self.path = 0;
        let Some(choice) = self.current().cloned() else {
            self.status = "No hay servicios instalados".into();
            return;
        };
        if !choice.native_activity
            && let Ok(s) = load_scenario(&choice.source)
        {
            self.consists.push(absolute(
                &choice.source.parent().unwrap().join(&s.train.consist),
            ));
            self.start_time_s = s.scenario.start_time_s.unwrap_or(43200.0);
            self.season = match s.scenario.season.as_deref() {
                Some("summer") => 1,
                Some("autumn") => 2,
                Some("winter") => 3,
                _ => 0,
            };
        }
        if let Some(root) = &choice.scenery_root {
            if let Some(content) = root.parent().and_then(Path::parent) {
                for con in files_with_extension(&content.join("TRAINS/CONSISTS"), "con") {
                    if !self.consists.contains(&con) {
                        self.consists.push(con);
                    }
                }
            }
            self.paths = files_with_extension(&root.join("PATHS"), "pat");
        }
        // An empty first choice means the consist/path authored by the selected activity.
        for con in &self.consists {
            if !self.consist_audits.contains_key(con) {
                self.consist_audits
                    .insert(con.clone(), self.auditor.inspect(con));
            }
        }
        self.status = "Elegí el servicio y pulsá Iniciar partida".into();
    }
    pub fn consist_label(&self) -> String {
        self.consists
            .get(self.consist)
            .map(|p| {
                p.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| "Formación de la actividad".into())
    }
    pub fn path_label(&self) -> String {
        if self.path == 0 {
            "Recorrido del servicio".into()
        } else {
            self.paths
                .get(self.path - 1)
                .map(|p| {
                    p.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .unwrap_or_default()
        }
    }
    pub fn consist_status(&self) -> String {
        self.consists
            .get(self.consist)
            .and_then(|p| self.consist_audits.get(p))
            .map(|r| r.label())
            .unwrap_or_else(|| "Se valida al importar la actividad".into())
    }
    pub fn prepare(&self) -> Result<QueuedPlayerLaunch, String> {
        self.prepare_in(&player_data_dir())
    }
    fn prepare_in(&self, data_dir: &Path) -> Result<QueuedPlayerLaunch, String> {
        let choice = self.current().ok_or("No hay un servicio seleccionado")?;
        let imported_dir = dispatch_network_dir(&choice.route_dir);
        let (mut value, mut scenario) = if choice.native_activity {
            let native = choice
                .scenery_root
                .as_deref()
                .ok_or("Falta la ruta original")?;
            let imported = openrailsrs_msts::import_activity_with_track(
                native,
                &choice.source,
                Some(&imported_dir),
            )
            .map_err(|e| e.to_string())?;
            let s: ScenarioFile = toml::from_str(&imported).map_err(|e| e.to_string())?;
            (
                toml::from_str::<toml::Value>(&imported).map_err(|e| e.to_string())?,
                s,
            )
        } else {
            let text = std::fs::read_to_string(&choice.source).map_err(|e| e.to_string())?;
            (
                toml::from_str::<toml::Value>(&text).map_err(|e| e.to_string())?,
                load_scenario(&choice.source).map_err(|e| e.to_string())?,
            )
        };
        scenario.route.path = if choice.native_activity || self.path != 0 {
            &imported_dir
        } else {
            &choice.route_dir
        }
        .to_string_lossy()
        .into_owned();
        for service in &mut scenario.extra_trains {
            if !Path::new(&service.consist).is_absolute() {
                let root = if choice.native_activity {
                    choice
                        .scenery_root
                        .as_deref()
                        .and_then(Path::parent)
                        .and_then(Path::parent)
                        .ok_or("No se encuentra el Content del tráfico")?
                } else {
                    choice.source.parent().unwrap_or(Path::new("."))
                };
                service.consist = absolute(&root.join(&service.consist))
                    .to_string_lossy()
                    .into_owned();
            }
            let audit = self.auditor.clone().inspect(Path::new(&service.consist));
            if !audit.player_ready() {
                return Err(format!("{}: {}", service.id, audit.label()));
            }
        }
        if let Some(con) = self.consists.get(self.consist) {
            scenario.train.consist = con.to_string_lossy().into_owned();
        } else if !Path::new(&scenario.train.consist).is_absolute() {
            let root = choice
                .scenery_root
                .as_deref()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or("No se encuentra el Content del material rodante")?;
            scenario.train.consist = root
                .join(&scenario.train.consist)
                .to_string_lossy()
                .into_owned();
        }
        let con = Path::new(&scenario.train.consist);
        let audit = self
            .consist_audits
            .get(con)
            .cloned()
            .unwrap_or_else(|| self.auditor.clone().inspect(con));
        if !audit.player_ready() {
            return Err(audit.label());
        }
        let consist = openrailsrs_train::load_consist_with_asset_root(
            con,
            openrailsrs_train::consist_asset_root(con),
        )
        .map_err(|e| e.to_string())?;
        let original_path = self.path == 0;
        if !original_path {
            let pat_path = self
                .paths
                .get(self.path - 1)
                .ok_or("Recorrido desconocido")?;
            let loaded =
                openrailsrs_route::load_route_from_dir(&imported_dir).map_err(|e| e.to_string())?;
            let pat =
                openrailsrs_formats::PathFile::from_path(pat_path).map_err(|e| e.to_string())?;
            let length = consist
                .vehicles
                .iter()
                .map(|v| match v {
                    openrailsrs_train::Vehicle::Loco(l) => l.length_m,
                    openrailsrs_train::Vehicle::Wagon(w) => w.length_m,
                })
                .sum();
            let hints = openrailsrs_msts::placement_for_pat_with_consist(
                &loaded.graph,
                &loaded.msts_aliases,
                pat_path,
                0.0,
                Some(length),
            )
            .map_err(|e| e.to_string())?;
            scenario.route.waypoints = openrailsrs_msts::pat_waypoints_with_offset(
                &loaded.graph,
                &loaded.msts_aliases,
                &pat,
                &hints.start,
                &hints.destination,
                hints.start_offset_m,
            )
            .map_err(|e| e.to_string())?;
            scenario.route.start = hints.start;
            scenario.route.destination = hints.destination;
            scenario.route.start_offset_m = Some(hints.start_offset_m);
            scenario.route.switches = hints.switches;
            scenario.route.stops.clear(); // A replacement PAT is an exploration, not the original stopping timetable.
            scenario.scenario.name = format!("Exploración · {}", self.path_label());
            scenario.scenario.description="Recorrido libre hasta el final del itinerario seleccionado. Respetá las señales y los límites de velocidad.".into();
        }
        scenario.scenario.start_time_s = Some(self.start_time_s.rem_euclid(86400.0));
        scenario.scenario.season =
            Some(["spring", "summer", "autumn", "winter"][self.season % 4].into());
        scenario.output.csv = "player-data/run.csv".into();
        scenario.output.metadata = "player-data/run.json".into();
        scenario.simulation.time_step = 0.05;
        // Validate content and stopping order before submitting a background scene load.
        openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
            .map_err(|e| e.to_string())?;
        let updated = toml::Value::try_from(&scenario).map_err(|e| e.to_string())?;
        for (key, entry) in updated.as_table().ok_or("Servicio inválido")? {
            value
                .as_table_mut()
                .ok_or("Servicio inválido")?
                .insert(key.clone(), entry.clone());
        }
        let text = toml::to_string_pretty(&value).map_err(|e| e.to_string())?;
        let path = absolute(&data_dir.join("launch.toml"));
        atomic_write(&path, text.as_bytes())?;
        Ok(QueuedPlayerLaunch {
            path,
            route_root: choice.scenery_root.clone(),
            weather: self.weather,
            resume: None,
        })
    }
}

pub fn cycle(index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        0
    } else {
        (index as i32 + delta).rem_euclid(len as i32) as usize
    }
}
fn route_label(s: &ServiceChoice) -> String {
    if s.scenery_root.is_some()
        || s.route_dir
            .file_name()
            .is_some_and(|f| f == "chiltern" || f == "chiltern_local")
    {
        "Chiltern".into()
    } else {
        s.route_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}
pub fn absolute(p: &Path) -> PathBuf {
    if let Ok(p) = p.canonicalize() {
        p
    } else if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}
fn files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension))
        })
        .map(|p| absolute(&p))
        .collect::<Vec<_>>();
    files.sort();
    files
}
pub fn default_chiltern_root() -> Option<PathBuf> {
    let p = std::env::var_os("CHILTERN_ROUTE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                PathBuf::from(h).join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern")
            })
        })?;
    p.join("WORLD").is_dir().then_some(p)
}
#[derive(Clone, Debug)]
pub struct QueuedPlayerLaunch {
    pub path: PathBuf,
    pub route_root: Option<PathBuf>,
    pub weather: PlayerWeather,
    pub resume: Option<PathBuf>,
}
#[derive(Resource, Default)]
pub struct PlayerLaunchQueue(pub Option<QueuedPlayerLaunch>);
#[derive(Resource, Clone, Debug, Default)]
pub struct ActivePlayerContent {
    pub route_root: Option<PathBuf>,
    pub description: String,
    pub weather: PlayerWeather,
}

/// Use the full imported network for dispatch/PAT choices on a compact service corridor.
pub fn dispatch_network_dir(corridor: &Path) -> PathBuf {
    if corridor.file_name().is_some_and(|n| n == "chiltern_local") {
        let full = corridor.parent().unwrap_or(Path::new(".")).join("chiltern");
        if full.join("track.toml").is_file() {
            return full;
        }
    }
    corridor.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_menu_service_is_written_and_loadable() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = tempfile::tempdir().unwrap();
        // A launch test must not rely on an installed MSTS Content. The project
        // Pullman physics fixtures intentionally omit its authored cab assets.
        let stock = dir.path().join("TRAINS/TRAINSET/DMU");
        let cons = dir.path().join("TRAINS/CONSISTS");
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::create_dir_all(&cons).unwrap();
        std::fs::write(
            stock.join("power.eng"),
            "Wagon ( power Mass ( 40t ) Size ( 3m 4m 20m ) ) Engine ( power MaxPower ( 500kW ) MaxForce ( 100kN ) )",
        )
        .unwrap();
        let con = cons.join("test.con");
        std::fs::write(
            &con,
            "Train ( TrainCfg ( test Engine ( EngineData ( power DMU ) UiD ( 0 ) ) ) )",
        )
        .unwrap();
        let mut menu = PlayerLaunchMenu::discover(&root, Some(dir.path().join("ROUTES/Chiltern")));
        let mut authored = load_scenario(&menu.current().unwrap().source).unwrap();
        authored.route.path = menu
            .current()
            .unwrap()
            .route_dir
            .to_string_lossy()
            .into_owned();
        authored.train.consist = con.to_string_lossy().into_owned();
        for service in &mut authored.extra_trains {
            service.consist = con.to_string_lossy().into_owned();
        }
        let source = dir.path().join("traffic.toml");
        std::fs::write(&source, toml::to_string_pretty(&authored).unwrap()).unwrap();
        menu.services[menu.service].source = source.clone();
        menu.consists = vec![con];
        menu.consist_audits.clear();
        menu.start_time_s = 45000.0;
        menu.season = 3;
        menu.weather = PlayerWeather::Rain;
        let request = menu.prepare_in(dir.path()).unwrap();
        let scenario = load_scenario(&request.path).unwrap();
        assert_eq!(scenario.route.stops.len(), 3);
        assert_eq!(scenario.scenario.start_time_s, Some(45000.0));
        assert_eq!(scenario.scenario.season.as_deref(), Some("winter"));
        assert_eq!(request.weather, PlayerWeather::Rain);
        assert!(crate::live::LiveDrive::from_scenario_path(&request.path).is_ok());
        assert_eq!(scenario.extra_trains.len(), 2);
        assert!(
            scenario
                .extra_trains
                .iter()
                .all(|s| Path::new(&s.consist).is_absolute())
        );

        let previous_launch = std::fs::read(&request.path).unwrap();
        authored.extra_trains[0].consist = dir
            .path()
            .join("missing.con")
            .to_string_lossy()
            .into_owned();
        std::fs::write(&source, toml::to_string_pretty(&authored).unwrap()).unwrap();
        let error = menu.prepare_in(dir.path()).unwrap_err();
        assert!(error.contains(&authored.extra_trains[0].id), "{error}");
        assert_eq!(std::fs::read(&request.path).unwrap(), previous_launch);
    }

    #[test]
    fn catalog_contains_whole_station_service() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let menu = PlayerLaunchMenu::discover(&root, None);
        assert!(
            menu.services
                .iter()
                .any(|s| s.name.contains("Northolt Park"))
        );
        assert_eq!(menu.path, 0);
        assert!(!menu.consists.is_empty());
        assert!(menu.current().unwrap().route_dir.is_absolute());
    }
}
