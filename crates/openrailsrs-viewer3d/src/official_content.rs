//! Cached official catalogue, background downloads and installed-content audit.
use crate::player_settings::{atomic_write, player_data_dir};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, mpsc},
};

#[derive(Clone, Debug, Deserialize)]
pub struct ContentAuthor {
    pub name: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct OfficialPackage {
    pub name: String,
    pub author: ContentAuthor,
    pub compensation: String,
    pub url: String,
    #[serde(rename = "downloadSize", default)]
    pub download_bytes: u64,
    #[serde(rename = "installSize", default)]
    pub install_bytes: u64,
}
impl OfficialPackage {
    pub fn id(&self) -> String {
        self.name
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }
    pub fn automatic(&self) -> bool {
        self.compensation == "free"
            && (self.url.starts_with("https://github.com/") && self.url.ends_with(".git")
                || (self.url.starts_with("https://static.openrails.org/")
                    || self.url.starts_with("https://ts-files.com/"))
                    && self.url.ends_with(".zip"))
    }
}
#[derive(Deserialize)]
struct Catalog {
    routes: Vec<OfficialPackage>,
}
struct Job {
    child: Option<Child>,
    cancel: PathBuf,
    events: Mutex<mpsc::Receiver<Value>>,
}
impl Drop for Job {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.cancel, b"cancel");
        if let Some(mut child) = self.child.take() {
            // Allow the installer to clean its own temporary directory.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
#[derive(Resource)]
pub struct OfficialContent {
    pub packages: Vec<OfficialPackage>,
    pub selected: usize,
    pub status: String,
    pub installed: Vec<PathBuf>,
    job: Option<Job>,
}
impl Default for OfficialContent {
    fn default() -> Self {
        let catalog: Catalog = serde_json::from_str(include_str!(
            "../../../docs/fixtures/content/official-catalog.json"
        ))
        .expect("bundled official catalogue");
        let selected = catalog
            .routes
            .iter()
            .position(|p| p.name == "Demo Model 1")
            .unwrap_or(0);
        let mut resource=Self {packages:catalog.routes,selected,status:"Elegí un paquete. Se instala por separado y se audita antes de ofrecer sus actividades.".into(),installed:vec![],job:None};
        resource.refresh_installed();
        resource
    }
}
impl OfficialContent {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn selected(&self) -> &OfficialPackage {
        &self.packages[self.selected]
    }
    pub fn cycle(&mut self, delta: i32) {
        self.selected =
            (self.selected as i32 + delta).rem_euclid(self.packages.len() as i32) as usize;
    }
    pub fn start(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una descarga en curso".into());
        }
        if !self.selected().automatic() {
            return Err("Este paquete requiere visitar la página de su autor".into());
        }
        let destination = player_data_dir().join("official-content");
        std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
        let cancel = destination.join(format!(
            ".cancel-{}-{}",
            std::process::id(),
            self.selected().id()
        ));
        if cancel.exists() {
            std::fs::remove_file(&cancel).map_err(|e| e.to_string())?;
        }
        // Retry preparation after cancellation without downloading the archive
        // again. An installed package retains the exact catalogue identity.
        let existing = self
            .installed
            .iter()
            .find(|path| {
                read_manifest(&path.join("openrailsrs-content.json")).is_some_and(|m| {
                    m["package"] == self.selected().name
                        && m["advertised_url"] == self.selected().url
                })
            })
            .cloned();
        if let Some(path) = existing {
            let (sender, events) = mpsc::channel();
            let cancel_worker = cancel.clone();
            std::thread::spawn(move || send_preparation(&sender, path, &cancel_worker));
            self.status = "Auditando el paquete ya descargado e importando su red…".into();
            self.job = Some(Job {
                child: None,
                cancel,
                events: Mutex::new(events),
            });
            return Ok(());
        }
        let helper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/download_official_content.py");
        let python = std::env::var_os("OPENRAILSRS_PYTHON").unwrap_or("python3".into());
        let mut child = Command::new(python)
            .arg(helper)
            .args(["--package", &self.selected().id()])
            .arg("--destination")
            .arg(destination)
            .arg("--cancel-file")
            .arg(&cancel)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Descarga sin salida de progreso")?;
        let (sender, events) = mpsc::channel();
        let cancel_worker = cancel.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.len() > 256 * 1024 {
                    break;
                }
                let Ok(event) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let complete = event["phase"] == "complete";
                if complete {
                    let Some(path) = event["path"].as_str().map(PathBuf::from) else {
                        break;
                    };
                    send_preparation(&sender, path, &cancel_worker);
                } else if sender.send(event).is_err() {
                    break;
                }
            }
        });
        self.status = format!("Resolviendo {}…", self.selected().name);
        self.job = Some(Job {
            child: Some(child),
            cancel,
            events: Mutex::new(events),
        });
        Ok(())
    }
    pub fn cancel(&mut self) -> Result<(), String> {
        let job = self.job.as_ref().ok_or("No hay una descarga activa")?;
        std::fs::write(&job.cancel, b"cancel").map_err(|e| e.to_string())?;
        self.status = "Cancelando la descarga y limpiando sus archivos temporales…".into();
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        let Some(job) = &mut self.job else {
            return false;
        };
        let mut events = vec![];
        let mut disconnected = false;
        let receiver = job.events.lock().unwrap();
        loop {
            match receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        drop(receiver);
        let mut ready = false;
        let mut done = false;
        for event in events {
            match event["phase"].as_str().unwrap_or("") {
                "download" | "extract" => {
                    let bytes = event["done_bytes"].as_f64().unwrap_or(0.);
                    let total = event["total_bytes"].as_f64();
                    self.status = format!(
                        "{}: {:.1} MiB{}",
                        if event["phase"] == "download" {
                            "Descargando"
                        } else {
                            "Descomprimiendo"
                        },
                        bytes / 1048576.,
                        total.map_or(String::new(), |n| format!(" / {:.1} MiB", n / 1048576.))
                    );
                }
                "audit" => {
                    self.status =
                        "Descarga terminada. Auditando formaciones e importando la red…".into()
                }
                "ready" => {
                    self.status = event["summary"]
                        .as_str()
                        .unwrap_or("Contenido preparado")
                        .into();
                    ready = true;
                    done = true;
                }
                "error" => {
                    self.status = format!(
                        "No se instaló: {}",
                        event["message"].as_str().unwrap_or("Error de descarga")
                    );
                    done = true;
                }
                _ => (),
            }
        }
        if disconnected && !done {
            self.status="El instalador terminó sin completar la preparación. Revisá Python 3 y los permisos de la carpeta de contenido.".into();
            done = true;
        }
        if done {
            self.job = None;
            self.refresh_installed();
        }
        ready
    }
    fn refresh_installed(&mut self) {
        let directory = player_data_dir().join("official-content");
        let Ok(root) = directory.canonicalize() else {
            self.installed.clear();
            return;
        };
        self.installed = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| canonical_inside(&e.path(), &root))
            .filter(|p| canonical_inside(&p.join("openrailsrs-content.json"), p).is_some())
            .collect();
        self.installed.sort();
    }
}

fn send_preparation(sender: &mpsc::Sender<Value>, path: PathBuf, cancel: &Path) {
    let _ = sender.send(serde_json::json!({"phase":"audit"}));
    let event = match prepare_installed(&path, Some(cancel)) {
        Ok(summary) => serde_json::json!({"phase":"ready","path":path,"summary":summary}),
        Err(message) => serde_json::json!({"phase":"error","message":message}),
    };
    let _ = sender.send(event);
}

fn read_manifest(path: &Path) -> Option<Value> {
    if std::fs::metadata(path).ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
fn canonical_inside(path: &Path, root: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().filter(|p| p.starts_with(root))
}

#[derive(Serialize, Deserialize)]
pub struct PreparedRoute {
    pub native: PathBuf,
    pub imported: PathBuf,
    pub activities: Vec<PathBuf>,
}
pub fn prepared_routes() -> Vec<PreparedRoute> {
    discover_prepared(&player_data_dir().join("official-content"))
}
fn discover_prepared(directory: &Path) -> Vec<PreparedRoute> {
    let Ok(directory) = directory.canonicalize() else {
        return vec![];
    };
    let mut result = vec![];
    for package in std::fs::read_dir(&directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| canonical_inside(&e.path(), &directory))
    {
        let Some(manifest) = canonical_inside(&package.join("openrailsrs-prepared.json"), &package)
        else {
            continue;
        };
        let Some(value) = read_manifest(&manifest) else {
            continue;
        };
        let Ok(routes) = serde_json::from_value::<Vec<PreparedRoute>>(value) else {
            continue;
        };
        // Content manifests never authorize paths outside their installation.
        result.extend(routes.into_iter().filter_map(|mut r| {
            r.native = canonical_inside(&r.native, &package)?;
            r.imported = canonical_inside(&r.imported, &package)?;
            canonical_inside(&r.imported.join("track.toml"), &package).filter(|p| p.is_file())?;
            r.activities = r
                .activities
                .iter()
                .map(|a| canonical_inside(a, &package).filter(|p| p.is_file()))
                .collect::<Option<Vec<_>>>()?;
            (r.native.is_dir() && r.imported.is_dir()).then_some(r)
        }));
    }
    result
}
pub fn prepare_installed(directory: &Path, cancel: Option<&Path>) -> Result<String, String> {
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let directory = directory.as_path();
    let check = || {
        if cancel.is_some_and(Path::exists) {
            Err(
                "Preparación cancelada; la descarga se conserva para volver a auditarla"
                    .to_string(),
            )
        } else {
            Ok(())
        }
    };
    check()?;
    let manifest = canonical_inside(&directory.join("openrailsrs-content.json"), directory)
        .and_then(|p| read_manifest(&p))
        .ok_or("Manifiesto ausente, inválido o fuera del paquete")?;
    let mut routes = vec![];
    let mut formations = vec![];
    let mut ready = 0;
    let mut seen_consists = std::collections::HashSet::new();
    for relative in manifest["routes"].as_array().ok_or("Paquete sin rutas")? {
        let native = directory
            .join(relative.as_str().ok_or("Ruta inválida")?)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !native.starts_with(directory) {
            return Err("Ruta fuera del paquete".into());
        }
        let root = native
            .parent()
            .and_then(Path::parent)
            .ok_or("Ruta sin raíz de contenido")?;
        if !root.starts_with(directory) {
            return Err("Raíz de contenido fuera del paquete".into());
        }
        let trains = openrailsrs_formats::resolve_path_case_insensitive(&root.join("TRAINS"))
            .ok_or("Paquete sin TRAINS")?;
        let trainset = openrailsrs_formats::resolve_path_case_insensitive(&trains.join("TRAINSET"))
            .ok_or("Paquete sin TRAINSET")?;
        let consists = openrailsrs_formats::resolve_path_case_insensitive(&trains.join("CONSISTS"))
            .ok_or("Paquete sin CONSISTS")?;
        if [&trains, &trainset, &consists]
            .iter()
            .any(|p| canonical_inside(p, directory).is_none())
        {
            return Err("Material rodante fuera del paquete".into());
        }
        let mut auditor = openrailsrs_train::ConsistAuditor::new(vec![trainset]);
        for file in std::fs::read_dir(consists)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("con")))
        {
            check()?;
            if !seen_consists.insert(file.clone()) {
                continue;
            }
            let audit = auditor.inspect(&file);
            if audit.player_ready() {
                ready += 1;
            }
            formations.push(audit);
        }
        let import_root = directory.join("openrailsrs-import");
        if import_root.exists() && canonical_inside(&import_root, directory).is_none() {
            return Err("Directorio de importación fuera del paquete".into());
        }
        std::fs::create_dir_all(&import_root).map_err(|e| e.to_string())?;
        let imported = import_root.join(native.file_name().ok_or("Ruta sin nombre")?);
        if imported.exists() && canonical_inside(&imported, directory).is_none() {
            return Err("Ruta importada fuera del paquete".into());
        }
        check()?;
        let track = openrailsrs_msts::import_route(&native).map_err(|e| e.to_string())?;
        check()?;
        atomic_write(&imported.join("track.toml"), track.as_bytes())?;
        let activities =
            openrailsrs_formats::resolve_path_case_insensitive(&native.join("ACTIVITIES"))
                .into_iter()
                .flat_map(|p| std::fs::read_dir(p).into_iter().flatten().flatten())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("act")))
                .collect();
        routes.push(PreparedRoute {
            native,
            imported,
            activities,
        });
    }
    atomic_write(
        &directory.join("openrailsrs-audit.json"),
        &serde_json::to_vec_pretty(&formations).map_err(|e| e.to_string())?,
    )?;
    atomic_write(
        &directory.join("openrailsrs-prepared.json"),
        &serde_json::to_vec_pretty(&routes).map_err(|e| e.to_string())?,
    )?;
    Ok(format!(
        "Contenido instalado por separado · {ready}/{} formaciones con recursos completos y tracción · {} actividades disponibles. Revisá las advertencias de compatibilidad al elegir la formación.",
        formations.len(),
        routes.iter().map(|r| r.activities.len()).sum::<usize>()
    ))
}

pub fn open_catalogue() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    command
        .arg("https://www.openrails.org/download/content/")
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> (PathBuf, PreparedRoute) {
        let package = root.join("official-content/package");
        let native = package.join("ROUTES/Route");
        let imported = package.join("openrailsrs-import/Route");
        std::fs::create_dir_all(&native).unwrap();
        std::fs::create_dir_all(&imported).unwrap();
        std::fs::write(imported.join("track.toml"), "graph").unwrap();
        let activity = native.join("test.act");
        std::fs::write(&activity, "activity").unwrap();
        (
            package,
            PreparedRoute {
                native,
                imported,
                activities: vec![activity],
            },
        )
    }
    fn publish(package: &Path, route: &PreparedRoute) {
        std::fs::write(
            package.join("openrailsrs-prepared.json"),
            serde_json::to_vec(&[route]).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn discovery_rejects_parent_traversal_in_every_prepared_path() {
        let temp = tempfile::tempdir().unwrap();
        let (package, mut route) = fixture(temp.path());
        publish(&package, &route);
        let directory = temp.path().join("official-content");
        assert_eq!(discover_prepared(&directory).len(), 1);
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("track.toml"), "outside").unwrap();
        std::fs::write(outside.join("test.act"), "outside").unwrap();
        let escaped = package.join("../../outside");
        let native = route.native.clone();
        route.native = escaped.clone();
        publish(&package, &route);
        assert!(discover_prepared(&directory).is_empty());
        route.native = native;
        let imported = route.imported.clone();
        route.imported = escaped.clone();
        publish(&package, &route);
        assert!(discover_prepared(&directory).is_empty());
        route.imported = imported;
        route.activities = vec![escaped.join("test.act")];
        publish(&package, &route);
        assert!(discover_prepared(&directory).is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn discovery_rejects_linked_track_and_linked_packages() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let (package, route) = fixture(temp.path());
        publish(&package, &route);
        let outside = temp.path().join("outside-track.toml");
        std::fs::write(&outside, "outside").unwrap();
        let track = route.imported.join("track.toml");
        std::fs::remove_file(&track).unwrap();
        symlink(&outside, &track).unwrap();
        let directory = temp.path().join("official-content");
        assert!(discover_prepared(&directory).is_empty());
        std::fs::remove_file(&track).unwrap();
        std::fs::write(&track, "graph").unwrap();
        let relocated = temp.path().join("relocated");
        std::fs::rename(&package, &relocated).unwrap();
        symlink(&relocated, &package).unwrap();
        assert!(discover_prepared(&directory).is_empty());
    }
}
