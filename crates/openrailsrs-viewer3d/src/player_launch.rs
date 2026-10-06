//! Route/service catalog and validated launch requests for the Bevy start menu.
use std::collections::{HashMap, HashSet};
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
    Snow,
    Overcast,
    Storm,
}
impl PlayerWeather {
    pub const ALL: [Self; 6] = [
        Self::Clear,
        Self::Rain,
        Self::Fog,
        Self::Snow,
        Self::Overcast,
        Self::Storm,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Despejado",
            Self::Rain => "Lluvia",
            Self::Fog => "Niebla",
            Self::Snow => "Nieve",
            Self::Overcast => "Nublado",
            Self::Storm => "Tormenta",
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
    pub edition: Option<String>,
}

// The menu owns its reply queue; cloning a live receiver would lose reports.
#[derive(Resource, Debug)]
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
    pub environment: crate::environment::EnvironmentSelection,
    pub status: String,
    pub consist_audits: HashMap<PathBuf, openrailsrs_train::ConsistAudit>,
    auditor: openrailsrs_train::ConsistAuditor,
    audits: crate::launch_audits::LaunchAudits,
    pending_reaudit: Option<PathBuf>,
}

impl Default for PlayerLaunchMenu {
    fn default() -> Self {
        Self::discover(Path::new("."), None)
    }
}

impl PlayerLaunchMenu {
    pub fn discover(project: &Path, scenery_root: Option<PathBuf>) -> Self {
        let mut services = vec![];
        // An external Argentine route must never replace Chiltern's scenery.
        let native = scenery_root
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("chiltern"))
            })
            .or_else(default_chiltern_root);
        services.extend(discover_example_services(project, native.as_deref()));
        // Native activities use the existing importer and the pinned route graph.
        if let Some(root) = native.as_ref()
            && let Some(route_dir) = services
                .iter()
                .find(|s| s.scenery_root.as_ref() == Some(root))
                .map(|s| s.route_dir.clone())
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
                    edition: None,
                });
            }
        }
        for route in crate::official_content::prepared_routes() {
            for source in route.activities {
                services.push(ServiceChoice {
                    name: format!(
                        "Actividad oficial · {}",
                        source.file_stem().unwrap_or_default().to_string_lossy()
                    ),
                    source,
                    route_dir: route.imported.clone(),
                    scenery_root: Some(route.native.clone()),
                    native_activity: true,
                    edition: route.edition.clone(),
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
            environment: default(),
            status: String::new(),
            consist_audits: HashMap::new(),
            audits: crate::launch_audits::LaunchAudits::new(),
            pending_reaudit: None,
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
    pub fn cycle_consist(&mut self, delta: i32) {
        self.consist = cycle(self.consist, self.consists.len(), delta);
        self.pending_reaudit = None;
        let mut paths = self.consists.clone();
        if self.consist < paths.len() {
            paths.swap(0, self.consist);
        }
        self.audits
            .request(self.auditor.trainset_roots.clone(), &paths, false);
        self.restore_cached_audits();
        self.update_audit_status();
    }
    pub fn refresh_choices(&mut self) {
        self.refresh_choices_with_library(&player_data_dir().join("rolling-stock/TRAINS/CONSISTS"));
    }
    /// Re-read manually completed resources without blocking or changing the choices.
    pub fn reaudit_selected(&mut self) -> Result<String, String> {
        let path = self
            .consists
            .get(self.consist)
            .cloned()
            .ok_or("No hay una formación seleccionada")?;
        self.auditor = openrailsrs_train::ConsistAuditor::new(self.auditor.trainset_roots.clone());
        if self.audits.error().is_some() {
            self.audits = crate::launch_audits::LaunchAudits::new();
        }
        self.consist_audits.clear();
        self.pending_reaudit = Some(path.clone());
        self.audits
            .request(self.auditor.trainset_roots.clone(), &[path], true);
        if let Some(error) = self.audits.error() {
            return Err(error.into());
        }
        let message = "Revisando los archivos de la formación…".to_string();
        self.status = message.clone();
        Ok(message)
    }
    pub fn selected_audit_pending(&self) -> bool {
        self.consists
            .get(self.consist)
            .is_some_and(|p| !self.consist_audits.contains_key(p))
    }
    pub fn reaudit_pending(&self) -> bool {
        self.pending_reaudit.is_some()
    }
    pub fn poll_consist_audits(&mut self) -> bool {
        if !self.audits.poll() {
            return false;
        }
        self.restore_cached_audits();
        if self.audits.error().is_some() {
            self.pending_reaudit = None;
            self.update_audit_status();
        } else if let Some(path) = &self.pending_reaudit
            && let Some(audit) = self.consist_audits.get(path)
        {
            self.status = format!("Auditoría actualizada: {}", audit.label());
            self.pending_reaudit = None;
        } else {
            self.update_audit_status();
        }
        true
    }
    fn restore_cached_audits(&mut self) {
        for path in &self.consists {
            if !self.consist_audits.contains_key(path)
                && let Some(audit) = self.audits.cached(&self.auditor.trainset_roots, path)
            {
                self.consist_audits.insert(path.clone(), audit.clone());
            }
        }
    }
    fn update_audit_status(&mut self) {
        self.status = if let Some(error) = self.audits.error() {
            error.into()
        } else if self.selected_audit_pending() {
            "Revisando los archivos de la formación… Podés seguir eligiendo ruta y servicio".into()
        } else {
            "Elegí el servicio y pulsá Iniciar partida".into()
        };
    }
    fn refresh_choices_with_library(&mut self, library: &Path) {
        self.pending_reaudit = None;
        self.consists.clear();
        self.paths.clear();
        self.consist = 0;
        self.path = 0;
        let Some(choice) = self.current().cloned() else {
            self.status = "No hay servicios instalados".into();
            return;
        };
        // Resolve rolling stock in the selected content pack, including routes
        // that reuse a folder name from another pack.
        self.auditor = openrailsrs_train::ConsistAuditor::new(
            choice
                .scenery_root
                .as_deref()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .map(|p| vec![p.join("TRAINS/TRAINSET")])
                .unwrap_or_default(),
        );
        self.consist_audits.clear();
        // Original author packages may supply rolling stock independently of a
        // route. Keep their native TRAINS layout in the user's writable library.
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
        for con in files_with_extension(library, "con") {
            if !self.consists.contains(&con) {
                self.consists.push(con);
            }
        }
        // Parsing every shared vehicle, shape and cab used to freeze this frame.
        // Audit the selected formation first; reuse completed reports on return.
        self.audits
            .request(self.auditor.trainset_roots.clone(), &self.consists, false);
        self.restore_cached_audits();
        self.update_audit_status();
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
            .map(|r| format!("{}\n{}", r.label(), r.compatibility_summary()))
            .unwrap_or_else(|| {
                if self.selected_audit_pending() {
                    "Revisando archivos de la formación…".into()
                } else {
                    "Se valida al importar la actividad".into()
                }
            })
    }
    pub fn prepare(&self) -> Result<QueuedPlayerLaunch, String> {
        self.prepare_in(&player_data_dir())
    }
    fn prepare_in(&self, data_dir: &Path) -> Result<QueuedPlayerLaunch, String> {
        if self.selected_audit_pending() {
            return Err("La formación todavía se está revisando; esperá su diagnóstico".into());
        }
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
            let authored = if Path::new(&scenario.train.consist).is_absolute() {
                PathBuf::from(&scenario.train.consist)
            } else {
                choice
                    .source
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(&scenario.train.consist)
            };
            if absolute(&authored) != absolute(con) {
                scenario.train.electric_pickups.clear();
            }
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
        let length = consist
            .vehicles
            .iter()
            .map(|v| match v {
                openrailsrs_train::Vehicle::Loco(l) => l.length_m,
                openrailsrs_train::Vehicle::Wagon(w) => w.length_m,
            })
            .sum();
        if original_path && choice.native_activity {
            let imported = openrailsrs_msts::import_activity_with_consist_length(
                choice
                    .scenery_root
                    .as_deref()
                    .ok_or("Falta la ruta original")?,
                &choice.source,
                &imported_dir,
                length,
            )
            .map_err(|e| e.to_string())?;
            let native: ScenarioFile = toml::from_str(&imported).map_err(|e| e.to_string())?;
            scenario.route = native.route;
            scenario.route.path = imported_dir.to_string_lossy().into_owned();
        }
        if !original_path {
            let pat_path = self
                .paths
                .get(self.path - 1)
                .ok_or("Recorrido desconocido")?;
            let loaded =
                openrailsrs_route::load_route_from_dir(&imported_dir).map_err(|e| e.to_string())?;
            let pat =
                openrailsrs_formats::PathFile::from_path(pat_path).map_err(|e| e.to_string())?;
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
            environment: crate::environment::EnvironmentSelection {
                manual_weather: self.weather,
                ..self.environment
            },
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
/// Discover complete scenario files, including nested examples. Reports,
/// overlays, timetables and campaigns do not deserialize as playable scenarios.
fn discover_example_services(project: &Path, chiltern: Option<&Path>) -> Vec<ServiceChoice> {
    let examples = absolute(&project.join("examples"));
    // Keep the full station journey as the default, independent of directory order.
    let mut files: Vec<_> = [
        "chiltern_extended",
        "chiltern_traffic",
        "chiltern_local",
        "chiltern",
        "smoke",
        "steam",
        "sce",
    ]
    .iter()
    .map(|folder| examples.join(folder).join("scenario.toml"))
    .collect();
    let mut pending = vec![examples.clone()];
    let mut discovered = Vec::new();
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "toml") {
                discovered.push(entry.path());
            }
        }
    }
    discovered.sort();
    files.extend(discovered);
    let mut seen = HashSet::new();
    let mut services = Vec::new();
    for path in files {
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains(".tmp."))
        {
            continue;
        }
        let source = absolute(&path);
        if !seen.insert(source.clone()) {
            continue;
        }
        let Ok(scenario) = load_scenario(&source) else {
            continue;
        };
        let Some(dir) = source.parent() else { continue };
        let route_dir = absolute(&dir.join(&scenario.route.path));
        if !route_dir.join("track.toml").is_file() {
            continue;
        }
        // A prepared native pilot owns its metadata; never substitute Chiltern
        // scenery for another route just because it lives under examples/.
        let native = dir
            .ancestors()
            .take_while(|d| d.starts_with(&examples))
            .find_map(|d| {
                let text = std::fs::read_to_string(d.join("native-content.json")).ok()?;
                let metadata: NativeContent = serde_json::from_str(&text).ok()?;
                let root = absolute(&d.join(metadata.route_root));
                root.join("WORLD").is_dir().then_some(root)
            });
        let scenery_root = native.or_else(|| {
            is_chiltern_corridor(&route_dir)
                .then(|| chiltern.map(Path::to_path_buf))
                .flatten()
        });
        services.push(ServiceChoice {
            name: scenario.scenario.name,
            source,
            route_dir,
            scenery_root,
            native_activity: false,
            edition: None,
        });
    }
    let mut names = HashMap::<String, usize>::new();
    for service in &services {
        *names.entry(service.name.clone()).or_default() += 1;
    }
    for service in &mut services {
        if names[&service.name] > 1 {
            let relative = service
                .source
                .strip_prefix(&examples)
                .unwrap_or(&service.source);
            service.name = format!("{} · {}", service.name, relative.display());
        }
    }
    services
}
fn route_label(s: &ServiceChoice) -> String {
    if let Some(edition) = &s.edition {
        return edition.clone();
    }
    if let Some(root) = &s.scenery_root {
        root.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else if is_chiltern_corridor(&s.route_dir) {
        "Chiltern".into()
    } else {
        s.route_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}

fn is_chiltern_corridor(route: &Path) -> bool {
    route.file_name().is_some_and(|f| {
        matches!(
            f.to_str(),
            Some("chiltern" | "chiltern_local" | "chiltern_extended" | "chiltern_traffic")
        )
    })
}

#[derive(Deserialize)]
struct NativeContent {
    route_root: PathBuf,
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
    pub environment: crate::environment::EnvironmentSelection,
    pub resume: Option<PathBuf>,
}
#[derive(Resource, Default)]
pub struct PlayerLaunchQueue(pub Option<QueuedPlayerLaunch>);
#[derive(Resource, Clone, Debug, Default)]
pub struct ActivePlayerContent {
    pub route_root: Option<PathBuf>,
    pub description: String,
    pub weather: PlayerWeather,
    pub environment: crate::environment::EnvironmentSelection,
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
    fn wait_for_audits(menu: &mut PlayerLaunchMenu) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while menu.consist_audits.len() < menu.consists.len() {
            assert!(std::time::Instant::now() < deadline, "{}", menu.status);
            menu.poll_consist_audits();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn wait_for_reaudit(menu: &mut PlayerLaunchMenu) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while menu.reaudit_pending() {
            assert!(std::time::Instant::now() < deadline, "{}", menu.status);
            menu.poll_consist_audits();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    #[ignore = "requires the original Chiltern installation; prints route-switch timings"]
    fn native_menu_route_switch_timing() {
        let native = PathBuf::from(
            std::env::var_os("OPENRAILSRS_NATIVE_ROUTE").expect("original Chiltern route"),
        );
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut menu = PlayerLaunchMenu::discover(&project, Some(native));
        wait_for_audits(&mut menu);
        let chiltern = menu.routes.iter().position(|r| r == "Chiltern").unwrap();
        let mitre = menu
            .routes
            .iter()
            .position(|r| r.to_ascii_lowercase().contains("mitre"))
            .unwrap();
        for _ in 0..3 {
            for route in [mitre, chiltern] {
                let start = std::time::Instant::now();
                menu.cycle_route(route as i32 - menu.route as i32);
                println!(
                    "MENU_SWITCH {} {:.3} ms · {} formations",
                    menu.routes[menu.route],
                    start.elapsed().as_secs_f64() * 1000.,
                    menu.consists.len()
                );
                wait_for_audits(&mut menu);
            }
        }
    }
    #[test]
    #[ignore = "requires the original Demo Model 1 and its imported graph"]
    fn native_demo_launch_uses_the_chosen_formation_head() {
        let content = PathBuf::from(
            std::env::var_os("OPENRAILSRS_NATIVE_DEMO").expect("original Demo Model 1 folder"),
        );
        let track =
            PathBuf::from(std::env::var_os("OPENRAILSRS_DEMO_TRACK").expect("imported SCE folder"));
        let native = content.join("ROUTES/SCE");
        let source = native.join("ACTIVITIES/MT_MT_0930 Edinburgh-Glasgow Queen Street.act");
        let con = content.join("TRAINS/CONSISTS/MT_MT_Class 47 & 6 mk2 PP.con");
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut menu = PlayerLaunchMenu::discover(&project, None);
        menu.services = vec![ServiceChoice {
            name: "Demo native".into(),
            source: source.clone(),
            route_dir: track.clone(),
            scenery_root: Some(native.clone()),
            native_activity: true,
            edition: None,
        }];
        menu.routes = vec!["SCE".into()];
        menu.route = 0;
        menu.service = 0;
        menu.consist = 0;
        menu.path = 0;
        menu.consist_audits
            .insert(con.clone(), menu.auditor.inspect(&con));
        menu.consists = vec![con];
        menu.start_time_s = 34140.0;
        menu.season = 1;
        let output = tempfile::tempdir().unwrap();
        let request = menu.prepare_in(output.path()).unwrap();
        let launched = load_scenario(&request.path).unwrap();
        let rear: ScenarioFile = toml::from_str(
            &openrailsrs_msts::import_activity_with_track(&native, &source, Some(&track)).unwrap(),
        )
        .unwrap();
        assert!(
            (launched.route.start_offset_m.unwrap()
                - rear.route.start_offset_m.unwrap()
                - 139.9032)
                .abs()
                < 0.01
        );
        assert_eq!(launched.route.path, track.to_string_lossy());
        assert_eq!(launched.scenario.start_time_s, Some(34140.0));
        assert_eq!(request.route_root, Some(native));
        assert_eq!(launched.route.destination, "n62");
        let stops = &launched.route.stops;
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0].name.as_deref(), Some("Edinburgh Waverley"));
        assert_eq!(stops[1].name.as_deref(), Some("Haymarket"));
        assert_eq!(stops[2].name.as_deref(), Some("Linlithgow"));
        assert_eq!(stops[2].dwell_s, 600.0);
        assert_eq!(stops[2].passengers_off, 40);
        if let Some(path) = std::env::var_os("OPENRAILSRS_DEMO_LAUNCH_OUT") {
            std::fs::copy(&request.path, path).unwrap();
        }
    }
    #[test]
    fn separately_installed_author_formation_can_be_reaudited_after_resource_changes() {
        let temp = tempfile::tempdir().unwrap();
        let trains = temp.path().join("rolling-stock/TRAINS");
        let library = trains.join("CONSISTS");
        let stock = trains.join("TRAINSET/AuthorStock");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::write(stock.join("original.eng"),"Wagon ( original Mass ( 40t ) Size ( 3m 4m 20m ) ) Engine ( original MaxPower ( 500kW ) )").unwrap();
        let consist = library.join("original.con");
        std::fs::write(
            &consist,
            "Train ( TrainCfg ( original Engine ( EngineData ( original AuthorStock ) ) ) )",
        )
        .unwrap();
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut menu = PlayerLaunchMenu::discover(&project, None);
        menu.refresh_choices_with_library(&library);
        wait_for_audits(&mut menu);
        assert!(menu.consists.contains(&consist));
        assert!(menu.consist_audits[&consist].player_ready());
        menu.consist = menu.consists.iter().position(|p| p == &consist).unwrap();
        menu.start_time_s = 12300.0;
        let selection = (
            menu.route,
            menu.service,
            menu.consist,
            menu.path,
            menu.start_time_s,
        );
        std::fs::write(stock.join("original.eng"), "Wagon ( original Mass ( 40t ) Size ( 3m 4m 20m ) WagonShape ( original.s ) ) Engine ( original MaxPower ( 500kW ) )").unwrap();
        menu.reaudit_selected().unwrap();
        wait_for_reaudit(&mut menu);
        assert!(!menu.consist_audits[&consist].player_ready());
        std::fs::write(stock.join("original.s"), "(shape (texture_filenames 0))").unwrap();
        menu.reaudit_selected().unwrap();
        wait_for_reaudit(&mut menu);
        assert!(menu.consist_audits[&consist].player_ready());
        assert!(menu.consist_audits[&consist].missing_resources.is_empty());
        std::fs::remove_file(stock.join("original.s")).unwrap();
        menu.reaudit_selected().unwrap();
        wait_for_reaudit(&mut menu);
        assert!(!menu.consist_audits[&consist].player_ready());
        assert_eq!(
            (
                menu.route,
                menu.service,
                menu.consist,
                menu.path,
                menu.start_time_s
            ),
            selection
        );
    }
    #[test]
    fn launch_waits_for_the_selected_audit_and_rejects_an_incomplete_formation() {
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let temp = tempfile::tempdir().unwrap();
        let mut menu = PlayerLaunchMenu::discover(&project, None);
        let service = menu
            .services
            .iter()
            .position(|s| s.source.ends_with("smoke/scenario.toml"))
            .unwrap();
        menu.service = service;
        let con = temp.path().join("missing.con");
        menu.consists = vec![con.clone()];
        menu.consist = 0;
        menu.consist_audits.clear();
        let error = menu.prepare_in(temp.path()).unwrap_err();
        assert!(error.contains("todavía se está revisando"), "{error}");
        assert!(std::fs::read_dir(temp.path()).unwrap().next().is_none());
        menu.consist_audits.insert(
            con.clone(),
            openrailsrs_train::ConsistAudit {
                path: con,
                errors: vec!["Falta el recurso original".into()],
                ..Default::default()
            },
        );
        let error = menu.prepare_in(temp.path()).unwrap_err();
        assert!(error.contains("Falta el recurso original"), "{error}");
        assert!(std::fs::read_dir(temp.path()).unwrap().next().is_none());
    }
    #[test]
    fn extended_and_traffic_examples_keep_native_chiltern_scenery() {
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let content = tempfile::tempdir().unwrap();
        let native = content.path().join("ROUTES/Chiltern");
        std::fs::create_dir_all(native.join("WORLD")).unwrap();
        let menu = PlayerLaunchMenu::discover(&project, Some(native.clone()));
        assert_eq!(menu.routes[0], "Chiltern");
        assert_eq!(menu.current().unwrap().scenery_root.as_ref(), Some(&native));
        for folder in [
            "chiltern",
            "chiltern_local",
            "chiltern_extended",
            "chiltern_traffic",
        ] {
            let source = absolute(&project.join("examples").join(folder).join("scenario.toml"));
            let service = menu.services.iter().find(|s| s.source == source).unwrap();
            assert_eq!(service.scenery_root.as_ref(), Some(&native), "{folder}");
        }
    }
    #[test]
    fn examples_catalog_discovers_nested_scenarios_and_skips_non_playable_files() {
        let project = tempfile::tempdir().unwrap();
        let examples = project.path().join("examples/custom/scenarios");
        std::fs::create_dir_all(&examples).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut scenario = load_scenario(root.join("examples/smoke/scenario.toml")).unwrap();
        scenario.route.path = "..".into();
        scenario.scenario.name = "Servicio de ejemplo".into();
        std::fs::write(examples.parent().unwrap().join("track.toml"), "").unwrap();
        for file in ["first.toml", "second.toml", "scenario.tmp.toml"] {
            std::fs::write(examples.join(file), toml::to_string(&scenario).unwrap()).unwrap();
        }
        std::fs::write(examples.join("run.toml"), "elapsed_s = 100\n").unwrap();
        std::fs::write(
            examples.join("scenario.overlay.toml"),
            "[train]\nstart_speed_mps = 1\n",
        )
        .unwrap();
        let services = discover_example_services(project.path(), None);
        assert_eq!(services.len(), 2);
        assert_ne!(services[0].name, services[1].name);
        assert!(services.iter().all(|s| s.scenery_root.is_none()));
        let menu = PlayerLaunchMenu::discover(&root, None);
        assert!(menu.services.iter().any(|s| {
            s.source
                .ends_with("mitre_campaign/scenarios/retiro_olivos.toml")
        }));
        assert!(
            menu.services
                .iter()
                .any(|s| s.source.ends_with("sce/scenario_multi_body.toml"))
        );
    }
    #[test]
    fn native_pilot_keeps_its_own_scenery_and_route_label() {
        let project = tempfile::tempdir().unwrap();
        let dir = project.path().join("examples/belgrano_cc");
        let native = project.path().join("Content/ROUTES/BelgranoCC");
        std::fs::create_dir_all(native.join("WORLD")).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut scenario = load_scenario(root.join("examples/smoke/scenario.toml")).unwrap();
        scenario.scenario.name = "Piloto nativo".into();
        scenario.route.path = ".".into();
        std::fs::write(
            dir.join("scenario.toml"),
            toml::to_string(&scenario).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("track.toml"), "").unwrap();
        std::fs::write(
            dir.join("native-content.json"),
            serde_json::json!({"route_root":native}).to_string(),
        )
        .unwrap();
        let menu = PlayerLaunchMenu::discover(project.path(), Some(native.clone()));
        let pilot = menu
            .services
            .iter()
            .find(|s| s.name == "Piloto nativo")
            .unwrap();
        assert_eq!(pilot.scenery_root.as_ref(), Some(&absolute(&native)));
        assert!(menu.routes.iter().any(|r| r == "BelgranoCC"));
        assert!(!menu.services.iter().any(|s| s.native_activity));
    }
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
        menu.consist_audits
            .insert(con.clone(), menu.auditor.inspect(&con));
        menu.consists = vec![con];
        menu.start_time_s = 45000.0;
        menu.season = 3;
        menu.weather = PlayerWeather::Rain;
        let request = menu.prepare_in(dir.path()).unwrap();
        let scenario = load_scenario(&request.path).unwrap();
        assert_eq!(scenario.route.stops.len(), 6);
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
