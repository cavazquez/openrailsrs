//! Resolve MSTS cab view assets (`.eng` → `CABVIEW3D/` → `.s` + `.cvf`).
//!
//! Open Rails uses `ORTS3DCabFile` on the lead `.eng`; OpenBVE/MSTS classic uses `CabView`.
//! The typed [`EngineCabView`] lives on [`crate::typed::EngineFile`]; this module resolves paths on disk.

use std::path::{Path, PathBuf};

use crate::encoding::resolve_path_case_insensitive;
use crate::typed::EngineCabView;

/// Resolved cab interior assets under a trainset folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCabAssets {
    pub cab_dir: PathBuf,
    pub shape_path: PathBuf,
    pub cvf_path: PathBuf,
}

/// Resolve cab shape + CVF from parsed `.eng` fields and a trainset root.
///
/// Priority: `ORTS3DCabFile` → `CabView` → first `.s` with sibling `.cvf` in a cabview folder.
pub fn resolve_cab_assets(trainset_root: &Path, cab: &EngineCabView) -> Option<ResolvedCabAssets> {
    if let Some(shape_ref) = cab.orts_3d_cab_shape.as_deref() {
        if let Some(assets) = resolve_from_shape_ref(trainset_root, shape_ref) {
            return Some(assets);
        }
    }
    if let Some(cvf_ref) = cab.cab_view_file.as_deref() {
        if let Some(assets) = resolve_from_cab_view_ref(trainset_root, cvf_ref) {
            return Some(assets);
        }
    }
    resolve_cab_assets_scan(trainset_root)
}

/// Scan `CABVIEW3D` / `CabView` folders for the first shape paired with a `.cvf`.
pub fn resolve_cab_assets_scan(trainset_root: &Path) -> Option<ResolvedCabAssets> {
    let cab_dir = find_cabview_dir(trainset_root)?;
    let shape_path = pick_cab_shape_in_dir(&cab_dir)?;
    let cvf_path = cvf_path_for_shape(&shape_path);
    Some(ResolvedCabAssets {
        cab_dir,
        shape_path,
        cvf_path,
    })
}

/// Resolve the cabview folder under a trainset (`CABVIEW3D`, `Cabview3d`, …).
pub fn find_cabview_dir(trainset_root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(trainset_root).ok()?;
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.eq_ignore_ascii_case("cabview3d")
            || name.eq_ignore_ascii_case("cabview")
            || name.eq_ignore_ascii_case("cabview2d")
        {
            candidates.push(path);
        }
    }
    candidates.sort_by(|a, b| {
        let a3d = a
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains('3'));
        let b3d = b
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains('3'));
        b3d.cmp(&a3d)
    });
    for dir in &candidates {
        if pick_cab_shape_in_dir(dir).is_some() {
            return Some(dir.clone());
        }
    }
    None
}

/// Pick the main cab `.s` (paired with `.cvf`, e.g. `PULLMAN_GR.s`).
pub fn pick_cab_shape_in_dir(cab_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(cab_dir).ok()?;
    let mut shapes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("s"))
        {
            shapes.push(path);
        }
    }
    for path in &shapes {
        let cvf = cvf_path_for_shape(path);
        if cvf.is_file() {
            return Some(path.clone());
        }
        if let Some(resolved) = resolve_path_case_insensitive(&cvf) {
            if resolved.is_file() {
                return Some(path.clone());
            }
        }
    }
    for preferred in ["cab.s", "Cab.s", "CAB.s"] {
        let path = cab_dir.join(preferred);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    shapes.into_iter().next()
}

fn resolve_from_shape_ref(trainset_root: &Path, shape_ref: &str) -> Option<ResolvedCabAssets> {
    let shape_name = normalize_asset_name(shape_ref);
    let cab_dir = find_cabview_dir(trainset_root)?;
    let shape_path = resolve_file_in_dir(&cab_dir, &shape_name)?;
    let cvf_path = resolve_cvf_in_dir(&cab_dir, &shape_path)?;
    Some(ResolvedCabAssets {
        cab_dir,
        shape_path,
        cvf_path,
    })
}

fn resolve_from_cab_view_ref(trainset_root: &Path, cvf_ref: &str) -> Option<ResolvedCabAssets> {
    let cvf_name = normalize_asset_name(cvf_ref);
    for dir in cabview_search_dirs(trainset_root) {
        let Some(cvf_path) = resolve_file_in_dir(&dir, &cvf_name) else {
            continue;
        };
        let stem = cvf_path.file_stem()?.to_str()?;
        let shape_path = resolve_file_in_dir(&dir, &format!("{stem}.s"))
            .or_else(|| resolve_file_in_dir(trainset_root, &format!("{stem}.s")))?;
        return Some(ResolvedCabAssets {
            cab_dir: dir,
            shape_path,
            cvf_path,
        });
    }
    None
}

fn cabview_search_dirs(trainset_root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(trainset_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.eq_ignore_ascii_case("cabview3d")
                || name.eq_ignore_ascii_case("cabview")
                || name.eq_ignore_ascii_case("cabview2d")
            {
                dirs.push(path);
            }
        }
    }
    dirs.sort_by(|a, b| {
        let a3d = a
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains('3'));
        let b3d = b
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains('3'));
        b3d.cmp(&a3d)
    });
    if dirs.is_empty() {
        dirs.push(trainset_root.to_path_buf());
    }
    dirs
}

fn resolve_file_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = dir.join(name);
    if path.is_file() {
        return Some(path);
    }
    resolve_path_case_insensitive(&path).filter(|p| p.is_file())
}

fn resolve_cvf_in_dir(_cab_dir: &Path, shape_path: &Path) -> Option<PathBuf> {
    let cvf = cvf_path_for_shape(shape_path);
    if cvf.is_file() {
        return Some(cvf);
    }
    resolve_path_case_insensitive(&cvf).filter(|p| p.is_file())
}

fn cvf_path_for_shape(shape_path: &Path) -> PathBuf {
    shape_path.with_extension("cvf")
}

fn normalize_asset_name(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c| c == '(' || c == ')')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::EngineCabView;

    fn temp_trainset(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "openrailsrs_cab_link_{label}_{}",
            std::process::id()
        ))
    }

    #[test]
    fn resolve_from_orts_3d_cab_file() {
        let dir = temp_trainset("orts3d");
        let cab3d = dir.join("Cabview3d");
        std::fs::create_dir_all(&cab3d).unwrap();
        std::fs::write(cab3d.join("PULLMAN_GR.s"), b"").unwrap();
        std::fs::write(cab3d.join("PULLMAN_GR.cvf"), b"").unwrap();

        let cab = EngineCabView {
            orts_3d_cab_shape: Some("PULLMAN_GR.s".into()),
            ..Default::default()
        };
        let assets = resolve_cab_assets(&dir, &cab).expect("resolved");
        assert!(assets.shape_path.ends_with("PULLMAN_GR.s"));
        assert!(assets.cvf_path.ends_with("PULLMAN_GR.cvf"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_from_cab_view_2d_ref() {
        let dir = temp_trainset("cabview2d");
        let cab2d = dir.join("CabView");
        std::fs::create_dir_all(&cab2d).unwrap();
        std::fs::write(cab2d.join("GP38.cvf"), b"").unwrap();
        std::fs::write(cab2d.join("GP38.s"), b"").unwrap();

        let cab = EngineCabView {
            cab_view_file: Some("GP38.cvf".into()),
            ..Default::default()
        };
        let assets = resolve_cab_assets(&dir, &cab).expect("resolved");
        assert!(assets.cvf_path.ends_with("GP38.cvf"));
        assert!(assets.shape_path.ends_with("GP38.s"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_prefers_3d_cabview_folder() {
        let dir = temp_trainset("scan3d");
        let cab2d = dir.join("CabView");
        let cab3d = dir.join("Cabview3d");
        std::fs::create_dir_all(&cab2d).unwrap();
        std::fs::create_dir_all(&cab3d).unwrap();
        std::fs::write(cab2d.join("old.cvf"), b"").unwrap();
        std::fs::write(cab2d.join("old.s"), b"").unwrap();
        std::fs::write(cab3d.join("PULLMAN_GR.s"), b"").unwrap();
        std::fs::write(cab3d.join("PULLMAN_GR.cvf"), b"").unwrap();

        let assets = resolve_cab_assets_scan(&dir).expect("scan");
        assert!(assets.shape_path.ends_with("PULLMAN_GR.s"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
