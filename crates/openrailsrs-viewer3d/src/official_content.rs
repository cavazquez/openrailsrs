//! Cached official catalogue, background downloads and installed-content audit.
use crate::player_settings::{atomic_write, player_data_dir};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, mpsc},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContentAuthor {
    pub name: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PreviousContentSource {
    pub name: String,
    pub url: String,
    #[serde(rename = "softwareVersion", default)]
    pub content_version: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OfficialPackage {
    pub name: String,
    pub author: ContentAuthor,
    pub compensation: String,
    pub url: String,
    #[serde(rename = "softwareVersion", default)]
    pub content_version: Option<String>,
    #[serde(rename = "previousSources", default)]
    pub previous_sources: Vec<PreviousContentSource>,
    #[serde(rename = "downloadSize", default)]
    pub download_bytes: u64,
    #[serde(rename = "installSize", default)]
    pub install_bytes: u64,
    #[serde(default)]
    pub original_confirmed: bool,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub requested_ref: Option<String>,
    #[serde(default)]
    pub package_type: Option<String>,
    #[serde(default)]
    pub resolved_commit: Option<String>,
    #[serde(default)]
    pub source_date: Option<String>,
    #[serde(default)]
    pub input_url: Option<String>,
}
impl OfficialPackage {
    fn matches_manifest(&self, manifest: &Value) -> bool {
        let matches = |name: &str, url: &str| {
            manifest["package"].as_str() == Some(name)
                && manifest["advertised_url"].as_str() == Some(url)
        };
        matches(&self.name, &self.url)
            || self
                .previous_sources
                .iter()
                .any(|source| matches(&source.name, &source.url))
    }
    fn version_for_manifest(&self, manifest: &Value) -> Option<&str> {
        if !self.matches_manifest(manifest) {
            return None;
        }
        let repository = manifest["revision"]["repository"].as_str()?;
        let matches = |url: &str| {
            url.strip_prefix("https://github.com/")
                .and_then(|path| path.strip_suffix(".git"))
                == Some(repository)
        };
        if matches(&self.url) {
            self.content_version.as_deref()
        } else {
            self.previous_sources
                .iter()
                .find(|source| matches(&source.url))
                .and_then(|source| source.content_version.as_deref())
        }
    }
    pub fn id(&self) -> String {
        if self.original_confirmed || self.requires_confirmation {
            return format!(
                "url-github-{:x}",
                Sha256::digest(self.url.to_ascii_lowercase().as_bytes())
            );
        }
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
fn valid_user_source(package: &OfficialPackage) -> bool {
    let Some(repo) = package
        .url
        .strip_prefix("https://github.com/")
        .and_then(|r| r.strip_suffix(".git"))
    else {
        return false;
    };
    let parts = repo.split('/').collect::<Vec<_>>();
    package.original_confirmed
        && package.compensation == "free"
        && package.name == repo
        && parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && ![".", ".."].contains(p)
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
}
struct Job {
    download: Option<u64>,
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
    pub queue: crate::download_queue::Queue,
    pub packages: Vec<OfficialPackage>,
    pub selected: usize,
    pub status: String,
    pub installed: Vec<PathBuf>,
    job: Option<Job>,
    pub url_preview: Option<OfficialPackage>,
    pub url_status: String,
    url_probe: Option<Mutex<mpsc::Receiver<Result<OfficialPackage, String>>>>,
}
impl Default for OfficialContent {
    fn default() -> Self {
        let catalog: Catalog =
            serde_json::from_str(openrailsrs_content::CATALOG).expect("bundled official catalogue");
        let mut packages = catalog.routes;
        let source_file = player_data_dir().join("original-url-sources.json");
        if std::fs::metadata(&source_file).is_ok_and(|m| m.len() <= 128 * 1024)
            && let Ok(bytes) = std::fs::read(source_file)
            && let Ok(extra) = serde_json::from_slice::<Vec<OfficialPackage>>(&bytes)
        {
            for package in extra.into_iter().take(32) {
                if valid_user_source(&package) && !packages.iter().any(|p| p.id() == package.id()) {
                    packages.push(package);
                }
            }
        }
        let selected = packages
            .iter()
            .position(|p| p.name == "Demo Model 1")
            .unwrap_or(0);
        let queue = crate::download_queue::Queue::load(
            &player_data_dir(),
            &packages.iter().map(OfficialPackage::id).collect::<Vec<_>>(),
        );
        let mut resource=Self {queue,packages,selected,status:"Elegí un paquete. Se instala por separado y se audita antes de ofrecer sus actividades.".into(),installed:vec![],job:None,url_preview:None,url_status:String::new(),url_probe:None};
        resource.refresh_installed();
        resource
    }
}
impl OfficialContent {
    pub fn inspect_url(&mut self, url: &str) -> Result<(), String> {
        if self.url_probe.is_some() {
            return Err("Resolviendo el enlace anterior".into());
        }
        if url.len() > 2048 {
            return Err("El enlace es demasiado largo".into());
        }
        self.url_preview = None;
        self.url_status = "Comprobando el enlace sin descargar…".into();
        let url = url.trim().to_owned();
        let (sender, receiver) = mpsc::channel();
        self.url_probe = Some(Mutex::new(receiver));
        std::thread::spawn(move || {
            let result = (|| {
                let output = openrailsrs_content::installer_command()?
                    .args(["--inspect-url", "--url", &url])
                    .output()
                    .map_err(|e| e.to_string())?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .take(2048)
                        .collect());
                }
                serde_json::from_slice(&output.stdout)
                    .map_err(|e| format!("Respuesta de origen inválida: {e}"))
            })();
            let _ = sender.send(result);
        });
        Ok(())
    }
    pub fn poll_url(&mut self) -> bool {
        let Some(probe) = &self.url_probe else {
            return false;
        };
        let Ok(probe) = probe.try_lock() else {
            return false;
        };
        let reply = match probe.try_recv() {
            Ok(reply) => reply,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("La comprobación terminó sin respuesta".into())
            }
        };
        drop(probe);
        self.url_probe = None;
        match reply {
            Ok(package) => {
                self.url_status = if package.requires_confirmation {
                    "Origen fuera del catálogo: verificá que sea el repositorio original del autor antes de confirmar.".into()
                } else {
                    "Origen reconocido en el catálogo. Revisá los datos antes de instalar.".into()
                };
                self.url_preview = Some(package);
            }
            Err(error) => self.url_status = error,
        }
        true
    }
    pub fn install_url(&mut self, confirm: bool) -> Result<(), String> {
        let mut package = self
            .url_preview
            .clone()
            .ok_or("Comprobá un enlace primero")?;
        if package.requires_confirmation && !confirm {
            return Err("Confirmá que este es el repositorio original del autor".into());
        }
        if package.requires_confirmation {
            package.original_confirmed = true;
            package.requires_confirmation = false;
            if !valid_user_source(&package) {
                return Err("El origen no es un repositorio admitido".into());
            }
        }
        if let Some(index) = self.packages.iter().position(|p| p.id() == package.id()) {
            if !self.packages[index].url.eq_ignore_ascii_case(&package.url) {
                return Err(
                    "El identificador ya pertenece a otro origen; la cola se conserva".into(),
                );
            }
            if self
                .queue
                .entries
                .iter()
                .any(|entry| entry.package == package.id() && entry.phase.pending())
            {
                return Err(
                    "Este origen ya está en la cola; se conserva la revisión solicitada".into(),
                );
            }
            self.selected = index;
            self.packages[index] = package;
        } else {
            if self
                .packages
                .iter()
                .filter(|p| p.original_confirmed)
                .count()
                >= 32
            {
                return Err("Se admiten hasta 32 orígenes agregados por URL".into());
            }
            self.selected = self.packages.len();
            self.packages.push(package);
            let extra: Vec<_> = self
                .packages
                .iter()
                .filter(|p| p.original_confirmed)
                .collect();
            atomic_write(
                &player_data_dir().join("original-url-sources.json"),
                &serde_json::to_vec_pretty(&extra).map_err(|e| e.to_string())?,
            )?;
        }
        self.start()
    }
    pub fn reaudit(&mut self, index: usize) -> Result<(), String> {
        if self.busy() {
            return Err("Ya hay una operación de contenido en curso".into());
        }
        let path = self
            .installed
            .get(index)
            .cloned()
            .ok_or("Paquete instalado no disponible")?;
        let cancel = openrailsrs_content::content_dir()
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
            download: None,
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
        if !self.selected().automatic() {
            return Err("Este paquete requiere visitar la página de su autor".into());
        }
        self.queue.enqueue(self.selected().id())?;
        self.queue.held = false;
        self.status = "Descarga agregada a la cola. Cada versión se instala por separado.".into();
        Ok(())
    }
    pub fn cancel_job(&mut self, id: u64) -> Result<(), String> {
        if self.job.as_ref().is_some_and(|j| j.download == Some(id)) {
            self.cancel()
        } else {
            self.queue.cancel_waiting(id)
        }
    }
    pub fn resume_queue(&mut self) {
        self.queue.held = false;
    }
    fn start_download(&mut self, index: usize, id: u64) -> Result<(), String> {
        let package = self
            .packages
            .get(index)
            .cloned()
            .ok_or("Paquete original no disponible")?;
        if self.busy() {
            return Err("Ya hay una descarga en curso".into());
        }
        if !package.automatic() {
            return Err("Este paquete requiere visitar la página de su autor".into());
        }
        let destination = openrailsrs_content::content_dir();
        std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
        let cancel = destination.join(format!(
            ".cancel-{}-{}-{id}",
            std::process::id(),
            package.id()
        ));
        if cancel.exists() {
            std::fs::remove_file(&cancel).map_err(|e| e.to_string())?;
        }
        let mut command = openrailsrs_content::installer_command()?;
        if let Some(url) = &package.input_url {
            command.args(["--url", url]);
            if package.original_confirmed {
                command.arg("--confirm-original");
            }
        } else {
            command.args(["--package", &package.id()]);
        }
        if let Some(reference) = &package.requested_ref {
            command.args(["--revision", reference]);
        }
        let mut child = command
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
        self.status = format!("Resolviendo {}…", package.name);
        self.job = Some(Job {
            download: Some(id),
            child: Some(child),
            cancel,
            events: Mutex::new(events),
        });
        self.queue
            .update(id, crate::download_queue::Phase::Running, &self.status);
        let _ = self.queue.persist();
        Ok(())
    }
    pub fn cancel(&mut self) -> Result<(), String> {
        let job = self.job.as_ref().ok_or("No hay una descarga activa")?;
        let status = "Cancelando la descarga y limpiando sus archivos temporales…";
        if let Some(id) = job.download {
            self.queue.request_cancel(id, status)?;
        }
        std::fs::write(&job.cancel, b"cancel").map_err(|e| e.to_string())?;
        self.status = status.into();
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        if self.job.is_none()
            && let Some((id, package)) = self.queue.next_job()
        {
            let result = self
                .packages
                .iter()
                .position(|p| p.id() == package)
                .ok_or("El paquete ya no está en el catálogo original".to_string())
                .and_then(|index| self.start_download(index, id));
            if let Err(error) = result {
                self.queue
                    .update(id, crate::download_queue::Phase::Failed, &error);
                self.status = error;
                let _ = self.queue.persist();
            }
        }
        let Some(job) = &mut self.job else {
            return false;
        };
        let download = job.download;
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
                    let bytes = event["done_bytes"].as_u64().unwrap_or(0);
                    let total = event["total_bytes"].as_u64();
                    if let Some(id) = download {
                        self.queue.progress(
                            id,
                            event["done_bytes"].as_u64().unwrap_or(0),
                            event["total_bytes"].as_u64(),
                        );
                    }
                    self.status = format!(
                        "{}: {}",
                        if event["phase"] == "download" {
                            "Descargando"
                        } else {
                            "Descomprimiendo"
                        },
                        crate::download_queue::progress_label(bytes, total)
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
        if let Some(id) = download {
            let cancelling =
                self.queue.entries.iter().any(|e| {
                    e.id == id && e.phase == crate::download_queue::Phase::CancelRequested
                });
            let phase = if ready {
                crate::download_queue::Phase::Ready
            } else if done {
                if cancelling {
                    crate::download_queue::Phase::Cancelled
                } else {
                    crate::download_queue::Phase::Failed
                }
            } else if cancelling {
                crate::download_queue::Phase::CancelRequested
            } else {
                crate::download_queue::Phase::Running
            };
            self.queue.update(id, phase, &self.status);
            if done {
                let _ = self.queue.persist();
            }
        }
        if done {
            self.job = None;
            self.refresh_installed();
        }
        ready
    }
    pub(crate) fn refresh_installed(&mut self) {
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
    // Only the bundled catalogue may add a version label. A local manifest
    // supplies the installed identity, never a new source or trusted version.
    let catalog: Catalog =
        serde_json::from_str(openrailsrs_content::CATALOG).expect("bundled official catalogue");
    let package = catalog
        .routes
        .iter()
        .find(|package| package.matches_manifest(manifest));
    let version = package.and_then(|package| package.version_for_manifest(manifest));
    let name = match version {
        Some(version) if !name.ends_with(&format!(" {version}")) => format!("{name} {version}"),
        Some(_) => name.into_owned(),
        None => package
            .and_then(|package| package.content_version.as_deref())
            .and_then(|version| name.strip_suffix(&format!(" {version}")))
            .unwrap_or(&name)
            .to_string(),
    };
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
fn package_edition_label(manifest: &Value) -> String {
    let name = manifest["package"]
        .as_str()
        .and_then(|name| name.rsplit(['/', '\\']).next())
        .map(str::trim)
        .filter(|name| {
            !name.is_empty() && !matches!(*name, "." | "..") && !name.chars().any(char::is_control)
        })
        .unwrap_or("Contenido");
    edition_label(Path::new(name), manifest)
}
pub fn installed_label(path: &Path) -> String {
    let Some(m) = read_manifest(&path.join("openrailsrs-content.json")) else {
        return path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    };
    package_edition_label(&m)
}
pub fn prepared_routes() -> Vec<PreparedRoute> {
    content_directories()
        .iter()
        .flat_map(|p| discover_prepared(p))
        .collect()
}
/// Bind prepared examples only to the verified author's installed route. An
/// unrelated package reusing the Chiltern folder name is never selected.
pub fn prepared_author_route(id: &str, name: &str) -> Option<PathBuf> {
    let catalog: Catalog = serde_json::from_str(openrailsrs_content::CATALOG).ok()?;
    let author = catalog.routes.iter().find(|p| p.id() == id)?;
    let mut copies = prepared_routes()
        .into_iter()
        .filter_map(|route| {
            if !route.native.file_name()?.eq_ignore_ascii_case(name)
                || !route.native.join("WORLD").is_dir()
            {
                return None;
            }
            let manifest = route
                .native
                .ancestors()
                .find_map(|p| read_manifest(&p.join("openrailsrs-content.json")))?;
            author.matches_manifest(&manifest).then(|| {
                (
                    manifest["source_date"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    route.native,
                )
            })
        })
        .collect::<Vec<_>>();
    copies.sort();
    copies.pop().map(|(_, p)| p)
}
/// Stock belongs to its author package even if importing the map has failed.
/// Reuse the same confined manifest paths as route preparation.
pub(crate) fn installed_stock_sources() -> Vec<(PathBuf, String)> {
    content_directories()
        .iter()
        .flat_map(|root| discover_stock_sources(root))
        .collect()
}
fn discover_stock_sources(root: &Path) -> Vec<(PathBuf, String)> {
    let Ok(root) = root.canonicalize() else {
        return vec![];
    };
    let mut sources = vec![];
    for package in std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| canonical_inside(&entry.path(), &root))
    {
        let Some(manifest) = canonical_inside(&package.join("openrailsrs-content.json"), &package)
            .and_then(|path| read_manifest(&path))
        else {
            continue;
        };
        for route in manifest["routes"]
            .as_array()
            .into_iter()
            .flatten()
            .take(2048)
        {
            let Some(native) = route
                .as_str()
                .and_then(|name| canonical_inside(&package.join(name), &package))
            else {
                continue;
            };
            let Some(content) = native
                .parent()
                .and_then(Path::parent)
                .filter(|path| path.starts_with(&package))
            else {
                continue;
            };
            let Some(consists) = openrailsrs_formats::resolve_path_case_insensitive(
                &content.join("TRAINS/CONSISTS"),
            )
            .and_then(|path| canonical_inside(&path, &package))
            .filter(|path| path.is_dir()) else {
                continue;
            };
            if !sources.iter().any(|(path, _)| path == &consists) {
                sources.push((consists, edition_label(&native, &manifest)));
            }
        }
        for stock in manifest["stock_files"]
            .as_array()
            .into_iter()
            .flatten()
            .take(20000)
        {
            let Some(path) = stock
                .as_str()
                .and_then(|s| canonical_inside(&package.join(s), &package))
            else {
                continue;
            };
            let Some(trainset) = path.ancestors().find(|p| {
                p.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("TRAINSET"))
            }) else {
                continue;
            };
            let Some(trains) = trainset.parent().filter(|p| p.starts_with(&package)) else {
                continue;
            };
            let Some(consists) =
                openrailsrs_formats::resolve_path_case_insensitive(&trains.join("CONSISTS"))
                    .and_then(|p| canonical_inside(&p, &package))
                    .filter(|p| p.is_dir())
            else {
                continue;
            };
            if !sources.iter().any(|(p, _)| p == &consists) {
                sources.push((consists, package_edition_label(&manifest)));
            }
        }
        if let Some(standalone) =
            canonical_inside(&package.join("openrailsrs-import/standalone"), &package)
                .filter(|p| p.is_dir())
        {
            sources.push((standalone, package_edition_label(&manifest)));
        }
    }
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    sources
}
fn content_directories() -> Vec<PathBuf> {
    let mut result = openrailsrs_content::content_roots(&player_data_dir());
    result.extend(
        openrailsrs_content::legacy_data_dirs()
            .into_iter()
            .map(|p| p.join("official-content")),
    );
    result
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
        // Route-only author packs can use the player's separate stock library.
        // Missing trains are reported by formation audits, not a route ban.
        let trains = openrailsrs_formats::resolve_path_case_insensitive(&root.join("TRAINS"))
            .unwrap_or_else(|| root.join("TRAINS"));
        let trainset = openrailsrs_formats::resolve_path_case_insensitive(&trains.join("TRAINSET"))
            .unwrap_or_else(|| trains.join("TRAINSET"));
        let consists = openrailsrs_formats::resolve_path_case_insensitive(&trains.join("CONSISTS"))
            .unwrap_or_else(|| trains.join("CONSISTS"));
        if [&trains, &trainset, &consists]
            .iter()
            .any(|p| p.exists() && canonical_inside(p, directory).is_none())
        {
            return Err("Material rodante fuera del paquete".into());
        }
        let mut auditor = openrailsrs_train::ConsistAuditor::new(vec![trainset]);
        for file in std::fs::read_dir(consists)
            .into_iter()
            .flatten()
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
    if routes.is_empty() {
        let stock = manifest["stock_files"]
            .as_array()
            .ok_or("El paquete no tiene rutas ni lista de vehículos")?;
        if stock.is_empty() || stock.len() > 20000 {
            return Err("Lista de vehículos vacía o demasiado grande".into());
        }
        let import = directory.join("openrailsrs-import");
        if import.exists() && canonical_inside(&import, directory).is_none() {
            return Err("Importación fuera del paquete".into());
        }
        let standalone = import.join("standalone");
        if standalone.exists() && canonical_inside(&standalone, directory).is_none() {
            return Err("Formaciones fuera del paquete".into());
        }
        std::fs::create_dir_all(&standalone).map_err(|e| e.to_string())?;
        let stock_paths = stock
            .iter()
            .map(|relative| {
                relative
                    .as_str()
                    .and_then(|name| canonical_inside(&directory.join(name), directory))
                    .filter(|path| path.is_file())
                    .ok_or("Vehículo ausente o fuera del paquete".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut trainsets = Vec::new();
        for path in &stock_paths {
            if let Some(root) = path.ancestors().find(|path| {
                path.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("TRAINSET"))
            }) && !trainsets.contains(&root.to_path_buf())
            {
                trainsets.push(root.to_path_buf());
            }
        }
        let mut auditor = openrailsrs_train::ConsistAuditor::new(trainsets.clone());
        // A train-only archive may already supply complete author formations.
        // Audit those unchanged; generated one-car choices remain additional
        // exploration options and never replace the author's composition.
        for trainset in &trainsets {
            let Some(consists) = trainset
                .parent()
                .and_then(|root| {
                    openrailsrs_formats::resolve_path_case_insensitive(&root.join("CONSISTS"))
                })
                .and_then(|path| canonical_inside(&path, directory))
            else {
                continue;
            };
            for file in std::fs::read_dir(consists)
                .into_iter()
                .flatten()
                .flatten()
                .take(4096)
            {
                check()?;
                let Some(file) = canonical_inside(&file.path(), directory).filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("con"))
                }) else {
                    continue;
                };
                if seen_consists.insert(file.clone()) {
                    let audit = auditor.inspect(&file);
                    ready += usize::from(audit.player_ready());
                    formations.push(audit);
                }
            }
        }
        for (index, path) in stock_paths.iter().enumerate() {
            check()?;
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            let kind = if extension.eq_ignore_ascii_case("eng") {
                "Engine"
            } else if extension.eq_ignore_ascii_case("wag") {
                "Wagon"
            } else {
                return Err("El archivo no es ENG/WAG".into());
            };
            let con = standalone.join(format!("vehicle-{index}.con"));
            let escaped =
                serde_json::to_string(&path.to_string_lossy()).map_err(|e| e.to_string())?;
            atomic_write(&con, format!("(Train ({kind} {escaped}))\n").as_bytes())?;
            let audit = auditor.inspect(&con);
            ready += usize::from(audit.player_ready());
            formations.push(audit);
        }
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
pub fn open_folder(path: &Path) -> Result<(), String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if !path.is_dir() {
        return Err("La carpeta no existe".into());
    }
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer");
    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
pub fn diagnostic_text(menu: &crate::player_launch::PlayerLaunchMenu) -> String {
    let mut lines = vec!["Diagnóstico de contenido openrailsrs".to_string()];
    if let Some(service) = menu.current() {
        lines.push(format!(
            "Escenario: {}\nServicio: {}",
            service
                .scenery_root
                .as_deref()
                .unwrap_or(&service.route_dir)
                .display(),
            service.source.display()
        ));
    }
    if let Some(consist) = menu.consists.get(menu.consist) {
        lines.push(format!("Formación: {}", consist.display()));
        if let Some(audit) = menu.consist_audits.get(consist) {
            for missing in &audit.missing_resources {
                lines.push(missing.guidance());
            }
        }
    }
    lines.push("Obtené los recursos únicamente de su origen original. Después de copiarlos, reauditá la formación.".into());
    lines.join("\n\n")
}
pub fn open_source_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://github.com/") && !url.starts_with("https://www.openrails.org/") {
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
    pub note: String,
    pub package_id: String,
}
pub fn missing_source(
    menu: &crate::player_launch::PlayerLaunchMenu,
) -> Option<MissingContentSource> {
    let consist = menu.consists.get(menu.consist)?;
    let audit = menu.consist_audits.get(consist)?;
    audit
        .missing_resources
        .iter()
        .filter(|r| r.required)
        .find_map(source_for_missing)
}
fn source_for_missing(
    missing: &openrailsrs_train::MissingResource,
) -> Option<MissingContentSource> {
    let catalog: Catalog = serde_json::from_str(openrailsrs_content::CATALOG).ok()?;
    // Use the origin of the actual resource/formation, never the selected
    // scenery or a similarly named model from another author. The immutable
    // catalogue supplies the URL: a downloaded manifest cannot redirect this
    // action through its revision.repository or download_url fields.
    let package = missing
        .destinations
        .iter()
        .chain(std::iter::once(&missing.referenced_by))
        .find_map(|path| {
            let manifest = path
                .ancestors()
                .find_map(|p| read_manifest(&p.join("openrailsrs-content.json")))?;
            catalog.routes.iter().find(|package| {
                package.automatic()
                    && package.url.starts_with("https://github.com/")
                    && package.url.ends_with(".git")
                    && package.matches_manifest(&manifest)
            })
        })
        .or_else(|| {
            let stock = missing.referenced_by.parent()?.file_name()?;
            (stock == "RF_Blue_Pullman"
                && openrailsrs_formats::resource_links::bundled_stock_root(&missing.referenced_by)
                    .is_some())
            .then(|| catalog.routes.iter().find(|p| p.id() == "chiltern-v4"))
            .flatten()
        })?;
    let repo = package
        .url
        .strip_prefix("https://github.com/")?
        .strip_suffix(".git")?;
    let normalized = missing.name.replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or("");
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
        title: format!("Repositorio original de {basename} · {repo}"),
        page: format!("https://github.com/{repo}/search?q={query}&type=code"),
        note: "La búsqueda y actualización usan exclusivamente el repositorio original del catálogo. Se conserva la copia anterior y se audita la nueva. Si el archivo no está allí, se informa el faltante y su destino.".into(),
        package_id: package.id(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_cancellation_is_not_sent_when_queue_persistence_fails() {
        use crate::download_queue::{Phase, Queue};
        let tmp = tempfile::tempdir().unwrap();
        let allowed = ["route".into()];
        let mut queue = Queue::load(tmp.path(), &allowed);
        let id = queue.enqueue("route".into()).unwrap();
        queue.update(id, Phase::Running, "Descargando");
        queue.persist().unwrap();
        let entries = queue.entries.clone();
        let (_sender, events) = mpsc::channel();
        let cancel = tmp.path().join("cancel-request");
        let mut content = OfficialContent {
            queue,
            packages: vec![],
            selected: 0,
            status: "Anterior".into(),
            installed: vec![],
            job: Some(Job {
                download: Some(id),
                child: None,
                cancel: cancel.clone(),
                events: Mutex::new(events),
            }),
            url_preview: None,
            url_status: String::new(),
            url_probe: None,
        };
        let path = tmp.path().join("download-queue.json");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(content.cancel_job(id).is_err());
        assert_eq!(content.queue.entries, entries);
        assert_eq!(content.status, "Anterior");
        assert!(!cancel.exists());
        std::fs::remove_dir(&path).unwrap();
        content.cancel_job(id).unwrap();
        assert!(cancel.is_file());
        assert_eq!(content.queue.entries[0].phase, Phase::CancelRequested);
        assert_eq!(
            Queue::load(tmp.path(), &allowed).entries[0].phase,
            Phase::Cancelled
        );
    }

    fn user_origin(url: &str) -> OfficialPackage {
        serde_json::from_value(serde_json::json!({
            "name": url.strip_prefix("https://github.com/").unwrap().trim_end_matches(".git"),
            "author": {"name": "Original"}, "compensation": "free", "url": url,
            "original_confirmed": true
        }))
        .unwrap()
    }

    #[test]
    fn url_origin_ids_are_distinct_and_match_the_installer() {
        let catalog: Catalog = serde_json::from_str(openrailsrs_content::CATALOG).unwrap();
        let urls = [
            "https://github.com/demo/model-1.git",
            "https://github.com/a-b/c.git",
            "https://github.com/a/b-c.git",
        ];
        let ids: Vec<_> = urls.iter().map(|url| user_origin(url).id()).collect();
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert!(
            ids.iter()
                .all(|id| !catalog.routes.iter().any(|p| p.id() == *id))
        );
        assert_eq!(user_origin("https://github.com/A-B/C.git").id(), ids[1]);
        assert_eq!(
            ids[1],
            format!("url-github-{:x}", Sha256::digest(urls[1].as_bytes()))
        );
    }

    #[test]
    fn mismatched_origin_cannot_replace_a_catalogue_package_or_its_queued_job() {
        let catalog: Catalog = serde_json::from_str(openrailsrs_content::CATALOG).unwrap();
        let package = catalog.routes.into_iter().find(|p| p.automatic()).unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut queue = crate::download_queue::Queue::load(data.path(), &[package.id()]);
        queue.enqueue(package.id()).unwrap();
        let mut forged = package.clone();
        forged.url = "https://github.com/Another/Origin.git".into();
        let mut content = OfficialContent {
            packages: vec![package.clone()],
            selected: 0,
            queue,
            status: String::new(),
            installed: vec![],
            job: None,
            url_preview: Some(forged),
            url_status: String::new(),
            url_probe: None,
        };
        assert!(
            content
                .install_url(true)
                .unwrap_err()
                .contains("otro origen")
        );
        assert_eq!(content.packages[0].url, package.url);
        assert_eq!(content.queue.entries.len(), 1);
        assert_eq!(content.queue.entries[0].package, package.id());
        let pending: serde_json::Value = serde_json::from_slice(
            &std::fs::read(data.path().join("download-queue.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(pending["entries"][0]["package"], package.id());
        let mut same_origin = package.clone();
        same_origin.requested_ref = Some("another-version".into());
        content.url_preview = Some(same_origin);
        assert!(
            content
                .install_url(true)
                .unwrap_err()
                .contains("revisión solicitada")
        );
        assert_eq!(content.packages[0].requested_ref, package.requested_ref);
        assert_eq!(content.queue.entries.len(), 1);
    }
    #[test]
    fn original_stock_is_discovered_without_a_prepared_route_and_confined_to_its_package() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("official-content");
        let package = root.join("author");
        let native = package.join("Routes/Map");
        let consists = package.join("Trains/Consists");
        std::fs::create_dir_all(&native).unwrap();
        std::fs::create_dir_all(&consists).unwrap();
        std::fs::write(
            package.join("openrailsrs-content.json"),
            r#"{"package":"Author route","routes":["Routes/Map","../another-package/Routes/Map"]}"#,
        )
        .unwrap();
        let found = discover_stock_sources(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, consists);
        assert!(!package.join("openrailsrs-prepared.json").exists());
        #[cfg(unix)]
        {
            let alias = temp.path().join("library-alias");
            std::os::unix::fs::symlink(&root, &alias).unwrap();
            assert_eq!(discover_stock_sources(&alias), found);
            let outside = temp.path().join("outside");
            std::fs::create_dir(&outside).unwrap();
            std::fs::remove_dir(&consists).unwrap();
            std::os::unix::fs::symlink(&outside, &consists).unwrap();
            assert!(discover_stock_sources(&root).is_empty());
        }
    }
    #[test]
    fn route_only_author_package_can_use_the_players_separate_trains() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path();
        let native = package.join("ROUTES/Test");
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(
            package.join("openrailsrs-content.json"),
            r#"{"package":"Route only","routes":["ROUTES/Test"]}"#,
        )
        .unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../openrailsrs-msts/tests/fixtures/minimal.tdb"),
            native.join("Test.tdb"),
        )
        .unwrap();
        let result = prepare_installed(package, None).unwrap();
        assert!(result.contains("0/0 formaciones"));
        assert!(package.join("openrailsrs-import/Test/track.toml").is_file());
        assert!(package.join("openrailsrs-prepared.json").is_file());
    }
    #[test]
    fn train_only_package_audits_author_formations_as_well_as_standalone_choices() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path();
        let stock = package.join("TRAINSET/Lab");
        let consists = package.join("CONSISTS");
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::create_dir_all(&consists).unwrap();
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/player_lab/vehicles");
        for name in ["lab.eng", "lab.wag", "lab.s", "coach.s", "lab.ace"] {
            std::fs::copy(source.join(name), stock.join(name)).unwrap();
        }
        let original = b"(Train (Engine \"Lab/lab.eng\") (Wagon \"Lab/lab.wag\"))\n";
        let con = consists.join("author.con");
        std::fs::write(&con, original).unwrap();
        std::fs::write(package.join("openrailsrs-content.json"),
            br#"{"package":"Author train","routes":[],"stock_files":["TRAINSET/Lab/lab.eng","TRAINSET/Lab/lab.wag"]}"#).unwrap();
        prepare_installed(package, None).unwrap();
        let audits: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(package.join("openrailsrs-audit.json")).unwrap())
                .unwrap();
        assert_eq!(audits.len(), 3);
        let authored = audits
            .iter()
            .find(|a| a["path"].as_str() == con.to_str())
            .expect("Author formation audited");
        assert_eq!(authored["vehicles"].as_u64(), Some(2));
        assert_eq!(std::fs::read(con).unwrap(), original);
        assert!(
            package
                .join("openrailsrs-import/standalone/vehicle-0.con")
                .is_file()
        );
        assert!(
            package
                .join("openrailsrs-import/standalone/vehicle-1.con")
                .is_file()
        );
    }
    #[test]
    fn stock_only_author_archive_can_supply_trainset_and_consists_at_its_root() {
        let t = tempfile::tempdir().unwrap();
        let library = t.path().join("official-content");
        let package = library.join("original");
        let consists = package.join("TGV/CONSISTS");
        let stock = package.join("TGV/TRAINSET/Train");
        std::fs::create_dir_all(&consists).unwrap();
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::write(stock.join("loco.eng"), b"author").unwrap();
        std::fs::write(package.join("openrailsrs-content.json"),
            br#"{"package":"Author train","routes":[],"stock_files":["TGV/TRAINSET/Train/loco.eng"]}"#).unwrap();
        let found = discover_stock_sources(&library);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, consists);
    }
    #[test]
    fn train_only_sources_share_the_package_identity_instead_of_stock_or_storage_names() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("official-content");
        let package = library.join("private-download-40f03ebf");
        let consists = package.join("TGV/CONSISTS");
        let stock = package.join("TGV/TRAINSET/Train");
        let standalone = package.join("openrailsrs-import/standalone");
        for path in [&consists, &stock, &standalone] {
            std::fs::create_dir_all(path).unwrap();
        }
        let engine = "Atl-M1-24047-att_FR-Sharpes.eng";
        std::fs::write(stock.join(engine), b"author").unwrap();
        let manifest = serde_json::json!({
            "package": "Jeanprisca/SNCF-TGV-Atlantique",
            "routes": [],
            "stock_files": [format!("TGV/TRAINSET/Train/{engine}")],
            "source_date": "2023-04-28T23:12:52Z",
            "revision": {"commit": "10967835842c02ed5ae4a3f480c1dca9fb4ef609"}
        });
        std::fs::write(
            package.join("openrailsrs-content.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let found = discover_stock_sources(&library);
        let expected = "SNCF-TGV-Atlantique · origen 2023-04-28 · 10967835";
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|(_, edition)| edition == expected));
        assert!(found.iter().any(|(path, _)| path == &consists));
        assert!(found.iter().any(|(path, _)| path == &standalone));
        assert_eq!(installed_label(&package), expected);
    }
    #[test]
    fn package_identity_is_a_display_name_without_control_or_parent_components() {
        for value in ["", ".", "..", "Author/..", "Author/", "Author/Train\nLabel"] {
            let manifest = serde_json::json!({"package": value});
            assert_eq!(
                package_edition_label(&manifest),
                "Contenido · descarga sin-id"
            );
        }
        assert_eq!(
            package_edition_label(&serde_json::json!({"package": "Author\\SNCF-TGV-Atlantique"})),
            "SNCF-TGV-Atlantique · descarga sin-id"
        );
    }
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
        let old = serde_json::json!({"package":"Chiltern", "advertised_url":"https://github.com/DocMartin7644/Chiltern-Route-v2.git", "download_sha256":"11111111aabbccdd", "revision":{"repository":"DocMartin7644/Chiltern-Route-v2"}});
        let new = serde_json::json!({"package":"Chiltern v4", "advertised_url":"https://github.com/DocMartin7644/Chiltern-Route-v4.git", "source_date":"2026-10-04T14:00:00Z", "revision":{"repository":"DocMartin7644/Chiltern-Route-v4", "commit":"22222222aabbccdd"}});
        assert_eq!(
            edition_label(native, &old),
            "Chiltern v2 · descarga 11111111"
        );
        assert_eq!(
            edition_label(native, &new),
            "Chiltern v4 · origen 2026-10-04 · 22222222"
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
        assert_eq!(
            edition_label(Path::new("Chiltern v4"), &new),
            "Chiltern v4 · origen 2026-10-04 · 22222222"
        );
        let unverified = serde_json::json!({"package":"Chiltern v4", "advertised_url":"https://github.com/another/route.git", "softwareVersion":"v99", "download_sha256":"33333333"});
        assert_eq!(
            edition_label(native, &unverified),
            "Chiltern · descarga 33333333"
        );
        let renamed = serde_json::json!({"package":"Chiltern", "advertised_url":"https://github.com/DocMartin7644/Chiltern-Route-v2.git", "source_date":"2026-09-09T15:48:33Z", "revision":{"repository":"DocMartin7644/Chiltern-Route-v4", "commit":"44444444aabbccdd"}});
        assert_eq!(
            edition_label(native, &renamed),
            "Chiltern v4 · origen 2026-09-09 · 44444444"
        );
        let future = serde_json::json!({"package":"Chiltern v4", "advertised_url":"https://github.com/DocMartin7644/Chiltern-Route-v4.git", "download_sha256":"55555555", "revision":{"repository":"DocMartin7644/Chiltern-Route-v5"}});
        assert_eq!(
            edition_label(Path::new("Chiltern v4"), &future),
            "Chiltern · descarga 55555555"
        );
    }
    fn missing(reference: &Path, name: &str) -> openrailsrs_train::MissingResource {
        openrailsrs_train::MissingResource {
            name: name.into(),
            referenced_by: reference.into(),
            destinations: vec![reference.parent().unwrap().join(name.replace('\\', "/"))],
            required: true,
        }
    }
    #[test]
    fn unknown_caf_placeholders_have_no_alternative_source() {
        let temp = tempfile::tempdir().unwrap();
        let resource = missing(&temp.path().join("caf6000_motor.eng"), "caf6000_motor.s");
        assert!(source_for_missing(&resource).is_none());
    }
    #[test]
    fn repository_search_uses_catalogue_origin_and_only_the_missing_basename() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let stock = root.join("TRAINS/TRAINSET/DMU");
        std::fs::create_dir_all(&stock).unwrap();
        std::fs::write(
            root.join("openrailsrs-content.json"),
            serde_json::json!({
                "package": "Chiltern",
                "advertised_url": "https://github.com/DocMartin7644/Chiltern-Route-v2.git",
                "revision": {"repository": "another-author/alternative"},
                "download_url": "https://unverified.example/locomotive.zip"
            })
            .to_string(),
        )
        .unwrap();
        let source = source_for_missing(&missing(
            &stock.join("existing.eng"),
            "..\\Common.Shape\\lócó motive.s",
        ))
        .unwrap();
        assert_eq!(
            source.page,
            "https://github.com/DocMartin7644/Chiltern-Route-v4/search?q=l%C3%B3c%C3%B3%20motive.s&type=code"
        );
        assert_eq!(source.package_id, "chiltern-v4");
        assert!(!source.page.contains(root.to_str().unwrap()));
        assert!(!source.title.contains("another-author"));
        // A stock file from a different location is not assigned the scenario's
        // repository, even if that scenario was installed from the catalogue.
        let other = tempfile::tempdir().unwrap();
        assert!(
            source_for_missing(&missing(
                &other.path().join("caf6000_motor.eng"),
                "caf6000_motor.s"
            ))
            .is_none()
        );
    }
    #[test]
    fn unlisted_origins_and_mismatched_packages_offer_no_download() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let resource = missing(&root.join("stock.eng"), "missing.s");
        for manifest in [
            serde_json::json!({"revision": {"repository": "author/route"}}),
            serde_json::json!({"package": "Chiltern", "advertised_url": "https://github.com/author/route.git"}),
            serde_json::json!({"package": "Another route", "advertised_url": "https://github.com/DocMartin7644/Chiltern-Route-v2.git"}),
            serde_json::json!({"package": "Chiltern v4", "advertised_url": "https://github.com/DocMartin7644/Chiltern-Route-v2.git"}),
            serde_json::json!({"package": "Demo Model 1", "advertised_url": "https://static.openrails.org/files/DemoModel1.zip", "revision": {"repository": "author/route"}}),
        ] {
            std::fs::write(root.join("openrailsrs-content.json"), manifest.to_string()).unwrap();
            assert!(source_for_missing(&resource).is_none());
        }
    }
}
