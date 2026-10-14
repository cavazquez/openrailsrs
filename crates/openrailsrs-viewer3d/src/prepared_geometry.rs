//! Versioned, source-addressed CPU geometry. No GPU handles or author files are
//! persisted, and all cache I/O runs in the existing preparation workers.
use crate::shapes::{LoadedShape, LoadedShapePart};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use openrailsrs_formats::ShapeFile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const SCHEMA: u32 = 1;
const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_CACHE: u64 = 256 * 1024 * 1024;
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);
fn compiler() -> &'static str {
    env!("OPENRAILSRS_GEOMETRY_COMPILER")
}

fn identity(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    let mut h = Sha256::new();
    h.update(
        path.canonicalize()
            .ok()?
            .as_os_str()
            .to_string_lossy()
            .as_bytes(),
    );
    h.update(std::fs::read(path).ok()?);
    if let Some(sd) = openrailsrs_formats::resolve_path_case_insensitive(&path.with_extension("sd"))
    {
        if sd.metadata().ok()?.len() > 1024 * 1024 {
            return None;
        }
        h.update(std::fs::read(sd).ok()?);
    }
    h.update(compiler().as_bytes());
    h.update(SCHEMA.to_le_bytes());
    Some(format!("{h:x}", h = h.finalize()))
}
pub fn source_identity(path: &Path) -> Option<String> {
    identity(path)
}
fn root() -> PathBuf {
    crate::player_settings::player_data_dir().join("cache/prepared-geometry")
}
#[derive(Serialize, Deserialize)]
struct CpuMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    indexed: bool,
}
impl CpuMesh {
    fn capture(mesh: &Mesh) -> Option<Self> {
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList
            || mesh.attributes().count() != 3
        {
            return None;
        }
        let VertexAttributeValues::Float32x3(positions) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
        else {
            return None;
        };
        let VertexAttributeValues::Float32x3(normals) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)?
        else {
            return None;
        };
        let VertexAttributeValues::Float32x2(uvs) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)? else {
            return None;
        };
        let indices = mesh
            .indices()
            .map_or_else(Vec::new, |v| v.iter().map(|i| i as u32).collect());
        Some(Self {
            positions: positions.clone(),
            normals: normals.clone(),
            uvs: uvs.clone(),
            indices,
            indexed: mesh.indices().is_some(),
        })
    }
    fn materialize(self) -> Option<Mesh> {
        let count = self.positions.len();
        if count > 1_000_000
            || self.normals.len() != count
            || self.uvs.len() != count
            || self.indices.len() > 6_000_000
            || !self.indices.len().is_multiple_of(3)
            || self.indices.iter().any(|i| *i as usize >= count)
            || self
                .positions
                .iter()
                .chain(&self.normals)
                .flatten()
                .any(|v| !v.is_finite())
            || self.uvs.iter().flatten().any(|v| !v.is_finite())
        {
            return None;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        if self.indexed {
            mesh.insert_indices(Indices::U32(self.indices));
        }
        Some(mesh)
    }
}
#[derive(Serialize, Deserialize)]
struct Part {
    mesh: CpuMesh,
    prim_state_idx: i32,
    sub_object_idx: u32,
    sort_index: u32,
    cab_matrix_idx: Option<usize>,
    texture_file: Option<String>,
    shader_name: Option<String>,
    solid_color: Option<[f32; 3]>,
    alpha_test_mode: i32,
    z_bias: Option<f32>,
    z_buf_mode: i32,
    light_mat_idx: Option<i32>,
    tex_addr_mode: Option<i32>,
    mip_map_lod_bias: Option<f32>,
    bounds_center: Option<[f32; 3]>,
    bounds_half_extent: Option<[f32; 3]>,
    lever_pivot_at_mesh_center: bool,
    lever_local_axis: Option<[f32; 3]>,
}
impl Part {
    fn capture(p: &LoadedShapePart) -> Option<Self> {
        Some(Self {
            mesh: CpuMesh::capture(&p.mesh)?,
            prim_state_idx: p.prim_state_idx,
            sub_object_idx: p.sub_object_idx,
            sort_index: p.sort_index,
            cab_matrix_idx: p.cab_matrix_idx,
            texture_file: p.texture_file.clone(),
            shader_name: p.shader_name.clone(),
            solid_color: p.solid_color,
            alpha_test_mode: p.alpha_test_mode,
            z_bias: p.z_bias,
            z_buf_mode: p.z_buf_mode,
            light_mat_idx: p.light_mat_idx,
            tex_addr_mode: p.tex_addr_mode,
            mip_map_lod_bias: p.mip_map_lod_bias,
            bounds_center: p.bounds_center.map(|v| v.to_array()),
            bounds_half_extent: p.bounds_half_extent.map(|v| v.to_array()),
            lever_pivot_at_mesh_center: p.lever_pivot_at_mesh_center,
            lever_local_axis: p.lever_local_axis.map(|v| v.to_array()),
        })
    }
    fn materialize(self) -> Option<LoadedShapePart> {
        Some(LoadedShapePart {
            mesh: self.mesh.materialize()?,
            prim_state_idx: self.prim_state_idx,
            sub_object_idx: self.sub_object_idx,
            sort_index: self.sort_index,
            cab_matrix_idx: self.cab_matrix_idx,
            texture_file: self.texture_file,
            shader_name: self.shader_name,
            solid_color: self.solid_color,
            alpha_test_mode: self.alpha_test_mode,
            z_bias: self.z_bias,
            z_buf_mode: self.z_buf_mode,
            light_mat_idx: self.light_mat_idx,
            tex_addr_mode: self.tex_addr_mode,
            mip_map_lod_bias: self.mip_map_lod_bias,
            bounds_center: self.bounds_center.map(Vec3::from_array),
            bounds_half_extent: self.bounds_half_extent.map(Vec3::from_array),
            lever_pivot_at_mesh_center: self.lever_pivot_at_mesh_center,
            lever_local_axis: self.lever_local_axis.map(Vec3::from_array),
        })
    }
}
#[derive(Serialize, Deserialize)]
struct Loaded {
    mesh: CpuMesh,
    texture_file: Option<String>,
    parts: Vec<Part>,
}
impl Loaded {
    fn capture(l: &LoadedShape) -> Option<Self> {
        Some(Self {
            mesh: CpuMesh::capture(&l.mesh)?,
            texture_file: l.texture_file.clone(),
            parts: l.parts.iter().map(Part::capture).collect::<Option<_>>()?,
        })
    }
    fn materialize(self) -> Option<LoadedShape> {
        Some(LoadedShape {
            mesh: self.mesh.materialize()?,
            texture_file: self.texture_file,
            parts: self
                .parts
                .into_iter()
                .map(Part::materialize)
                .collect::<Option<_>>()?,
        })
    }
}
#[derive(Serialize, Deserialize)]
struct File {
    schema: u32,
    key: String,
    shape: ShapeFile,
    base: Loaded,
    lods: Vec<Option<Loaded>>,
}
pub fn read(path: &Path) -> Option<(ShapeFile, LoadedShape, Vec<Option<LoadedShape>>)> {
    if disabled() {
        MISSES.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    read_at(path, &root())
}
fn read_at(path: &Path, root: &Path) -> Option<(ShapeFile, LoadedShape, Vec<Option<LoadedShape>>)> {
    let _timer = crate::performance::ShapeTimer::start("geometry_cache_read", path);
    let key = identity(path)?;
    let file = root.join(format!("{key}.json"));
    let cached = (|| {
        if file.is_symlink() || file.metadata().ok()?.len() > MAX_FILE {
            return None;
        }
        let value: File = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        if value.schema != SCHEMA
            || value.key != key
            || value.lods.len() > 32
            || value.shape.points.len() > 1_000_000
            || value.shape.matrices.len() > 65536
        {
            return None;
        }
        let lods = value
            .lods
            .into_iter()
            .map(|l| l.map(Loaded::materialize).transpose_option())
            .collect::<Option<Vec<_>>>()?;
        Some((value.shape, value.base.materialize()?, lods))
    })();
    if cached.is_some() {
        HITS.fetch_add(1, Ordering::Relaxed);
    } else {
        MISSES.fetch_add(1, Ordering::Relaxed);
    }
    cached
}
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            Some(v) => v.map(Some),
            None => Some(None),
        }
    }
}
pub fn write_verified(
    path: &Path,
    source: Option<&str>,
    shape: &ShapeFile,
    base: &LoadedShape,
    lods: &[Option<LoadedShape>],
) {
    if disabled() {
        return;
    }
    // A source edited during preparation must never cache old geometry under
    // the newly written source's digest. Skipping is safer than stale reuse.
    if let Some(source) = source {
        write_at_verified(path, shape, base, lods, &root(), Some(source));
    }
}
fn disabled() -> bool {
    std::env::var("OPENRAILSRS_GEOMETRY_CACHE").is_ok_and(|s| s.eq_ignore_ascii_case("off"))
}
#[cfg(test)]
fn write_at(
    path: &Path,
    shape: &ShapeFile,
    base: &LoadedShape,
    lods: &[Option<LoadedShape>],
    root: &Path,
) {
    write_at_verified(path, shape, base, lods, root, None);
}
fn write_at_verified(
    path: &Path,
    shape: &ShapeFile,
    base: &LoadedShape,
    lods: &[Option<LoadedShape>],
    root: &Path,
    source: Option<&str>,
) {
    let _timer = crate::performance::ShapeTimer::start("geometry_cache_write", path);
    let result = (|| {
        let key = identity(path)?;
        if source.is_some_and(|expected| expected != key) {
            return None;
        }
        let data = File {
            schema: SCHEMA,
            key: key.clone(),
            shape: shape.clone(),
            base: Loaded::capture(base)?,
            lods: lods
                .iter()
                .map(|l| l.as_ref().map(Loaded::capture).transpose_option())
                .collect::<Option<_>>()?,
        };
        let bytes = serde_json::to_vec(&data).ok()?;
        if bytes.len() as u64 > MAX_FILE {
            return None;
        }
        std::fs::create_dir_all(root).ok()?;
        let file = root.join(format!("{key}.json"));
        // Concurrent workers use distinct temporary names; the final key can be
        // safely replaced because identical source/options produce identical data.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let temp = root.join(format!(".{key}-{stamp}.tmp"));
        use std::io::Write;
        let saved = (|| {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .ok()?;
            f.write_all(&bytes).ok()?;
            std::fs::rename(&temp, &file).ok()
        })();
        if saved.is_none() {
            let _ = std::fs::remove_file(&temp);
        }
        let mut entries = std::fs::read_dir(root)
            .ok()?
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let m = e.metadata().ok()?;
                (p.extension().is_some_and(|e| e == "json") && !p.is_symlink()).then_some((
                    m.modified().ok()?,
                    m.len(),
                    p,
                ))
            })
            .collect::<Vec<_>>();
        let mut total = entries.iter().map(|e| e.1).sum::<u64>();
        entries.sort_by_key(|e| e.0);
        for (_, size, path) in entries {
            if total <= MAX_CACHE {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
        saved
    })();
    let _ = result;
}
pub fn report() -> serde_json::Value {
    serde_json::json!({"hits":HITS.load(Ordering::Relaxed),"misses":MISSES.load(Ordering::Relaxed),"schema":SCHEMA,"max_disk_mib":256,"gpu_handles_cached":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_geometry_matches_fresh_and_source_descriptor_or_damage_invalidates_it() {
        let temp = tempfile::tempdir().unwrap();
        let original = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/routes/test/SHAPES/yard_shed.s");
        let path = temp.path().join("model.s");
        std::fs::copy(original, &path).unwrap();
        let (shape, loaded) = crate::shapes::load_shape_file_and_loaded(&path, None).unwrap();
        assert!(
            CpuMesh::capture(&loaded.mesh).is_some(),
            "base attributes {:?}, indices {}",
            loaded
                .mesh
                .attributes()
                .map(|(a, _)| a.name)
                .collect::<Vec<_>>(),
            loaded.mesh.indices().is_some()
        );
        assert!(
            loaded
                .parts
                .iter()
                .all(|p| CpuMesh::capture(&p.mesh).is_some()),
            "part attributes {:?}",
            loaded
                .parts
                .iter()
                .map(|p| p.mesh.attributes().map(|(a, _)| a.name).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        );
        let cache = temp.path().join("cache");
        write_at(&path, &shape, &loaded, &[], &cache);
        let (_, fresh, _) = read_at(&path, &cache).expect("Persistent geometry hit");
        assert_eq!(fresh.parts.len(), loaded.parts.len());
        for (a, b) in fresh.parts.iter().zip(&loaded.parts) {
            assert_eq!(
                a.mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .get_bytes(),
                b.mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .get_bytes()
            );
            assert_eq!(
                a.mesh
                    .attribute(Mesh::ATTRIBUTE_NORMAL)
                    .unwrap()
                    .get_bytes(),
                b.mesh
                    .attribute(Mesh::ATTRIBUTE_NORMAL)
                    .unwrap()
                    .get_bytes()
            );
            assert_eq!(
                a.mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap().get_bytes(),
                b.mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap().get_bytes()
            );
            assert_eq!(
                a.mesh.indices().map(|i| i.iter().collect::<Vec<_>>()),
                b.mesh.indices().map(|i| i.iter().collect::<Vec<_>>())
            );
            assert_eq!(
                (a.prim_state_idx, a.sub_object_idx, a.alpha_test_mode),
                (b.prim_state_idx, b.sub_object_idx, b.alpha_test_mode)
            );
        }
        let cached = cache.join(format!("{}.json", identity(&path).unwrap()));
        std::fs::write(&cached, b"damaged").unwrap();
        assert!(read_at(&path, &cache).is_none());
        write_at(&path, &shape, &loaded, &[], &cache);
        std::fs::write(path.with_extension("sd"), "shape ( ESD_SubObj ( 1 ) )").unwrap();
        assert!(read_at(&path, &cache).is_none());
        std::fs::remove_file(path.with_extension("sd")).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.push(b' ');
        let consumed = identity(&path).unwrap();
        std::fs::write(&path, bytes).unwrap();
        assert!(read_at(&path, &cache).is_none());
        write_at_verified(&path, &shape, &loaded, &[], &cache, Some(&consumed));
        assert!(
            read_at(&path, &cache).is_none(),
            "An edited source cannot receive stale prepared geometry"
        );
    }
}
