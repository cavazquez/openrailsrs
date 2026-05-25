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

use std::path::Path;

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
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
    },
    Forest {
        uid: u32,
        tree_texture: Option<String>,
        position: Vec3,
        /// `(min, max)` random scale factors from `ScaleRange`.
        scale_range: Option<[f64; 2]>,
        /// Patch width/depth in metres from `Area` (optional).
        patch_size: Option<[f64; 2]>,
        /// Tree count from `Population` (default when absent).
        population: u32,
    },
    Track {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
    },
    Dyntrack {
        uid: u32,
        position: Vec3,
        qdir: Option<[f64; 4]>,
    },
    Signal {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
    },
    Other {
        tag: String,
        uid: Option<u32>,
        position: Option<Vec3>,
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
            WorldItem::Other { .. } => "Other",
        }
    }

    pub fn uid(&self) -> Option<u32> {
        match self {
            WorldItem::Static { uid, .. }
            | WorldItem::Forest { uid, .. }
            | WorldItem::Track { uid, .. }
            | WorldItem::Dyntrack { uid, .. }
            | WorldItem::Signal { uid, .. } => Some(*uid),
            WorldItem::Other { uid, .. } => *uid,
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        match self {
            WorldItem::Static { file_name, .. }
            | WorldItem::Track { file_name, .. }
            | WorldItem::Signal { file_name, .. } => file_name.as_deref(),
            WorldItem::Forest { tree_texture, .. } => tree_texture.as_deref(),
            _ => None,
        }
    }

    pub fn position(&self) -> Option<Vec3> {
        match self {
            WorldItem::Static { position, .. }
            | WorldItem::Forest { position, .. }
            | WorldItem::Track { position, .. }
            | WorldItem::Dyntrack { position, .. }
            | WorldItem::Signal { position, .. } => Some(*position),
            WorldItem::Other { position, .. } => *position,
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
        let text = crate::encoding::read_msts_file_to_string(path)?;
        let ast = parse_from_first_paren(&text)?;
        let (tile_x, tile_z) = parse_tile_xz_from_filename(path).unwrap_or((0, 0));
        Ok(Self::from_ast(&ast, tile_x, tile_z))
    }
}

fn parse_tile_xz_from_filename(path: &Path) -> Option<(i32, i32)> {
    // Filenames look like `w-001000-001000.w` or `w+000123-000456.w`.
    let stem = path.file_stem()?.to_string_lossy();
    let rest = stem.strip_prefix("w-").or_else(|| stem.strip_prefix('w'))?;
    let mut parts = rest.split(['-', '_']).filter(|p| !p.is_empty());
    let x = parts.next()?.parse::<i32>().ok()?;
    let z = parts.next()?.parse::<i32>().ok()?;
    Some((x, z))
}

fn collect_items(ast: &Ast) -> Vec<WorldItem> {
    let mut out = Vec::new();
    let Ast::List(root) = ast else {
        return out;
    };
    // The top-level `Tr_Worldfile` block contains the entries; some routes nest
    // the items directly at the root.
    let entries = if matches_head(root, "Tr_Worldfile") {
        &root[1..]
    } else {
        &root[..]
    };

    for entry in entries {
        if let Ast::List(items) = entry {
            if let Some(item) = parse_world_item(items) {
                out.push(item);
            }
        }
    }
    out
}

fn parse_world_item(items: &[Ast]) -> Option<WorldItem> {
    let tag = match items.first()? {
        Ast::Atom(Atom::Symbol(s)) => s.clone(),
        _ => return None,
    };

    let uid = find_uid(items);
    let position = find_position(items);
    let file_name = find_named_string(items, "FileName");
    let qdir = find_quaternion(items);

    Some(match tag.as_str() {
        s if s.eq_ignore_ascii_case("Static") => WorldItem::Static {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            qdir,
        },
        s if s.eq_ignore_ascii_case("Forest") => WorldItem::Forest {
            uid: uid.unwrap_or(0),
            tree_texture: find_named_string(items, "TreeTexture")
                .or_else(|| find_named_string(items, "FileName")),
            position: position.unwrap_or_default(),
            scale_range: find_named_pair(items, "ScaleRange"),
            patch_size: find_named_pair(items, "Area"),
            population: find_named_u32(items, "Population").unwrap_or(DEFAULT_FOREST_POPULATION),
        },
        s if s.eq_ignore_ascii_case("TrackObj") => WorldItem::Track {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            qdir,
        },
        s if s.eq_ignore_ascii_case("Dyntrack") => WorldItem::Dyntrack {
            uid: uid.unwrap_or(0),
            position: position.unwrap_or_default(),
            qdir,
        },
        s if s.eq_ignore_ascii_case("Signal") => WorldItem::Signal {
            uid: uid.unwrap_or(0),
            file_name,
            position: position.unwrap_or_default(),
            qdir,
        },
        _ => WorldItem::Other { tag, uid, position },
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

fn find_quaternion(items: &[Ast]) -> Option<[f64; 4]> {
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, "QDirection") || matches_head(sub, "Matrix3x3") {
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
