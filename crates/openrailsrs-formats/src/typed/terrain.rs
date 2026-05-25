//! MSTS terrain tile (`.y`) metadata and `_Y.RAW` elevation grids.
//!
//! Normal-resolution tiles use a 256×256 grid of uint16 samples spaced
//! [`TerrainSamples::sample_size`] metres apart (typically 8 m → 2048 m tile).
//! Elevation in metres: `sample_floor + raw * sample_scale`.

use std::path::{Path, PathBuf};

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::parser::parse_from_first_paren;

/// Sample decoding parameters from a terrain `.y` / `.t` tile description.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainSamples {
    pub nsamples: u32,
    pub sample_floor: f64,
    pub sample_scale: f64,
    pub sample_size: f64,
    pub y_buffer_file: String,
}

impl Default for TerrainSamples {
    fn default() -> Self {
        Self {
            nsamples: 256,
            sample_floor: 0.0,
            sample_scale: 0.25,
            sample_size: 8.0,
            y_buffer_file: String::new(),
        }
    }
}

/// Parsed MSTS terrain tile header (elevation sampling only — no texture layers yet).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainFile {
    pub tile_x: i32,
    pub tile_z: i32,
    pub samples: TerrainSamples,
}

/// Row-major `nsamples × nsamples` elevation field (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct ElevationGrid {
    pub nsamples: usize,
    pub elevations: Vec<f32>,
}

/// Plain mesh buffers for a terrain patch or tile (headless — no GPU types).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl ElevationGrid {
    pub fn elevation_at(&self, i: usize, j: usize) -> f32 {
        self.elevations[j * self.nsamples + i]
    }

    /// Bilinear sample at tile-local `(x, z)` metres (clamped to the grid).
    pub fn sample_bilinear(&self, x: f64, z: f64, sample_size: f64) -> f32 {
        let max = (self.nsamples - 1) as f64;
        let u = (x / sample_size).clamp(0.0, max);
        let v = (z / sample_size).clamp(0.0, max);
        let i0 = u.floor() as usize;
        let j0 = v.floor() as usize;
        let i1 = (i0 + 1).min(self.nsamples - 1);
        let j1 = (j0 + 1).min(self.nsamples - 1);
        let fu = (u - i0 as f64) as f32;
        let fv = (v - j0 as f64) as f32;

        let h00 = self.elevation_at(i0, j0);
        let h10 = self.elevation_at(i1, j0);
        let h01 = self.elevation_at(i0, j1);
        let h11 = self.elevation_at(i1, j1);

        let h0 = h00 * (1.0 - fu) + h10 * fu;
        let h1 = h01 * (1.0 - fu) + h11 * fu;
        h0 * (1.0 - fv) + h1 * fv
    }
}

impl TerrainFile {
    pub fn from_ast(ast: &Ast, tile_x: i32, tile_z: i32) -> Result<Self, FormatError> {
        let samples = parse_terrain_samples(ast)?;
        Ok(Self {
            tile_x,
            tile_z,
            samples,
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let text = crate::encoding::read_msts_file_to_string(path)?;
        let ast = parse_from_first_paren(&text)?;
        let (tile_x, tile_z) = parse_tile_xz_from_filename(path).unwrap_or((0, 0));
        Self::from_ast(&ast, tile_x, tile_z)
    }

    /// Resolve the `_Y.RAW` path next to the `.y` tile file.
    pub fn y_raw_path(&self, tile_path: &Path) -> PathBuf {
        let name = self.samples.y_buffer_file.trim();
        if name.is_empty() {
            return tile_path.with_extension("raw");
        }
        tile_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(name)
    }
}

/// Decode a MSTS `_Y.RAW` height buffer using tile sample parameters.
pub fn read_y_raw(path: &Path, params: &TerrainSamples) -> Result<ElevationGrid, FormatError> {
    let nsamples = params.nsamples as usize;
    let expected = nsamples
        .checked_mul(nsamples)
        .and_then(|n| n.checked_mul(2))
        .ok_or_else(|| FormatError::UnexpectedAtom {
            key: "grid".into(),
            context: "terrain y raw".into(),
            expected: "grid dimensions overflow".into(),
        })?;

    let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedAtom {
        key: "read".into(),
        context: path.display().to_string(),
        expected: e.to_string(),
    })?;
    if bytes.len() != expected {
        return Err(FormatError::UnexpectedAtom {
            key: "size".into(),
            context: path.display().to_string(),
            expected: format!(
                "expected {expected} bytes for {}×{} uint16 grid, got {}",
                nsamples,
                nsamples,
                bytes.len()
            ),
        });
    }

    let mut elevations = Vec::with_capacity(nsamples * nsamples);
    for chunk in bytes.chunks_exact(2) {
        let raw = u16::from_le_bytes([chunk[0], chunk[1]]);
        let h = params.sample_floor + f64::from(raw) * params.sample_scale;
        elevations.push(h as f32);
    }

    Ok(ElevationGrid {
        nsamples,
        elevations,
    })
}

/// Build one OR-style terrain patch: 17×17 vertices, 16×16 cells, alternating diagonals.
pub fn build_patch_mesh_data(
    grid: &ElevationGrid,
    sample_size: f64,
    patch_x: u32,
    patch_z: u32,
    patch_size_m: f64,
) -> TerrainMeshData {
    const PATCH_CELLS: u32 = 16;
    const PATCH_VERTS: u32 = PATCH_CELLS + 1;
    let cell = sample_size;
    let origin_x = patch_x as f64 * patch_size_m;
    let origin_z = patch_z as f64 * patch_size_m;

    let mut positions = Vec::with_capacity((PATCH_VERTS * PATCH_VERTS) as usize);
    let mut uvs = Vec::with_capacity((PATCH_VERTS * PATCH_VERTS) as usize);

    for vz in 0..PATCH_VERTS {
        for vx in 0..PATCH_VERTS {
            let lx = origin_x + vx as f64 * cell;
            let lz = origin_z + vz as f64 * cell;
            let y = grid.sample_bilinear(lx, lz, sample_size);
            positions.push([lx as f32, y, lz as f32]);
            uvs.push([
                vx as f32 / PATCH_CELLS as f32,
                vz as f32 / PATCH_CELLS as f32,
            ]);
        }
    }

    let normals = compute_vertex_normals(&positions, PATCH_VERTS as usize, PATCH_VERTS as usize);
    let mut indices = Vec::with_capacity((PATCH_CELLS * PATCH_CELLS * 6) as usize);

    for z in 0..PATCH_CELLS {
        for x in 0..PATCH_CELLS {
            let i00 = z * PATCH_VERTS + x;
            let i10 = i00 + 1;
            let i01 = i00 + PATCH_VERTS;
            let i11 = i01 + 1;
            if (x & 1) == (z & 1) {
                indices.extend([i00, i11, i01, i00, i10, i11]);
            } else {
                indices.extend([i00, i10, i01, i01, i10, i11]);
            }
        }
    }

    TerrainMeshData {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// Merge all 16×16 patches of a tile into one mesh (one draw call per tile).
pub fn build_tile_mesh_data(grid: &ElevationGrid, sample_size: f64) -> TerrainMeshData {
    const PATCHES_PER_SIDE: u32 = 16;
    const PATCH_SIZE_M: f64 = 128.0;
    let mut out = TerrainMeshData {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
    };

    for pz in 0..PATCHES_PER_SIDE {
        for px in 0..PATCHES_PER_SIDE {
            let patch = build_patch_mesh_data(grid, sample_size, px, pz, PATCH_SIZE_M);
            let base = out.positions.len() as u32;
            out.positions.extend(patch.positions);
            out.normals.extend(patch.normals);
            out.uvs.extend(patch.uvs);
            out.indices
                .extend(patch.indices.into_iter().map(|i| i + base));
        }
    }
    out
}

fn compute_vertex_normals(positions: &[[f32; 3]], width: usize, height: usize) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; width * height];
    let idx = |x: usize, z: usize| z * width + x;

    for z in 0..height.saturating_sub(1) {
        for x in 0..width.saturating_sub(1) {
            let p00 = positions[idx(x, z)];
            let p10 = positions[idx(x + 1, z)];
            let p01 = positions[idx(x, z + 1)];
            let e1 = [p10[0] - p00[0], p10[1] - p00[1], p10[2] - p00[2]];
            let e2 = [p01[0] - p00[0], p01[1] - p00[1], p01[2] - p00[2]];
            let n = cross(e1, e2);
            for &i in &[idx(x, z), idx(x + 1, z), idx(x, z + 1)] {
                normals[i][0] += n[0];
                normals[i][1] += n[1];
                normals[i][2] += n[2];
            }
        }
    }

    for n in &mut normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-6 {
            n[0] /= len;
            n[1] /= len;
            n[2] /= len;
        } else {
            *n = [0.0, 1.0, 0.0];
        }
    }
    normals
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn parse_terrain_samples(ast: &Ast) -> Result<TerrainSamples, FormatError> {
    let mut out = TerrainSamples::default();
    walk_ast(ast, &mut |items| {
        if items.len() < 2 {
            return;
        }
        let Ast::Atom(Atom::Symbol(key)) = &items[0] else {
            return;
        };
        let val = &items[1];
        match key.as_str() {
            "terrain_nsamples" => {
                if let Some(n) = atom_num(val) {
                    out.nsamples = n.max(1.0) as u32;
                }
            }
            "terrain_sample_floor" => {
                if let Some(v) = atom_num(val) {
                    out.sample_floor = v;
                }
            }
            "terrain_sample_scale" => {
                if let Some(v) = atom_num(val) {
                    out.sample_scale = v;
                }
            }
            "terrain_sample_size" => {
                if let Some(v) = atom_num(val) {
                    out.sample_size = v;
                }
            }
            "terrain_sample_ybuffer" => {
                if let Some(s) = atom_str(val) {
                    out.y_buffer_file = s;
                }
            }
            _ => {}
        }
    });
    if out.nsamples == 0 {
        return Err(FormatError::MissingField {
            key: "terrain_nsamples".into(),
            context: "terrain tile".into(),
        });
    }
    Ok(out)
}

fn walk_ast(ast: &Ast, f: &mut dyn FnMut(&[Ast])) {
    if let Ast::List(items) = ast {
        f(items);
        for item in items {
            walk_ast(item, f);
        }
    }
}

fn unwrap_single(ast: &Ast) -> &Ast {
    match ast {
        Ast::List(items) if items.len() == 1 => &items[0],
        other => other,
    }
}

fn atom_num(ast: &Ast) -> Option<f64> {
    match unwrap_single(ast) {
        Ast::Atom(Atom::Number(v)) => Some(*v),
        Ast::Atom(Atom::Integer(v)) => Some(*v as f64),
        _ => None,
    }
}

fn atom_str(ast: &Ast) -> Option<String> {
    match unwrap_single(ast) {
        Ast::Atom(Atom::String(s)) | Ast::Atom(Atom::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Parse `+000000+000000` / `+001000-001000` style terrain tile filenames.
pub fn parse_tile_xz_from_filename(path: &Path) -> Option<(i32, i32)> {
    let stem = path.file_stem()?.to_string_lossy();
    if stem.len() < 14 {
        return None;
    }
    let x_part = &stem[0..7];
    let z_part = &stem[7..14];
    if !matches!(x_part.as_bytes().first(), Some(b'+') | Some(b'-'))
        || !matches!(z_part.as_bytes().first(), Some(b'+') | Some(b'-'))
    {
        return None;
    }
    let x: i32 = x_part.parse().ok()?;
    let z: i32 = z_part.parse().ok()?;
    Some((x, z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn parse_minimal_terrain_tile() {
        let tf = TerrainFile::from_path(fixture("minimal_terrain.y")).expect("parse terrain");
        assert_eq!(tf.tile_x, 0);
        assert_eq!(tf.tile_z, 0);
        assert_eq!(tf.samples.nsamples, 256);
        assert!((tf.samples.sample_size - 8.0).abs() < 1e-6);
        assert!((tf.samples.sample_scale - 0.25).abs() < 1e-6);
    }

    #[test]
    fn read_y_raw_fixture_has_expected_size() {
        let tf = TerrainFile::from_path(fixture("minimal_terrain.y")).unwrap();
        let raw = fixture("minimal_terrain_y.raw");
        let grid = read_y_raw(&raw, &tf.samples).expect("read raw");
        assert_eq!(grid.nsamples, 256);
        assert_eq!(grid.elevations.len(), 256 * 256);
    }

    #[test]
    fn patch_mesh_uses_alternating_diagonals() {
        let tf = TerrainFile::from_path(fixture("minimal_terrain.y")).unwrap();
        let grid = read_y_raw(&fixture("minimal_terrain_y.raw"), &tf.samples).unwrap();
        let mesh = build_patch_mesh_data(&grid, tf.samples.sample_size, 0, 0, 128.0);
        assert_eq!(mesh.positions.len(), 17 * 17);
        assert_eq!(mesh.indices.len(), 16 * 16 * 6);
        // First cell (0,0): diagonal (0&1)==(0&1) → tri (0, 18, 17) ...
        assert_eq!(mesh.indices[0..6], [0, 18, 17, 0, 1, 18]);
    }

    #[test]
    fn tile_mesh_merges_sixteen_patches() {
        let tf = TerrainFile::from_path(fixture("minimal_terrain.y")).unwrap();
        let grid = read_y_raw(&fixture("minimal_terrain_y.raw"), &tf.samples).unwrap();
        let mesh = build_tile_mesh_data(&grid, tf.samples.sample_size);
        assert_eq!(mesh.positions.len(), 17 * 17 * 16 * 16);
        assert_eq!(mesh.indices.len(), 16 * 16 * 6 * 16 * 16);
    }

    #[test]
    fn parse_smoke_terrain_y_buffer_name() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/routes/test/TERRAIN/+000000+000000.y");
        let tf = TerrainFile::from_path(&path).expect("parse smoke terrain");
        assert_eq!(tf.samples.y_buffer_file, "+000000+000000_y.raw");
    }

    #[test]
    fn bilinear_sample_at_grid_point_matches_cell() {
        let grid = ElevationGrid {
            nsamples: 3,
            elevations: vec![0.0, 10.0, 20.0, 1.0, 11.0, 21.0, 2.0, 12.0, 22.0],
        };
        assert!((grid.sample_bilinear(8.0, 0.0, 8.0) - 10.0).abs() < 1e-4);
    }
}
