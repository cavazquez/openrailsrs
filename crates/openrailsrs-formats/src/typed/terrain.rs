//! MSTS terrain tile (`.y`) metadata, `_Y.RAW` / `_F.RAW` grids, and OR-style patch meshes.

use std::path::{Path, PathBuf};

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::parser::parse_from_first_paren;

const PATCH_CELLS: u32 = 16;
const PATCH_VERTS: u32 = PATCH_CELLS + 1;
const PATCH_SIZE_M: f64 = 128.0;
const VERTEX_HIDDEN_FLAG: u8 = 0x04;

/// Sample decoding parameters from a terrain `.y` / `.t` tile description.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainSamples {
    pub nsamples: u32,
    pub sample_floor: f64,
    pub sample_scale: f64,
    pub sample_size: f64,
    pub y_buffer_file: String,
    pub e_buffer_file: String,
    pub n_buffer_file: String,
    pub f_buffer_file: String,
}

impl Default for TerrainSamples {
    fn default() -> Self {
        Self {
            nsamples: 256,
            sample_floor: 0.0,
            sample_scale: 0.25,
            sample_size: 8.0,
            y_buffer_file: String::new(),
            e_buffer_file: String::new(),
            n_buffer_file: String::new(),
            f_buffer_file: String::new(),
        }
    }
}

/// One terrain texture slot from `terrain_texslot`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainTexSlot {
    pub filename: String,
    pub a: i32,
    pub b: i32,
}

/// UV transform block from `terrain_uvcalc` (OR stores `d` as float).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainUvCalc {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: f64,
}

/// Shader entry from `terrain_shader` in a `.y` tile.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainShader {
    pub name: String,
    pub texslots: Vec<TerrainTexSlot>,
    pub uvcalcs: Vec<TerrainUvCalc>,
}

/// One patch from `terrain_patchset_patch`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainPatch {
    pub flags: u32,
    pub center_x: f32,
    pub average_y: f32,
    pub center_z: f32,
    pub factor_y: f32,
    pub range_y: f32,
    pub radius_m: f32,
    pub shader_index: i32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub b: f32,
    pub c: f32,
    pub error_bias: f32,
}

impl TerrainPatch {
    pub fn drawing_enabled(&self) -> bool {
        (self.flags & 1) == 0
    }

    pub fn water_enabled(&self) -> bool {
        (self.flags & 0xC0) != 0
    }

    /// OR `TerrainPrimitive` camera-relative anchor (not for world `Transform` placement).
    ///
    /// Viewer3D places patches via patch index × 128 m inside the tile; adding this offset on
    /// top of mesh vertices that already span `patch_x × 128` duplicates the shift (notably +2048 m on Z).
    pub fn patch_translation(&self) -> (f32, f32) {
        let cx = self.center_x - 1024.0;
        let cz = self.center_z - 1024.0 + 2048.0;
        (cx, cz)
    }
}

/// A patch set (`terrain_patchset`) inside `terrain_patches`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainPatchSet {
    pub distance: i32,
    pub npatches: u32,
    pub patches: Vec<TerrainPatch>,
}

impl TerrainPatchSet {
    pub fn patch_at(&self, x: u32, z: u32) -> Option<&TerrainPatch> {
        let n = self.npatches;
        if x >= n || z >= n {
            return None;
        }
        self.patches.get((z * n + x) as usize)
    }

    pub fn primary(&self) -> Option<&TerrainPatchSet> {
        Some(self)
    }
}

/// Parsed MSTS terrain tile header.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainFile {
    pub tile_x: i32,
    pub tile_z: i32,
    pub samples: TerrainSamples,
    pub shaders: Vec<TerrainShader>,
    pub patch_sets: Vec<TerrainPatchSet>,
}

impl TerrainFile {
    pub fn from_ast(ast: &Ast, tile_x: i32, tile_z: i32) -> Result<Self, FormatError> {
        let samples = parse_terrain_samples(ast)?;
        let shaders = parse_terrain_shaders(ast);
        let patch_sets = parse_terrain_patch_sets(ast);
        Ok(Self {
            tile_x,
            tile_z,
            samples,
            shaders,
            patch_sets,
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let (tile_x, tile_z) = parse_tile_xz_from_filename(path).unwrap_or((0, 0));
        Self::from_path_with_coords(path, tile_x, tile_z)
    }

    /// Load a tile when the display tile indices are already known (hash-named `TILES/*.t`).
    pub fn from_path_with_coords(
        path: impl AsRef<Path>,
        tile_x: i32,
        tile_z: i32,
    ) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedToken {
            offset: 0,
            message: format!("failed to read {}: {e}", path.display()),
        })?;
        if is_terrain_binary_file(&bytes) {
            return Self::from_binary_tile(&bytes, path, tile_x, tile_z);
        }
        let text = crate::msts_file_text::read_msts_file_decoded(path)?;
        let ast = parse_from_first_paren(&text)?;
        Self::from_ast(&ast, tile_x, tile_z)
    }

    /// Parse a binary `JINX0t6b` terrain tile, falling back to `from_y_raw_sidecar`
    /// if binary parsing fails (e.g., unexpected format variant).
    fn from_binary_tile(
        bytes: &[u8],
        tile_path: &Path,
        tile_x: i32,
        tile_z: i32,
    ) -> Result<Self, FormatError> {
        match parse_terrain_binary(bytes) {
            Ok((samples, shaders, patch_sets)) => Ok(Self {
                tile_x,
                tile_z,
                samples,
                shaders,
                patch_sets,
            }),
            Err(e) => {
                // Warn and fall back to the _y.raw sidecar approach so the
                // viewer still renders something rather than crashing.
                eprintln!(
                    "openrailsrs-formats: binary terrain parse failed for {} ({e}), \
                     falling back to _y.raw sidecar with default floor/scale",
                    tile_path.display()
                );
                Self::from_y_raw_sidecar(tile_path, tile_x, tile_z)
            }
        }
    }

    /// Minimal tile metadata from the paired `_y.raw` heightfield (Chiltern `JINX0t6b` `.t`).
    pub fn from_y_raw_sidecar(
        tile_path: &Path,
        tile_x: i32,
        tile_z: i32,
    ) -> Result<Self, FormatError> {
        let stem = tile_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let y_name = format!("{stem}_y.raw");
        let y_path = tile_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&y_name);
        let y_bytes = std::fs::read(&y_path).map_err(|e| FormatError::UnexpectedToken {
            offset: 0,
            message: format!("failed to read {}: {e}", y_path.display()),
        })?;
        let cell_count = y_bytes.len() / 2;
        let nsamples = (cell_count as f64).sqrt().round() as u32;
        if nsamples == 0 || (nsamples as usize) * (nsamples as usize) * 2 != y_bytes.len() {
            return Err(FormatError::UnexpectedToken {
                offset: 0,
                message: format!(
                    "unexpected _y.raw size {} for {}",
                    y_bytes.len(),
                    y_path.display()
                ),
            });
        }
        Ok(Self {
            tile_x,
            tile_z,
            samples: TerrainSamples {
                nsamples,
                sample_floor: 0.0,
                sample_scale: 0.25,
                sample_size: 8.0,
                y_buffer_file: y_name,
                ..TerrainSamples::default()
            },
            shaders: Vec::new(),
            patch_sets: Vec::new(),
        })
    }

    pub fn primary_patch_set(&self) -> Option<&TerrainPatchSet> {
        self.patch_sets.first()
    }

    pub fn has_textured_patches(&self) -> bool {
        self.primary_patch_set().is_some() && !self.shaders.is_empty()
    }

    /// Resolve the `_Y.RAW` path next to the `.y` tile file.
    pub fn y_raw_path(&self, tile_path: &Path) -> PathBuf {
        raw_buffer_path(tile_path, &self.samples.y_buffer_file)
    }

    pub fn f_raw_path(&self, tile_path: &Path) -> PathBuf {
        raw_buffer_path(tile_path, &self.samples.f_buffer_file)
    }
}

fn is_terrain_binary_file(bytes: &[u8]) -> bool {
    let raw = crate::encoding::utf16le_msts_to_latin_bytes(bytes).unwrap_or_else(|| bytes.to_vec());
    raw.windows(8)
        .any(|w| w.starts_with(b"JINX0t") && w.len() >= 8 && w[7] == b'b')
}

// ─── OR SBR binary terrain token IDs ────────────────────────────────────────
// Source: Open Rails `Orts.Parsers.Msts/TokenID.cs` (sequential enum, offset 0)
const TOK_TERRAIN: u16 = 136;
const TOK_TERRAIN_SAMPLES: u16 = 139;
const TOK_TERRAIN_NSAMPLES: u16 = 140;
const TOK_TERRAIN_SAMPLE_FLOOR: u16 = 142;
const TOK_TERRAIN_SAMPLE_SCALE: u16 = 143;
const TOK_TERRAIN_SAMPLE_SIZE: u16 = 144;
const TOK_TERRAIN_SAMPLE_FBUFFER: u16 = 145;
const TOK_TERRAIN_SAMPLE_YBUFFER: u16 = 146;
const TOK_TERRAIN_SAMPLE_EBUFFER: u16 = 147;
const TOK_TERRAIN_SAMPLE_NBUFFER: u16 = 148;
const TOK_TERRAIN_SHADERS: u16 = 151;
const TOK_TERRAIN_SHADER: u16 = 152;
const TOK_TERRAIN_TEXSLOTS: u16 = 153;
const TOK_TERRAIN_TEXSLOT: u16 = 154;
const TOK_TERRAIN_UVCALCS: u16 = 155;
const TOK_TERRAIN_UVCALC: u16 = 156;
const TOK_TERRAIN_PATCHES: u16 = 157;
const TOK_TERRAIN_PATCHSETS: u16 = 158;
const TOK_TERRAIN_PATCHSET: u16 = 159;
const TOK_TERRAIN_PATCHSET_DISTANCE: u16 = 160;
const TOK_TERRAIN_PATCHSET_NPATCHES: u16 = 161;
const TOK_TERRAIN_PATCHSET_PATCHES: u16 = 163;
const TOK_TERRAIN_PATCHSET_PATCH: u16 = 164;

// ─── SBR binary reader ───────────────────────────────────────────────────────

/// Cursor over a raw SBR binary payload (after the 16-byte SIMISA header and
/// the 16-byte `JINX0t?b` sub-header with padding).
struct SbrCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> SbrCursor<'a> {
    fn new(buf: &'a [u8], offset: usize) -> Self {
        Self { buf, pos: offset }
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Option<u8> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Some(v)
    }

    fn read_u16(&mut self) -> Option<u16> {
        if self.remaining() < 2 {
            return None;
        }
        let v = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Some(v)
    }

    fn read_u32(&mut self) -> Option<u32> {
        if self.remaining() < 4 {
            return None;
        }
        let v = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Some(v)
    }

    fn read_i32(&mut self) -> Option<i32> {
        self.read_u32().map(|v| v as i32)
    }

    fn read_f32(&mut self) -> Option<f32> {
        if self.remaining() < 4 {
            return None;
        }
        let v = f32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Some(v)
    }

    /// Read a Kuju UTF-16LE string: `u16 count` + `count × 2` bytes.
    fn read_kuju_string(&mut self) -> Option<String> {
        let count = self.read_u16()? as usize;
        if count == 0 {
            return Some(String::new());
        }
        if self.remaining() < count * 2 {
            return None;
        }
        let raw = &self.buf[self.pos..self.pos + count * 2];
        self.pos += count * 2;
        let chars: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&chars))
    }

    /// Read an SBR block header and consume the label byte(s).
    /// Returns `(token_id, block_end_pos)` where `block_end_pos` is the byte
    /// position AFTER all content of this block.  The cursor is left at the
    /// start of the block content (after label bytes).
    fn read_block_header(&mut self) -> Option<(u16, usize)> {
        let tok = self.read_u16()?;
        let _flg = self.read_u16()?;
        let rem = self.read_u32()? as usize;
        let block_end = self.pos + rem;
        if block_end > self.buf.len() {
            return None;
        }
        // Label byte: number of UTF-16 chars (each 2 bytes) in the label.
        let label_len = self.read_u8()? as usize;
        self.pos += label_len * 2; // Skip label text
        Some((tok, block_end))
    }
}

// ─── Binary terrain parser ───────────────────────────────────────────────────

/// Parse MSTS binary terrain (JINX0t6b) payload, extracting `TerrainSamples`
/// and `TerrainPatchSet`s.
///
/// The payload is the raw file bytes (including the SIMISA header).
fn parse_terrain_binary(
    bytes: &[u8],
) -> Result<(TerrainSamples, Vec<TerrainShader>, Vec<TerrainPatchSet>), FormatError> {
    let bad = |msg: &str| FormatError::UnexpectedToken {
        offset: 0,
        message: msg.to_owned(),
    };

    let payload =
        crate::msts_simisa::decode_simisa_container(bytes).map_err(|_| bad("SIMISA decode"))?;
    if payload.is_text {
        return Err(bad(
            "expected binary terrain (JINX0t6b) but got text payload",
        ));
    }

    // Skip to first binary block. The SIMISA decoder uses `find_binary_start`
    // which only knows shape/world tokens, so for terrain we manually seek to
    // the fixed offset: 8 bytes JINX header + 8 bytes padding = 16 bytes.
    let data_offset = {
        let body = &payload.bytes;
        let candidate = 16usize;
        // Verify: the u16 at that position should be the `terrain` token (136).
        if candidate + 2 <= body.len() {
            let tok = u16::from_le_bytes([body[candidate], body[candidate + 1]]);
            if tok == TOK_TERRAIN {
                candidate
            } else {
                // Fall back to OR's heuristic offset
                payload.data_offset
            }
        } else {
            payload.data_offset
        }
    };

    let mut cur = SbrCursor::new(&payload.bytes, data_offset);

    // Outer `terrain` block
    let (tok, terrain_end) = cur
        .read_block_header()
        .ok_or_else(|| bad("terrain: truncated outer block header"))?;
    if tok != TOK_TERRAIN {
        return Err(bad(&format!(
            "terrain: expected terrain token 136, got {tok}"
        )));
    }

    let mut samples = TerrainSamples::default();
    let mut shaders: Vec<TerrainShader> = Vec::new();
    let mut patch_sets: Vec<TerrainPatchSet> = Vec::new();

    while cur.pos < terrain_end {
        let Some((child_tok, child_end)) = cur.read_block_header() else {
            break;
        };

        match child_tok {
            TOK_TERRAIN_SAMPLES => {
                parse_samples_block(&mut cur, child_end, &mut samples);
            }
            TOK_TERRAIN_SHADERS => {
                shaders = parse_shaders_block(&mut cur, child_end);
            }
            TOK_TERRAIN_PATCHES => {
                parse_patches_block(&mut cur, child_end, &mut patch_sets);
            }
            _ => {}
        }

        cur.pos = child_end.min(terrain_end);
    }

    if samples.nsamples == 0 {
        return Err(bad("terrain: terrain_nsamples not found or zero"));
    }

    Ok((samples, shaders, patch_sets))
}

fn parse_samples_block(cur: &mut SbrCursor<'_>, block_end: usize, out: &mut TerrainSamples) {
    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        match tok {
            TOK_TERRAIN_NSAMPLES => {
                if let Some(v) = cur.read_i32() {
                    out.nsamples = v.max(1) as u32;
                }
            }
            TOK_TERRAIN_SAMPLE_FLOOR => {
                if let Some(v) = cur.read_f32() {
                    out.sample_floor = v as f64;
                }
            }
            TOK_TERRAIN_SAMPLE_SCALE => {
                if let Some(v) = cur.read_f32() {
                    out.sample_scale = v as f64;
                }
            }
            TOK_TERRAIN_SAMPLE_SIZE => {
                if let Some(v) = cur.read_f32() {
                    out.sample_size = v as f64;
                }
            }
            TOK_TERRAIN_SAMPLE_YBUFFER => {
                if let Some(s) = cur.read_kuju_string() {
                    out.y_buffer_file = s;
                }
            }
            TOK_TERRAIN_SAMPLE_EBUFFER => {
                if let Some(s) = cur.read_kuju_string() {
                    out.e_buffer_file = s;
                }
            }
            TOK_TERRAIN_SAMPLE_NBUFFER => {
                if let Some(s) = cur.read_kuju_string() {
                    out.n_buffer_file = s;
                }
            }
            TOK_TERRAIN_SAMPLE_FBUFFER => {
                if let Some(s) = cur.read_kuju_string() {
                    out.f_buffer_file = s;
                }
            }
            _ => {}
        }
        cur.pos = end.min(block_end);
    }
}

// ─── Shader block parsers ───────────────────────────────────────────────────

/// Parse a `terrain_shaders` block.
///
/// OR reads: `i32 count` (raw, not a sub-block), then `count` × `terrain_shader` sub-blocks.
fn parse_shaders_block(cur: &mut SbrCursor<'_>, block_end: usize) -> Vec<TerrainShader> {
    let count = cur.read_i32().unwrap_or(0).max(0) as usize;
    let mut shaders = Vec::with_capacity(count.min(64));

    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        if tok == TOK_TERRAIN_SHADER {
            if let Some(shader) = parse_shader_block(cur, end) {
                shaders.push(shader);
            }
        }
        cur.pos = end.min(block_end);
    }
    shaders
}

/// Parse one `terrain_shader` block.
///
/// OR reads: `ReadString()` shader name directly (no sub-block wrapper), then sub-blocks for
/// `terrain_texslots` and `terrain_uvcalcs`.
fn parse_shader_block(cur: &mut SbrCursor<'_>, block_end: usize) -> Option<TerrainShader> {
    // Shader name is a raw Kuju string at the start of the block content.
    let name = cur.read_kuju_string().unwrap_or_default();
    let mut texslots = Vec::new();
    let mut uvcalcs = Vec::new();

    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        match tok {
            TOK_TERRAIN_TEXSLOTS => {
                texslots = parse_texslots_block(cur, end);
            }
            TOK_TERRAIN_UVCALCS => {
                uvcalcs = parse_uvcalcs_block(cur, end);
            }
            _ => {}
        }
        cur.pos = end.min(block_end);
    }

    Some(TerrainShader {
        name,
        texslots,
        uvcalcs,
    })
}

/// Parse a `terrain_texslots` block.
///
/// OR reads: `ReadUInt()` count (raw), then `count` × `terrain_texslot` sub-blocks.
/// Each texslot: `ReadString()` filename, `ReadInt()` A, `ReadInt()` B.
fn parse_texslots_block(cur: &mut SbrCursor<'_>, block_end: usize) -> Vec<TerrainTexSlot> {
    let count = cur.read_u32().unwrap_or(0) as usize;
    let mut slots = Vec::with_capacity(count.min(16));

    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        if tok == TOK_TERRAIN_TEXSLOT {
            let filename = cur.read_kuju_string().unwrap_or_default();
            let a = cur.read_i32().unwrap_or(0);
            let b = cur.read_i32().unwrap_or(0);
            slots.push(TerrainTexSlot { filename, a, b });
        }
        cur.pos = end.min(block_end);
    }
    slots
}

/// Parse a `terrain_uvcalcs` block.
///
/// OR reads: `ReadUInt()` count (raw), then `count` × `terrain_uvcalc` sub-blocks.
/// Each uvcalc: `ReadInt()` × 3, `ReadFloat()` D.
fn parse_uvcalcs_block(cur: &mut SbrCursor<'_>, block_end: usize) -> Vec<TerrainUvCalc> {
    let count = cur.read_u32().unwrap_or(0) as usize;
    let mut calcs = Vec::with_capacity(count.min(8));

    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        if tok == TOK_TERRAIN_UVCALC {
            let a = cur.read_i32().unwrap_or(0);
            let b = cur.read_i32().unwrap_or(0);
            let c = cur.read_i32().unwrap_or(0);
            let d = cur.read_f32().unwrap_or(0.0) as f64;
            calcs.push(TerrainUvCalc { a, b, c, d });
        }
        cur.pos = end.min(block_end);
    }
    calcs
}

fn parse_patches_block(cur: &mut SbrCursor<'_>, block_end: usize, out: &mut Vec<TerrainPatchSet>) {
    // terrain_patches contains one terrain_patchsets block
    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        if tok == TOK_TERRAIN_PATCHSETS {
            parse_patchsets_block(cur, end, out);
        }
        cur.pos = end.min(block_end);
    }
}

fn parse_patchsets_block(
    cur: &mut SbrCursor<'_>,
    block_end: usize,
    out: &mut Vec<TerrainPatchSet>,
) {
    // `terrain_patchsets` content starts with a raw i32 count, then sub-blocks.
    let _count = cur.read_i32().unwrap_or(0);

    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        if tok == TOK_TERRAIN_PATCHSET {
            if let Some(ps) = parse_patchset_block(cur, end) {
                out.push(ps);
            }
        }
        cur.pos = end.min(block_end);
    }
}

fn parse_patchset_block(cur: &mut SbrCursor<'_>, block_end: usize) -> Option<TerrainPatchSet> {
    let mut distance = 0i32;
    let mut npatches = 0u32;
    let mut patches: Vec<TerrainPatch> = Vec::new();

    while cur.pos < block_end {
        let (tok, end) = cur.read_block_header()?;
        match tok {
            TOK_TERRAIN_PATCHSET_DISTANCE => {
                if let Some(v) = cur.read_i32() {
                    distance = v;
                }
            }
            TOK_TERRAIN_PATCHSET_NPATCHES => {
                if let Some(v) = cur.read_i32() {
                    npatches = v.max(0) as u32;
                }
            }
            TOK_TERRAIN_PATCHSET_PATCHES => {
                parse_patchset_patches(cur, end, npatches, &mut patches);
            }
            _ => {}
        }
        cur.pos = end.min(block_end);
    }

    if npatches == 0 {
        return None;
    }
    Some(TerrainPatchSet {
        distance,
        npatches,
        patches,
    })
}

fn parse_patchset_patches(
    cur: &mut SbrCursor<'_>,
    block_end: usize,
    npatches: u32,
    out: &mut Vec<TerrainPatch>,
) {
    let expected = (npatches as usize).saturating_mul(npatches as usize);
    out.reserve(expected);

    while cur.pos < block_end {
        let Some((tok, end)) = cur.read_block_header() else {
            break;
        };
        if tok == TOK_TERRAIN_PATCHSET_PATCH {
            if let Some(p) = parse_patch_raw(cur) {
                out.push(p);
            }
        }
        cur.pos = end.min(block_end);
    }
}

/// `terrain_patchset_patch` content is raw binary (no nested sub-blocks):
/// u32 flags, f32 cx, f32 ay, f32 cz, f32 fy, f32 ry, f32 rm,
/// i32 shader, f32 X, Y, W, B, C, H, f32 error_bias.
fn parse_patch_raw(cur: &mut SbrCursor<'_>) -> Option<TerrainPatch> {
    let flags = cur.read_u32()?;
    let center_x = cur.read_f32()?;
    let average_y = cur.read_f32()?;
    let center_z = cur.read_f32()?;
    let factor_y = cur.read_f32()?;
    let range_y = cur.read_f32()?;
    let radius_m = cur.read_f32()?;
    let shader_index = cur.read_i32()?;
    let x = cur.read_f32()?;
    let y = cur.read_f32()?;
    let w = cur.read_f32()?;
    let b = cur.read_f32()?;
    let c = cur.read_f32()?;
    let h = cur.read_f32()?;
    let error_bias = cur.read_f32()?;
    Some(TerrainPatch {
        flags,
        center_x,
        average_y,
        center_z,
        factor_y,
        range_y,
        radius_m,
        shader_index,
        x,
        y,
        w,
        h,
        b,
        c,
        error_bias,
    })
}

fn raw_buffer_path(tile_path: &Path, name: &str) -> PathBuf {
    let name = name.trim();
    if name.is_empty() {
        return tile_path.with_extension("raw");
    }
    tile_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

/// Row-major `nsamples × nsamples` elevation field (metres).
#[derive(Clone, Debug, PartialEq)]
pub struct ElevationGrid {
    pub nsamples: usize,
    pub elevations: Vec<f32>,
}

/// Row-major feature flags from `_F.RAW` (OR `TerrainFlagsFile`).
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureGrid {
    pub nsamples: usize,
    pub flags: Vec<u8>,
}

impl FeatureGrid {
    pub fn is_vertex_hidden(&self, x: usize, z: usize) -> bool {
        if x >= self.nsamples || z >= self.nsamples {
            return false;
        }
        (self.flags[z * self.nsamples + x] & VERTEX_HIDDEN_FLAG) == VERTEX_HIDDEN_FLAG
    }

    pub fn hidden_count(&self) -> usize {
        self.flags
            .iter()
            .filter(|b| (**b & VERTEX_HIDDEN_FLAG) == VERTEX_HIDDEN_FLAG)
            .count()
    }

    /// True if any vertex in the 17×17 patch grid is marked hidden in `_F.RAW`.
    pub fn patch_has_hidden_vertices(&self, patch_x: u32, patch_z: u32) -> bool {
        for z in 0..=PATCH_CELLS {
            for x in 0..=PATCH_CELLS {
                let ux = patch_x * PATCH_CELLS + x;
                let uz = patch_z * PATCH_CELLS + z;
                if self.is_vertex_hidden(ux as usize, uz as usize) {
                    return true;
                }
            }
        }
        false
    }
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

/// Decode `_F.RAW` feature flags (OR: hidden when `(byte & 0x04) != 0`).
pub fn read_f_raw(path: &Path, params: &TerrainSamples) -> Result<FeatureGrid, FormatError> {
    let nsamples = params.nsamples as usize;
    let expected = nsamples
        .checked_mul(nsamples)
        .ok_or_else(|| FormatError::UnexpectedAtom {
            key: "grid".into(),
            context: "terrain f raw".into(),
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
                "expected {expected} bytes for {}×{} feature grid, got {}",
                nsamples,
                nsamples,
                bytes.len()
            ),
        });
    }

    Ok(FeatureGrid {
        nsamples,
        flags: bytes,
    })
}

/// Affine UV for one patch vertex (OR `TerrainPrimitive.GetVertexBuffer`).
pub fn patch_affine_uv(patch: &TerrainPatch, u: f32, v: f32) -> [f32; 2] {
    [
        u * patch.w + v * patch.b + patch.x,
        u * patch.c + v * patch.h + patch.y,
    ]
}

fn sample_hidden(
    hidden: Option<&FeatureGrid>,
    patch_x: u32,
    patch_z: u32,
    vx: u32,
    vz: u32,
) -> bool {
    hidden.is_some_and(|grid| {
        let ux = patch_x * PATCH_CELLS + vx;
        let uz = patch_z * PATCH_CELLS + vz;
        grid.is_vertex_hidden(ux as usize, uz as usize)
    })
}

fn append_patch_indices(
    indices: &mut Vec<u32>,
    hidden: Option<&FeatureGrid>,
    patch_x: u32,
    patch_z: u32,
    skip_hidden: bool,
) {
    for z in 0..PATCH_CELLS {
        for x in 0..PATCH_CELLS {
            let i00 = z * PATCH_VERTS + x;
            let i10 = i00 + 1;
            let i01 = i00 + PATCH_VERTS;
            let i11 = i01 + 1;

            let h00 = sample_hidden(hidden, patch_x, patch_z, x, z);
            let h10 = sample_hidden(hidden, patch_x, patch_z, x + 1, z);
            let h01 = sample_hidden(hidden, patch_x, patch_z, x, z + 1);
            let h11 = sample_hidden(hidden, patch_x, patch_z, x + 1, z + 1);

            if skip_hidden {
                if (x & 1) == (z & 1) {
                    if !(h00 || h11 || h01) {
                        indices.extend([i00, i11, i01]);
                    }
                    if !(h00 || h10 || h11) {
                        indices.extend([i00, i10, i11]);
                    }
                } else if !(h00 || h10 || h01) {
                    indices.extend([i00, i10, i01]);
                } else if !(h01 || h10 || h11) {
                    indices.extend([i01, i10, i11]);
                }
            } else if (x & 1) == (z & 1) {
                indices.extend([i00, i11, i01, i00, i10, i11]);
            } else {
                indices.extend([i00, i10, i01, i01, i10, i11]);
            }
        }
    }
}

/// Build one OR-style terrain patch with optional affine UVs and hidden triangles.
pub fn build_patch_mesh_data_ex(
    grid: &ElevationGrid,
    sample_size: f64,
    patch_x: u32,
    patch_z: u32,
    patch: Option<&TerrainPatch>,
    hidden: Option<&FeatureGrid>,
    skip_hidden_tris: bool,
) -> TerrainMeshData {
    let cell = sample_size;
    let origin_x = patch_x as f64 * PATCH_SIZE_M;
    let origin_z = patch_z as f64 * PATCH_SIZE_M;

    let mut positions = Vec::with_capacity((PATCH_VERTS * PATCH_VERTS) as usize);
    let mut uvs = Vec::with_capacity((PATCH_VERTS * PATCH_VERTS) as usize);

    for vz in 0..PATCH_VERTS {
        for vx in 0..PATCH_VERTS {
            let lx = origin_x + vx as f64 * cell;
            let lz = origin_z + vz as f64 * cell;
            let y = grid.sample_bilinear(lx, lz, sample_size);
            positions.push([lx as f32, y, lz as f32]);
            uvs.push(if let Some(p) = patch {
                patch_affine_uv(p, vx as f32, vz as f32)
            } else {
                [
                    vx as f32 / PATCH_CELLS as f32,
                    vz as f32 / PATCH_CELLS as f32,
                ]
            });
        }
    }

    let normals = compute_vertex_normals(&positions, PATCH_VERTS as usize, PATCH_VERTS as usize);
    let mut indices = Vec::with_capacity((PATCH_CELLS * PATCH_CELLS * 6) as usize);
    append_patch_indices(&mut indices, hidden, patch_x, patch_z, skip_hidden_tris);

    TerrainMeshData {
        positions,
        normals,
        uvs,
        indices,
    }
}

/// Build one OR-style terrain patch: 17×17 vertices, 16×16 cells, alternating diagonals.
pub fn build_patch_mesh_data(
    grid: &ElevationGrid,
    sample_size: f64,
    patch_x: u32,
    patch_z: u32,
    patch_size_m: f64,
) -> TerrainMeshData {
    let _ = patch_size_m;
    build_patch_mesh_data_ex(grid, sample_size, patch_x, patch_z, None, None, false)
}

/// Merge all patches of a tile into one mesh (legacy fallback without patch metadata).
pub fn build_tile_mesh_data(grid: &ElevationGrid, sample_size: f64) -> TerrainMeshData {
    const PATCHES_PER_SIDE: u32 = 16;
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
            "terrain_sample_ebuffer" => {
                if let Some(s) = atom_str(val) {
                    out.e_buffer_file = s;
                }
            }
            "terrain_sample_nbuffer" => {
                if let Some(s) = atom_str(val) {
                    out.n_buffer_file = s;
                }
            }
            "terrain_sample_fbuffer" => {
                if let Some(s) = atom_str(val) {
                    out.f_buffer_file = s;
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

fn parse_terrain_shaders(ast: &Ast) -> Vec<TerrainShader> {
    let mut out = Vec::new();
    walk_named_blocks(ast, "terrain_shaders", &mut |block| {
        for item in block {
            if list_head_ast(item) == Some("terrain_shader") {
                if let Some(shader) = parse_terrain_shader(item) {
                    out.push(shader);
                }
            }
        }
    });
    out
}

fn parse_terrain_shader(ast: &Ast) -> Option<TerrainShader> {
    let Ast::List(items) = ast else {
        return None;
    };
    let name = items.iter().skip(1).find_map(find_first_string)?;
    let mut texslots = Vec::new();
    let mut uvcalcs = Vec::new();
    walk_named_blocks(ast, "terrain_texslots", &mut |block| {
        for item in block {
            if list_head_ast(item) == Some("terrain_texslot") {
                if let Some(slot) = parse_terrain_texslot(item) {
                    texslots.push(slot);
                }
            }
        }
    });
    walk_named_blocks(ast, "terrain_uvcalcs", &mut |block| {
        for item in block {
            if list_head_ast(item) == Some("terrain_uvcalc") {
                if let Some(calc) = parse_terrain_uvcalc(item) {
                    uvcalcs.push(calc);
                }
            }
        }
    });
    Some(TerrainShader {
        name,
        texslots,
        uvcalcs,
    })
}

fn find_first_string(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(Atom::String(s)) | Ast::Atom(Atom::Symbol(s)) => Some(s.clone()),
        Ast::List(items) => items.iter().find_map(find_first_string),
        _ => None,
    }
}

fn parse_terrain_texslot(ast: &Ast) -> Option<TerrainTexSlot> {
    let Ast::List(items) = ast else {
        return None;
    };
    let filename = items.iter().skip(1).find_map(find_first_string)?;
    let nums: Vec<i32> = collect_numbers(ast)
        .into_iter()
        .skip(1)
        .map(|n| n as i32)
        .collect();
    Some(TerrainTexSlot {
        filename,
        a: nums.first().copied().unwrap_or(0),
        b: nums.get(1).copied().unwrap_or(0),
    })
}

fn parse_terrain_uvcalc(ast: &Ast) -> Option<TerrainUvCalc> {
    let nums = collect_numbers(ast);
    if nums.len() < 5 {
        return None;
    }
    Some(TerrainUvCalc {
        a: nums[1] as i32,
        b: nums[2] as i32,
        c: nums[3] as i32,
        d: nums[4],
    })
}

fn parse_terrain_patch_sets(ast: &Ast) -> Vec<TerrainPatchSet> {
    let mut out = Vec::new();
    walk_named_blocks(ast, "terrain_patches", &mut |block| {
        for item in block {
            if list_head_ast(item) == Some("terrain_patchset") {
                if let Some(set) = parse_terrain_patch_set(item) {
                    out.push(set);
                }
            }
        }
    });
    out
}

fn parse_terrain_patch_set(ast: &Ast) -> Option<TerrainPatchSet> {
    let mut distance = 0;
    let mut npatches = 0u32;
    let mut patches = Vec::new();
    walk_ast(ast, &mut |items| {
        if items.len() < 2 {
            return;
        }
        let Ast::Atom(Atom::Symbol(key)) = &items[0] else {
            return;
        };
        match key.as_str() {
            "terrain_patchset_distance" => {
                if let Some(v) = atom_num(&items[1]) {
                    distance = v as i32;
                }
            }
            "terrain_patchset_npatches" => {
                if let Some(v) = atom_num(&items[1]) {
                    npatches = v.max(0.0) as u32;
                }
            }
            _ => {}
        }
    });
    walk_named_blocks(ast, "terrain_patchset_patches", &mut |block| {
        for item in block {
            if list_head_ast(item) == Some("terrain_patchset_patch") {
                if let Some(patch) = parse_terrain_patch(item) {
                    patches.push(patch);
                }
            }
        }
    });
    if npatches == 0 {
        return None;
    }
    Some(TerrainPatchSet {
        distance,
        npatches,
        patches,
    })
}

fn parse_terrain_patch(ast: &Ast) -> Option<TerrainPatch> {
    let nums = collect_numbers(ast);
    if nums.len() < 15 {
        return None;
    }
    Some(TerrainPatch {
        flags: nums[0] as u32,
        center_x: nums[1] as f32,
        average_y: nums[2] as f32,
        center_z: nums[3] as f32,
        factor_y: nums[4] as f32,
        range_y: nums[5] as f32,
        radius_m: nums[6] as f32,
        shader_index: nums[7] as i32,
        x: nums[8] as f32,
        y: nums[9] as f32,
        w: nums[10] as f32,
        h: nums[11] as f32,
        b: nums[12] as f32,
        c: nums[13] as f32,
        error_bias: nums[14] as f32,
    })
}

fn walk_named_blocks(ast: &Ast, name: &str, f: &mut dyn FnMut(&[Ast])) {
    walk_ast(ast, &mut |items| {
        if items.len() >= 2 && list_head(items) == Some(name) {
            f(&items[1..]);
        }
    });
}

fn list_head(items: &[Ast]) -> Option<&str> {
    match items.first()? {
        Ast::Atom(Atom::Symbol(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn list_head_ast(ast: &Ast) -> Option<&str> {
    match ast {
        Ast::List(items) => list_head(items),
        _ => None,
    }
}

fn collect_numbers(ast: &Ast) -> Vec<f64> {
    let mut out = Vec::new();
    flatten_numbers(ast, &mut out);
    out
}

fn flatten_numbers(ast: &Ast, out: &mut Vec<f64>) {
    match ast {
        Ast::List(items) => {
            for item in items {
                flatten_numbers(item, out);
            }
        }
        Ast::Atom(Atom::Number(v)) => out.push(*v),
        Ast::Atom(Atom::Integer(v)) => out.push(*v as f64),
        _ => {}
    }
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
        assert!(tf.shaders.is_empty());
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

    #[test]
    fn parse_terrain_with_patches_fixture() {
        let tf = TerrainFile::from_path(fixture("terrain_with_patches.y")).expect("parse");
        assert_eq!(tf.shaders.len(), 1);
        assert_eq!(tf.shaders[0].texslots.len(), 2);
        let set = tf.primary_patch_set().expect("patch set");
        assert_eq!(set.npatches, 2);
        assert_eq!(set.patches.len(), 4);
    }

    #[test]
    fn patch_translation_includes_or_z_shift_for_camera_space() {
        let patch = TerrainPatch {
            flags: 0,
            center_x: 1024.0,
            average_y: 0.0,
            center_z: 1024.0,
            factor_y: 0.0,
            range_y: 0.0,
            radius_m: 64.0,
            shader_index: 0,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            b: 0.0,
            c: 0.0,
            error_bias: 0.0,
        };
        let (cx, cz) = patch.patch_translation();
        assert!((cx).abs() < 1e-3);
        assert!(
            (cz - 2048.0).abs() < 1e-3,
            "OR adds +2048 on Z; viewer must not stack this on patch_index×128"
        );
    }

    #[test]
    fn patch_affine_uv_matches_or_formula() {
        let patch = TerrainPatch {
            flags: 0,
            center_x: 64.0,
            average_y: 0.0,
            center_z: 64.0,
            factor_y: 0.0,
            range_y: 0.0,
            radius_m: 64.0,
            shader_index: 0,
            x: 0.1,
            y: 0.2,
            w: 0.01,
            h: 0.02,
            b: 0.03,
            c: 0.04,
            error_bias: 0.0,
        };
        let uv = patch_affine_uv(&patch, 0.0, 0.0);
        assert!((uv[0] - 0.1).abs() < 1e-5);
        assert!((uv[1] - 0.2).abs() < 1e-5);
        let uv16 = patch_affine_uv(&patch, 16.0, 16.0);
        assert!((uv16[0] - (0.1 + 16.0 * 0.01 + 16.0 * 0.03)).abs() < 1e-4);
    }

    #[test]
    fn hidden_vertices_reduce_index_count() {
        let tf = TerrainFile::from_path(fixture("terrain_with_patches.y")).unwrap();
        let grid = read_y_raw(&fixture("minimal_terrain_y.raw"), &tf.samples).unwrap();
        let fgrid = read_f_raw(&fixture("terrain_with_hole_f.raw"), &tf.samples).unwrap();
        assert!(fgrid.hidden_count() > 0);
        let patch = &tf.primary_patch_set().unwrap().patches[0];
        let full = build_patch_mesh_data_ex(
            &grid,
            tf.samples.sample_size,
            0,
            0,
            Some(patch),
            None,
            false,
        );
        let holed = build_patch_mesh_data_ex(
            &grid,
            tf.samples.sample_size,
            0,
            0,
            Some(patch),
            Some(&fgrid),
            true,
        );
        assert!(holed.indices.len() < full.indices.len());
    }

    /// Verify that the binary terrain parser extracts shader and texture slot data
    /// from a real Chiltern `JINX0t6b` tile (enables textured terrain patches).
    #[test]
    fn parse_chiltern_binary_terrain_has_shaders() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/TILES/-11cf06a4.t");
        if !path.exists() {
            return; // Skip when Chiltern data is absent (CI)
        }
        let tf = TerrainFile::from_path(&path).expect("parse binary terrain tile");
        assert!(
            !tf.shaders.is_empty(),
            "binary tile should have at least one shader"
        );
        let shader = &tf.shaders[0];
        assert!(
            !shader.texslots.is_empty(),
            "first shader should have at least one texture slot"
        );
        assert!(
            !shader.texslots[0].filename.is_empty(),
            "first texslot should have a filename"
        );
        assert!(
            tf.has_textured_patches(),
            "has_textured_patches() should be true when shaders are populated"
        );
    }

    /// Verify that the binary terrain parser reads correct floor/scale from a
    /// real Chiltern `JINX0t6b` tile rather than using hardcoded defaults.
    #[test]
    fn parse_chiltern_binary_terrain_floor_and_scale() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/TILES/-11cf06a4.t");
        if !path.exists() {
            return; // Skip when Chiltern data is absent (CI)
        }
        let tf = TerrainFile::from_path(&path).expect("parse binary terrain tile");
        assert_eq!(tf.samples.nsamples, 256);
        assert!(
            (tf.samples.sample_floor - (-63.0)).abs() < 0.01,
            "sample_floor should be -63, got {}",
            tf.samples.sample_floor
        );
        assert!(
            (tf.samples.sample_scale - 0.001953125).abs() < 1e-6,
            "sample_scale should be ~1/512, got {}",
            tf.samples.sample_scale
        );
        assert!(
            (tf.samples.sample_size - 8.0).abs() < 0.01,
            "sample_size should be 8m, got {}",
            tf.samples.sample_size
        );
        assert!(
            !tf.samples.y_buffer_file.is_empty(),
            "y_buffer_file should not be empty"
        );
        // Also verify the patch sets are parsed
        assert!(
            !tf.patch_sets.is_empty(),
            "should have at least one patch set"
        );
        let ps = tf.primary_patch_set().unwrap();
        assert_eq!(ps.npatches, 16, "Chiltern tile should have 16x16 patches");
        assert_eq!(ps.patches.len(), 256, "should have 256 patch descriptors");
    }

    #[test]
    fn feature_grid_hidden_flag() {
        let grid = FeatureGrid {
            nsamples: 2,
            flags: vec![0, 0x04, 0, 0],
        };
        assert!(!grid.is_vertex_hidden(0, 0));
        assert!(grid.is_vertex_hidden(1, 0));
    }
}
