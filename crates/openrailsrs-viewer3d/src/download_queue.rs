//! Persisted job identities, never persisted URLs or authentication data.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub package: String,
    pub phase: Phase,
    pub done_bytes: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
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
        entries.retain(|e| {
            e.id < 1_000_000_000_000_000_000
                && allowed.contains(&e.package)
                && e.message.len() <= 4096
        });
        for e in &mut entries {
            if e.phase.pending() {
                e.phase = Phase::Queued;
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
        self.entries
            .retain(|e| e.phase.pending() || self.next.saturating_sub(e.id) < 24);
        if self.entries.len() >= 32 {
            return Err("La cola está llena; cancelá o limpiá trabajos terminados".into());
        }
        let id = self.next;
        self.next += 1;
        self.entries.push(Entry {
            id,
            package,
            phase: Phase::Queued,
            done_bytes: 0,
            total_bytes: None,
            message: String::new(),
        });
        self.persist()?;
        Ok(id)
    }
    pub fn persist(&self) -> Result<(), String> {
        crate::player_settings::atomic_write(
            &self.path,
            &serde_json::to_vec_pretty(&File {
                schema: 1,
                entries: self.entries.clone(),
            })
            .map_err(|e| e.to_string())?,
        )
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
            e.total_bytes = total;
        }
    }
    pub fn cancel_waiting(&mut self, id: u64) -> Result<(), String> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or("El trabajo ya no existe")?;
        if entry.phase != Phase::Queued {
            return Err("Este trabajo requiere cancelar su operación activa".into());
        }
        entry.phase = Phase::Cancelled;
        entry.message = "Cancelada antes de empezar".into();
        self.persist()
    }
    pub fn clear_finished(&mut self) -> Result<(), String> {
        self.entries.retain(|e| e.phase.pending());
        self.persist()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
