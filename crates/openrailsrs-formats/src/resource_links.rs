//! Explicitly authorized original installations; no recursive search or download.
use std::{
    path::{Component, Path, PathBuf},
    sync::{OnceLock, RwLock},
};
fn roots() -> &'static RwLock<Vec<PathBuf>> {
    static R: OnceLock<RwLock<Vec<PathBuf>>> = OnceLock::new();
    R.get_or_init(Default::default)
}
pub fn configured() -> Vec<PathBuf> {
    roots().read().unwrap_or_else(|e| e.into_inner()).clone()
}
pub fn configure(selected: &[PathBuf]) -> Result<(), String> {
    if selected.len() > 8 {
        return Err("Se admiten hasta ocho instalaciones originales compartidas".into());
    }
    let mut checked = vec![];
    for root in selected {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let trainset =
            crate::encoding::resolve_path_case_insensitive_direct(&root.join("TRAINS/TRAINSET"))
                .ok_or("La instalación debe conservar TRAINS/TRAINSET")?;
        if !trainset.is_dir()
            || !trainset
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&root)
        {
            return Err("TRAINSET sale de la instalación".into());
        }
        if !checked.contains(&root) {
            checked.push(root);
        }
    }
    *roots().write().unwrap_or_else(|e| e.into_inner()) = checked;
    Ok(())
}
pub(crate) fn resolve_missing(path: &Path) -> Option<PathBuf> {
    resolve_with(path, &configured())
}
pub fn resolve_with(path: &Path, selected: &[PathBuf]) -> Option<PathBuf> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if ![
        "eng", "wag", "con", "s", "sd", "ace", "dds", "ktx2", "wav", "sms", "cvf", "inc", "dat",
    ]
    .contains(&extension.as_str())
    {
        return None;
    }
    let components: Vec<_> = path.components().collect();
    let start=components.iter().position(|c|matches!(c,Component::Normal(n) if ["TRAINS","ROUTES","GLOBAL"].iter().any(|tag|n.eq_ignore_ascii_case(tag))))?;
    let suffix: PathBuf = components[start..].iter().collect();
    // Never interpret parent traversals supplied by a missing-resource name.
    if suffix
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    for root in selected.iter().take(8) {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        let Some(candidate) =
            crate::encoding::resolve_path_case_insensitive_direct(&root.join(&suffix))
        else {
            continue;
        };
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        if candidate.is_file() && candidate.starts_with(root) {
            return Some(candidate);
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_native_reference_is_confined_and_does_not_scan_elsewhere() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("original");
        let data = root.join("TRAINS/TRAINSET/link_test_only_092");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("car.wag"), "source").unwrap();
        let requested = Path::new("/missing/TRAINS/TRAINSET/link_test_only_092/CAR.WAG");
        assert!(resolve_with(requested, &[]).is_none());
        assert_eq!(
            resolve_with(requested, std::slice::from_ref(&root)).unwrap(),
            data.join("car.wag")
        );
        assert!(
            resolve_with(
                Path::new("/missing/TRAINS/../outside/car.wag"),
                std::slice::from_ref(&root)
            )
            .is_none()
        );
        #[cfg(unix)]
        {
            let outside = temp.path().join("outside.wag");
            std::fs::write(&outside, "private").unwrap();
            std::os::unix::fs::symlink(&outside, data.join("escape.wag")).unwrap();
            assert!(
                resolve_with(
                    Path::new("/missing/TRAINS/TRAINSET/link_test_only_092/escape.wag"),
                    std::slice::from_ref(&root)
                )
                .is_none()
            );
        }
    }
}
