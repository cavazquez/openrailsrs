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
    collections::BTreeMap,
    ffi::OsString,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
const SCHEMA: u32 = 1;
const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_CACHE: u64 = 256 * 1024 * 1024;
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);
// The JSON schema is unchanged. The envelope is a separate, bounded disk format.
const FORMAT: u32 = 2;
const MAGIC: &[u8; 8] = b"ORSGEOZ2";
const HEADER: usize = 60;
const EXTENSION: &str = "geometry.zst";
const LOCK_FILE: &str = ".budget.lock";
const MAX_ENTRIES: usize = 16_384;
static WRITES: AtomicU64 = AtomicU64::new(0);
static LEGACY_HITS: AtomicU64 = AtomicU64::new(0);
static INVALID_ENTRIES: AtomicU64 = AtomicU64::new(0);
static ADMISSION_SKIPS: AtomicU64 = AtomicU64::new(0);
static EVICTIONS: AtomicU64 = AtomicU64::new(0);
static INDEX_SCANS: AtomicU64 = AtomicU64::new(0);
static STORED_BYTES: AtomicU64 = AtomicU64::new(0);
static JSON_BYTES: AtomicU64 = AtomicU64::new(0);
static DISK_BYTES: AtomicU64 = AtomicU64::new(0);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WRITER: OnceLock<Mutex<Writer>> = OnceLock::new();

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
    for (name, compressed) in [
        (format!("{key}.{EXTENSION}"), true),
        (format!("{key}.json"), false),
    ] {
        let file = root.join(&name);
        if std::fs::symlink_metadata(&file).is_err() {
            continue;
        }
        let cached = (|| {
            let bytes = read_bounded(&file)?;
            let json = if compressed { decode(&bytes)? } else { bytes };
            materialize(&json, &key)
        })();
        if cached.is_some() {
            HITS.fetch_add(1, Ordering::Relaxed);
            if !compressed {
                LEGACY_HITS.fetch_add(1, Ordering::Relaxed);
            }
            // This is an in-memory LRU touch; no synchronous timestamp writes.
            if let Ok(mut writer) = writer().lock() {
                writer.touch(root, name.into());
            }
            return cached;
        }
        INVALID_ENTRIES.fetch_add(1, Ordering::Relaxed);
    }
    MISSES.fetch_add(1, Ordering::Relaxed);
    None
}
fn materialize(
    json: &[u8],
    key: &str,
) -> Option<(ShapeFile, LoadedShape, Vec<Option<LoadedShape>>)> {
    let value: File = serde_json::from_slice(json).ok()?;
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
}
// Both metadata and the read itself are bounded, including a file growing while read.
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_FILE).then_some(bytes)
}
struct BoundedJson(Vec<u8>);
impl Write for BoundedJson {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if data.len() as u64 > MAX_FILE.saturating_sub(self.0.len() as u64) {
            return Err(std::io::Error::other(
                "prepared geometry JSON exceeds 32 MiB",
            ));
        }
        self.0.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(json: &[u8]) -> Option<Vec<u8>> {
    if json.is_empty() || json.len() as u64 > MAX_FILE {
        return None;
    }
    let packed = zstd::bulk::compress(json, 1).ok()?;
    if packed.len().checked_add(HEADER)? as u64 > MAX_FILE {
        return None;
    }
    let mut bytes = Vec::with_capacity(HEADER + packed.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT.to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(packed.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(json));
    bytes.extend_from_slice(&packed);
    Some(bytes)
}
fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < HEADER
        || bytes.len() as u64 > MAX_FILE
        || &bytes[..8] != MAGIC
        || u32::from_le_bytes(bytes[8..12].try_into().ok()?) != FORMAT
    {
        return None;
    }
    let plain_len = u64::from_le_bytes(bytes[12..20].try_into().ok()?);
    let packed_len = u64::from_le_bytes(bytes[20..28].try_into().ok()?);
    if plain_len == 0 || plain_len > MAX_FILE || packed_len != (bytes.len() - HEADER) as u64 {
        return None;
    }
    let packed = &bytes[HEADER..];
    // Reject appended data and concatenated frames, even with a forged packed length.
    if zstd::zstd_safe::find_frame_compressed_size(packed).ok()? != packed.len() {
        return None;
    }
    let mut decoder = zstd::bulk::Decompressor::new().ok()?;
    decoder
        .set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(25))
        .ok()?;
    let json = decoder.decompress(packed, plain_len as usize).ok()?;
    if json.len() as u64 != plain_len || Sha256::digest(&json)[..] != bytes[28..60] {
        return None;
    }
    Some(json)
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
        let mut json = BoundedJson(Vec::new());
        serde_json::to_writer(&mut json, &data).ok()?;
        let bytes = encode(&json.0)?;
        // Compression is outside the serialized admission/publication section.
        writer()
            .lock()
            .ok()?
            .publish(root, &key, &bytes, MAX_CACHE)?;
        WRITES.fetch_add(1, Ordering::Relaxed);
        STORED_BYTES.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        JSON_BYTES.fetch_add(json.0.len() as u64, Ordering::Relaxed);
        Some(())
    })();
    if result.is_none() {
        ADMISSION_SKIPS.fetch_add(1, Ordering::Relaxed);
    }
}
fn writer() -> &'static Mutex<Writer> {
    WRITER.get_or_init(|| Mutex::new(Writer::default()))
}
#[derive(Clone)]
struct Entry {
    bytes: u64,
    age: u128,
    managed: bool,
    temporary: bool,
}
#[derive(Default)]
struct Writer {
    root: PathBuf,
    entries: BTreeMap<OsString, Entry>,
    used: u64,
    scanned: bool,
    epoch: u64,
    stamp: Option<SystemTime>,
    clock: u128,
    scans: u64,
    peak_reserved: u64,
}
fn timestamp(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn managed_name(name: &std::ffi::OsStr) -> (bool, bool) {
    let Some(name) = name.to_str() else {
        return (false, false);
    };
    let (key, suffix, temporary) = if let Some(rest) = name.strip_prefix('.') {
        if rest.len() < 65 {
            return (false, false);
        }
        let (Some(key), Some(suffix)) = (rest.get(..64), rest.get(64..)) else {
            return (false, false);
        };
        (key, suffix, true)
    } else {
        if name.len() < 65 {
            return (false, false);
        }
        let (Some(key), Some(suffix)) = (name.get(..64), name.get(64..)) else {
            return (false, false);
        };
        (key, suffix, false)
    };
    let valid_key = key.bytes().all(|v| v.is_ascii_hexdigit());
    let managed = valid_key
        && if temporary {
            suffix.starts_with('-') && suffix.ends_with(".tmp")
        } else {
            suffix == ".json" || suffix == format!(".{EXTENSION}")
        };
    (managed, temporary)
}
impl Writer {
    fn select(&mut self, root: &Path) {
        if self.root != root {
            *self = Self {
                root: root.to_owned(),
                ..Self::default()
            };
        }
    }
    fn age(&mut self) -> u128 {
        self.clock = self
            .clock
            .saturating_add(1)
            .max(timestamp(SystemTime::now()));
        self.clock
    }
    fn touch(&mut self, root: &Path, name: OsString) {
        self.select(root);
        let age = self.age();
        if let Some(entry) = self.entries.get_mut(&name) {
            entry.age = age;
        } else if !self.scanned && self.entries.len() < MAX_ENTRIES {
            self.entries.insert(
                name,
                Entry {
                    bytes: 0,
                    age,
                    managed: true,
                    temporary: false,
                },
            );
        }
    }
    fn scan(&mut self) -> Option<()> {
        let mut entries = BTreeMap::new();
        let mut used = 0u64;
        for item in std::fs::read_dir(&self.root).ok()? {
            let item = item.ok()?;
            if entries.len() >= MAX_ENTRIES {
                return None;
            }
            let metadata = std::fs::symlink_metadata(item.path()).ok()?;
            // This cache is flat. Do not recurse into or delete foreign directories.
            if metadata.is_dir() {
                return None;
            }
            let name = item.file_name();
            let (managed, temporary) = managed_name(&name);
            let age = self
                .entries
                .get(&name)
                .map(|entry| entry.age)
                .unwrap_or_else(|| timestamp(metadata.modified().unwrap_or(UNIX_EPOCH)));
            used = used.checked_add(metadata.len())?;
            entries.insert(
                name,
                Entry {
                    bytes: metadata.len(),
                    age,
                    managed: managed && metadata.is_file(),
                    temporary,
                },
            );
        }
        self.entries = entries;
        self.used = used;
        self.scanned = true;
        self.scans += 1;
        INDEX_SCANS.fetch_add(1, Ordering::Relaxed);
        DISK_BYTES.store(used, Ordering::Relaxed);
        Some(())
    }
    fn remove(&mut self, name: &std::ffi::OsStr) {
        if let Some(entry) = self.entries.remove(name) {
            self.used -= entry.bytes;
        }
    }
    fn insert(&mut self, name: OsString, bytes: u64, temporary: bool) {
        self.remove(&name);
        let age = self.age();
        self.used += bytes;
        self.entries.insert(
            name,
            Entry {
                bytes,
                age,
                managed: true,
                temporary,
            },
        );
    }
    fn publish(&mut self, root: &Path, key: &str, bytes: &[u8], budget: u64) -> Option<()> {
        self.select(root);
        if key.len() != 64
            || !key.bytes().all(|v| v.is_ascii_hexdigit())
            || bytes.len() as u64 > MAX_FILE
            || bytes.is_empty()
        {
            return None;
        }
        std::fs::create_dir_all(root).ok()?;
        let lock_path = root.join(LOCK_FILE);
        if lock_path.is_symlink() {
            return None;
        }
        let mut lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .ok()?;
        // Other processes may use the same player directory. Busy means rebuild,
        // never wait on foreground rendering or publish outside the shared quota.
        lock.try_lock().ok()?;
        let mut epoch_bytes = [0u8; 8];
        match lock.metadata().ok()?.len() {
            0 => {
                lock.write_all(&epoch_bytes).ok()?;
            }
            8 => {
                lock.read_exact(&mut epoch_bytes).ok()?;
            }
            _ => return None,
        }
        let epoch = u64::from_le_bytes(epoch_bytes);
        let stamp = std::fs::metadata(root).ok()?.modified().ok();
        if !self.scanned || self.epoch != epoch || self.stamp != stamp {
            self.scan()?;
        }
        self.epoch = epoch;
        let target: OsString = format!("{key}.{EXTENSION}").into();
        let legacy: OsString = format!("{key}.json").into();
        let mut changed = false;
        let result = (|| {
            let incoming = bytes.len() as u64;
            if self.used.checked_add(incoming)? > budget {
                let mut candidates = self
                    .entries
                    .iter()
                    .filter(|(name, entry)| entry.managed && **name != target && **name != legacy)
                    .map(|(name, entry)| ((!entry.temporary, entry.age, name.clone()), entry.bytes))
                    .collect::<Vec<_>>();
                candidates.sort_by(|a, b| a.0.cmp(&b.0));
                for ((_, _, name), _) in candidates {
                    if self.used.checked_add(incoming)? <= budget {
                        break;
                    }
                    if std::fs::remove_file(root.join(&name)).is_ok() {
                        self.remove(&name);
                        changed = true;
                        EVICTIONS.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            // Reserve the full temporary size while the old target is still live.
            let reserved = self.used.checked_add(incoming)?;
            if reserved > budget || self.entries.len() >= MAX_ENTRIES {
                return None;
            }
            self.peak_reserved = self.peak_reserved.max(reserved);
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temp: OsString = format!(".{key}-{}-{sequence}.tmp", std::process::id()).into();
            let temp_path = root.join(&temp);
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .ok()?;
            changed = true;
            // Reserving the complete size before the first write also covers failures.
            self.insert(temp.clone(), incoming, true);
            let written = output.write_all(bytes).is_ok();
            drop(output);
            if written && std::fs::rename(&temp_path, root.join(&target)).is_ok() {
                self.remove(&temp);
                self.insert(target.clone(), incoming, false);
                if std::fs::remove_file(root.join(&legacy)).is_ok() {
                    self.remove(&legacy);
                }
                return Some(());
            }
            if std::fs::remove_file(&temp_path).is_ok() {
                self.remove(&temp);
            }
            // If deletion fails, retain the entire reservation. A subsequent scan
            // may reduce it to actual length, but admission must not undercount it.
            None
        })();
        if changed {
            self.epoch = epoch.wrapping_add(1);
            if lock.seek(SeekFrom::Start(0)).is_err()
                || lock.write_all(&self.epoch.to_le_bytes()).is_err()
            {
                self.scanned = false;
            }
        }
        self.stamp = std::fs::metadata(root).ok().and_then(|m| m.modified().ok());
        DISK_BYTES.store(self.used, Ordering::Relaxed);
        result
    }
}
pub fn report() -> serde_json::Value {
    serde_json::json!({"hits":HITS.load(Ordering::Relaxed),"misses":MISSES.load(Ordering::Relaxed),
        "schema":SCHEMA,"format":FORMAT,"compression":"zstd-1","max_disk_mib":256,
        "max_entry_mib":32,"gpu_handles_cached":false,"writes":WRITES.load(Ordering::Relaxed),
        "legacy_hits":LEGACY_HITS.load(Ordering::Relaxed),"invalid_entries":INVALID_ENTRIES.load(Ordering::Relaxed),
        "admission_skips":ADMISSION_SKIPS.load(Ordering::Relaxed),"evictions":EVICTIONS.load(Ordering::Relaxed),
        "inventory_scans":INDEX_SCANS.load(Ordering::Relaxed),"disk_bytes":DISK_BYTES.load(Ordering::Relaxed),
        "stored_bytes_written":STORED_BYTES.load(Ordering::Relaxed),"json_bytes_written":JSON_BYTES.load(Ordering::Relaxed)})
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
        let lods = vec![None, Some(loaded.clone())];
        write_at(&path, &shape, &loaded, &lods, &cache);
        let (cached_shape, fresh, cached_lods) =
            read_at(&path, &cache).expect("Persistent geometry hit");
        assert_eq!(
            serde_json::to_value(&cached_shape).unwrap(),
            serde_json::to_value(&shape).unwrap()
        );
        assert_eq!(
            serde_json::to_value(Loaded::capture(&fresh).unwrap()).unwrap(),
            serde_json::to_value(Loaded::capture(&loaded).unwrap()).unwrap()
        );
        assert_eq!(cached_lods.len(), 2);
        assert!(cached_lods[0].is_none());
        assert_eq!(
            serde_json::to_value(Loaded::capture(cached_lods[1].as_ref().unwrap()).unwrap())
                .unwrap(),
            serde_json::to_value(Loaded::capture(&loaded).unwrap()).unwrap()
        );
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
        let cached = cache.join(format!("{}.{EXTENSION}", identity(&path).unwrap()));
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
    fn fixture() -> (tempfile::TempDir, PathBuf, ShapeFile, LoadedShape) {
        let temp = tempfile::tempdir().unwrap();
        let original = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/routes/test/SHAPES/yard_shed.s");
        let path = temp.path().join("model.s");
        std::fs::copy(original, &path).unwrap();
        let (shape, loaded) = crate::shapes::load_shape_file_and_loaded(&path, None).unwrap();
        (temp, path, shape, loaded)
    }
    fn file_json(key: &str, shape: &ShapeFile, base: &LoadedShape) -> Vec<u8> {
        serde_json::to_vec(&File {
            schema: SCHEMA,
            key: key.to_owned(),
            shape: shape.clone(),
            base: Loaded::capture(base).unwrap(),
            lods: vec![],
        })
        .unwrap()
    }
    fn disk_size(root: &Path) -> u64 {
        std::fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                std::fs::symlink_metadata(entry.unwrap().path())
                    .unwrap()
                    .len()
            })
            .sum()
    }
    #[test]
    fn compressed_envelope_rejects_damage_truncation_bombs_and_inconsistent_lengths() {
        let plain = vec![b'x'; 1024 * 1024];
        let good = encode(&plain).unwrap();
        assert!(good.len() < plain.len() / 10);
        assert_eq!(decode(&good).unwrap(), plain);
        let mut cases = vec![good[..HEADER - 1].to_vec(), good[..good.len() - 1].to_vec()];
        for offset in [0, 8, 12, 20, 28, HEADER] {
            let mut bad = good.clone();
            bad[offset] ^= 1;
            cases.push(bad);
        }
        for length in [0, 128, MAX_FILE + 1, u64::MAX] {
            let mut bad = good.clone();
            bad[12..20].copy_from_slice(&length.to_le_bytes());
            cases.push(bad);
        }
        // Valid frames cannot smuggle a second frame or trailing junk into the entry.
        for extra in [vec![0], zstd::bulk::compress(b"second frame", 1).unwrap()] {
            let mut bad = good.clone();
            bad.extend_from_slice(&extra);
            let length = (bad.len() - HEADER) as u64;
            bad[20..28].copy_from_slice(&length.to_le_bytes());
            cases.push(bad);
        }
        for bad in cases {
            assert!(decode(&bad).is_none());
        }
        assert!(encode(&[]).is_none());
    }
    #[test]
    fn legacy_geometry_retains_semantic_validation_and_migrates_only_after_publication() {
        let (temp, path, shape, loaded) = fixture();
        let cache = temp.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let key = identity(&path).unwrap();
        let json = file_json(&key, &shape, &loaded);
        let legacy = cache.join(format!("{key}.json"));
        std::fs::write(&legacy, &json).unwrap();
        assert!(read_at(&path, &cache).is_some());
        let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
        let mut variants = Vec::new();
        let mut bad = value.clone();
        bad["schema"] = (SCHEMA + 1).into();
        variants.push(bad);
        let mut bad = value.clone();
        bad["key"] = "different source".into();
        variants.push(bad);
        let mut bad = value.clone();
        bad["lods"] = vec![serde_json::Value::Null; 33].into();
        variants.push(bad);
        let mut bad = value.clone();
        bad["base"]["mesh"]["normals"] = serde_json::json!([]);
        variants.push(bad);
        let mut bad = value.clone();
        bad["base"]["mesh"]["indices"] = serde_json::json!([u32::MAX, 0, 0]);
        variants.push(bad);
        for bad in variants {
            let bytes = serde_json::to_vec(&bad).unwrap();
            assert!(materialize(&bytes, &key).is_none());
            assert!(materialize(&decode(&encode(&bytes).unwrap()).unwrap(), &key).is_none());
        }
        write_at(&path, &shape, &loaded, &[], &cache);
        assert!(!legacy.exists());
        assert!(cache.join(format!("{key}.{EXTENSION}")).exists());
        assert!(read_at(&path, &cache).is_some());
        // An incompatible version of the producer is not revived via the legacy path.
        std::fs::remove_file(cache.join(format!("{key}.{EXTENSION}"))).unwrap();
        std::fs::write(cache.join(format!("{}.json", "f".repeat(64))), &json).unwrap();
        assert!(read_at(&path, &cache).is_none());
    }
    #[test]
    fn bounded_file_reads_reject_large_and_non_regular_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.geometry.zst");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(read_bounded(&path).is_none());
        assert!(read_bounded(temp.path()).is_none());
        #[cfg(unix)]
        {
            let link = temp.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(read_bounded(&link).is_none());
        }
        assert_eq!(
            managed_name(std::ffi::OsStr::new(&format!("{}é.json", "a".repeat(63)))),
            (false, false)
        );
    }
    #[test]
    fn quota_counts_legacy_and_temporaries_and_keeps_a_previous_entry_on_failed_admission() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let key = "a".repeat(64);
        let other = "b".repeat(64);
        let orphan = "c".repeat(64);
        let legacy = root.join(format!("{other}.json"));
        std::fs::write(&legacy, [0; 70]).unwrap();
        let abandoned = root.join(format!(".{orphan}-old.tmp"));
        std::fs::write(&abandoned, [0; 40]).unwrap();
        let author = root.join("keep.txt");
        std::fs::write(&author, [42; 10]).unwrap();
        let blob = encode(b"retained geometry").unwrap();
        let budget = 8 + 10 + blob.len() as u64 + 70;
        let mut writer = Writer::default();
        writer.publish(root, &key, &blob, budget).unwrap();
        assert!(
            !abandoned.exists(),
            "orphan reservations must be reclaimed before useful entries"
        );
        assert!(legacy.exists());
        assert_eq!(std::fs::read(&author).unwrap(), [42; 10]);
        assert!(disk_size(root) <= budget);
        assert_eq!(writer.scans, 1);
        // The current destination is protected until a replacement is fully written.
        let previous = std::fs::read(root.join(format!("{key}.{EXTENSION}"))).unwrap();
        assert!(
            writer
                .publish(root, &key, &vec![0; budget as usize], budget)
                .is_none()
        );
        assert_eq!(
            std::fs::read(root.join(format!("{key}.{EXTENSION}"))).unwrap(),
            previous
        );
        assert!(writer.peak_reserved <= budget);
        assert_eq!(
            writer.scans, 1,
            "own writes/evictions do not rescan the directory"
        );
        assert_eq!(writer.used, disk_size(root));
    }
    #[test]
    fn lru_hits_and_external_writers_refresh_the_shared_inventory_without_rescanning_each_write() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let c = "c".repeat(64);
        let blob = encode(b"one complete geometry").unwrap();
        let budget = 8 + 3 * blob.len() as u64;
        let mut first = Writer::default();
        first.publish(root, &a, &blob, budget).unwrap();
        first.publish(root, &b, &blob, budget).unwrap();
        first.touch(root, format!("{a}.{EXTENSION}").into());
        let mut second = Writer::default();
        second.publish(root, &c, &blob, budget).unwrap();
        assert_eq!(first.scans, 1);
        first.publish(root, &a, &blob, budget).unwrap();
        assert_eq!(
            first.scans, 2,
            "another cooperating writer changes the epoch"
        );
        assert!(root.join(format!("{a}.{EXTENSION}")).exists());
        assert!(
            !root.join(format!("{b}.{EXTENSION}")).exists(),
            "the untouched entry is evicted first"
        );
        assert!(root.join(format!("{c}.{EXTENSION}")).exists());
        assert_eq!(first.used, disk_size(root));
        assert!(first.peak_reserved <= budget);
    }
    #[test]
    fn concurrent_workers_publish_only_complete_entries_and_never_exceed_the_reserved_budget() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_owned();
        let writer = std::sync::Arc::new(Mutex::new(Writer::default()));
        let payloads = (0..4)
            .map(|n| encode(&vec![b'a' + n; 16_384]).unwrap())
            .collect::<Vec<_>>();
        let budget = 8 + 4 * payloads.iter().map(Vec::len).max().unwrap() as u64;
        std::thread::scope(|scope| {
            for payload in &payloads {
                let root = &root;
                let writer = &writer;
                scope.spawn(move || {
                    for n in 0..12 {
                        let key = format!("{n:064x}");
                        writer
                            .lock()
                            .unwrap()
                            .publish(root, &key, payload, budget)
                            .unwrap();
                        // Eviction may race the read; any visible entry must be intact.
                        if let Some(bytes) = read_bounded(&root.join(format!("{key}.{EXTENSION}")))
                        {
                            let plain = decode(&bytes).expect("no partial cache publication");
                            assert_eq!(plain.len(), 16_384);
                            assert!(plain.iter().all(|b| *b == plain[0]));
                        }
                    }
                });
            }
        });
        let writer = writer.lock().unwrap();
        assert_eq!(writer.scans, 1);
        assert_eq!(writer.used, disk_size(&root));
        assert!(writer.peak_reserved <= budget);
        assert!(disk_size(&root) <= budget);
        assert!(
            !std::fs::read_dir(&root)
                .unwrap()
                .any(|e| managed_name(&e.unwrap().file_name()).1)
        );
    }
}
