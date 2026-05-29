//! Parser for MSTS Shape (`.s`) ASCII files.
//!
//! MSTS shapes come in two flavours: an S-expression ASCII form and a
//! "binary tokenized" form sharing the same tag schema.  This module parses
//! ASCII directly and converts the binary token stream into equivalent
//! S-expression text first.
//!
//! The grammar fragments we care about (lenient — unknown sub-fields are
//! ignored):
//!
//! ```text
//! ( shape
//!     ( shape_header ... )
//!     ( volumes ... )
//!     ( shader_names ... )
//!     ( texture_filenames "tex0.ace" "tex1.ace" ... )
//!     ( points <count> ( point x y z ) ... )
//!     ( uv_points <count> ( uv_point u v ) ... )
//!     ( normals <count> ( vector x y z ) ... )
//!     ( prim_states <count> ( prim_state ... ) ... )
//!     ( lod_controls <count>
//!         ( lod_control
//!             ( distance_levels_header )
//!             ( distance_levels <count>
//!                 ( distance_level
//!                     ( distance_level_header ( dlevel_selection <m> ) ... )
//!                     ( sub_objects <count> ( sub_object
//!                         ( vertices ... )
//!                         ( primitives <count>
//!                             ( prim_state_idx <i> )
//!                             ( indexed_trilist
//!                                 ( vertex_idxs <count> i j k ... )
//!                             )
//!                         )
//!                     ) ... )
//!                 )
//!             )
//!         )
//!     )
//!     ( matrices <count> ( matrix "name" m11 m12 ... m43 ) ... )
//! )
//! ```
//!
//! Anything beyond the fields surfaced in [`ShapeFile`] is intentionally
//! ignored — Fase 23 will extend the model when it actually needs them.

use crate::ast::{Ast, Atom};
use crate::encoding::decode_msts_bytes;
use crate::error::FormatError;
use crate::msts_simisa::decode_simisa_container;
use crate::parser::parse_from_first_paren;
use crate::shape_binary::binary_shape_to_ascii;

use super::atom_to_number;
use super::atom_to_string;

/// 3-component vector (point / normal).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// 2-component texture coordinate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub u: f64,
    pub v: f64,
}

/// One row in `prim_states`: ties together a shader, a texture index and a
/// vertex state.  We keep raw numeric ids so the consumer (Fase 23) can resolve
/// them against `texture_filenames` / `shader_names`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimState {
    /// Optional human-readable name (`prim_state "name" ...`).
    pub name: Option<String>,
    /// Raw `prim_state` flags when present.
    pub flags: i32,
    /// Index into [`ShapeFile::shader_names`] (-1 = unknown).
    pub shader_idx: i32,
    /// Index into the `texture_filenames` list (-1 = none).
    pub texture_idx: i32,
    /// All texture indices declared by `tex_idxs`, excluding the leading count.
    pub tex_indices: Vec<i32>,
    /// Index into `vtx_states` when present (-1 = unknown).
    pub vertex_state_idx: i32,
    /// Optional z-bias value carried by later MSTS/Open Rails prim_state layouts.
    pub z_bias: Option<f64>,
}

/// A single triangle list block (`indexed_trilist`).
///
/// `vertex_indices` holds raw indices in groups of three (i, j, k); the helper
/// [`Primitive::triangle_count`] returns `vertex_indices.len() / 3`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitive {
    /// Index into the parent shape's `prim_states` table (-1 if missing).
    pub prim_state_idx: i32,
    /// Flat list of vertex indices (length is a multiple of 3).
    pub vertex_indices: Vec<u32>,
}

impl Primitive {
    pub fn triangle_count(&self) -> usize {
        self.vertex_indices.len() / 3
    }
}

/// A MSTS `vertex` entry.
///
/// Primitive `vertex_idxs` refer to this table first; each vertex then points
/// into the global point, normal and UV arrays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vertex {
    /// Index into [`ShapeFile::points`].
    pub point_idx: i32,
    /// Index into [`ShapeFile::normals`].
    pub normal_idx: i32,
    /// Indices into [`ShapeFile::uvs`]. MSTS may carry more than one UV set.
    pub uv_indices: Vec<i32>,
}

/// A `sub_object` inside a `distance_level`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubObject {
    /// Vertex count declared by `vertices <count>`.
    pub vertex_count: usize,
    /// Vertex table addressed by [`Primitive::vertex_indices`].
    pub vertices: Vec<Vertex>,
    pub primitives: Vec<Primitive>,
}

/// One LOD entry: a draw distance and the meshes drawn at that distance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistanceLevel {
    /// Selection distance in metres (`dlevel_selection`).
    pub selection_m: f64,
    pub sub_objects: Vec<SubObject>,
}

/// A `lod_control` block: a list of distance levels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LodControl {
    pub distance_levels: Vec<DistanceLevel>,
}

/// 4x3 transform matrix (MSTS stores 12 floats per matrix).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Matrix43 {
    pub rows: [[f64; 3]; 4],
}

/// Named transform inside `matrices`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedMatrix {
    pub name: String,
    pub matrix: Matrix43,
}

/// Parsed `.s` ASCII file.
#[derive(Clone, Debug, Default)]
pub struct ShapeFile {
    /// Texture filenames declared via `texture_filenames`.
    pub texture_filenames: Vec<String>,
    /// Shader names declared via `shader_names`.
    pub shader_names: Vec<String>,
    pub points: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub normals: Vec<Vec3>,
    pub prim_states: Vec<PrimState>,
    pub lod_controls: Vec<LodControl>,
    pub matrices: Vec<NamedMatrix>,
}

impl ShapeFile {
    /// Parse from a pre-built AST.
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let texture_filenames = collect_texture_filenames(ast);
        let shader_names = collect_shader_names(ast);
        let points = collect_points(ast);
        let uvs = collect_uv_points(ast);
        let normals = collect_normals(ast);
        let prim_states = collect_prim_states(ast);
        let lod_controls = collect_lod_controls(ast);
        let matrices = collect_matrices(ast);

        Ok(Self {
            texture_filenames,
            shader_names,
            points,
            uvs,
            normals,
            prim_states,
            lod_controls,
            matrices,
        })
    }

    /// Read and parse a `.s` file from disk (ASCII, zlib-compressed ASCII, or binary tokenized).
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedToken {
            offset: 0,
            message: format!("failed to read {}: {e}", path.display()),
        })?;
        let text = shape_text_from_bytes(&bytes)?;
        let ast = parse_from_first_paren(&text)?;
        Self::from_ast(&ast)
    }
}

fn shape_text_from_bytes(bytes: &[u8]) -> Result<String, FormatError> {
    if bytes.len() >= 16 && bytes.starts_with(b"SIMISA") {
        let payload = decode_simisa_container(bytes)?;
        if payload.is_text {
            return Ok(decode_msts_bytes(&payload.bytes[payload.data_offset..]));
        }
        return binary_shape_to_ascii(&payload);
    }
    if is_binary_shape(bytes) {
        let payload = crate::msts_simisa::SimisaPayload {
            bytes: bytes.to_vec(),
            is_text: false,
            data_offset: 0,
            token_offset: 0,
        };
        return binary_shape_to_ascii(&payload);
    }
    Ok(decode_msts_bytes(bytes))
}

/// Returns true if the byte stream looks like a MSTS binary tokenized shape.
///
/// MSTS shapes always start with a SIMISA-style 32-byte header.  In ASCII
/// form, the bytes after the header are printable S-expression characters.
/// Binary shapes contain raw token bytes (often `0x00`–`0x1F` outside of
/// whitespace) almost immediately.  We sample a window after the header and
/// declare it binary if more than ~10% of those bytes are non-printable.
fn is_binary_shape(bytes: &[u8]) -> bool {
    let scan_start = 32.min(bytes.len());
    let scan_end = (scan_start + 256).min(bytes.len());
    let window = &bytes[scan_start..scan_end];
    if window.is_empty() {
        return false;
    }
    let suspicious = window
        .iter()
        .filter(|&&b| b != b'\n' && b != b'\r' && b != b'\t' && (b < 0x20 || b == 0x7F))
        .count();
    suspicious * 10 > window.len()
}

fn collect_texture_filenames(ast: &Ast) -> Vec<String> {
    let mut out = Vec::new();
    walk_named_list(ast, "texture_filenames", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::Atom(Atom::String(s)) = item {
                out.push(s.clone());
            }
        }
    });
    if out.is_empty() {
        walk_named_list(ast, "images", &mut |items| {
            for item in items.iter().skip(1) {
                if let Ast::List(sub) = item {
                    if matches_head(sub, "image") {
                        for inner in sub.iter().skip(1) {
                            if let Ast::Atom(Atom::String(s)) = inner {
                                out.push(s.clone());
                            }
                        }
                    }
                }
            }
        });
    }
    out
}

fn collect_shader_names(ast: &Ast) -> Vec<String> {
    let mut out = Vec::new();
    walk_named_list(ast, "shader_names", &mut |items| {
        for item in items.iter().skip(1) {
            match item {
                Ast::Atom(Atom::String(s)) => out.push(s.clone()),
                Ast::List(sub) if matches_head(sub, "named_shader") => {
                    if let Some(name) = sub.iter().skip(1).find_map(|a| match a {
                        Ast::Atom(at) => atom_to_string(at),
                        _ => None,
                    }) {
                        out.push(name);
                    }
                }
                _ => {}
            }
        }
    });
    out
}

fn collect_points(ast: &Ast) -> Vec<Vec3> {
    let mut out = Vec::new();
    walk_named_list(ast, "points", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "point") {
                    if let Some(v) = parse_vec3(sub) {
                        out.push(v);
                    }
                }
            }
        }
    });
    out
}

fn collect_uv_points(ast: &Ast) -> Vec<Vec2> {
    let mut out = Vec::new();
    walk_named_list(ast, "uv_points", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "uv_point") {
                    if let Some(v) = parse_vec2(sub) {
                        out.push(v);
                    }
                }
            }
        }
    });
    out
}

fn collect_normals(ast: &Ast) -> Vec<Vec3> {
    let mut out = Vec::new();
    walk_named_list(ast, "normals", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "vector") || matches_head(sub, "normal") {
                    if let Some(v) = parse_vec3(sub) {
                        out.push(v);
                    }
                }
            }
        }
    });
    out
}

fn collect_prim_states(ast: &Ast) -> Vec<PrimState> {
    let mut out = Vec::new();
    walk_named_list(ast, "prim_states", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "prim_state") {
                    out.push(parse_prim_state(sub));
                }
            }
        }
    });
    out
}

fn parse_prim_state(items: &[Ast]) -> PrimState {
    // Layout (lenient): ( prim_state ["name"] <flags> <shader_idx> ( tex_idxs <count> <i> ... ) ... )
    let mut name: Option<String> = None;
    let mut flags: i32 = 0;
    let mut shader_idx: i32 = -1;
    let mut texture_idx: i32 = -1;
    let mut tex_indices = Vec::new();
    let mut vertex_state_idx: i32 = -1;
    let mut z_bias: Option<f64> = None;
    let mut top_level_nums = Vec::new();

    for item in items.iter().skip(1) {
        match item {
            Ast::Atom(Atom::String(s)) if name.is_none() => name = Some(s.clone()),
            Ast::Atom(at) => {
                if let Some(n) = atom_to_number(at) {
                    top_level_nums.push(n);
                }
            }
            Ast::List(sub) if matches_head(sub, "tex_idxs") => {
                tex_indices = parse_counted_i32_list(sub);
                texture_idx = tex_indices.first().copied().unwrap_or(-1);
            }
            Ast::List(sub) if matches_head(sub, "flags") => {
                if let Some(n) = first_number_after_head(sub) {
                    flags = n as i32;
                }
            }
            Ast::List(sub) if matches_head(sub, "shader_idx") => {
                if let Some(n) = first_number_after_head(sub) {
                    shader_idx = n as i32;
                }
            }
            Ast::List(sub)
                if matches_head(sub, "ivtx_state") || matches_head(sub, "vtx_state_idx") =>
            {
                if let Some(n) = first_number_after_head(sub) {
                    vertex_state_idx = n as i32;
                }
            }
            Ast::List(sub) if matches_head(sub, "zbias") || matches_head(sub, "z_bias") => {
                z_bias = first_number_after_head(sub);
            }
            _ => {}
        }
    }

    if let Some(n) = top_level_nums.first() {
        flags = *n as i32;
    }
    if let Some(n) = top_level_nums.get(1) {
        shader_idx = *n as i32;
    }
    if let Some(n) = top_level_nums.get(2) {
        vertex_state_idx = *n as i32;
    }
    if z_bias.is_none() {
        z_bias = top_level_nums.get(3).copied();
    }

    PrimState {
        name,
        flags,
        shader_idx,
        texture_idx,
        tex_indices,
        vertex_state_idx,
        z_bias,
    }
}

fn collect_lod_controls(ast: &Ast) -> Vec<LodControl> {
    let mut out = Vec::new();
    walk_named_list(ast, "lod_controls", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "lod_control") {
                    out.push(parse_lod_control(sub));
                }
            }
        }
    });
    out
}

fn parse_lod_control(items: &[Ast]) -> LodControl {
    let mut distance_levels = Vec::new();
    for item in items.iter().skip(1) {
        if let Ast::List(sub) = item {
            if matches_head(sub, "distance_levels") {
                for child in sub.iter().skip(1) {
                    if let Ast::List(dl) = child {
                        if matches_head(dl, "distance_level") {
                            distance_levels.push(parse_distance_level(dl));
                        }
                    }
                }
            }
        }
    }
    LodControl { distance_levels }
}

fn parse_distance_level(items: &[Ast]) -> DistanceLevel {
    let mut selection_m = 0.0;
    let mut sub_objects = Vec::new();

    for item in items.iter().skip(1) {
        let Ast::List(sub) = item else { continue };
        if matches_head(sub, "distance_level_header") {
            if let Some(s) = find_first_named_number(sub, "dlevel_selection") {
                selection_m = s;
            }
        } else if matches_head(sub, "sub_objects") {
            for child in sub.iter().skip(1) {
                if let Ast::List(so) = child {
                    if matches_head(so, "sub_object") {
                        sub_objects.push(parse_sub_object(so));
                    }
                }
            }
        }
    }

    DistanceLevel {
        selection_m,
        sub_objects,
    }
}

fn parse_sub_object(items: &[Ast]) -> SubObject {
    let mut vertex_count: usize = 0;
    let mut vertices = Vec::new();
    let mut primitives = Vec::new();

    for item in items.iter().skip(1) {
        let Ast::List(sub) = item else { continue };
        if matches_head(sub, "vertices") {
            if let Some(Ast::Atom(at)) = sub.get(1) {
                if let Some(n) = atom_to_number(at) {
                    vertex_count = n as usize;
                }
            }
            for child in sub.iter().skip(2) {
                let Ast::List(vertex) = child else { continue };
                if matches_head(vertex, "vertex") {
                    if let Some(parsed) = parse_vertex(vertex) {
                        vertices.push(parsed);
                    }
                }
            }
        } else if matches_head(sub, "primitives") {
            // ( primitives <count> ( prim_state_idx ... ) ( indexed_trilist ... ) ... )
            let mut current_state_idx: i32 = -1;
            for child in sub.iter().skip(1) {
                let Ast::List(prim) = child else { continue };
                if matches_head(prim, "prim_state_idx") {
                    if let Some(Ast::Atom(at)) = prim.get(1) {
                        if let Some(n) = atom_to_number(at) {
                            current_state_idx = n as i32;
                        }
                    }
                } else if matches_head(prim, "indexed_trilist") {
                    let mut p = Primitive {
                        prim_state_idx: current_state_idx,
                        vertex_indices: Vec::new(),
                    };
                    for inner in prim.iter().skip(1) {
                        if let Ast::List(idx) = inner {
                            if matches_head(idx, "vertex_idxs") {
                                for v in idx.iter().skip(1) {
                                    if let Ast::Atom(at) = v {
                                        if let Some(n) = atom_to_number(at) {
                                            if n >= 0.0 {
                                                p.vertex_indices.push(n as u32);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // The first numeric in `vertex_idxs` is the index count (we collected it as
                    // an index too — drop it to keep the list as raw indices).
                    if !p.vertex_indices.is_empty() {
                        p.vertex_indices.remove(0);
                    }
                    primitives.push(p);
                }
            }
        }
    }

    SubObject {
        vertex_count,
        vertices,
        primitives,
    }
}

fn parse_vertex(items: &[Ast]) -> Option<Vertex> {
    let nums: Vec<i32> = items
        .iter()
        .skip(1)
        .filter_map(|a| match a {
            Ast::Atom(at) => atom_to_number(at).map(|n| n as i32),
            _ => None,
        })
        .collect();
    if nums.len() < 3 {
        return None;
    }

    let mut uv_indices = Vec::new();
    for item in items.iter().skip(1) {
        let Ast::List(sub) = item else { continue };
        if matches_head(sub, "vertex_uvs") {
            uv_indices.extend(sub.iter().skip(2).filter_map(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as i32),
                _ => None,
            }));
            break;
        }
    }

    Some(Vertex {
        // Layout: flags, point index, normal index, color1, color2, vertex_uvs.
        point_idx: nums[1],
        normal_idx: nums[2],
        uv_indices,
    })
}

fn collect_matrices(ast: &Ast) -> Vec<NamedMatrix> {
    let mut out = Vec::new();
    walk_named_list(ast, "matrices", &mut |items| {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "matrix") {
                    if let Some(m) = parse_named_matrix(sub) {
                        out.push(m);
                    }
                }
            }
        }
    });
    out
}

fn parse_named_matrix(items: &[Ast]) -> Option<NamedMatrix> {
    let name = items.iter().skip(1).find_map(|a| match a {
        Ast::Atom(at) => atom_to_string(at),
        _ => None,
    })?;
    let nums: Vec<f64> = items
        .iter()
        .skip(1)
        .filter_map(|a| match a {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        })
        .collect();
    if nums.len() < 12 {
        return None;
    }
    let mut rows = [[0.0; 3]; 4];
    for (i, chunk) in nums[..12].chunks(3).enumerate() {
        rows[i][0] = chunk[0];
        rows[i][1] = chunk[1];
        rows[i][2] = chunk[2];
    }
    Some(NamedMatrix {
        name,
        matrix: Matrix43 { rows },
    })
}

// ── small helpers ────────────────────────────────────────────────────────────

fn matches_head(items: &[Ast], expected: &str) -> bool {
    matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(expected))
}

fn parse_vec3(items: &[Ast]) -> Option<Vec3> {
    let nums: Vec<f64> = items
        .iter()
        .skip(1)
        .filter_map(|a| match a {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        })
        .collect();
    if nums.len() < 3 {
        return None;
    }
    Some(Vec3 {
        x: nums[0],
        y: nums[1],
        z: nums[2],
    })
}

fn parse_vec2(items: &[Ast]) -> Option<Vec2> {
    let nums: Vec<f64> = items
        .iter()
        .skip(1)
        .filter_map(|a| match a {
            Ast::Atom(at) => atom_to_number(at),
            _ => None,
        })
        .collect();
    if nums.len() < 2 {
        return None;
    }
    Some(Vec2 {
        u: nums[0],
        v: nums[1],
    })
}

fn first_number_after_head(items: &[Ast]) -> Option<f64> {
    items.iter().skip(1).find_map(|a| match a {
        Ast::Atom(at) => atom_to_number(at),
        _ => None,
    })
}

fn parse_counted_i32_list(items: &[Ast]) -> Vec<i32> {
    items
        .iter()
        .skip(2)
        .filter_map(|a| match a {
            Ast::Atom(at) => atom_to_number(at).map(|n| n as i32),
            _ => None,
        })
        .collect()
}

fn find_first_named_number(items: &[Ast], name: &str) -> Option<f64> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, name) {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

/// Walk every list node and run `f` on lists whose head symbol equals `name`.
fn walk_named_list<F: FnMut(&[Ast])>(ast: &Ast, name: &str, f: &mut F) {
    let Ast::List(items) = ast else { return };
    if matches_head(items, name) {
        f(items);
    }
    for child in items {
        walk_named_list(child, name, f);
    }
}
