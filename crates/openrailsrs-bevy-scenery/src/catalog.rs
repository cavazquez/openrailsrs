//! Catálogo CPU de assets de una ruta MSTS/OR (shapes, texturas, tsection).
//!
//! Distinto de [`crate::MstsRouteCatalogAsset`] (manifiesto JSON `.routecat` del AssetServer).
//! Aquí vive el índice runtime compartido por viewer3d (`RouteAssets`) y render3d (`AssetIndex`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openrailsrs_formats::{
    TSectionCatalog, resolve_hazard_shape_name, resolve_route_relative_file,
};

use crate::textures::{
    TextureEnvironment, TextureFlags, global_assets_dirs, index_textures_tree,
    index_trainset_textures, resolve_shape_path, resolve_shape_path_in_dirs,
    resolve_texture_with_index, shape_file_basename, shape_search_dirs,
};

/// Índice case-insensitive de `.s` / `.ace` + catálogo `tsection` para una ruta.
#[derive(Clone, Debug)]
pub struct MstsRouteCatalog {
    pub route_dir: PathBuf,
    pub msts_root: PathBuf,
    shapes: HashMap<String, PathBuf>,
    textures: HashMap<String, PathBuf>,
    pub tsection: TSectionCatalog,
}

impl MstsRouteCatalog {
    /// Escanea SHAPES/TEXTURES una vez con precedencia **ruta > pack > GLOBAL**
    /// (trainsets solo para texturas, prioridad más baja).
    pub fn build(route_dir: &Path, msts_root: &Path) -> Self {
        let mut shapes = HashMap::new();
        let mut textures = HashMap::new();

        // Baja → alta: insert sobrescribe.
        index_trainset_textures(&mut textures, msts_root);

        for global in global_assets_dirs(route_dir, msts_root) {
            index_shapes_root(&mut shapes, &global);
            index_textures_tree(&mut textures, &global);
        }

        if let Some(pack) = route_pack_dir(route_dir, msts_root) {
            index_shapes_root(&mut shapes, &pack);
            index_textures_tree(&mut textures, &pack);
        }

        index_shapes_root(&mut shapes, route_dir);
        index_textures_tree(&mut textures, route_dir);
        for sub in ["Alias", "alias"] {
            index_textures_tree(&mut textures, &route_dir.join(sub));
        }

        let tsection = load_tsection_catalog(route_dir, msts_root);

        Self {
            route_dir: route_dir.to_path_buf(),
            msts_root: msts_root.to_path_buf(),
            shapes,
            textures,
            tsection,
        }
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn shapes(&self) -> &HashMap<String, PathBuf> {
        &self.shapes
    }

    pub fn textures(&self) -> &HashMap<String, PathBuf> {
        &self.textures
    }

    /// Lookup por basename (case-insensitive) + fallback a búsqueda en disco.
    pub fn resolve_shape(&self, file_name: &str) -> Option<PathBuf> {
        if file_name.is_empty() {
            return None;
        }
        let base = shape_file_basename(file_name);
        if let Some(path) = self.shapes.get(&base.to_ascii_lowercase()) {
            return Some(path.clone());
        }
        let dirs = shape_search_dirs(&self.route_dir, &self.msts_root);
        let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
        resolve_shape_path_in_dirs(&refs, file_name)
    }

    /// Scenery / Hazard / TrackObj con reglas Open Rails de precedencia de búsqueda.
    pub fn resolve_world_shape(&self, kind: &str, file_name: &str) -> Option<PathBuf> {
        if file_name.is_empty() {
            return None;
        }
        if kind == "Hazard" {
            let shape_name = resolve_hazard_shape_name(&self.route_dir, file_name)?;
            let base = shape_file_basename(&shape_name);
            for global in global_assets_dirs(&self.route_dir, &self.msts_root) {
                if let Some(path) = resolve_shape_path(&global, base) {
                    return Some(path);
                }
            }
            return self.resolve_shape(base);
        }
        if let Some(path) = resolve_route_relative_file(&self.route_dir, file_name) {
            return Some(path);
        }
        let base = shape_file_basename(file_name);
        if kind == "TrackObj" {
            for global in global_assets_dirs(&self.route_dir, &self.msts_root) {
                if let Some(path) = resolve_shape_path(&global, base) {
                    return Some(path);
                }
            }
            if let Some(path) = resolve_shape_path(&self.msts_root.join("GLOBAL"), base) {
                return Some(path);
            }
        }
        self.resolve_shape(base)
    }

    /// `TrackObj`: `FileName` y/o `SectionIdx` → `tsection.dat`.
    pub fn resolve_trackobj_shape(
        &self,
        file_name: Option<&str>,
        section_idx: Option<u32>,
    ) -> Option<PathBuf> {
        let name = file_name
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                section_idx.and_then(|idx| self.tsection.shape_file_name(idx).map(str::to_string))
            })?;
        if let Some(path) = resolve_route_relative_file(&self.route_dir, &name) {
            return Some(path);
        }
        self.resolve_world_shape("TrackObj", &name)
    }

    pub fn resolve_texture(
        &self,
        dirs: &[&Path],
        file_name: &str,
        env: &TextureEnvironment,
        flags: TextureFlags,
    ) -> Option<PathBuf> {
        resolve_texture_with_index(&self.textures, dirs, file_name, env, flags)
    }
}

/// Pack MSTS de la ruta (`Content/<stem>/`), case-insensitive.
pub fn route_pack_dir(route_dir: &Path, msts_root: &Path) -> Option<PathBuf> {
    let stem = route_dir.file_name()?.to_str()?;
    let pack = msts_root.join(stem);
    if pack.is_dir() {
        return Some(pack);
    }
    let Ok(rd) = std::fs::read_dir(msts_root) else {
        return None;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(stem))
        {
            return Some(path);
        }
    }
    None
}

fn load_tsection_catalog(route_dir: &Path, msts_root: &Path) -> TSectionCatalog {
    if let Ok(catalog) = TSectionCatalog::load_for_route(route_dir) {
        if !catalog.shapes.is_empty() {
            return catalog;
        }
    }
    for candidate in msts_route_dir_candidates(route_dir, msts_root) {
        if let Ok(catalog) = TSectionCatalog::load_for_route(&candidate) {
            if !catalog.shapes.is_empty() {
                return catalog;
            }
        }
    }
    TSectionCatalog::load_for_route(route_dir).unwrap_or_default()
}

fn msts_route_dir_candidates(route_dir: &Path, msts_root: &Path) -> Vec<PathBuf> {
    let Some(stem) = route_dir.file_name().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let mut candidates = vec![
        msts_root.join(stem).join("ROUTES").join(stem),
        msts_root.join("ROUTES").join(stem),
    ];
    if let Ok(entries) = std::fs::read_dir(msts_root) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(stem)
            {
                let pack = entry.path();
                candidates.push(pack.join("ROUTES").join(stem));
                if let Ok(route_entries) = std::fs::read_dir(pack.join("ROUTES")) {
                    for route in route_entries.flatten() {
                        if route
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(stem)
                        {
                            candidates.push(route.path());
                        }
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .filter(|p| {
            p.is_dir()
                && ([
                    "OpenRails/tsection.dat",
                    "openrails/tsection.dat",
                    "tsection.dat",
                ]
                .iter()
                .any(|rel| p.join(rel).is_file())
                    || p.join("WORLD").is_dir()
                    || p.join("world").is_dir())
        })
        .collect()
}

fn index_shapes_root(map: &mut HashMap<String, PathBuf>, root: &Path) {
    for subdir in ["SHAPES", "shapes"] {
        index_shapes_tree(map, &root.join(subdir));
    }
    index_shapes_tree(map, root);
}

/// Indexa recursivamente `.s` (última escritura gana — usar capas de baja→alta prioridad).
pub fn index_shapes_tree(map: &mut HashMap<String, PathBuf>, root: &Path) {
    if !root.is_dir() {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(root) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            index_shapes_tree(map, &path);
            continue;
        }
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("s"))
        {
            continue;
        }
        if let Some(name) = path.file_name() {
            map.insert(name.to_string_lossy().to_ascii_lowercase(), path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn route_shape_overrides_global_same_basename() {
        let tmp = tempfile::tempdir().unwrap();
        let msts = tmp.path().join("Content");
        let route = msts.join("ROUTES").join("Demo");
        write_file(
            &msts.join("GLOBAL/SHAPES/shared.s"),
            "SIMISA@@@@@@@@@@JINX0s1t______\nshape ()\n",
        );
        write_file(
            &route.join("SHAPES/shared.s"),
            "SIMISA@@@@@@@@@@JINX0s1t______\nshape ( route )\n",
        );
        write_file(
            &route.join("SHAPES/RouteOnly.s"),
            "SIMISA@@@@@@@@@@JINX0s1t______\nshape ()\n",
        );

        let catalog = MstsRouteCatalog::build(&route, &msts);
        let shared = catalog.resolve_shape("shared.s").expect("shared");
        assert!(
            shared.starts_with(&route),
            "ruta debe ganar sobre GLOBAL: {}",
            shared.display()
        );
        assert!(catalog.resolve_shape("ROUTEONLY.s").unwrap().is_file());
        assert_eq!(
            catalog.resolve_shape("Shared.S").as_ref(),
            Some(&shared),
            "lookup case-insensitive"
        );
    }

    #[test]
    fn pack_shape_overrides_global_but_not_route() {
        let tmp = tempfile::tempdir().unwrap();
        let msts = tmp.path().join("Content");
        let route = msts.join("ROUTES").join("Demo");
        let pack = msts.join("Demo");
        write_file(
            &msts.join("GLOBAL/SHAPES/tier.s"),
            "SIMISA@@@@@@@@@@JINX0s1t______\nshape ( global )\n",
        );
        write_file(
            &pack.join("SHAPES/tier.s"),
            "SIMISA@@@@@@@@@@JINX0s1t______\nshape ( pack )\n",
        );
        write_file(
            &route.join("SHAPES/tier.s"),
            "SIMISA@@@@@@@@@@JINX0s1t______\nshape ( route )\n",
        );

        let catalog = MstsRouteCatalog::build(&route, &msts);
        let path = catalog.resolve_shape("tier.s").unwrap();
        assert!(
            path.starts_with(&route),
            "ruta > pack > GLOBAL: {}",
            path.display()
        );

        // Sin shape de ruta: gana el pack.
        fs::remove_file(route.join("SHAPES/tier.s")).unwrap();
        let catalog = MstsRouteCatalog::build(&route, &msts);
        let path = catalog.resolve_shape("tier.s").unwrap();
        assert!(
            path.starts_with(&pack),
            "pack > GLOBAL: {}",
            path.display()
        );
    }

    #[test]
    fn texture_route_overrides_global() {
        let tmp = tempfile::tempdir().unwrap();
        let msts = tmp.path().join("Content");
        let route = msts.join("ROUTES").join("Demo");
        write_file(&msts.join("GLOBAL/TEXTURES/shared.ace"), "ace-global");
        write_file(&route.join("TEXTURES/shared.ace"), "ace-route");

        let catalog = MstsRouteCatalog::build(&route, &msts);
        let key = "shared.ace";
        let path = catalog.textures().get(key).expect("indexed");
        assert!(
            path.starts_with(&route),
            "textura de ruta debe ganar: {}",
            path.display()
        );
    }

    #[test]
    fn shape_search_dirs_preserve_route_before_global() {
        let tmp = tempfile::tempdir().unwrap();
        let msts = tmp.path().join("Content");
        let route = msts.join("ROUTES").join("Demo");
        fs::create_dir_all(route.join("SHAPES")).unwrap();
        fs::create_dir_all(msts.join("GLOBAL/SHAPES")).unwrap();

        let dirs = shape_search_dirs(&route, &msts);
        let route_pos = dirs.iter().position(|d| d == &route).expect("route");
        let global_pos = dirs
            .iter()
            .position(|d| d == &msts.join("GLOBAL"))
            .expect("global");
        assert!(
            route_pos < global_pos,
            "route debe ir antes que GLOBAL en búsqueda: {dirs:?}"
        );
    }
}
