//! Cooperative I/O/parser cancellation scoped to one background worker thread.
use crate::FormatError;
use std::{
    cell::RefCell,
    marker::PhantomData,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
type Epoch = Option<(Arc<AtomicU64>, u64)>;
thread_local! {static EPOCH:RefCell<Epoch>=const{RefCell::new(None)};}
pub struct Scope {
    previous: Epoch,
    _thread: PhantomData<Rc<()>>,
}
impl Scope {
    pub fn new(latest: Arc<AtomicU64>, generation: u64) -> Self {
        let previous = EPOCH.with(|e| e.replace(Some((latest, generation))));
        Self {
            previous,
            _thread: PhantomData,
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        EPOCH.with(|e| {
            e.replace(self.previous.take());
        });
    }
}
pub fn cancelled() -> bool {
    EPOCH.with(|e| {
        e.borrow()
            .as_ref()
            .is_some_and(|(latest, generation)| latest.load(Ordering::Relaxed) != *generation)
    })
}
pub fn check() -> Result<(), FormatError> {
    if cancelled() {
        Err(FormatError::Cancelled)
    } else {
        Ok(())
    }
}
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    if EPOCH.with(|e| e.borrow().is_none()) {
        return std::fs::read(path);
    }
    let mut reader = Reader(std::fs::File::open(path)?);
    let mut output = vec![];
    reader.read_to_end(&mut output)?;
    Ok(output)
}
pub(crate) struct Reader<R>(pub R);
impl<R: std::io::Read> std::io::Read for Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, std::io::Error> {
        if cancelled() {
            return Err(std::io::Error::other(
                "Lectura cancelada por una selección nueva",
            ));
        }
        self.0.read(buffer)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_is_thread_local_and_nested_scope_restores_parent() {
        let token = Arc::new(AtomicU64::new(1));
        let outer = Scope::new(token.clone(), 1);
        assert!(!cancelled());
        {
            let inner = Scope::new(token.clone(), 0);
            assert!(matches!(check(), Err(FormatError::Cancelled)));
            drop(inner);
        }
        assert!(!cancelled());
        token.store(2, Ordering::Relaxed);
        assert!(cancelled());
        assert!(!std::thread::spawn(cancelled).join().unwrap());
        drop(outer);
        assert!(!cancelled());
    }
}
