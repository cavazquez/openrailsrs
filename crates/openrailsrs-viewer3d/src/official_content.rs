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
        let catalog: Catalog =
            serde_json::from_str(openrailsrs_content::CATALOG).expect("bundled official catalogue");
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
    pub fn reaudit(&mut self, index: usize) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una operación de contenido en curso".into());
        }
        let path = self
            .installed
            .get(index)
            .cloned()
            .ok_or("Paquete instalado no disponible")?;
        let cancel = player_data_dir()
            .join("official-content")
            .join(format!(".cancel-{}-audit-{index}", std::process::id()));
        std::fs::create_dir_all(cancel.parent().unwrap()).map_err(|e| e.to_string())?;
        if cancel.exists() {
            std::fs::remove_file(&cancel).map_err(|e| e.to_string())?;
        }
        let (sender, events) = mpsc::channel();
        let cancel_worker = cancel.clone();
        std::thread::spawn(move || send_preparation(&sender, path, &cancel_worker));
        self.status = "Reauditando la copia elegida sin conexión ni descarga…".into();
        self.job = Some(Job {
            child: None,
            cancel,
            events: Mutex::new(events),
        });
        Ok(())
    }
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
        let mut child = openrailsrs_content::installer_command()?
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
        self.installed = content_directories()
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .flat_map(|root| {
                std::fs::read_dir(&root)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|e| canonical_inside(&e.path(), &root))
                    .filter(|p| canonical_inside(&p.join("openrailsrs-content.json"), p).is_some())
                    .collect::<Vec<_>>()
            })
            .collect();
        self.installed.sort();
        self.installed.dedup();
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
    #[serde(default)]
    pub edition: Option<String>,
}

fn edition_label(native: &Path, manifest: &Value) -> String {
    let name = native.file_name().unwrap_or_default().to_string_lossy();
    let id = manifest["revision"]["commit"]
        .as_str()
        .or_else(|| manifest["download_sha256"].as_str())
        .unwrap_or("sin-id")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(8)
        .collect::<String>();
    let date = manifest["source_date"]
        .as_str()
        .or_else(|| manifest["revision"]["published_at"].as_str())
        .filter(|d| {
            d.len() >= 10
                && d.as_bytes()[..10]
                    .iter()
                    .all(|c| c.is_ascii_digit() || *c == b'-')
        });
    match date {
        Some(date) => format!("{name} · origen {} · {id}", &date[..10]),
        None => format!("{name} · descarga {id}"),
    }
}
pub fn installed_label(path: &Path) -> String {
    let Some(m) = read_manifest(&path.join("openrailsrs-content.json")) else {
        return path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    };
    let name = m["package"].as_str().unwrap_or("Contenido");
    edition_label(Path::new(name), &m)
}
pub fn prepared_routes() -> Vec<PreparedRoute> {
    content_directories()
        .iter()
        .flat_map(|p| discover_prepared(p))
        .collect()
}
fn content_directories() -> Vec<PathBuf> {
    std::iter::once(player_data_dir())
        .chain(openrailsrs_content::legacy_data_dirs())
        .map(|p| p.join("official-content"))
        .collect()
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
        let content_manifest =
            canonical_inside(&package.join("openrailsrs-content.json"), &package)
                .and_then(|p| read_manifest(&p))
                .unwrap_or(Value::Null);
        // Content manifests never authorize paths outside their installation.
        result.extend(routes.into_iter().filter_map(|mut r| {
            r.native = canonical_inside(&r.native, &package)?;
            r.edition = Some(edition_label(&r.native, &content_manifest));
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
            edition: Some(edition_label(&native, &manifest)),
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
    open_source_url("https://www.openrails.org/download/content/")
}
pub fn open_source_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://github.com/")
        && !url.starts_with("https://vapor3d.punchinout.net/")
        && !url.starts_with("https://www.openrails.org/")
    {
        return Err("El origen no es uno de los autores verificados".into());
    }
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
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[derive(Clone)]
pub struct MissingContentSource {
    pub title: String,
    pub page: String,
    pub download_page: Option<String>,
    pub note: String,
    pub package_id: Option<String>,
}
pub fn missing_source(
    menu: &crate::player_launch::PlayerLaunchMenu,
) -> Option<MissingContentSource> {
    let consist = menu.consists.get(menu.consist)?;
    let audit = menu.consist_audits.get(consist)?;
    source_for_missing(
        consist,
        &audit.errors,
        menu.current().and_then(|s| s.scenery_root.as_deref()),
    )
}
fn source_for_missing(
    consist: &Path,
    errors: &[String],
    native: Option<&Path>,
) -> Option<MissingContentSource> {
    if errors.is_empty() {
        return None;
    }
    let hint = format!("{} {}", consist.display(), errors.join(" ")).to_ascii_lowercase();
    if hint.contains("caf6000") || hint.contains("caf_6000") {
        return Some(MissingContentSource {
            title:"CAF 6000 · Vapor3D / A. Asensio".into(),
            page:"https://vapor3d.punchinout.net/s6000.html".into(),
            download_page:Some("https://vapor3d.punchinout.net/downloads/downloads_s6000.html".into()),
            note:"El autor ofrece el modelo gratuito en RAR (22,7 MB), con cabina 2D. La cabina 3D es opcional de pago. Los nombres CAF del ejemplo Mitre son marcadores: este paquete es una alternativa, no una coincidencia de archivos verificada. Conservá los nombres del autor y elegí su formación después de instalarla.".into(),
            package_id:None,
        });
    }
    let native = native?;
    let manifest = native
        .ancestors()
        .find_map(|p| read_manifest(&p.join("openrailsrs-content.json")))?;
    let repo = manifest["revision"]["repository"].as_str()?;
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
    {
        return None;
    }
    let detail = errors
        .iter()
        .find_map(|s| s.find("Falta").map(|i| &s[i..]))
        .unwrap_or(&errors[0]);
    let file = detail
        .split_whitespace()
        .map(|s| s.trim_matches(['(', ')', '"', '\'', ':', ',']))
        .find(|s| {
            [".eng", ".wag", ".s", ".ace", ".cvf", ".sms", ".wav", ".inc"]
                .iter()
                .any(|ext| s.to_ascii_lowercase().ends_with(ext))
        })
        .unwrap_or("")
        .replace('\\', "/");
    let basename = file.rsplit('/').next().unwrap_or("");
    let query = basename
        .as_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"._-".contains(b) {
                (*b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect::<String>();
    Some(MissingContentSource {
        title:format!("Repositorio original · {repo}"),
        page:format!("https://github.com/{repo}/search?q={query}&type=code"),
        download_page:None,
        note:"Buscá el archivo por su nombre en el repositorio del autor. Actualizar el paquete conserva las copias anteriores; se audita antes de ofrecer sus actividades.".into(),
        package_id:serde_json::from_str::<Catalog>(openrailsrs_content::CATALOG).ok()?.routes.iter().find(|p|p.automatic() && manifest["advertised_url"].as_str()==Some(p.url.as_str())).map(OfficialPackage::id),
    })
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
                edition: None,
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
    #[test]
    fn editions_remain_distinct_with_author_date_and_identifier() {
        let native = Path::new("ROUTES/Chiltern");
        let old = serde_json::json!({"download_sha256":"11111111aabbccdd"});
        let new = serde_json::json!({"source_date":"2026-10-04T14:00:00Z", "revision":{"commit":"22222222aabbccdd"}});
        assert_eq!(edition_label(native, &old), "Chiltern · descarga 11111111");
        assert_eq!(
            edition_label(native, &new),
            "Chiltern · origen 2026-10-04 · 22222222"
        );
        assert_ne!(edition_label(native, &old), edition_label(native, &new));
        let temp = tempfile::tempdir().unwrap();
        let (package, route) = fixture(temp.path());
        publish(&package, &route);
        std::fs::write(package.join("openrailsrs-content.json"), old.to_string()).unwrap();
        let second = temp.path().join("official-content/other");
        let mut other = PreparedRoute {
            native: second.join("ROUTES/Chiltern"),
            imported: second.join("openrailsrs-import/Chiltern"),
            activities: vec![],
            edition: None,
        };
        std::fs::create_dir_all(&other.native).unwrap();
        std::fs::create_dir_all(&other.imported).unwrap();
        std::fs::write(other.imported.join("track.toml"), "graph").unwrap();
        std::fs::write(second.join("openrailsrs-content.json"), new.to_string()).unwrap();
        // A legacy prepared manifest has no edition field; derive it at discovery.
        other.edition = None;
        publish(&second, &other);
        let found = discover_prepared(&temp.path().join("official-content"));
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|r| r.edition.is_some()));
    }
    #[test]
    fn missing_sources_do_not_claim_caf_placeholders_are_a_verified_match() {
        let source = source_for_missing(
            Path::new("caf6000.con"),
            &["Falta caf6000_motor.s".into()],
            None,
        )
        .unwrap();
        assert!(source.page.starts_with("https://vapor3d.punchinout.net/"));
        assert!(source.note.contains("no una coincidencia"));
        assert!(source.package_id.is_none());
        assert!(source_for_missing(Path::new("caf6000.con"), &[], None).is_none());
    }
    #[test]
    fn repository_search_sends_only_basename_and_rejects_invalid_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let native = root.join("ROUTES/Chiltern");
        std::fs::create_dir_all(&native).unwrap();
        let manifest = root.join("openrailsrs-content.json");
        std::fs::write(
            &manifest,
            serde_json::json!({"revision":{"repository":"author/route"}}).to_string(),
        )
        .unwrap();
        let source = source_for_missing(
            Path::new("test.con"),
            &["Vehículo 1 (existing.eng): Falta /private/user/locomotive.s".into()],
            Some(&native),
        )
        .unwrap();
        assert_eq!(
            source.page,
            "https://github.com/author/route/search?q=locomotive.s&type=code"
        );
        std::fs::write(
            manifest,
            serde_json::json!({"revision":{"repository":"../evil"}}).to_string(),
        )
        .unwrap();
        assert!(
            source_for_missing(
                Path::new("test.con"),
                &["Falta car.s".into()],
                Some(&native)
            )
            .is_none()
        );
    }
}
