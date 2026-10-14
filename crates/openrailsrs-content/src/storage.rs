//! The library can live on another writable disk without moving saves or settings.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Default, Serialize, Deserialize)]
struct Location {
    current: PathBuf,
    previous: Vec<PathBuf>,
}
fn read(data: &Path) -> Location {
    let path = data.join("content-location.json");
    std::fs::metadata(&path)
        .ok()
        .filter(|m| m.len() <= 16384)
        .and_then(|_| std::fs::read(path).ok())
        .and_then(|b| serde_json::from_slice::<Location>(&b).ok())
        .filter(|l| l.current.is_absolute() && l.previous.len() <= 16)
        .unwrap_or_default()
}
pub fn content_dir() -> PathBuf {
    let data = crate::data_dir();
    let location = read(&data);
    if location.current.is_absolute() {
        location.current
    } else {
        data.join("official-content")
    }
}
pub fn content_roots(data: &Path) -> Vec<PathBuf> {
    let location = read(data);
    let mut result = vec![data.join("official-content")];
    for path in std::iter::once(location.current).chain(location.previous) {
        if path.is_absolute() && !result.contains(&path) {
            result.push(path);
        }
    }
    result
}
pub fn select_content_dir(data: &Path, directory: &Path) -> Result<PathBuf, String> {
    if !directory.is_absolute() || directory.starts_with("/snap") {
        return Err("Elegí una carpeta absoluta escribible fuera de /snap".into());
    }
    let directory = directory
        .canonicalize()
        .map_err(|e| format!("Carpeta no disponible: {e}"))?;
    if !directory.is_dir() || directory.starts_with("/snap") {
        return Err("La carpeta debe existir y permitir escritura".into());
    }
    // Keep managed downloads apart from the files the player already owns.
    let target = directory.join("openrailsrs-content");
    if target.is_symlink() {
        return Err("La carpeta de descargas no puede ser un enlace".into());
    }
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("Sin permiso para usar esta carpeta: {e}"))?;
    let probe = target.join(format!(".write-check-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| format!("La carpeta no admite escritura: {e}"))?;
    std::fs::remove_file(probe).map_err(|e| e.to_string())?;
    let mut previous = content_roots(data);
    previous.retain(|p| p != &target);
    if previous.len() > 16 {
        return Err(
            "Hay dieciséis carpetas anteriores; conservá una ubicación estable para la biblioteca"
                .into(),
        );
    }
    let next = Location {
        current: target.clone(),
        previous,
    };
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let temp = data.join(format!("content-location.{}.tmp", std::process::id()));
    let result = (|| {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temp, data.join("content-location.json")).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result.map(|()| target)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_storage_keeps_old_packages_and_reloads_the_choice() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("player");
        let old = data.join("official-content/old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("original"), b"preserve").unwrap();
        let disk = t.path().join("disk");
        std::fs::create_dir(&disk).unwrap();
        let selected = select_content_dir(&data, &disk).unwrap();
        assert_eq!(read(&data).current, selected);
        assert_eq!(
            content_roots(&data),
            vec![data.join("official-content"), selected]
        );
        assert_eq!(std::fs::read(old.join("original")).unwrap(), b"preserve");
        assert!(select_content_dir(&data, Path::new("relative")).is_err());
        assert!(select_content_dir(&data, Path::new("/snap/openrailsrs/6")).is_err());
    }
    #[test]
    fn changing_storage_twice_preserves_all_installed_editions() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("player");
        let first = t.path().join("first");
        let second = t.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let a = select_content_dir(&data, &first).unwrap();
        std::fs::create_dir(a.join("old-version")).unwrap();
        std::fs::write(a.join("old-version/original"), b"author original").unwrap();
        let b = select_content_dir(&data, &second).unwrap();
        assert_eq!(read(&data).current, b);
        assert!(content_roots(&data).contains(&a));
        assert!(content_roots(&data).contains(&b));
        assert_eq!(
            std::fs::read(a.join("old-version/original")).unwrap(),
            b"author original"
        );
        select_content_dir(&data, &first).unwrap();
        assert_eq!(content_roots(&data).len(), 3);
    }
    #[test]
    fn rejected_destination_does_not_replace_the_valid_preference() {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("player");
        let directory = t.path().join("valid");
        std::fs::create_dir(&directory).unwrap();
        let valid = select_content_dir(&data, &directory).unwrap();
        let file = t.path().join("file");
        std::fs::write(&file, b"not a directory").unwrap();
        for path in [&file, &t.path().join("missing")] {
            assert!(select_content_dir(&data, path).is_err());
            assert_eq!(read(&data).current, valid);
        }
        #[cfg(unix)]
        {
            let outside = t.path().join("outside");
            let selected = t.path().join("linked");
            std::fs::create_dir(&outside).unwrap();
            std::fs::create_dir(&selected).unwrap();
            std::os::unix::fs::symlink(&outside, selected.join("openrailsrs-content")).unwrap();
            assert!(select_content_dir(&data, &selected).is_err());
            assert_eq!(read(&data).current, valid);
            assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
        }
    }
}
