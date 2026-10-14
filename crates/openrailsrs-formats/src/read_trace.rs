//! Conservative dependency tracing for concurrent native importers. Concurrent
//! reads may add extra dependencies, but a worker's read is never omitted.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};
#[derive(Default)]
struct Dependencies {
    paths: BTreeMap<PathBuf, Option<String>>,
    changed: bool,
}
type Paths = Mutex<Dependencies>;
fn traces() -> &'static Mutex<Vec<Weak<Paths>>> {
    static TRACES: OnceLock<Mutex<Vec<Weak<Paths>>>> = OnceLock::new();
    TRACES.get_or_init(Default::default)
}
pub struct ReadTrace(Arc<Paths>);
impl Default for ReadTrace {
    fn default() -> Self {
        let paths = Arc::new(Paths::default());
        let mut traces = traces().lock().unwrap_or_else(|e| e.into_inner());
        traces.retain(|p| p.strong_count() > 0);
        traces.push(Arc::downgrade(&paths));
        Self(paths)
    }
}
impl ReadTrace {
    pub fn paths(&self) -> Vec<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .keys()
            .cloned()
            .collect()
    }
    pub fn fingerprints(&self) -> Vec<(PathBuf, Option<String>)> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .iter()
            .map(|(p, d)| (p.clone(), d.clone()))
            .collect()
    }
    pub fn consistent(&self) -> bool {
        !self.0.lock().unwrap_or_else(|e| e.into_inner()).changed
    }
}
pub fn record(path: &Path) {
    record_hash(path, None);
}
pub fn record_bytes(path: &Path, bytes: &[u8]) {
    if traces()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
    {
        return;
    }
    record_hash(path, Some(format!("{:x}", Sha256::digest(bytes))));
}
fn record_hash(path: &Path, digest: Option<String>) {
    let mut traces = traces().lock().unwrap_or_else(|e| e.into_inner());
    traces.retain(|p| p.strong_count() > 0);
    if traces.is_empty() {
        return;
    }
    let path = path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    });
    for trace in traces.iter().filter_map(Weak::upgrade) {
        let mut paths = trace.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(previous), Some(next)) = (
            paths.paths.get(&path).and_then(Option::as_ref),
            digest.as_ref(),
        ) && previous != next
        {
            paths.changed = true;
        }
        if digest.is_some() || !paths.paths.contains_key(&path) {
            paths.paths.insert(path.clone(), digest.clone());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_reads_of_changed_source_make_the_import_uncacheable() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("tsection.dat");
        std::fs::write(&file, b"before").unwrap();
        let trace = ReadTrace::default();
        record(&file);
        record_bytes(&file, b"before");
        record(&file);
        record_bytes(&file, b"before");
        assert!(trace.consistent());
        record_bytes(&file, b"after");
        record_bytes(&file, b"before");
        assert!(!trace.consistent());
        assert!(trace.paths().contains(&file));
    }
}
