//! Bounded derivative cache with byte identities and namespace invalidation.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);
static WRITES: AtomicU64 = AtomicU64::new(0);
const MAX_ENTRY_BYTES: usize = 64 * 1024 * 1024;
pub fn report() -> serde_json::Value {
    serde_json::json!({"hits":HITS.load(Ordering::Relaxed),"misses":MISSES.load(Ordering::Relaxed),"writes":WRITES.load(Ordering::Relaxed),"max_disk_mib":256,"max_entry_mib":64,"schema":1})
}
#[derive(Serialize, Deserialize)]
struct Dependency {
    path: PathBuf,
    digest: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct File<T> {
    schema: u32,
    compiler: String,
    files: Vec<Dependency>,
    directories: Vec<Dependency>,
    value: T,
}
fn digest(path: &Path) -> Option<String> {
    if path.metadata().ok()?.len() > 128 * 1024 * 1024 {
        return None;
    }
    Some(format!("{:x}", Sha256::digest(std::fs::read(path).ok()?)))
}
fn namespace(path: &Path) -> Option<String> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path).ok()? {
        if names.len() == 20000 {
            return None;
        }
        let entry = entry.ok()?;
        let kind = entry.file_type().ok()?;
        names.push(format!(
            "{}:{:?}",
            entry.file_name().to_string_lossy(),
            (kind.is_dir(), kind.is_symlink())
        ));
    }
    names.sort();
    Some(format!("{:x}", Sha256::digest(names.join("\n").as_bytes())))
}
fn path(key: &str) -> PathBuf {
    crate::data_dir()
        .join("cache/source")
        .join(format!("{:x}.json", Sha256::digest(key.as_bytes())))
}
pub fn get<T: DeserializeOwned>(key: &str, compiler: &str) -> Option<T> {
    if disabled() {
        MISSES.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    let value = get_at(&path(key), compiler);
    if value.is_some() {
        HITS.fetch_add(1, Ordering::Relaxed);
    } else {
        MISSES.fetch_add(1, Ordering::Relaxed);
    }
    value
}
fn get_at<T: DeserializeOwned>(path: &Path, compiler: &str) -> Option<T> {
    if path.is_symlink() || path.metadata().ok()?.len() > MAX_ENTRY_BYTES as u64 {
        return None;
    }
    let cached: File<T> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if cached.schema != 1
        || cached.compiler != compiler
        || cached.files.len() > 20000
        || cached.directories.len() > 20000
        || cached.files.iter().any(|d| digest(&d.path) != d.digest)
        || cached
            .directories
            .iter()
            .any(|d| namespace(&d.path) != d.digest)
    {
        return None;
    }
    Some(cached.value)
}
pub fn put<T: Serialize>(
    key: &str,
    compiler: &str,
    files: &[PathBuf],
    directories: &[PathBuf],
    value: &T,
) {
    if disabled() {
        return;
    }
    let _ = put_at(&path(key), compiler, files, directories, value);
}
pub fn put_verified<T: Serialize>(
    key: &str,
    compiler: &str,
    files: &[(PathBuf, Option<String>)],
    directories: &[PathBuf],
    value: &T,
) {
    if disabled() {
        return;
    }
    // Validate the bytes actually consumed by the importer. A file edited
    // during parsing must not associate an old result with its new identity.
    if files
        .iter()
        .any(|(p, expected)| digest(p) != *expected || p.exists() && expected.is_none())
    {
        return;
    }
    let written = put_at_checked(
        &path(key),
        compiler,
        &files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
        directories,
        value,
        Some(files),
    );
    if written.is_some() {
        WRITES.fetch_add(1, Ordering::Relaxed);
    }
}
fn disabled() -> bool {
    std::env::var("OPENRAILSRS_SOURCE_CACHE").is_ok_and(|s| s.eq_ignore_ascii_case("off"))
}
fn put_at<T: Serialize>(
    path: &Path,
    compiler: &str,
    files: &[PathBuf],
    directories: &[PathBuf],
    value: &T,
) -> Option<()> {
    put_at_checked(path, compiler, files, directories, value, None)
}
fn put_at_checked<T: Serialize>(
    path: &Path,
    compiler: &str,
    files: &[PathBuf],
    directories: &[PathBuf],
    value: &T,
    verified: Option<&[(PathBuf, Option<String>)]>,
) -> Option<()> {
    if files.len() > 20000 || directories.len() > 20000 {
        return None;
    }
    let mut dirs = directories.to_vec();
    dirs.extend(
        files
            .iter()
            .filter_map(|p| p.parent().map(Path::to_path_buf)),
    );
    dirs.sort();
    dirs.dedup();
    if dirs.len() > 20000 {
        return None;
    }
    let dependencies = verified.map_or_else(
        || {
            files
                .iter()
                .map(|p| Dependency {
                    path: p.clone(),
                    digest: digest(p),
                })
                .collect::<Vec<_>>()
        },
        |files| {
            files
                .iter()
                .map(|(p, d)| Dependency {
                    path: p.clone(),
                    digest: d.clone(),
                })
                .collect()
        },
    );
    let namespaces = dirs
        .iter()
        .map(|p| Dependency {
            path: p.clone(),
            digest: namespace(p),
        })
        .collect::<Vec<_>>();
    // Missing optional files/directories are dependencies too. Existing inputs
    // which cannot be fully checked must never produce a reusable result.
    if dependencies
        .iter()
        .chain(&namespaces)
        .any(|d| d.path.exists() && d.digest.is_none())
    {
        return None;
    }
    let data = File {
        schema: 1,
        compiler: compiler.to_string(),
        files: dependencies,
        directories: namespaces,
        value,
    };
    let bytes = serde_json::to_vec(&data).ok()?;
    if bytes.len() > MAX_ENTRY_BYTES {
        return None;
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let temp = path.with_extension(format!("{stamp}.tmp"));
    use std::io::Write;
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .ok()?;
        f.write_all(&bytes).ok()?;
        std::fs::rename(&temp, path).ok()
    })();
    if result.is_none() {
        let _ = std::fs::remove_file(temp);
    }
    if result.is_some() {
        prune(path.parent()?);
    }
    result
}
fn prune(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.is_symlink() || p.extension().is_none_or(|x| x != "json") {
                return None;
            }
            let m = p.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), p))
        })
        .collect::<Vec<_>>();
    let mut bytes = files.iter().map(|(_, n, _)| n).sum::<u64>();
    files.sort_by_key(|(at, _, _)| *at);
    for (_, n, p) in files {
        if bytes <= 256 * 1024 * 1024 {
            break;
        }
        if std::fs::remove_file(p).is_ok() {
            bytes = bytes.saturating_sub(n);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_includes_namespace_parser_and_corruption_invalidate_the_result() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let input = source.join("program.dat");
        std::fs::write(&input, "old").unwrap();
        let cached = t.path().join("cache/result.json");
        put_at(
            &cached,
            "parser-v1",
            std::slice::from_ref(&input),
            &[],
            &vec![1, 2, 3],
        )
        .unwrap();
        assert_eq!(
            get_at::<Vec<u32>>(&cached, "parser-v1"),
            Some(vec![1, 2, 3])
        );
        assert!(get_at::<Vec<u32>>(&cached, "parser-v2").is_none());
        std::fs::write(&input, "new").unwrap();
        assert!(get_at::<Vec<u32>>(&cached, "parser-v1").is_none());
        put_at(
            &cached,
            "parser-v1",
            std::slice::from_ref(&input),
            &[],
            &vec![1, 2, 3],
        )
        .unwrap();
        std::fs::write(source.join("override.dat"), "override").unwrap();
        assert!(get_at::<Vec<u32>>(&cached, "parser-v1").is_none());
        std::fs::write(&cached, "damaged").unwrap();
        assert!(get_at::<Vec<u32>>(&cached, "parser-v1").is_none());
    }
}
