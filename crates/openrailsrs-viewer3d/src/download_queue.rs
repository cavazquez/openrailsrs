//! Persisted job identities, never persisted URLs or authentication data.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
const MAX_JOB_ID: u64 = 1_000_000_000_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Running,
    CancelRequested,
    Ready,
    Failed,
    Cancelled,
}
impl Phase {
    pub fn pending(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::CancelRequested)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "En cola",
            Self::Running => "En curso",
            Self::CancelRequested => "Cancelando",
            Self::Ready => "Instalada",
            Self::Failed => "Falló",
            Self::Cancelled => "Cancelada",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub package: String,
    pub phase: Phase,
    pub done_bytes: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
}
impl Entry {
    pub fn progress_label(&self) -> String {
        progress_label(self.done_bytes, self.total_bytes)
    }
}

/// A server may omit Content-Length or report extraction totals separately.
/// Contradictory totals are unknown, rather than evidence of completion.
pub fn progress_label(done: u64, total: Option<u64>) -> String {
    let mib = done as f64 / 1_048_576.;
    match total.filter(|&n| n > 0 && n >= done) {
        Some(n) => format!(
            "{mib:.1} / {:.1} MiB · {:.0}%",
            n as f64 / 1_048_576.,
            done as f64 / n as f64 * 100.
        ),
        None => format!("{mib:.1} MiB · tamaño total desconocido"),
    }
}
#[derive(Serialize, Deserialize)]
struct File {
    schema: u32,
    entries: Vec<Entry>,
}
pub struct Queue {
    pub entries: Vec<Entry>,
    pub held: bool,
    path: PathBuf,
    next: u64,
}
impl Queue {
    pub fn load(data: &Path, allowed: &[String]) -> Self {
        let path = data.join("download-queue.json");
        let file = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= 256 * 1024)
            .and_then(|_| std::fs::read(&path).ok())
            .and_then(|b| serde_json::from_slice::<File>(&b).ok());
        let mut entries = file
            .filter(|f| f.schema == 1 && f.entries.len() <= 32)
            .map_or(vec![], |f| f.entries);
        let mut identities = std::collections::HashMap::new();
        for e in &entries {
            *identities.entry(e.id).or_insert(0usize) += 1;
        }
        let mut pending_packages = std::collections::HashSet::new();
        entries.retain(|e| {
            e.id > 0
                && e.id < MAX_JOB_ID
                && identities[&e.id] == 1
                && allowed.contains(&e.package)
                && e.message.len() <= 4096
                && (!e.phase.pending() || pending_packages.insert(e.package.clone()))
        });
        for e in &mut entries {
            e.total_bytes = e.total_bytes.filter(|&n| n > 0 && n >= e.done_bytes);
            if e.phase == Phase::CancelRequested {
                e.phase = Phase::Cancelled;
                e.message = "Cancelación conservada al volver a abrir.".into();
            } else if e.phase.pending() {
                e.phase = Phase::Queued;
                e.done_bytes = 0;
                e.total_bytes = None;
                e.message = "Pausada al volver a abrir. Retomá la cola cuando quieras.".into();
            }
        }
        let next = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        Self {
            held: entries.iter().any(|e| e.phase.pending()),
            entries,
            path,
            next,
        }
    }
    pub fn enqueue(&mut self, package: String) -> Result<u64, String> {
        if !(1..MAX_JOB_ID).contains(&self.next) {
            return Err("La cola agotó sus identificadores; limpiá los trabajos terminados y volvé a abrir.".into());
        }
        if self
            .entries
            .iter()
            .any(|e| e.package == package && e.phase.pending())
        {
            return Err(
                "Esta ruta ya está en la cola; se conservará una sola instalación por versión"
                    .into(),
            );
        }
        let mut entries = self.entries.clone();
        entries.retain(|e| e.phase.pending() || self.next.saturating_sub(e.id) < 24);
        if entries.len() >= 32 {
            return Err("La cola está llena; cancelá o limpiá trabajos terminados".into());
        }
        let id = self.next;
        entries.push(Entry {
            id,
            package,
            phase: Phase::Queued,
            done_bytes: 0,
            total_bytes: None,
            message: String::new(),
        });
        self.commit(entries, id + 1)?;
        Ok(id)
    }
    fn commit(&mut self, entries: Vec<Entry>, next: u64) -> Result<(), String> {
        self.write_entries(&entries)?;
        self.entries = entries;
        self.next = next;
        Ok(())
    }
    fn write_entries(&self, entries: &[Entry]) -> Result<(), String> {
        crate::player_settings::atomic_write(
            &self.path,
            &serde_json::to_vec_pretty(&File {
                schema: 1,
                entries: entries.to_vec(),
            })
            .map_err(|e| e.to_string())?,
        )
    }
    pub fn persist(&self) -> Result<(), String> {
        self.write_entries(&self.entries)
    }
    pub fn next_job(&self) -> Option<(u64, String)> {
        if self.held {
            return None;
        }
        self.entries
            .iter()
            .find(|e| e.phase == Phase::Queued)
            .map(|e| (e.id, e.package.clone()))
    }
    pub fn update(&mut self, id: u64, phase: Phase, message: &str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.phase = phase;
            e.message = message.chars().take(1024).collect();
        }
    }
    pub fn progress(&mut self, id: u64, done: u64, total: Option<u64>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.done_bytes = done;
            e.total_bytes = total.filter(|&n| n > 0 && n >= done);
        }
    }
    pub fn cancel_waiting(&mut self, id: u64) -> Result<(), String> {
        let mut entries = self.entries.clone();
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or("El trabajo ya no existe")?;
        if entry.phase != Phase::Queued {
            return Err("Este trabajo requiere cancelar su operación activa".into());
        }
        entry.phase = Phase::Cancelled;
        entry.message = "Cancelada antes de empezar".into();
        self.commit(entries, self.next)
    }
    pub fn request_cancel(&mut self, id: u64, message: &str) -> Result<(), String> {
        let mut entries = self.entries.clone();
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or("El trabajo ya no existe")?;
        if !entry.phase.pending() {
            return Err("Este trabajo ya terminó".into());
        }
        entry.phase = Phase::CancelRequested;
        entry.message = message.chars().take(1024).collect();
        self.commit(entries, self.next)
    }
    pub fn clear_finished(&mut self) -> Result<(), String> {
        let entries = self
            .entries
            .iter()
            .filter(|e| e.phase.pending())
            .cloned()
            .collect();
        self.commit(entries, self.next)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovered_last_valid_id_cannot_create_an_unrecoverable_job() {
        let tmp = tempfile::tempdir().unwrap();
        let mut q = Queue::load(tmp.path(), &["route".into()]);
        q.enqueue("route".into()).unwrap();
        q.entries[0].id = MAX_JOB_ID - 1;
        q.persist().unwrap();
        let mut q = Queue::load(tmp.path(), &["route".into(), "other".into()]);
        let entries = q.entries.clone();
        let bytes = std::fs::read(&q.path).unwrap();
        assert!(q.enqueue("other".into()).is_err());
        assert_eq!(q.entries, entries);
        assert_eq!(q.next, MAX_JOB_ID);
        assert_eq!(std::fs::read(&q.path).unwrap(), bytes);
    }
    #[test]
    fn failed_writes_do_not_publish_or_consume_queue_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut q = Queue::load(tmp.path(), &["route".into()]);
        let id = q.enqueue("route".into()).unwrap();
        let entries = q.entries.clone();
        let next = q.next;
        let good_path = q.path.clone();
        let blocked = tmp.path().join("not-a-directory");
        std::fs::write(&blocked, "blocked").unwrap();
        q.path = blocked.join("queue.json");
        assert!(q.enqueue("other".into()).is_err());
        assert_eq!(q.entries, entries);
        assert_eq!(q.next, next);
        assert!(q.cancel_waiting(id).is_err());
        assert_eq!(q.entries, entries);
        q.entries[0].phase = Phase::Ready;
        let ready = q.entries.clone();
        assert!(q.clear_finished().is_err());
        assert_eq!(q.entries, ready);
        q.path = good_path;
        assert_eq!(q.enqueue("other".into()).unwrap(), next);
    }
    #[test]
    fn ambiguous_ids_are_removed_and_cancel_request_survives_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let make = |id, phase| Entry {
            id,
            package: "route".into(),
            phase,
            done_bytes: 400,
            total_bytes: Some(1),
            message: String::new(),
        };
        std::fs::write(
            tmp.path().join("download-queue.json"),
            serde_json::to_vec(&File {
                schema: 1,
                entries: vec![
                    make(1, Phase::Ready),
                    make(1, Phase::Failed),
                    make(2, Phase::CancelRequested),
                ],
            })
            .unwrap(),
        )
        .unwrap();
        let q = Queue::load(tmp.path(), &["route".into()]);
        assert_eq!(q.entries.len(), 1);
        assert_eq!(q.entries[0].id, 2);
        assert_eq!(q.entries[0].phase, Phase::Cancelled);
        assert_eq!(q.entries[0].total_bytes, None);
        assert!(q.next_job().is_none());
    }
    #[test]
    fn progress_has_no_false_completion_when_total_is_missing_or_wrong() {
        for total in [None, Some(0), Some(9)] {
            assert!(progress_label(10, total).contains("desconocido"));
        }
        assert!(progress_label(10, Some(20)).ends_with("50%"));
        assert!(progress_label(u64::MAX, Some(u64::MAX)).ends_with("100%"));
        let tmp = tempfile::tempdir().unwrap();
        let mut q = Queue::load(tmp.path(), &["route".into()]);
        let id = q.enqueue("route".into()).unwrap();
        q.progress(id, 10, Some(1));
        assert_eq!(q.entries[0].total_bytes, None);
        q.progress(id, 10, Some(20));
        q.persist().unwrap();
        let r = Queue::load(tmp.path(), &["route".into()]);
        assert_eq!(
            (r.entries[0].done_bytes, r.entries[0].total_bytes),
            (0, None)
        );
        assert!(r.held);
    }
    #[test]
    fn jobs_cancel_independently_and_restart_requires_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let allowed = vec!["chiltern-v4".into(), "demo-model-1".into()];
        let mut q = Queue::load(tmp.path(), &allowed);
        let a = q.enqueue(allowed[0].clone()).unwrap();
        let b = q.enqueue(allowed[1].clone()).unwrap();
        assert!(q.enqueue(allowed[0].clone()).is_err());
        q.cancel_waiting(a).unwrap();
        assert_eq!(q.next_job().unwrap().0, b);
        let mut restored = Queue::load(tmp.path(), &allowed);
        assert!(restored.held);
        assert!(restored.next_job().is_none());
        restored.held = false;
        assert_eq!(restored.next_job().unwrap().0, b);
    }
}
