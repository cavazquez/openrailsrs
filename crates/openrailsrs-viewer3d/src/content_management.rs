//! Removal is a reversible move of one managed installation. Manual content
//! directories and paths inside another installation are never removal targets.
use crate::{player_settings::atomic_write, saved_game::SavedGame};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug)]
pub struct RemovalPlan {
    pub path: PathBuf,
    pub label: String,
    pub saves: Vec<String>,
    pub packages: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashEntry {
    pub schema: u32,
    pub directory: String,
    pub label: String,
    pub original_name: String,
}
fn managed(data: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = data
        .join("official-content")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if path.parent() != Some(root.as_path()) || !path.is_dir() {
        return Err("Sólo se pueden retirar copias instaladas en la biblioteca del visor".into());
    }
    let manifest = path
        .join("openrailsrs-content.json")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !manifest.starts_with(&path) {
        return Err("El manifiesto sale de la instalación".into());
    }
    Ok(path)
}
fn files(data: &Path) -> Vec<PathBuf> {
    let mut files = (1..=3)
        .map(|i| data.join(format!("save-{i}.json")))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    if let Ok(history) = std::fs::read_dir(data.join("history")) {
        files.extend(
            history
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .take(256),
        );
    }
    files
}
fn references(value: &serde_json::Value, path: &Path) -> bool {
    match value {
        serde_json::Value::String(s) => {
            Path::new(s).is_absolute() && Path::new(s).starts_with(path)
        }
        serde_json::Value::Array(a) => a.iter().any(|v| references(v, path)),
        serde_json::Value::Object(a) => a.values().any(|v| references(v, path)),
        _ => false,
    }
}
pub fn inspect_removal(data: &Path, path: &Path) -> Result<RemovalPlan, String> {
    let path = managed(data, path)?;
    let mut plan = RemovalPlan {
        label: crate::official_content::installed_label(&path),
        path: path.clone(),
        saves: vec![],
        packages: vec![],
    };
    for file in files(data) {
        openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
        if let Ok(game) = SavedGame::read(&file) {
            let route = game
                .route_root
                .as_ref()
                .is_some_and(|p| p.starts_with(&path));
            let scenario = toml::from_str::<toml::Value>(&game.scenario_toml)
                .ok()
                .and_then(|v| serde_json::to_value(v).ok())
                .is_some_and(|v| references(&v, &path));
            if route || scenario {
                plan.saves.push(format!(
                    "{} · {}",
                    file.file_name().unwrap_or_default().to_string_lossy(),
                    game.train_name
                ));
            }
        }
    }
    if let Ok(packages) = std::fs::read_dir(data.join("official-content")) {
        for package in packages.flatten().take(512) {
            let candidate = package.path();
            if candidate == path {
                continue;
            }
            for name in ["openrailsrs-prepared.json", "openrailsrs-ready.json"] {
                let Some(file) = candidate.join(name).canonicalize().ok().filter(|p| {
                    candidate
                        .canonicalize()
                        .ok()
                        .is_some_and(|root| p.starts_with(root))
                }) else {
                    continue;
                };
                if std::fs::metadata(&file).is_ok_and(|m| m.len() <= 8 * 1024 * 1024)
                    && let Ok(bytes) = std::fs::read(file)
                    && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                    && references(&value, &path)
                {
                    plan.packages
                        .push(crate::official_content::installed_label(&candidate));
                    break;
                }
            }
        }
    }
    Ok(plan)
}
pub fn remove(data: &Path, path: &Path) -> Result<TrashEntry, String> {
    let plan = inspect_removal(data, path)?;
    let original_name = plan
        .path
        .file_name()
        .ok_or("Copia sin nombre")?
        .to_string_lossy()
        .into_owned();
    let directory = format!(
        "content-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let root = data.join("content-trash");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let destination = root.join(&directory);
    if destination.exists() {
        return Err("Identificador de recuperación ocupado; intentá nuevamente".into());
    }
    let entry = TrashEntry {
        schema: 1,
        directory,
        label: plan.label,
        original_name,
    };
    // Own the wrapper; the author's tree is moved as an unchanged child.
    std::fs::create_dir(&destination).map_err(|e| e.to_string())?;
    let package = destination.join("package");
    if let Err(error) = std::fs::rename(&plan.path, &package) {
        let _ = std::fs::remove_dir(&destination);
        return Err(error.to_string());
    }
    if let Err(error) = atomic_write(
        &destination.join("recovery.json"),
        &serde_json::to_vec_pretty(&entry).map_err(|e| e.to_string())?,
    ) {
        let _ = std::fs::rename(&package, &plan.path);
        let _ = std::fs::remove_dir(&destination);
        return Err(error);
    }
    Ok(entry)
}
fn leaf(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
}
pub fn trash(data: &Path) -> Vec<TrashEntry> {
    let root = data.join("content-trash");
    let mut entries = vec![];
    if let Ok(files) = std::fs::read_dir(&root) {
        for file in files.flatten().take(128) {
            if let Ok(path) = file.path().canonicalize()
                && root
                    .canonicalize()
                    .is_ok_and(|r| path.parent() == Some(r.as_path()))
            {
                let manifest = path.join("recovery.json");
                if manifest.canonicalize().is_ok_and(|p| p.starts_with(&path))
                    && std::fs::metadata(&manifest).is_ok_and(|m| m.len() <= 16384)
                    && let Ok(bytes) = std::fs::read(manifest)
                    && let Ok(entry) = serde_json::from_slice::<TrashEntry>(&bytes)
                    && entry.schema == 1
                    && leaf(&entry.directory)
                    && leaf(&entry.original_name)
                    && path
                        .file_name()
                        .is_some_and(|n| n == entry.directory.as_str())
                {
                    entries.push(entry);
                }
            }
        }
    }
    entries
}
pub fn restore(data: &Path, entry: &TrashEntry) -> Result<PathBuf, String> {
    if entry.schema != 1 || !leaf(&entry.directory) || !leaf(&entry.original_name) {
        return Err("Referencia de recuperación inválida".into());
    }
    let root = data
        .join("content-trash")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let source = root
        .join(&entry.directory)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if source.parent() != Some(root.as_path()) {
        return Err("La copia sale de la carpeta de recuperación".into());
    }
    let package = source
        .join("package")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if package.parent() != Some(source.as_path()) || !package.is_dir() {
        return Err("Contenido recuperable inválido".into());
    }
    let destination = data.join("official-content").join(&entry.original_name);
    if destination.symlink_metadata().is_ok() {
        return Err("La versión ya está instalada; se conserva la copia recuperable".into());
    }
    std::fs::rename(&package, &destination).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(source.join("recovery.json"));
    let _ = std::fs::remove_dir(source);
    Ok(destination)
}
#[derive(Clone, Debug)]
pub struct VersionDiff {
    pub before: String,
    pub after: String,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub details: Vec<String>,
    pub saves: Vec<String>,
}
fn inventory(root: &Path) -> Result<std::collections::BTreeMap<PathBuf, String>, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(directory)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !entry.file_name().to_string_lossy().starts_with('.') {
                    pending.push(path);
                }
                continue;
            }
            if kind.is_file()
                && !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("openrailsrs-")
            {
                let hash = openrailsrs_train::consist_audit::content_file_sha256(&path)
                    .map_err(|e| e.to_string())?;
                out.insert(
                    path.strip_prefix(root).map_err(|e| e.to_string())?.into(),
                    hash,
                );
                if out.len() > 100_000 {
                    return Err("Paquete demasiado grande para comparar en esta versión".into());
                }
            }
        }
    }
    Ok(out)
}
pub fn compare(data: &Path, before: &Path, after: &Path) -> Result<VersionDiff, String> {
    let before = managed(data, before)?;
    let after = managed(data, after)?;
    if before == after {
        return Err("Elegí dos copias diferentes".into());
    }
    let a = inventory(&before)?;
    let b = inventory(&after)?;
    let mut diff = VersionDiff {
        before: crate::official_content::installed_label(&before),
        after: crate::official_content::installed_label(&after),
        added: 0,
        removed: 0,
        changed: 0,
        details: vec![],
        saves: inspect_removal(data, &before)?.saves,
    };
    for (path, hash) in &b {
        let kind = if !a.contains_key(path) {
            diff.added += 1;
            Some("Nuevo")
        } else if a[path] != *hash {
            diff.changed += 1;
            Some("Modificado")
        } else {
            None
        };
        if let Some(kind) = kind
            && diff.details.len() < 128
        {
            diff.details.push(format!("{kind}: {}", path.display()));
        }
    }
    for path in a.keys().filter(|p| !b.contains_key(*p)) {
        diff.removed += 1;
        if diff.details.len() < 128 {
            diff.details.push(format!("Retirado: {}", path.display()));
        }
    }
    Ok(diff)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_is_reversible_and_rejects_unmanaged_or_overlapping_destination() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("official-content/route-123");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("openrailsrs-content.json"), b"{}").unwrap();
        std::fs::write(package.join("original.eng"), b"original").unwrap();
        std::fs::write(
            package.join("openrailsrs-recovery.json"),
            b"author-reserved-name-is-legal",
        )
        .unwrap();
        assert!(remove(temp.path(), temp.path()).is_err());
        let entry = remove(temp.path(), &package).unwrap();
        assert!(!package.exists());
        assert_eq!(trash(temp.path()).len(), 1);
        std::fs::create_dir(&package).unwrap();
        assert!(restore(temp.path(), &entry).is_err());
        std::fs::remove_dir(&package).unwrap();
        restore(temp.path(), &entry).unwrap();
        assert_eq!(
            std::fs::read(package.join("openrailsrs-recovery.json")).unwrap(),
            b"author-reserved-name-is-legal"
        );
        assert_eq!(
            std::fs::read(package.join("original.eng")).unwrap(),
            b"original"
        );
    }
    #[test]
    fn version_comparison_counts_changed_added_and_removed_original_files() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("official-content/a");
        let b = t.path().join("official-content/b");
        for p in [&a, &b] {
            std::fs::create_dir_all(p).unwrap();
            std::fs::write(p.join("openrailsrs-content.json"), b"{}").unwrap();
        }
        std::fs::write(a.join("same.eng"), b"before").unwrap();
        std::fs::write(b.join("same.eng"), b"after").unwrap();
        std::fs::write(a.join("retired.s"), b"old").unwrap();
        std::fs::write(b.join("new.s"), b"new").unwrap();
        let diff = compare(t.path(), &a, &b).unwrap();
        assert_eq!((diff.changed, diff.added, diff.removed), (1, 1, 1));
        assert_eq!(std::fs::read(a.join("same.eng")).unwrap(), b"before");
        assert!(compare(t.path(), &a, &a).is_err());
    }
}
