//! One cancellable I/O worker for menu audits, with a cache per content pack.
//! No shape parsing or waits belong to the Bevy update thread.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
};

use openrailsrs_train::{ConsistAudit, ConsistAuditor};

#[derive(Debug)]
struct Request {
    generation: u64,
    revision: u64,
    roots: Vec<PathBuf>,
    paths: Vec<PathBuf>,
}

#[derive(Debug)]
struct Reply {
    generation: u64,
    roots: Vec<PathBuf>,
    audit: ConsistAudit,
}

#[derive(Debug)]
struct Worker {
    requests: Sender<Request>,
    replies: Mutex<Receiver<Reply>>,
    generation: Arc<AtomicU64>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Leaving the menu stops the old batch after its current formation.
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(crate) struct LaunchAudits {
    worker: Arc<Worker>,
    generation: u64,
    revisions: HashMap<Vec<PathBuf>, u64>,
    cache: HashMap<Vec<PathBuf>, HashMap<PathBuf, ConsistAudit>>,
    pending: usize,
    error: Option<String>,
}

impl LaunchAudits {
    pub fn new() -> Self {
        Self::with_inspector(ConsistAuditor::inspect)
    }

    fn with_inspector(
        mut inspect: impl FnMut(&mut ConsistAuditor, &Path) -> ConsistAudit + Send + 'static,
    ) -> Self {
        let (requests, incoming) = mpsc::channel::<Request>();
        let (outgoing, replies) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let latest = generation.clone();
        let spawned = std::thread::Builder::new()
            .name("menu-consist-audit".into())
            .spawn(move || {
                let mut auditors: HashMap<Vec<PathBuf>, (u64, ConsistAuditor)> = HashMap::new();
                while let Ok(mut request) = incoming.recv() {
                    // Rapid clicks replace queued work instead of multiplying workers.
                    while let Ok(next) = incoming.try_recv() {
                        request = next;
                    }
                    let (revision, auditor) =
                        auditors.entry(request.roots.clone()).or_insert_with(|| {
                            (request.revision, ConsistAuditor::new(request.roots.clone()))
                        });
                    if *revision != request.revision {
                        *revision = request.revision;
                        *auditor = ConsistAuditor::new(request.roots.clone());
                    }
                    for path in request.paths {
                        if latest.load(Ordering::Relaxed) != request.generation {
                            break;
                        }
                        let audit = inspect(auditor, &path);
                        if outgoing
                            .send(Reply {
                                generation: request.generation,
                                roots: request.roots.clone(),
                                audit,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        Self {
            worker: Arc::new(Worker {
                requests,
                replies: Mutex::new(replies),
                generation,
            }),
            generation: 0,
            revisions: HashMap::new(),
            cache: HashMap::new(),
            pending: 0,
            error: spawned
                .err()
                .map(|e| format!("No se pudo iniciar la revisión: {e}")),
        }
    }

    pub fn cached(&self, roots: &[PathBuf], path: &Path) -> Option<&ConsistAudit> {
        self.cache.get(roots)?.get(path)
    }

    pub fn request(&mut self, roots: Vec<PathBuf>, paths: &[PathBuf], refresh: bool) {
        self.poll();
        let revision = self.revisions.entry(roots.clone()).or_default();
        if refresh {
            *revision += 1;
            self.cache.remove(&roots);
        }
        let revision = *revision;
        self.generation = self.worker.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let paths: Vec<_> = paths
            .iter()
            .filter(|p| self.cached(&roots, p).is_none())
            .cloned()
            .collect();
        self.pending = paths.len();
        if let Err(e) = self.worker.requests.send(Request {
            generation: self.generation,
            revision,
            roots,
            paths,
        }) {
            self.error = Some(format!("No se pudo revisar el contenido: {e}"));
            self.pending = 0;
        }
    }

    /// Drain completed reports without waiting for the worker or its filesystem I/O.
    pub fn poll(&mut self) -> bool {
        let Ok(replies) = self.worker.replies.try_lock() else {
            return false;
        };
        let mut changed = false;
        loop {
            match replies.try_recv() {
                Ok(reply) if reply.generation == self.generation => {
                    self.cache
                        .entry(reply.roots)
                        .or_default()
                        .insert(reply.audit.path.clone(), reply.audit);
                    self.pending = self.pending.saturating_sub(1);
                    changed = true;
                }
                Ok(_) => (), // A previous selection cannot overwrite the current one.
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.pending > 0 {
                        self.error = Some(
                            "La revisión de contenido se interrumpió; intentá reauditar".into(),
                        );
                        self.pending = 0;
                        changed = true;
                    }
                    break;
                }
            }
        }
        changed
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    fn report(auditor: &ConsistAuditor, path: &Path) -> ConsistAudit {
        ConsistAudit {
            path: path.into(),
            vehicles: 1,
            powered_vehicles: 1,
            warnings: vec![auditor.trainset_roots[0].display().to_string()],
            ..ConsistAudit::default()
        }
    }

    fn finish(audits: &mut LaunchAudits) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while audits.pending > 0 {
            assert!(Instant::now() < deadline, "audit worker did not finish");
            audits.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(audits.error().is_none(), "{:?}", audits.error());
    }

    #[test]
    fn revisiting_a_route_reuses_reports_but_other_content_roots_are_distinct() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let mut audits = LaunchAudits::with_inspector(move |a, p| {
            counter.fetch_add(1, Ordering::Relaxed);
            report(a, p)
        });
        let con = PathBuf::from("same.con");
        let first = vec![PathBuf::from("first/TRAINSET")];
        let second = vec![PathBuf::from("second/TRAINSET")];
        for roots in [&first, &second, &first] {
            audits.request(roots.clone(), std::slice::from_ref(&con), false);
            finish(&mut audits);
            assert_eq!(
                audits.cached(roots, &con).unwrap().warnings[0],
                roots[0].display().to_string()
            );
        }
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        audits.request(first.clone(), std::slice::from_ref(&con), true);
        finish(&mut audits);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            3,
            "explicit reaudit must reread resources"
        );
    }

    #[test]
    fn blocked_io_does_not_block_polling_or_switches_and_stale_results_are_discarded() {
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let seen = Arc::new(Mutex::new(vec![]));
        let inspected = seen.clone();
        let mut audits = LaunchAudits::with_inspector(move |a, p| {
            inspected.lock().unwrap().push(p.to_path_buf());
            if p == Path::new("blocked.con") {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            report(a, p)
        });
        let roots = vec![PathBuf::from("content/TRAINSET")];
        audits.request(
            roots.clone(),
            &["blocked.con".into(), "obsolete.con".into()],
            false,
        );
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        // These calls must return while the inspector is still waiting for release.
        assert!(!audits.poll());
        audits.request(roots.clone(), &["skipped.con".into()], false);
        audits.request(roots.clone(), &["current.con".into()], false);
        assert!(audits.cached(&roots, Path::new("current.con")).is_none());
        release.send(()).unwrap();
        finish(&mut audits);
        assert!(audits.cached(&roots, Path::new("blocked.con")).is_none());
        assert!(audits.cached(&roots, Path::new("current.con")).is_some());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![PathBuf::from("blocked.con"), PathBuf::from("current.con")]
        );
    }

    #[test]
    fn dropping_the_menu_cancels_its_remaining_batch() {
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (finished, stopped) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut audits = LaunchAudits::with_inspector(move |a, p| {
            count.fetch_add(1, Ordering::Relaxed);
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
            finished.send(()).unwrap();
            report(a, p)
        });
        audits.request(
            vec!["root".into()],
            &["first.con".into(), "never.con".into()],
            false,
        );
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(audits);
        release.send(()).unwrap();
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
