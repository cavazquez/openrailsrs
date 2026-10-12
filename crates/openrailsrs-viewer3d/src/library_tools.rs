//! Library operations run outside the Bevy frame thread. Mutations finish as one
//! operation and report errors; imported settings require a separate UI action.
use crate::{
    content_management::{RemovalPlan, TrashEntry, VersionDiff},
    player_settings::PlayerSettings,
};
use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
};
pub enum Action {
    Scan,
    Export,
    Import(PathBuf),
    Inspect(PathBuf),
    Remove(PathBuf),
    Restore(TrashEntry),
    Compare(PathBuf, PathBuf),
}
enum Reply {
    Inventory(Vec<PathBuf>, Vec<TrashEntry>),
    Exported(PathBuf),
    Imported(Box<crate::player_backup::Imported>),
    Plan(RemovalPlan),
    Removed(TrashEntry),
    Restored(PathBuf),
    Compared(VersionDiff),
}
#[derive(Resource, Default)]
pub struct LibraryTools {
    pub backups: Vec<PathBuf>,
    pub trash: Vec<TrashEntry>,
    pub plan: Option<RemovalPlan>,
    pub comparison: Option<VersionDiff>,
    pub compare_base: Option<PathBuf>,
    pub imported_settings: Option<PlayerSettings>,
    pub status: String,
    pending: Option<Mutex<mpsc::Receiver<Result<Reply, String>>>>,
    epoch: std::sync::Arc<std::sync::atomic::AtomicU64>,
    readonly: bool,
}
impl LibraryTools {
    pub fn cancel_read(&mut self) {
        if self.readonly && self.pending.is_some() {
            self.epoch
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.pending = None;
            self.status = "Consulta cancelada al salir de la biblioteca".into();
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn request(&mut self, action: Action) -> Result<(), String> {
        if self.busy() {
            return Err("Esperá que termine la operación actual de la biblioteca".into());
        }
        self.readonly = matches!(
            action,
            Action::Scan | Action::Inspect(_) | Action::Compare(_, _)
        );
        let generation = self
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let latest = self.epoch.clone();
        let data = crate::player_settings::player_data_dir();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(Mutex::new(rx));
        self.status = "Procesando en segundo plano… Podés seguir usando el menú.".into();
        std::thread::spawn(move || {
            let _scope = openrailsrs_formats::cancellation::Scope::new(latest, generation);
            let result = std::panic::catch_unwind(|| match action {
                Action::Scan => Ok(Reply::Inventory(
                    crate::player_backup::list(&data),
                    crate::content_management::trash(&data),
                )),
                Action::Export => crate::player_backup::export(&data).map(Reply::Exported),
                Action::Import(path) => crate::player_backup::import(&data, &path)
                    .map(|result| Reply::Imported(Box::new(result))),
                Action::Inspect(path) => {
                    crate::content_management::inspect_removal(&data, &path).map(Reply::Plan)
                }
                Action::Remove(path) => {
                    crate::content_management::remove(&data, &path).map(Reply::Removed)
                }
                Action::Restore(entry) => {
                    crate::content_management::restore(&data, &entry).map(Reply::Restored)
                }
                Action::Compare(a, b) => {
                    crate::content_management::compare(&data, &a, &b).map(Reply::Compared)
                }
            })
            .unwrap_or_else(|_| Err("No se pudo completar la operación de contenido".into()));
            let _ = tx.send(result);
        });
        Ok(())
    }
    /// Return whether installed content changed, requiring a menu index refresh.
    pub fn poll(&mut self) -> Option<bool> {
        let receiver = self.pending.as_ref()?.try_lock().ok()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("La operación terminó sin respuesta".into())
            }
        };
        drop(receiver);
        self.pending = None;
        let mut content_changed = false;
        match result {
            Err(error) => self.status = error,
            Ok(reply) => match reply {
                Reply::Inventory(backups, trash) => {
                    self.backups = backups;
                    self.trash = trash;
                    self.status = "Biblioteca lista".into();
                }
                Reply::Exported(path) => {
                    self.status = format!("Respaldo creado: {}", path.display());
                    self.backups.insert(0, path);
                }
                Reply::Imported(imported) => {
                    let imported = *imported;
                    self.status = format!(
                        "Restauradas {} partidas como copias nuevas. Contenido pendiente: {}",
                        imported.files.len(),
                        if imported.missing.is_empty() {
                            "ninguno".into()
                        } else {
                            imported.missing.join(", ")
                        }
                    );
                    self.imported_settings = imported.settings;
                }
                Reply::Plan(plan) => {
                    self.status = "Revisá las dependencias antes de retirar esta copia".into();
                    self.plan = Some(plan);
                }
                Reply::Removed(entry) => {
                    self.status = format!(
                        "{} retirada. Podés recuperarla desde esta biblioteca.",
                        entry.label
                    );
                    self.trash.push(entry);
                    self.plan = None;
                    content_changed = true;
                }
                Reply::Restored(path) => {
                    self.status = format!("Copia recuperada: {}", path.display());
                    self.trash = crate::content_management::trash(
                        &crate::player_settings::player_data_dir(),
                    );
                    content_changed = true;
                }
                Reply::Compared(diff) => {
                    self.status = format!(
                        "{} archivos nuevos · {} modificados · {} retirados",
                        diff.added, diff.changed, diff.removed
                    );
                    self.comparison = Some(diff);
                }
            },
        }
        Some(content_changed)
    }
}
