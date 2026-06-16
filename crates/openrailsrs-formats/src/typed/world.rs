//! Parser for MSTS World tile (`.w`) ASCII files.
//!
//! World tiles place objects (static meshes, forests, track segments, signals,
//! …) in the local coordinate space of a 2 km × 2 km tile.  Each entry has a
//! tag that identifies its kind, a `UiD` (unique within the tile), an optional
//! reference to a `.s` shape via `FileName`, and a position/orientation.
//!
//! The parser is deliberately lenient: unknown tags are surfaced as
//! [`WorldItem::Other`] so callers can still see what the route ships.  Global
//! coordinate resolution is not done here — that requires the world tile
//! origin and is left for Fase 23.

use std::borrow::Cow;
use std::path::Path;

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::msts_file_text::read_msts_file_decoded;
use crate::parser::parse_from_first_paren;

use super::atom_to_number;
use super::atom_to_string;
use super::shape::Vec3;

/// Default tree count when a `.w` `Forest` omits `Population`.
pub const DEFAULT_FOREST_POPULATION: u32 = 48;

/// Kind-aware view of a world item.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldItem {
    Static {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
    },
    Forest {
        uid: u32,
        tree_texture: Option<String>,
        position: Vec3,
        /// `(min, max)` random scale factors from `ScaleRange`.
        scale_range: Option<[f64; 2]>,
        /// Patch width/depth in metres from `Area` (optional).
        patch_size: Option<[f64; 2]>,
        /// Billboard width/height in metres from `TreeSize` (optional).
        tree_size: Option<[f64; 2]>,
        /// Tree count from `Population` (default when absent).
        population: u32,
    },
    Track {
        uid: u32,
        section_idx: Option<u32>,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
    },
    Dyntrack {
        uid: u32,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
    },
    Signal {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
    },
    /// Horizontal water surface (`HWater` in `.w`).
    HWater {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        /// Width and depth in metres from `Size`.
        size: [f64; 2],
    },
    /// Textured ground decal (`Transfer` in `.w`).
    Transfer {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        width: f64,
        height: f64,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
    },
    Other {
        tag: String,
        uid: Option<u32>,
        position: Option<Vec3>,
        file_name: Option<String>,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
    },
}

impl WorldItem {
    pub fn kind(&self) -> &'static str {
        match self {
            WorldItem::Static { .. } => "Static",
            WorldItem::Forest { .. } => "Forest",
            WorldItem::Track { .. } => "TrackObj",
            WorldItem::Dyntrack { .. } => "Dyntrack",
            WorldItem::Signal { .. } => "Signal",
            WorldItem::HWater { .. } => "HWater",
            WorldItem::Transfer { .. } => "Transfer",
            WorldItem::Other { .. } => "Other",
        }
    }

    pub fn uid(&self) -> Option<u32> {
        match self {
            WorldItem::Static { uid, .. }
            | WorldItem::Forest { uid, .. }
            | WorldItem::Track { uid, .. }
            | WorldItem::Dyntrack { uid, .. }
            | WorldItem::Signal { uid, .. }
            | WorldItem::HWater { uid, .. }
            | WorldItem::Transfer { uid, .. } => Some(*uid),
            WorldItem::Other { uid, .. } => *uid,
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        match self {
            WorldItem::Static { file_name, .. }
            | WorldItem::Track { file_name, .. }
            | WorldItem::Signal { file_name, .. }
            | WorldItem::HWater { file_name, .. }
            | WorldItem::Transfer { file_name, .. } => file_name.as_deref(),
            WorldItem::Forest { tree_texture, .. } => tree_texture.as_deref(),
            WorldItem::Other { file_name, .. } => file_name.as_deref(),
            _ => None,
        }
    }

    pub fn position(&self) -> Option<Vec3> {
        match self {
            WorldItem::Static { position, .. }
            | WorldItem::Forest { position, .. }
            | WorldItem::Track { position, .. }
            | WorldItem::Dyntrack { position, .. }
            | WorldItem::Signal { position, .. }
            | WorldItem::HWater { position, .. }
            | WorldItem::Transfer { position, .. } => Some(*position),
            WorldItem::Other { position, .. } => *position,
        }
    }

    pub fn qdirection(&self) -> Option<[f64; 4]> {
        match self {
            WorldItem::Static { qdir, .. }
            | WorldItem::Track { qdir, .. }
            | WorldItem::Dyntrack { qdir, .. }
            | WorldItem::Signal { qdir, .. }
            | WorldItem::Other { qdir, .. } => *qdir,
            _ => None,
        }
    }

    pub fn matrix3x3(&self) -> Option<[f64; 9]> {
        match self {
            WorldItem::Static { matrix3x3, .. }
            | WorldItem::Track { matrix3x3, .. }
            | WorldItem::Dyntrack { matrix3x3, .. }
            | WorldItem::Signal { matrix3x3, .. }
            | WorldItem::Other { matrix3x3, .. } => *matrix3x3,
            _ => None,
        }
    }

    /// `TrackObj` → `TrackShape` index in `tsection.dat`.
    pub fn section_idx(&self) -> Option<u32> {
        match self {
            WorldItem::Track { section_idx, .. } => *section_idx,
            _ => None,
        }
    }
}

/// Parsed `.w` world tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldFile {
    pub tile_x: i32,
    pub tile_z: i32,
    pub items: Vec<WorldItem>,
}

impl WorldFile {
    pub fn from_ast(ast: &Ast, tile_x: i32, tile_z: i32) -> Self {
        let items = collect_items(ast);
        Self {
            tile_x,
            tile_z,
            items,
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let text = read_msts_file_decoded(path)?;
        let ast = load_world_ast(&text)?;
        let (tile_x, tile_z) = parse_tile_xz_from_filename(path).unwrap_or((0, 0));
        Ok(Self::from_ast(&ast, tile_x, tile_z))
    }
}

/// JINX-decompiled `.w` tiles parse correctly from raw text; classic routes that
/// use `Name ( … )` blocks need [`normalize_world_text`]. Prefer whichever yields
/// more scenery entries when both parse.
fn load_world_ast(text: &str) -> Result<Ast, FormatError> {
    let raw = parse_from_first_paren(text).ok();
    let normalized = normalize_world_text(text);
    let norm = parse_from_first_paren(&normalized).ok();
    match (raw, norm) {
        (Some(a), Some(b)) => Ok(select_better_world_ast(&a, &b)),
        (Some(a), None) => Ok(a),
        (None, Some(b)) => Ok(b),
        (None, None) => parse_from_first_paren(text),
    }
}

fn select_better_world_ast(a: &Ast, b: &Ast) -> Ast {
    let count_a = collect_items(a).len();
    let count_b = collect_items(b).len();
    if count_a >= count_b {
        a.clone()
    } else {
        b.clone()
    }
}

/// MSTS world text often uses `Name ( ... )` blocks instead of canonical
/// S-expressions `( Name ... )`.  The generic parser expects the latter, so
/// convert only symbol-prefix block openers while leaving existing canonical
/// blocks, strings and scalar values untouched.
fn normalize_world_text(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 8);
    let bytes = source.as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    let mut prev_non_ws: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            in_string = !in_string;
            out.push(b as char);
            prev_non_ws = Some(b);
            i += 1;
            continue;
        }

        if !in_string && is_symbol_start(b) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_symbol_continue(bytes[i]) {
                i += 1;
            }
            let end = i;
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }

            if j < bytes.len() && bytes[j] == b'(' && prev_non_ws != Some(b'(') {
                out.push_str("( ");
                out.push_str(&source[start..end]);
                out.push(' ');
                prev_non_ws = Some(b'(');
                i = j + 1;
                continue;
            }

            out.push_str(&source[start..end]);
            prev_non_ws = Some(bytes[end - 1]);
            continue;
        }

        out.push(b as char);
        if !b.is_ascii_whitespace() {
            prev_non_ws = Some(b);
        }
        i += 1;
    }

    out
}

fn is_symbol_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_symbol_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn parse_tile_xz_from_filename(path: &Path) -> Option<(i32, i32)> {
    // Signed, exactly as Open Rails: `w-006074+014924.w` → (-6074, 14924).
    crate::msts_tile_name::parse_world_w_tile_xz(path)
}

fn collect_items(ast: &Ast) -> Vec<WorldItem> {
    let Ast::List(root) = ast else {
        return Vec::new();
    };
    flatten_world_entries(root)
        .into_iter()
        .filter_map(|items| parse_world_item(&items))
        .collect()
}

/// JINX-decompiled `.w` tiles often wrap every object in one `Transfer` block and
/// flatten each entry to `( UiD ( n ) Width ( w ) … )` instead of typed wrappers.
fn flatten_world_entries(root: &[Ast]) -> Vec<Vec<Ast>> {
    if root.len() <= 1 {
        return Vec::new();
    }
    if matches_head(root, "Tr_Worldfile") {
        return root[1..]
            .iter()
            .flat_map(|entry| match entry {
                Ast::List(inner) => flatten_world_entries(inner),
                _ => Vec::new(),
            })
            .collect();
    }
    if matches_head(root, "Transfer") {
        return root[1..]
            .iter()
            .filter_map(|entry| match entry {
                Ast::List(items) if matches_head(items, "UiD") => Some(items.clone()),
                _ => None,
            })
            .collect();
    }
    if is_object_entry(root) {
        return vec![root.to_vec()];
    }
    root[1..]
        .iter()
        .filter_map(|entry| match entry {
            Ast::List(items) if is_object_entry(items) => Some(items.clone()),
            _ => None,
        })
        .collect()
}

fn is_object_entry(items: &[Ast]) -> bool {
    matches!(
        items.first(),
        Some(Ast::Atom(Atom::Symbol(head)))
            if matches!(
                head.as_str(),
                "Static" | "TrackObj" | "Forest" | "Transfer" | "Dyntrack" | "Signal" | "HWater"
                    | "UiD"
            )
    )
}

/// JINX flat `( UiD ( 75 ) Width ( 30 ) … )` → nested `( UiD ( 75 ) ) ( Width ( 30 ) ) …`.
fn normalize_jinx_flat_fields(items: &[Ast]) -> Cow<'_, [Ast]> {
    if !is_jinx_flat_alternating(items) {
        return Cow::Borrowed(items);
    }
    let mut out = Vec::with_capacity(items.len() / 2);
    let mut i = 0usize;
    while i + 1 < items.len() {
        let (Ast::Atom(Atom::Symbol(key)), Ast::List(val)) = (&items[i], &items[i + 1]) else {
            break;
        };
        let mut sub = vec![Ast::Atom(Atom::Symbol(key.clone()))];
        if val.len() == 1 {
            sub.push(val[0].clone());
        } else {
            sub.extend(val.iter().cloned());
        }
        out.push(Ast::List(sub));
        i += 2;
    }
    if out.is_empty() {
        Cow::Borrowed(items)
    } else {
        Cow::Owned(out)
    }
}

fn is_jinx_flat_alternating(items: &[Ast]) -> bool {
    if items.len() < 4 {
        return false;
    }
    if !matches!(items.first(), Some(Ast::Atom(Atom::Symbol(_)))) {
        return false;
    }
    if !matches!(items.get(1), Some(Ast::List(_))) {
        return false;
    }
    matches!(items.get(2), Some(Ast::Atom(Atom::Symbol(_))))
}

fn infer_object_tag(fields: &[Ast]) -> Option<String> {
    if find_named_f64(fields, "Width").is_some() && find_named_f64(fields, "Height").is_some() {
        return Some("Transfer".into());
    }
    if find_named_u32(fields, "SectionIdx").is_some() {
        return Some("TrackObj".into());
    }
    if find_named_u32(fields, "Population").is_some()
        || find_named_string(fields, "TreeTexture").is_some()
    {
        return Some("Forest".into());
    }
    if find_named_pair(fields, "Size").is_some() {
        return Some("HWater".into());
    }
    if find_named_string(fields, "FileName").is_some() {
        return Some("Static".into());
    }
    None
}

fn parse_world_item(items: &[Ast]) -> Option<WorldItem> {
    let tag = match items.first()? {
        Ast::Atom(Atom::Symbol(s)) => s.clone(),
        _ => return None,
    };

    let normalized = normalize_jinx_flat_fields(items);
    let fields = normalized.as_ref();

    let effective_tag = if tag.eq_ignore_ascii_case("UiD") {
        infer_object_tag(fields)?
    } else {
        tag
    };

    let uid = find_uid(fields);
    let position = find_position(fields);
    let file_name = find_named_string(fields, "FileName");
    let qdir = find_qdirection(fields);
    let matrix3x3 = find_matrix3x3(fields);

    Some(match effective_tag.as_str() {
        s if s.eq_ignore_ascii_case("Static") => WorldItem::Static {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            qdir,
            matrix3x3,
        },
        s if s.eq_ignore_ascii_case("Forest") => WorldItem::Forest {
            uid: uid.unwrap_or(0),
            tree_texture: find_named_string(fields, "TreeTexture")
                .or_else(|| find_named_string(fields, "FileName")),
            position: position.unwrap_or_default(),
            scale_range: find_named_pair(fields, "ScaleRange"),
            patch_size: find_named_pair(fields, "Area"),
            tree_size: find_named_pair(fields, "TreeSize"),
            population: find_named_u32(fields, "Population").unwrap_or(DEFAULT_FOREST_POPULATION),
        },
        s if s.eq_ignore_ascii_case("TrackObj") => WorldItem::Track {
            uid: uid.unwrap_or(0),
            section_idx: find_named_u32(fields, "SectionIdx"),
            file_name,
            position: position.unwrap_or_default(),
            qdir,
            matrix3x3,
        },
        s if s.eq_ignore_ascii_case("Dyntrack") => WorldItem::Dyntrack {
            uid: uid.unwrap_or(0),
            position: position.unwrap_or_default(),
            qdir,
            matrix3x3,
        },
        s if s.eq_ignore_ascii_case("Signal") => WorldItem::Signal {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            qdir,
            matrix3x3,
        },
        s if s.eq_ignore_ascii_case("HWater") => WorldItem::HWater {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            size: find_named_pair(fields, "Size").unwrap_or([100.0, 100.0]),
        },
        s if s.eq_ignore_ascii_case("Transfer") => WorldItem::Transfer {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            width: find_named_f64(fields, "Width").unwrap_or(10.0),
            height: find_named_f64(fields, "Height").unwrap_or(10.0),
            qdir,
            matrix3x3,
        },
        _ => WorldItem::Other {
            tag: effective_tag,
            uid,
            position,
            file_name,
            qdir,
            matrix3x3,
        },
    })
}

fn find_uid(items: &[Ast]) -> Option<u32> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, "UiD") {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        return Some(n as u32);
                    }
                }
            }
        }
    }
    None
}

fn find_position(items: &[Ast]) -> Option<Vec3> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, "Position") {
                let nums: Vec<f64> = sub
                    .iter()
                    .skip(1)
                    .filter_map(|a| match a {
                        Ast::Atom(at) => atom_to_number(at),
                        _ => None,
                    })
                    .collect();
                if nums.len() >= 3 {
                    return Some(Vec3 {
                        x: nums[0],
                        y: nums[1],
                        z: nums[2],
                    });
                }
            }
        }
    }
    None
}

fn find_qdirection(items: &[Ast]) -> Option<[f64; 4]> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, "QDirection") {
                let nums: Vec<f64> = sub
                    .iter()
                    .skip(1)
                    .filter_map(|a| match a {
                        Ast::Atom(at) => atom_to_number(at),
                        _ => None,
                    })
                    .collect();
                if nums.len() >= 4 {
                    return Some([nums[0], nums[1], nums[2], nums[3]]);
                }
            }
        }
    }
    None
}

fn find_matrix3x3(items: &[Ast]) -> Option<[f64; 9]> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, "Matrix3x3") {
                let nums: Vec<f64> = sub
                    .iter()
                    .skip(1)
                    .filter_map(|a| match a {
                        Ast::Atom(at) => atom_to_number(at),
                        _ => None,
                    })
                    .collect();
                if nums.len() >= 9 {
                    return Some([
                        nums[0], nums[1], nums[2], nums[3], nums[4], nums[5], nums[6], nums[7],
                        nums[8],
                    ]);
                }
            }
        }
    }
    None
}

fn find_named_string(items: &[Ast], key: &str) -> Option<String> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
                for child in sub.iter().skip(1) {
                    if let Ast::Atom(at) = child {
                        if let Some(s) = atom_to_string(at) {
                            return Some(s);
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_named_pair(items: &[Ast], key: &str) -> Option<[f64; 2]> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
                let nums: Vec<f64> = sub
                    .iter()
                    .skip(1)
                    .filter_map(|a| match a {
                        Ast::Atom(at) => atom_to_number(at),
                        _ => None,
                    })
                    .collect();
                if nums.len() >= 2 {
                    return Some([nums[0], nums[1]]);
                }
            }
        }
    }
    None
}

fn find_named_f64(items: &[Ast], key: &str) -> Option<f64> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
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

fn find_named_u32(items: &[Ast], key: &str) -> Option<u32> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        return Some(n.max(0.0) as u32);
                    }
                }
            }
        }
    }
    None
}

fn matches_head(items: &[Ast], expected: &str) -> bool {
    matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod watersnake_jinx_tests {
    use super::*;
    use crate::ast::Ast;
    use crate::msts_file_text::read_msts_file_decoded;
    use crate::parser::parse_from_first_paren;
    use std::path::PathBuf;

    #[test]
    fn watersnake_jinx_transfer_and_uid_parsing() {
        let path = PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("routes/NewForestRouteV3/Routes/Watersnake/world/w-006144+014900.w");
        if !path.is_file() {
            return;
        }
        let text = read_msts_file_decoded(&path).expect("decode");
        let ast = parse_from_first_paren(&text).expect("parse");
        let Ast::List(root) = &ast else {
            return;
        };
        let entries = flatten_world_entries(root);
        eprintln!("flattened {} entries", entries.len());
        for (i, items) in entries.iter().enumerate().take(6) {
            let fields = normalize_jinx_flat_fields(items);
            let uid = find_uid(fields.as_ref());
            eprintln!(
                "{i}: infer={:?} uid={uid:?} w={:?} h={:?} file={:?}",
                infer_object_tag(fields.as_ref()),
                find_named_f64(fields.as_ref(), "Width"),
                find_named_f64(fields.as_ref(), "Height"),
                find_named_string(fields.as_ref(), "FileName"),
            );
        }
        let world = WorldFile::from_path(&path).expect("world");
        let transfers: Vec<_> = world
            .items
            .iter()
            .filter(|i| i.kind() == "Transfer")
            .collect();
        assert_eq!(
            transfers.len(),
            3,
            "expected 3 transfers in tunnel tile, got {}",
            transfers.len()
        );
        assert!(
            transfers.iter().any(|t| t.uid() == Some(75)),
            "missing transfer uid 75"
        );
    }
}
