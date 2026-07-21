//! Case-insensitive WORLD / TILES path resolution for Linux hosts.
//!
//! MSTS content often mixes `WORLD`/`World`, `.w`/`.W`, and hash `.t`/`.T` names.
//! Lookups by tile coordinates and directory scans must share the same semantics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::encoding::resolve_path_case_insensitive;
use crate::msts_tile_name::{
    msts_tile_name_from_xz, parse_world_w_tile_xz, world_w_filename_from_tile_xz,
};

/// Find a direct child directory whose name matches `name` case-insensitively.
///
/// When several matches exist, the lexicographically smallest path is returned
/// (deterministic on Linux).
pub fn find_named_subdir(parent: &Path, name: &str) -> Option<PathBuf> {
    let exact = parent.join(name);
    if exact.is_dir() {
        return Some(exact);
    }
    let want = name.to_ascii_lowercase();
    let mut matches = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&want))
        {
            matches.push(path);
        }
    }
    matches.sort();
    matches.into_iter().next()
}

/// All WORLD-like directories under a route (`WORLD`, `world`, `World`, …).
pub fn world_subdirs(route_dir: &Path) -> Vec<PathBuf> {
    named_subdirs(route_dir, "world")
}

/// All TILES-like directories under a route.
pub fn tiles_subdirs(route_dir: &Path) -> Vec<PathBuf> {
    named_subdirs(route_dir, "tiles")
}

/// All TERRAIN-like directories under a route (legacy `.y` tiles).
pub fn terrain_subdirs(route_dir: &Path) -> Vec<PathBuf> {
    named_subdirs(route_dir, "terrain")
}

fn named_subdirs(parent: &Path, name: &str) -> Vec<PathBuf> {
    let want = name.to_ascii_lowercase();
    let mut matches = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return matches;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&want))
        {
            matches.push(path);
        }
    }
    matches.sort();
    matches
}

/// Resolve `WORLD/w±XXXXXX±ZZZZZZ.w` for tile coords (folder + filename case-insensitive).
pub fn resolve_world_tile_file(route_dir: &Path, tile_x: i32, tile_z: i32) -> Option<PathBuf> {
    let name = world_w_filename_from_tile_xz(tile_x, tile_z);
    for dir in world_subdirs(route_dir) {
        let candidate = dir.join(&name);
        if let Some(resolved) = resolve_path_case_insensitive(&candidate) {
            if resolved.is_file() {
                return Some(resolved);
            }
        }
        // Filename may differ only in case from the canonical stem/extension.
        if let Some(found) = find_file_case_insensitive(&dir, &name) {
            return Some(found);
        }
    }
    None
}

/// Resolve `TILES/<hash>.t` for tile coords (folder + filename case-insensitive).
pub fn resolve_hash_terrain_tile_file(
    route_dir: &Path,
    tile_x: i32,
    tile_z: i32,
) -> Option<PathBuf> {
    let hash = msts_tile_name_from_xz(tile_x, tile_z);
    let names = [
        format!("{}.t", hash.to_ascii_lowercase()),
        format!("{}.t", hash),
        format!("{}.T", hash.to_ascii_lowercase()),
    ];
    for dir in tiles_subdirs(route_dir) {
        for name in &names {
            let candidate = dir.join(name);
            if let Some(resolved) = resolve_path_case_insensitive(&candidate) {
                if resolved.is_file() {
                    return Some(resolved);
                }
            }
            if let Some(found) = find_file_case_insensitive(&dir, name) {
                return Some(found);
            }
        }
        // Last resort: match stem case-insensitively with .t/.T.
        let want_stem = hash.to_ascii_lowercase();
        if let Some(found) = find_terrain_t_by_stem(&dir, &want_stem) {
            return Some(found);
        }
    }
    None
}

fn find_file_case_insensitive(dir: &Path, filename: &str) -> Option<PathBuf> {
    let want = filename.to_ascii_lowercase();
    let mut matches = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().to_ascii_lowercase() == want)
        {
            matches.push(path);
        }
    }
    matches.sort();
    matches.into_iter().next()
}

fn find_terrain_t_by_stem(dir: &Path, stem_lower: &str) -> Option<PathBuf> {
    let mut matches = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext_ok = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("t"));
        let stem_ok = path
            .file_stem()
            .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(stem_lower));
        if ext_ok && stem_ok {
            matches.push(path);
        }
    }
    matches.sort();
    matches.into_iter().next()
}

/// Scan WORLD dirs for `.w`/`.W` files → `(tile_x, tile_z, path)`.
///
/// Duplicate coordinates keep the lexicographically smallest path and are
/// reported via `duplicates` when provided.
pub fn scan_world_tile_files(route_dir: &Path) -> Vec<(i32, i32, PathBuf)> {
    let mut catalog: HashMap<(i32, i32), PathBuf> = HashMap::new();
    for dir in world_subdirs(route_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("w"))
            {
                continue;
            }
            let Some(xz) = parse_world_w_tile_xz(&path) else {
                continue;
            };
            catalog_insert(&mut catalog, xz, path);
        }
    }
    let mut out: Vec<_> = catalog.into_iter().map(|(xz, p)| (xz.0, xz.1, p)).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    out
}

/// Build `(tile_x, tile_z) → path` catalog for WORLD tiles.
pub fn build_world_tile_catalog(route_dir: &Path) -> HashMap<(i32, i32), PathBuf> {
    scan_world_tile_files(route_dir)
        .into_iter()
        .map(|(x, z, p)| ((x, z), p))
        .collect()
}

/// Terrain hash tiles discovered from WORLD `.w` names, resolving `.t` case-insensitively.
pub fn scan_hash_terrain_tiles_from_world(route_dir: &Path) -> Vec<(i32, i32, PathBuf)> {
    let mut catalog: HashMap<(i32, i32), PathBuf> = HashMap::new();
    for (tile_x, tile_z, _) in scan_world_tile_files(route_dir) {
        if let Some(path) = resolve_hash_terrain_tile_file(route_dir, tile_x, tile_z) {
            catalog_insert(&mut catalog, (tile_x, tile_z), path);
        }
    }
    let mut out: Vec<_> = catalog.into_iter().map(|(xz, p)| (xz.0, xz.1, p)).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    out
}

/// Build terrain hash-tile catalog keyed by coordinates.
pub fn build_terrain_tile_catalog(route_dir: &Path) -> HashMap<(i32, i32), PathBuf> {
    scan_hash_terrain_tiles_from_world(route_dir)
        .into_iter()
        .map(|(x, z, p)| ((x, z), p))
        .collect()
}

fn catalog_insert(catalog: &mut HashMap<(i32, i32), PathBuf>, key: (i32, i32), path: PathBuf) {
    match catalog.entry(key) {
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(path);
        }
        std::collections::hash_map::Entry::Occupied(mut slot) => {
            let existing = slot.get();
            let same_ignore_case = existing
                .to_string_lossy()
                .eq_ignore_ascii_case(path.to_string_lossy().as_ref());
            if !same_ignore_case {
                eprintln!(
                    "openrailsrs-formats: duplicate tile {:?}: keeping {} over {}",
                    key,
                    if path < *existing {
                        path.display()
                    } else {
                        existing.display()
                    },
                    if path < *existing {
                        existing.display()
                    } else {
                        path.display()
                    }
                );
            }
            if path < *existing {
                slot.insert(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_mixed_case_world_and_w_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("World");
        std::fs::create_dir_all(&world).expect("mkdir");
        let name = world_w_filename_from_tile_xz(-6080, 14925);
        // Store as uppercase extension / mixed stem prefix.
        let on_disk = world.join(name.to_ascii_uppercase());
        std::fs::write(&on_disk, b"dummy").expect("write");

        let resolved = resolve_world_tile_file(dir.path(), -6080, 14925).expect("resolve");
        assert!(resolved.is_file());
        assert!(
            resolved
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("w"))
        );

        let catalog = build_world_tile_catalog(dir.path());
        assert_eq!(
            catalog.get(&(-6080, 14925)).map(|p| p.as_path()),
            Some(resolved.as_path())
        );
    }

    #[test]
    fn resolves_mixed_case_tiles_hash_t() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("WORLD");
        let tiles = dir.path().join("Tiles");
        std::fs::create_dir_all(&world).expect("mkdir world");
        std::fs::create_dir_all(&tiles).expect("mkdir tiles");

        let w_name = world_w_filename_from_tile_xz(-6080, 14925);
        std::fs::write(world.join(&w_name), b"w").expect("write w");

        let hash = msts_tile_name_from_xz(-6080, 14925);
        let t_path = tiles.join(format!("{}.T", hash.to_ascii_uppercase()));
        std::fs::write(&t_path, b"t").expect("write t");

        let resolved = resolve_hash_terrain_tile_file(dir.path(), -6080, 14925).expect("t");
        assert!(resolved.is_file());

        let catalog = build_terrain_tile_catalog(dir.path());
        assert!(catalog.contains_key(&(-6080, 14925)));
    }

    #[test]
    fn scan_world_accepts_uppercase_w_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("WORLD");
        std::fs::create_dir_all(&world).expect("mkdir");
        let name = world_w_filename_from_tile_xz(1, 2).replace(".w", ".W");
        std::fs::write(world.join(&name), b"x").expect("write");
        let scanned = scan_world_tile_files(dir.path());
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].0, 1);
        assert_eq!(scanned[0].1, 2);
    }
}
