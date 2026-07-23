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
use crate::parser::parse_from_first_paren;

use super::atom_to_number;
use super::atom_to_string;
use super::shape::Vec3;

/// Default tree count when a `.w` `Forest` omits `Population`.
pub const DEFAULT_FOREST_POPULATION: u32 = 48;

/// One authored segment inside WORLD `Dyntrack` / `TrackSections` (Open Rails `DyntrackObj.TrackSection`).
///
/// - `is_curved == 0`: `param1` = length (m), `param2` unused
/// - `is_curved != 0`: `param1` = arc (radians), `param2` = radius (m)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DyntrackSection {
    pub is_curved: u32,
    pub uid: u32,
    pub param1: f32,
    pub param2: f32,
}

impl DyntrackSection {
    pub fn is_curve(&self) -> bool {
        self.is_curved != 0 && self.param2.abs() > 1e-6 && self.param1.abs() > 1e-9
    }

    /// Straight length (m), or arc length `radius * |arc_rad|` for curves.
    pub fn travel_length_m(&self) -> f32 {
        if self.is_curve() {
            self.param2.abs() * self.param1.abs()
        } else {
            self.param1.abs()
        }
    }

    /// Curve angle in degrees when curved (Open Rails stores arc in radians).
    pub fn curve_angle_deg(&self) -> Option<f32> {
        self.is_curve().then_some(self.param1.to_degrees())
    }

    pub fn curve_radius_m(&self) -> Option<f32> {
        self.is_curve().then_some(self.param2)
    }
}

/// `(db, item_id)` from WORLD `TrItemId` (db 0 = TDB, db 1 = RDB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldTrItemRef {
    pub db: u32,
    pub item_id: u32,
}

/// Kind-aware view of a world item.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldItem {
    Static {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// From preceding `Tr_Watermark` (HideWire uses levels 2/3).
        static_detail_level: u32,
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
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    Track {
        uid: u32,
        section_idx: Option<u32>,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// From preceding `Tr_Watermark` (HideWire uses levels 2/3).
        static_detail_level: u32,
    },
    Dyntrack {
        uid: u32,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// From preceding `Tr_Watermark` (HideWire uses levels 2/3).
        static_detail_level: u32,
        /// WORLD `SectionIdx` (path / elevation index in Open Rails).
        section_idx: Option<u32>,
        /// Up to five authored `TrackSection` entries (`SectionCurve` + params).
        track_sections: Vec<DyntrackSection>,
    },
    Signal {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// Bitmask of installed `SignalSubObj` entries from `sigcfg.dat` (OR `SignalSubObj`).
        signal_sub_obj: u32,
        /// Head units: `(SubObj index, TDB TrItemId)` from `SignalUnit` / nested `TrItemId`.
        signal_units: Vec<SignalUnitRef>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Wayside speed post (`Speedpost` in `.w`).
    Speedpost {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// All `TrItemId` pairs in file order.
        tr_item_refs: Vec<WorldTrItemRef>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Ambient sound region anchored to track (`SoundRegion` in `.w`).
    SoundRegion {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        tdb_id: u32,
        tr_item_id: u32,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Horizontal water surface (`HWater` in `.w`).
    HWater {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        /// Width and depth in metres from `Size`.
        size: [f64; 2],
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
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
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Road traffic spawner (`CarSpawner` in `.w`); poses come from RDB `TrItemId (1 …)`.
    CarSpawner {
        uid: u32,
        car_frequency: f64,
        car_av_speed: f64,
        /// `ORTSListName` when present (OpenRails multi-list `carspawn.dat`).
        list_name: Option<String>,
        /// RDB item ids (`TrItemId` with database index 1), typically start then end.
        rdb_tr_item_ids: Vec<u32>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Fuel / water / container pickup (`Pickup` in `.w`); `FileName` is a route `.s`.
    Pickup {
        uid: u32,
        file_name: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// First value of `PickupType` (5=water, 6/2=coal, 7=diesel, …).
        pickup_type: Option<u32>,
        /// TDB `TrItemId` item ids (database index 0).
        tr_item_ids: Vec<u32>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Animal / worker hazard (`Hazard` in `.w`); `FileName` is a `.haz` config.
    Hazard {
        uid: u32,
        /// WORLD `FileName` — typically `crow.haz` (not a mesh).
        haz_file: Option<String>,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// TDB item id from `TrItemId (0 id)` only (db must be 0).
        tr_item_id: Option<u32>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Station platform marker (`Platform` in `.w`).
    Platform {
        uid: u32,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        file_name: Option<String>,
        /// From `PlatformData` (hex or decimal).
        platform_data: Option<u32>,
        /// All `TrItemId` pairs in file order.
        tr_item_refs: Vec<WorldTrItemRef>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    /// Siding marker (`Siding` in `.w`).
    Siding {
        uid: u32,
        position: Vec3,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        file_name: Option<String>,
        /// All `TrItemId` pairs in file order.
        tr_item_refs: Vec<WorldTrItemRef>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
    Other {
        tag: String,
        uid: Option<u32>,
        position: Option<Vec3>,
        file_name: Option<String>,
        qdir: Option<[f64; 4]>,
        matrix3x3: Option<[f64; 9]>,
        /// From preceding `Tr_Watermark`.
        static_detail_level: u32,
    },
}

/// One WORLD `SignalUnit ( SubObj ( TrItemId db itemId ) )` entry (#37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalUnitRef {
    /// Index into `sigcfg` `SignalShape.SignalSubObjs`.
    pub sub_obj: u32,
    /// TDB `TrItem` id for this head.
    pub tr_item_id: u32,
}

impl WorldItem {
    pub fn kind(&self) -> &'static str {
        match self {
            WorldItem::Static { .. } => "Static",
            WorldItem::Forest { .. } => "Forest",
            WorldItem::Track { .. } => "TrackObj",
            WorldItem::Dyntrack { .. } => "Dyntrack",
            WorldItem::Signal { .. } => "Signal",
            WorldItem::Speedpost { .. } => "Speedpost",
            WorldItem::SoundRegion { .. } => "SoundRegion",
            WorldItem::HWater { .. } => "HWater",
            WorldItem::Transfer { .. } => "Transfer",
            WorldItem::CarSpawner { .. } => "CarSpawner",
            WorldItem::Pickup { .. } => "Pickup",
            WorldItem::Hazard { .. } => "Hazard",
            WorldItem::Platform { .. } => "Platform",
            WorldItem::Siding { .. } => "Siding",
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
            | WorldItem::Speedpost { uid, .. }
            | WorldItem::SoundRegion { uid, .. }
            | WorldItem::HWater { uid, .. }
            | WorldItem::Transfer { uid, .. }
            | WorldItem::CarSpawner { uid, .. }
            | WorldItem::Pickup { uid, .. }
            | WorldItem::Hazard { uid, .. }
            | WorldItem::Platform { uid, .. }
            | WorldItem::Siding { uid, .. } => Some(*uid),
            WorldItem::Other { uid, .. } => *uid,
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        match self {
            WorldItem::Static { file_name, .. }
            | WorldItem::Track { file_name, .. }
            | WorldItem::Signal { file_name, .. }
            | WorldItem::Speedpost { file_name, .. }
            | WorldItem::SoundRegion { file_name, .. }
            | WorldItem::HWater { file_name, .. }
            | WorldItem::Transfer { file_name, .. }
            | WorldItem::Pickup { file_name, .. }
            | WorldItem::Platform { file_name, .. }
            | WorldItem::Siding { file_name, .. } => file_name.as_deref(),
            WorldItem::Hazard { haz_file, .. } => haz_file.as_deref(),
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
            | WorldItem::Speedpost { position, .. }
            | WorldItem::SoundRegion { position, .. }
            | WorldItem::HWater { position, .. }
            | WorldItem::Transfer { position, .. }
            | WorldItem::CarSpawner { position, .. }
            | WorldItem::Pickup { position, .. }
            | WorldItem::Hazard { position, .. }
            | WorldItem::Platform { position, .. }
            | WorldItem::Siding { position, .. } => Some(*position),
            WorldItem::Other { position, .. } => *position,
        }
    }

    pub fn qdirection(&self) -> Option<[f64; 4]> {
        match self {
            WorldItem::Static { qdir, .. }
            | WorldItem::Track { qdir, .. }
            | WorldItem::Dyntrack { qdir, .. }
            | WorldItem::Signal { qdir, .. }
            | WorldItem::Speedpost { qdir, .. }
            | WorldItem::SoundRegion { qdir, .. }
            | WorldItem::Transfer { qdir, .. }
            | WorldItem::CarSpawner { qdir, .. }
            | WorldItem::Pickup { qdir, .. }
            | WorldItem::Hazard { qdir, .. }
            | WorldItem::Platform { qdir, .. }
            | WorldItem::Siding { qdir, .. }
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
            | WorldItem::Speedpost { matrix3x3, .. }
            | WorldItem::SoundRegion { matrix3x3, .. }
            | WorldItem::Transfer { matrix3x3, .. }
            | WorldItem::Pickup { matrix3x3, .. }
            | WorldItem::Hazard { matrix3x3, .. }
            | WorldItem::Platform { matrix3x3, .. }
            | WorldItem::Siding { matrix3x3, .. }
            | WorldItem::Other { matrix3x3, .. } => *matrix3x3,
            _ => None,
        }
    }

    /// TDB `TrItemId`s referenced by this world object (db == 0), in file order.
    pub fn tr_item_ids(&self) -> Vec<u32> {
        match self {
            WorldItem::Signal { signal_units, .. } => {
                let mut ids: Vec<u32> = signal_units.iter().map(|u| u.tr_item_id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids
            }
            WorldItem::Pickup { tr_item_ids, .. } => tr_item_ids.clone(),
            WorldItem::Speedpost { tr_item_refs, .. }
            | WorldItem::Platform { tr_item_refs, .. }
            | WorldItem::Siding { tr_item_refs, .. } => tr_item_refs
                .iter()
                .filter(|r| r.db == 0)
                .map(|r| r.item_id)
                .collect(),
            WorldItem::SoundRegion { tr_item_id, .. } => {
                vec![*tr_item_id]
            }
            WorldItem::Hazard {
                tr_item_id: Some(id),
                ..
            } => vec![*id],
            _ => Vec::new(),
        }
    }

    /// Signal head units (`SignalUnit` / `SignalSubObj` bitmask) when this is a Signal.
    pub fn signal_units(&self) -> &[SignalUnitRef] {
        match self {
            WorldItem::Signal { signal_units, .. } => signal_units.as_slice(),
            _ => &[],
        }
    }

    /// WORLD `SignalSubObj` bitmask (installed optional heads/decor).
    pub fn signal_sub_obj_mask(&self) -> Option<u32> {
        match self {
            WorldItem::Signal { signal_sub_obj, .. } => Some(*signal_sub_obj),
            _ => None,
        }
    }

    /// `TrackObj` → `TrackShape` index in `tsection.dat`, or Dyntrack `SectionIdx`.
    pub fn section_idx(&self) -> Option<u32> {
        match self {
            WorldItem::Track { section_idx, .. } | WorldItem::Dyntrack { section_idx, .. } => {
                *section_idx
            }
            _ => None,
        }
    }

    /// Authored Dyntrack subsections (`TrackSections`), empty for other kinds.
    pub fn dyntrack_sections(&self) -> &[DyntrackSection] {
        match self {
            WorldItem::Dyntrack { track_sections, .. } => track_sections.as_slice(),
            _ => &[],
        }
    }

    /// Detail band from `Tr_Watermark` (0 when absent). HideWire uses 2/3.
    pub fn static_detail_level(&self) -> u32 {
        match self {
            WorldItem::Static {
                static_detail_level,
                ..
            }
            | WorldItem::Forest {
                static_detail_level,
                ..
            }
            | WorldItem::Track {
                static_detail_level,
                ..
            }
            | WorldItem::Dyntrack {
                static_detail_level,
                ..
            }
            | WorldItem::Signal {
                static_detail_level,
                ..
            }
            | WorldItem::Speedpost {
                static_detail_level,
                ..
            }
            | WorldItem::SoundRegion {
                static_detail_level,
                ..
            }
            | WorldItem::HWater {
                static_detail_level,
                ..
            }
            | WorldItem::Transfer {
                static_detail_level,
                ..
            }
            | WorldItem::CarSpawner {
                static_detail_level,
                ..
            }
            | WorldItem::Pickup {
                static_detail_level,
                ..
            }
            | WorldItem::Hazard {
                static_detail_level,
                ..
            }
            | WorldItem::Platform {
                static_detail_level,
                ..
            }
            | WorldItem::Siding {
                static_detail_level,
                ..
            }
            | WorldItem::Other {
                static_detail_level,
                ..
            } => *static_detail_level,
        }
    }
}

/// Parsed `.w` world tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldFile {
    pub tile_x: i32,
    pub tile_z: i32,
    pub items: Vec<WorldItem>,
    /// Objects skipped because they lacked `Position` and/or orientation (#140).
    pub skipped_invalid_pose: usize,
}

impl WorldFile {
    pub fn from_ast(ast: &Ast, tile_x: i32, tile_z: i32) -> Self {
        let (items, skipped_invalid_pose) = collect_items(ast);
        Self {
            tile_x,
            tile_z,
            items,
            skipped_invalid_pose,
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedToken {
            offset: 0,
            message: format!("failed to read {}: {e}", path.display()),
        })?;
        Self::from_bytes(&bytes, Some(path))
    }

    /// Parse world tile bytes; optional `path_hint` supplies tile XZ from the filename.
    pub fn from_bytes(bytes: &[u8], path_hint: Option<&Path>) -> Result<Self, FormatError> {
        let text = crate::msts_file_text::decode_msts_file_bytes(bytes)?;
        let ast = load_world_ast(&text)?;
        let (tile_x, tile_z) = path_hint
            .and_then(parse_tile_xz_from_filename)
            .unwrap_or((0, 0));
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

fn select_better_world_ast(raw: &Ast, normalized: &Ast) -> Ast {
    let (items_raw, _) = collect_items(raw);
    let (items_norm, _) = collect_items(normalized);
    let count_raw = items_raw.len();
    let count_norm = items_norm.len();
    // Prefer the richer parse.
    if count_raw > count_norm {
        return raw.clone();
    }
    if count_norm > count_raw {
        return normalized.clone();
    }
    // Tie: prefer the parse that keeps more non-zero UiDs. Name-normalization can
    // turn JINX flat `UiD ( n ) Width ( w )` bags into forms where find_uid fails
    // (Watersnake), while classic `Name ( … )` routes usually differ in count.
    let uids_raw = items_raw
        .iter()
        .filter(|i| i.uid().unwrap_or(0) != 0)
        .count();
    let uids_norm = items_norm
        .iter()
        .filter(|i| i.uid().unwrap_or(0) != 0)
        .count();
    if uids_raw >= uids_norm {
        raw.clone()
    } else {
        normalized.clone()
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

fn collect_items(ast: &Ast) -> (Vec<WorldItem>, usize) {
    let Ast::List(root) = ast else {
        return (Vec::new(), 0);
    };
    let entries = flatten_world_entries(root);
    // One level per flattened object, same order as `flatten_world_entries`.
    let levels = collect_object_watermark_levels(root);
    let mut items = Vec::with_capacity(entries.len());
    let mut skipped_invalid_pose = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let level = levels.get(i).copied().unwrap_or(0);
        match parse_world_item(entry) {
            ParseWorldItem::Item(mut item) => {
                set_static_detail_level(&mut item, level);
                items.push(item);
            }
            ParseWorldItem::SkippedInvalidPose => skipped_invalid_pose += 1,
            ParseWorldItem::Ignore => {}
        }
    }
    (items, skipped_invalid_pose)
}

enum ParseWorldItem {
    Item(WorldItem),
    SkippedInvalidPose,
    Ignore,
}

/// Shape-bearing WORLD kinds that Open Rails refuses without Matrix3x3/QDirection.
fn world_tag_requires_orientation(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "static"
            | "trackobj"
            | "dyntrack"
            | "signal"
            | "speedpost"
            | "soundregion"
            | "transfer"
            | "carspawner"
            | "pickup"
            | "hazard"
            | "platform"
            | "siding"
    )
}

/// Kinds that must have an authored `Position` (Forest/HWater included; Other optional).
fn world_tag_requires_position(tag: &str) -> bool {
    !tag.eq_ignore_ascii_case("other")
        && (world_tag_requires_orientation(tag)
            || tag.eq_ignore_ascii_case("forest")
            || tag.eq_ignore_ascii_case("hwater"))
}

/// Ordered `static_detail_level` for each object emitted by [`flatten_world_entries`].
fn collect_object_watermark_levels(root: &[Ast]) -> Vec<u32> {
    let body = if matches_head(root, "Tr_Worldfile") {
        &root[1..]
    } else {
        root
    };
    let mut watermark = 0u32;
    let mut levels = Vec::new();
    walk_watermark_entries(body, &mut watermark, &mut levels);
    levels
}

/// Walk worldfile entries in flatten order, pushing the current watermark per object.
fn walk_watermark_entries(entries: &[Ast], watermark: &mut u32, levels: &mut Vec<u32>) {
    let mut i = 0usize;
    while i < entries.len() {
        match &entries[i] {
            Ast::List(items) if matches_head(items, "Tr_Watermark") => {
                if let Some(level) = parse_watermark_level(items) {
                    *watermark = level;
                }
                i += 1;
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("Tr_Watermark") => {
                if let Some(Ast::List(vals)) = entries.get(i + 1) {
                    if let Some(level) = parse_watermark_level_from_values(vals) {
                        *watermark = level;
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            // JINX Transfer wrapper: emit levels for unwrapped children (not the wrapper).
            Ast::List(items) if matches_head(items, "Transfer") && !transfer_looks_typed(items) => {
                walk_jinx_transfer_watermark_children(&items[1..], watermark, levels);
                i += 1;
            }
            Ast::List(items) if is_object_entry(items) => {
                levels.push(*watermark);
                i += 1;
            }
            Ast::Atom(Atom::Symbol(tag)) if is_object_tag(tag) => {
                levels.push(*watermark);
                // Flat `TrackObj ( … )` — skip the following field list.
                if matches!(entries.get(i + 1), Some(Ast::List(_))) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Ast::List(items) => {
                // Non-object list: flatten only emits direct object-entry children.
                walk_watermark_entries(&items[1..], watermark, levels);
                i += 1;
            }
            _ => i += 1,
        }
    }
}

fn walk_jinx_transfer_watermark_children(
    children: &[Ast],
    watermark: &mut u32,
    levels: &mut Vec<u32>,
) {
    for entry in children {
        match entry {
            Ast::List(items) if matches_head(items, "Tr_Watermark") => {
                if let Some(level) = parse_watermark_level(items) {
                    *watermark = level;
                }
            }
            Ast::List(items) if is_object_entry(items) => {
                levels.push(*watermark);
            }
            _ => {}
        }
    }
}

fn set_static_detail_level(item: &mut WorldItem, level: u32) {
    match item {
        WorldItem::Static {
            static_detail_level,
            ..
        }
        | WorldItem::Forest {
            static_detail_level,
            ..
        }
        | WorldItem::Track {
            static_detail_level,
            ..
        }
        | WorldItem::Dyntrack {
            static_detail_level,
            ..
        }
        | WorldItem::Signal {
            static_detail_level,
            ..
        }
        | WorldItem::Speedpost {
            static_detail_level,
            ..
        }
        | WorldItem::SoundRegion {
            static_detail_level,
            ..
        }
        | WorldItem::HWater {
            static_detail_level,
            ..
        }
        | WorldItem::Transfer {
            static_detail_level,
            ..
        }
        | WorldItem::CarSpawner {
            static_detail_level,
            ..
        }
        | WorldItem::Pickup {
            static_detail_level,
            ..
        }
        | WorldItem::Hazard {
            static_detail_level,
            ..
        }
        | WorldItem::Platform {
            static_detail_level,
            ..
        }
        | WorldItem::Siding {
            static_detail_level,
            ..
        }
        | WorldItem::Other {
            static_detail_level,
            ..
        } => *static_detail_level = level,
    }
}

fn parse_watermark_level_from_values(vals: &[Ast]) -> Option<u32> {
    match vals.first()? {
        Ast::Atom(at) => atom_to_u32(at),
        Ast::List(inner) => parse_watermark_level_from_values(inner),
    }
}

fn parse_watermark_level(items: &[Ast]) -> Option<u32> {
    match items.get(1)? {
        Ast::Atom(at) => atom_to_u32(at),
        Ast::List(inner) => parse_watermark_level_from_values(inner),
    }
}

fn atom_to_u32(at: &Atom) -> Option<u32> {
    if let Some(n) = atom_to_number(at) {
        return Some(n.round() as u32);
    }
    if let Atom::Symbol(s) | Atom::String(s) = at {
        if let Ok(v) = s.trim().parse::<u32>() {
            return Some(v);
        }
    }
    None
}

/// Classic typed `Transfer` objects use nested field lists (`FileName`, `Position`, …).
/// JINX wrappers only contain flat `UiD (…) Width (…) …` bags — no typed field heads.
fn transfer_looks_typed(items: &[Ast]) -> bool {
    items.iter().skip(1).any(|entry| match entry {
        Ast::List(inner) => matches!(
            inner.first(),
            Some(Ast::Atom(Atom::Symbol(head)))
                if matches!(
                    head.as_str(),
                    "FileName" | "Position" | "QDirection" | "Width" | "Height"
                        | "StaticDetailLevel" | "VDbId" | "Matrix3x3"
                )
        ),
        _ => false,
    })
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
        // Nested classic Transfer (smoke / Chiltern) must not use the JINX unwrap path.
        if transfer_looks_typed(root) {
            return vec![root.to_vec()];
        }
        // JINX wrapper: keep UiD bags and sibling object entries in file order (#92).
        return root[1..]
            .iter()
            .filter_map(|entry| match entry {
                Ast::List(items) if is_object_entry(items) => Some(items.clone()),
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

fn is_object_tag(tag: &str) -> bool {
    tag.eq_ignore_ascii_case("Static")
        || tag.eq_ignore_ascii_case("TrackObj")
        || tag.eq_ignore_ascii_case("Forest")
        || tag.eq_ignore_ascii_case("Transfer")
        || tag.eq_ignore_ascii_case("Dyntrack")
        || tag.eq_ignore_ascii_case("Signal")
        || tag.eq_ignore_ascii_case("Speedpost")
        || tag.eq_ignore_ascii_case("SoundRegion")
        || tag.eq_ignore_ascii_case("HWater")
        || tag.eq_ignore_ascii_case("CarSpawner")
        || tag.eq_ignore_ascii_case("Pickup")
        || tag.eq_ignore_ascii_case("Hazard")
        || tag.eq_ignore_ascii_case("Platform")
        || tag.eq_ignore_ascii_case("Siding")
        || tag.eq_ignore_ascii_case("LevelCr")
        || tag.eq_ignore_ascii_case("CollideObject")
        || tag.eq_ignore_ascii_case("Gantry")
        || tag.eq_ignore_ascii_case("UiD")
}

fn is_object_entry(items: &[Ast]) -> bool {
    matches!(
        items.first(),
        Some(Ast::Atom(Atom::Symbol(head))) if is_object_tag(head)
    )
}

/// JINX flat `( UiD ( 75 ) Width ( 30 ) … )` → nested `( UiD ( 75 ) ) ( Width ( 30 ) ) …`.
///
/// When `items` is a typed object (`Signal`, `Static`, …), the type atom is kept and
/// pairing starts at the first field so we do not absorb `UiD` into the type head.
/// Bare `UiD` bags are the flat record itself — pair from index 0.
fn normalize_jinx_flat_fields(items: &[Ast]) -> Cow<'_, [Ast]> {
    let field_start = if is_object_entry(items) && !matches_head(items, "UiD") {
        1
    } else {
        0
    };
    let fields = &items[field_start.min(items.len())..];
    if !is_jinx_flat_alternating(fields) {
        return Cow::Borrowed(items);
    }
    let mut out = Vec::with_capacity(fields.len() / 2 + field_start);
    if field_start == 1 {
        out.push(items[0].clone());
    }
    let mut i = 0usize;
    while i + 1 < fields.len() {
        let (Ast::Atom(Atom::Symbol(key)), Ast::List(val)) = (&fields[i], &fields[i + 1]) else {
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
    if out.len() <= field_start {
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
    // Dyntrack also has SectionIdx; detect authored TrackSections first (#87).
    if fields.iter().any(|item| {
        matches!(
            item,
            Ast::List(sub)
                if matches!(
                    sub.first(),
                    Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case("TrackSections")
                )
        )
    }) {
        return Some("Dyntrack".into());
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
    if find_named_f64(fields, "CarFrequency").is_some()
        || find_named_f64(fields, "CarAvSpeed").is_some()
        || find_named_string(fields, "ORTSListName").is_some()
    {
        return Some("CarSpawner".into());
    }
    if find_named_u32(fields, "PickupType").is_some() {
        return Some("Pickup".into());
    }
    if find_named_string(fields, "FileName")
        .is_some_and(|f| f.to_ascii_lowercase().ends_with(".haz"))
    {
        return Some("Hazard".into());
    }
    if find_named_string(fields, "FileName").is_some() {
        return Some("Static".into());
    }
    None
}

fn parse_world_item(items: &[Ast]) -> ParseWorldItem {
    let tag = match items.first() {
        Some(Ast::Atom(Atom::Symbol(s))) => s.clone(),
        _ => return ParseWorldItem::Ignore,
    };

    let normalized = normalize_jinx_flat_fields(items);
    let fields = normalized.as_ref();

    let effective_tag = if tag.eq_ignore_ascii_case("UiD") {
        match infer_object_tag(fields) {
            Some(t) => t,
            None => return ParseWorldItem::Ignore,
        }
    } else {
        tag
    };

    let uid = find_uid(fields);
    let position = find_position(fields);
    let file_name = find_named_string(fields, "FileName");
    let qdir = find_qdirection(fields);
    let matrix3x3 = find_matrix3x3(fields);

    // Open Rails omits incomplete objects instead of spawning at identity (#140).
    if world_tag_requires_position(&effective_tag) && position.is_none() {
        return ParseWorldItem::SkippedInvalidPose;
    }
    if world_tag_requires_orientation(&effective_tag) && qdir.is_none() && matrix3x3.is_none() {
        return ParseWorldItem::SkippedInvalidPose;
    }

    let position_or_zero = position.unwrap_or_default();

    ParseWorldItem::Item(match effective_tag.as_str() {
        s if s.eq_ignore_ascii_case("Static") => WorldItem::Static {
            uid: uid.unwrap_or(0),
            file_name,
            position: position_or_zero,
            qdir,
            matrix3x3,
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Forest") => WorldItem::Forest {
            uid: uid.unwrap_or(0),
            tree_texture: find_named_string(fields, "TreeTexture")
                .or_else(|| find_named_string(fields, "FileName")),
            position: position_or_zero,
            scale_range: find_named_pair(fields, "ScaleRange"),
            patch_size: find_named_pair(fields, "Area"),
            tree_size: find_named_pair(fields, "TreeSize"),
            population: find_named_u32(fields, "Population").unwrap_or(DEFAULT_FOREST_POPULATION),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("TrackObj") => WorldItem::Track {
            uid: uid.unwrap_or(0),
            section_idx: find_named_u32(fields, "SectionIdx"),
            file_name,
            position: position_or_zero,
            qdir,
            matrix3x3,
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Dyntrack") => WorldItem::Dyntrack {
            uid: uid.unwrap_or(0),
            position: position_or_zero,
            qdir,
            matrix3x3,
            static_detail_level: 0,
            section_idx: find_named_u32(fields, "SectionIdx"),
            track_sections: parse_dyntrack_sections(fields),
        },
        s if s.eq_ignore_ascii_case("Signal") => WorldItem::Signal {
            uid: uid.unwrap_or(0),
            file_name,
            position: position_or_zero,
            qdir,
            matrix3x3,
            signal_sub_obj: find_signal_sub_obj_mask(fields),
            signal_units: parse_signal_units(fields),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Speedpost") => WorldItem::Speedpost {
            uid: uid.unwrap_or(0),
            file_name,
            position: position_or_zero,
            qdir,
            matrix3x3,
            tr_item_refs: find_tr_item_refs(fields),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("SoundRegion") => {
            let (tdb_id, tr_item_id) = find_tr_item_id_pair(fields).unwrap_or((0, 0));
            WorldItem::SoundRegion {
                uid: uid.unwrap_or(0),
                file_name,
                position: position_or_zero,
                qdir,
                matrix3x3,
                tdb_id,
                tr_item_id,
                static_detail_level: 0,
            }
        }
        s if s.eq_ignore_ascii_case("HWater") => WorldItem::HWater {
            uid: uid.unwrap_or(0),
            file_name,
            position: position_or_zero,
            size: find_named_pair(fields, "Size").unwrap_or([100.0, 100.0]),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Transfer") => WorldItem::Transfer {
            uid: uid.unwrap_or(0),
            file_name,
            position: position_or_zero,
            width: find_named_f64(fields, "Width").unwrap_or(10.0),
            height: find_named_f64(fields, "Height").unwrap_or(10.0),
            qdir,
            matrix3x3,
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("CarSpawner") => WorldItem::CarSpawner {
            uid: uid.unwrap_or(0),
            car_frequency: find_named_f64(fields, "CarFrequency").unwrap_or(5.0),
            car_av_speed: find_named_f64(fields, "CarAvSpeed").unwrap_or(20.0),
            list_name: find_named_string(fields, "ORTSListName"),
            rdb_tr_item_ids: find_rdb_tr_item_ids(fields),
            position: position_or_zero,
            qdir,
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Pickup") => WorldItem::Pickup {
            uid: uid.unwrap_or(0),
            file_name,
            position: position_or_zero,
            qdir,
            matrix3x3,
            pickup_type: find_named_u32(fields, "PickupType"),
            tr_item_ids: find_tdb_tr_item_ids(fields),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Hazard") => WorldItem::Hazard {
            uid: uid.unwrap_or(0),
            haz_file: file_name,
            position: position_or_zero,
            qdir,
            matrix3x3,
            // Only TDB (db == 0); RDB collisions must not become TDB ids (#105).
            tr_item_id: find_tr_item_id_pair(fields).and_then(|(db, id)| (db == 0).then_some(id)),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Platform") => WorldItem::Platform {
            uid: uid.unwrap_or(0),
            position: position_or_zero,
            qdir,
            matrix3x3,
            file_name,
            platform_data: find_platform_data(fields),
            tr_item_refs: find_tr_item_refs(fields),
            static_detail_level: 0,
        },
        s if s.eq_ignore_ascii_case("Siding") => WorldItem::Siding {
            uid: uid.unwrap_or(0),
            position: position_or_zero,
            qdir,
            matrix3x3,
            file_name,
            tr_item_refs: find_tr_item_refs(fields),
            static_detail_level: 0,
        },
        _ => WorldItem::Other {
            tag: effective_tag,
            uid,
            position,
            file_name,
            qdir,
            matrix3x3,
            static_detail_level: 0,
        },
    })
}

/// Last matching `UiD` wins (Open Rails sequential assign).
fn find_uid(items: &[Ast]) -> Option<u32> {
    let mut found = None;
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, "UiD") {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        found = Some(n as u32);
                    }
                }
            }
        }
    }
    found
}

/// Last matching `Position` wins.
fn find_position(items: &[Ast]) -> Option<Vec3> {
    let mut found = None;
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
                    found = Some(Vec3 {
                        x: nums[0],
                        y: nums[1],
                        z: nums[2],
                    });
                }
            }
        }
    }
    found
}

/// Last matching `QDirection` wins.
fn find_qdirection(items: &[Ast]) -> Option<[f64; 4]> {
    let mut found = None;
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
                    found = Some([nums[0], nums[1], nums[2], nums[3]]);
                }
            }
        }
    }
    found
}

/// Last matching `Matrix3x3` wins.
fn find_matrix3x3(items: &[Ast]) -> Option<[f64; 9]> {
    let mut found = None;
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
                    found = Some([
                        nums[0], nums[1], nums[2], nums[3], nums[4], nums[5], nums[6], nums[7],
                        nums[8],
                    ]);
                }
            }
        }
    }
    found
}

/// Last matching named string field wins.
fn find_named_string(items: &[Ast], key: &str) -> Option<String> {
    let mut found = None;
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
                for child in sub.iter().skip(1) {
                    if let Ast::Atom(at) = child {
                        if let Some(s) = atom_to_string(at) {
                            found = Some(s);
                            break;
                        }
                    }
                }
            }
        }
    }
    found
}

/// Last matching named pair wins.
fn find_named_pair(items: &[Ast], key: &str) -> Option<[f64; 2]> {
    let mut found = None;
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
                    found = Some([nums[0], nums[1]]);
                }
            }
        }
    }
    found
}

/// Last matching named f64 wins.
fn find_named_f64(items: &[Ast], key: &str) -> Option<f64> {
    let mut found = None;
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        found = Some(n);
                    }
                }
            }
        }
    }
    found
}

/// Last matching named u32 wins.
fn find_named_u32(items: &[Ast], key: &str) -> Option<u32> {
    let mut found = None;
    for item in items {
        if let Ast::List(sub) = item {
            if matches_head(sub, key) {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        found = Some(n.max(0.0) as u32);
                    }
                }
            }
        }
    }
    found
}

/// Last matching `PlatformData` (decimal or hex symbol like `00000002`).
fn find_platform_data(items: &[Ast]) -> Option<u32> {
    let mut found = None;
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        if !matches_head(sub, "PlatformData") {
            continue;
        }
        if let Some(Ast::Atom(at)) = sub.get(1) {
            if let Some(n) = atom_to_number(at) {
                found = Some(n.max(0.0) as u32);
            } else if let Atom::Symbol(s) | Atom::String(s) = at {
                let t = s.trim();
                if let Ok(v) = u32::from_str_radix(t, 16) {
                    found = Some(v);
                } else if let Ok(v) = t.parse::<u32>() {
                    found = Some(v);
                }
            }
        }
    }
    found
}

/// First `( TrItemId db item_id )` pair (SoundRegion / Hazard convenience).
fn find_tr_item_id_pair(items: &[Ast]) -> Option<(u32, u32)> {
    find_tr_item_refs(items).first().map(|r| (r.db, r.item_id))
}

/// All `TrItemId` pairs in file order (additive; not last-wins).
fn find_tr_item_refs(items: &[Ast]) -> Vec<WorldTrItemRef> {
    let mut out = Vec::new();
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("TrItemId") {
            continue;
        }
        let nums: Vec<u32> = sub
            .iter()
            .skip(1)
            .filter_map(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                _ => None,
            })
            .collect();
        if nums.len() >= 2 {
            out.push(WorldTrItemRef {
                db: nums[0],
                item_id: nums[1],
            });
        }
    }
    out
}

/// Collect TDB item ids from `( TrItemId 0 item_id )` pairs (database index 0 = track DB).
fn find_tdb_tr_item_ids(items: &[Ast]) -> Vec<u32> {
    let mut out = Vec::new();
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("TrItemId") {
            continue;
        }
        let nums: Vec<u32> = sub
            .iter()
            .skip(1)
            .filter_map(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                _ => None,
            })
            .collect();
        if nums.len() >= 2 && nums[0] == 0 {
            out.push(nums[1]);
        }
    }
    out
}

/// Collect RDB item ids from `( TrItemId 1 item_id )` pairs (database index 1 = road DB).
fn find_rdb_tr_item_ids(items: &[Ast]) -> Vec<u32> {
    let mut out = Vec::new();
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("TrItemId") {
            continue;
        }
        let nums: Vec<u32> = sub
            .iter()
            .skip(1)
            .filter_map(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                _ => None,
            })
            .collect();
        if nums.len() >= 2 && nums[0] == 1 {
            out.push(nums[1]);
        }
    }
    out
}

/// Parse Dyntrack `TrackSections ( TrackSection ( SectionCurve ( c ) uid p1 p2 ) … )`.
fn parse_dyntrack_sections(items: &[Ast]) -> Vec<DyntrackSection> {
    let mut out = Vec::new();
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("TrackSections") {
            continue;
        }
        for entry in sub.iter().skip(1) {
            let Ast::List(sec) = entry else {
                continue;
            };
            let Some(Ast::Atom(Atom::Symbol(sec_tag))) = sec.first() else {
                continue;
            };
            if !sec_tag.eq_ignore_ascii_case("TrackSection") {
                continue;
            }
            // OR: TrackSection ( SectionCurve ( isCurved ) uid param1 param2 )
            let mut is_curved = 0u32;
            let mut nums: Vec<f64> = Vec::new();
            let mut i = 1usize;
            while i < sec.len() {
                match &sec[i] {
                    Ast::List(curve)
                        if matches!(
                            curve.first(),
                            Some(Ast::Atom(Atom::Symbol(s)))
                                if s.eq_ignore_ascii_case("SectionCurve")
                        ) =>
                    {
                        is_curved = curve
                            .get(1)
                            .and_then(|a| match a {
                                Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                                _ => None,
                            })
                            .unwrap_or(0);
                        i += 1;
                    }
                    Ast::Atom(Atom::Symbol(s)) if s.eq_ignore_ascii_case("SectionCurve") => {
                        // Flat: SectionCurve isCurved uid p1 p2
                        if let Some(Ast::Atom(at)) = sec.get(i + 1) {
                            is_curved = atom_to_number(at).map(|n| n as u32).unwrap_or(0);
                        }
                        i += 2;
                        while nums.len() < 3 {
                            if let Some(Ast::Atom(at)) = sec.get(i) {
                                if let Some(n) = atom_to_number(at) {
                                    nums.push(n);
                                    i += 1;
                                    continue;
                                }
                            }
                            break;
                        }
                    }
                    Ast::Atom(at) => {
                        if let Some(n) = atom_to_number(at) {
                            nums.push(n);
                        }
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            if nums.len() >= 3 {
                out.push(DyntrackSection {
                    is_curved,
                    uid: nums[0] as u32,
                    param1: nums[1] as f32,
                    param2: nums[2] as f32,
                });
            }
        }
    }
    out
}

/// Parse `SignalSubObj ( 00000007 )` bitmask (hex or decimal flags); last wins.
fn find_signal_sub_obj_mask(items: &[Ast]) -> u32 {
    let mut found = 0u32;
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("SignalSubObj") {
            continue;
        }
        if let Some(Ast::Atom(at)) = sub.get(1) {
            if let Some(n) = atom_to_number(at) {
                found = n as u32;
            } else if let Atom::Symbol(s) | Atom::String(s) = at {
                if let Ok(v) = u32::from_str_radix(s.trim(), 16) {
                    found = v;
                }
            }
        }
    }
    found
}

/// Parse `SignalUnits ( N ( SignalUnit SubObj ( TrItemId db itemId ) ) … )`.
///
/// Also accepts the rare flattened form `SignalUnit ( SubObj db itemId )`.
fn parse_signal_units(items: &[Ast]) -> Vec<SignalUnitRef> {
    let mut out = Vec::new();
    for item in items {
        let Ast::List(sub) = item else {
            continue;
        };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        if !tag.eq_ignore_ascii_case("SignalUnits") {
            continue;
        }
        for unit in sub.iter().skip(1) {
            let Ast::List(unit_sub) = unit else {
                continue;
            };
            let Some(Ast::Atom(Atom::Symbol(unit_tag))) = unit_sub.first() else {
                continue;
            };
            if !unit_tag.eq_ignore_ascii_case("SignalUnit") {
                continue;
            }
            let sub_obj = unit_sub.get(1).and_then(|a| match a {
                Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                _ => None,
            });
            // Nested `TrItemId ( db itemId )` (Chiltern / OR form).
            if let (Some(sub_obj), Some((_, tr_item_id))) =
                (sub_obj, find_tr_item_id_pair(unit_sub))
            {
                out.push(SignalUnitRef {
                    sub_obj,
                    tr_item_id,
                });
                continue;
            }
            // Flattened legacy: SignalUnit ( subObj db itemId )
            let flat_nums: Vec<u32> = unit_sub
                .iter()
                .skip(1)
                .filter_map(|a| match a {
                    Ast::Atom(at) => atom_to_number(at).map(|n| n as u32),
                    _ => None,
                })
                .collect();
            if flat_nums.len() >= 3 {
                out.push(SignalUnitRef {
                    sub_obj: flat_nums[0],
                    tr_item_id: flat_nums[2],
                });
            }
        }
    }
    out
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
        // Local Watersnake installs vary; require at least one typed Transfer with uid 75.
        assert!(
            !transfers.is_empty(),
            "expected Transfer items in tunnel tile, got 0"
        );
        assert!(
            transfers.iter().any(|t| t.uid() == Some(75)),
            "missing transfer uid 75 (got {} transfers)",
            transfers.len()
        );
    }
}

#[cfg(test)]
mod typed_transfer_tests {
    use super::*;

    #[test]
    fn nested_transfer_not_unwrapped_as_jinx() {
        let text = r#"
SIMISA@@@@@@@@@@JINX0w0t______
( Tr_Worldfile
    ( Transfer
        ( UiD 7 )
        ( FileName "yard.ace" )
        ( Position 140.0 0.1 40.0 )
        ( Width 20.0 )
        ( Height 12.0 )
        ( QDirection 0.0 0.0 0.0 1.0 )
    )
)
        "#;
        let ast = load_world_ast(text).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 1);
        let Some(WorldItem::Transfer {
            uid,
            file_name,
            width,
            height,
            ..
        }) = world.items.first()
        else {
            panic!("expected typed Transfer, got {:?}", world.items);
        };
        assert_eq!(*uid, 7);
        assert_eq!(file_name.as_deref(), Some("yard.ace"));
        assert!((*width - 20.0).abs() < 1e-6);
        assert!((*height - 12.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod pickup_hazard_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_pickup_and_hazard_nested() {
        // Head-outside blocks like Chiltern; `load_world_ast` normalizes them.
        let text = r#"
SIMISA@@@@@@@@@@JINX0w0t______
Tr_Worldfile (
	Pickup (
		UiD ( 512 )
		PickupType ( 5 0 )
		TrItemId ( 0 768 )
		FileName ( RF_GW_WaterColumn.s )
		Position ( -857.82 94.1064 -499.292 )
		QDirection ( 0 0.983255 0 0.182235 )
	)
	Hazard (
		UiD ( 1351 )
		TrItemId ( 0 4938 )
		FileName ( crow.haz )
		Position ( 157.607 116.26 -917.42 )
		QDirection ( 0 0.448149 0 0.893959 )
	)
)
        "#;
        let ast = load_world_ast(text).expect("parse");
        fn dump(ast: &Ast, indent: usize) {
            let pad = "  ".repeat(indent);
            match ast {
                Ast::Atom(a) => eprintln!("{pad}{a:?}"),
                Ast::List(items) => {
                    eprintln!("{pad}(");
                    for it in items.iter().take(40) {
                        dump(it, indent + 1);
                    }
                    if items.len() > 40 {
                        eprintln!("{pad}  ...");
                    }
                    eprintln!("{pad})");
                }
            }
        }
        dump(&ast, 0);
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(
            world.items.iter().filter(|i| i.kind() == "Pickup").count(),
            1
        );
        assert_eq!(
            world.items.iter().filter(|i| i.kind() == "Hazard").count(),
            1
        );
        let Some(WorldItem::Pickup {
            file_name,
            pickup_type,
            tr_item_ids,
            ..
        }) = world.items.iter().find(|i| i.kind() == "Pickup")
        else {
            panic!("expected Pickup");
        };
        assert_eq!(file_name.as_deref(), Some("RF_GW_WaterColumn.s"));
        assert_eq!(*pickup_type, Some(5));
        assert_eq!(tr_item_ids, &vec![768]);
        let Some(WorldItem::Hazard {
            haz_file,
            tr_item_id,
            ..
        }) = world.items.iter().find(|i| i.kind() == "Hazard")
        else {
            panic!("expected Hazard");
        };
        assert_eq!(haz_file.as_deref(), Some("crow.haz"));
        assert_eq!(*tr_item_id, Some(4938));
    }

    #[test]
    fn chiltern_tiles_count_pickup_and_hazard() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Some(home) = home else {
            return;
        };
        let route = home.join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        let pickup_tile = route.join("WORLD/w-006100+014936.w");
        let hazard_tile = route.join("WORLD/w-006097+014940.w");
        if !pickup_tile.is_file() || !hazard_tile.is_file() {
            return;
        }
        let pickups = WorldFile::from_path(&pickup_tile).expect("pickup tile");
        let n_pickup = pickups
            .items
            .iter()
            .filter(|i| i.kind() == "Pickup")
            .count();
        assert!(
            n_pickup >= 2,
            "expected ≥2 Pickup on water-column tile, got {n_pickup}"
        );
        let hazards = WorldFile::from_path(&hazard_tile).expect("hazard tile");
        assert_eq!(
            hazards
                .items
                .iter()
                .filter(|i| i.kind() == "Hazard")
                .count(),
            2
        );
    }
}

#[cfg(test)]
mod watermark_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn tr_watermark_assigns_static_detail_level() {
        // Nested S-expr; UiD atoms (not nested lists) match find_uid.
        let src = r#"
(Tr_Worldfile
  (TrackObj (UiD 1) (SectionIdx 10) (Position 0 0 0) (QDirection 0 0 0 1))
  (Tr_Watermark 2)
  (TrackObj (UiD 2) (SectionIdx 11) (Position 1 0 0) (QDirection 0 0 0 1))
  (Tr_Watermark 3)
  (Dyntrack (UiD 3) (Position 2 0 0) (QDirection 0 0 0 1))
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        let levels: Vec<(u32, u32)> = world
            .items
            .iter()
            .filter_map(|i| Some((i.uid()?, i.static_detail_level())))
            .collect();
        assert_eq!(levels, vec![(1, 0), (2, 2), (3, 3)]);
    }

    #[test]
    fn tr_watermark_applies_to_all_object_kinds() {
        let src = r#"
(Tr_Worldfile
  (Tr_Watermark 5)
  (Static (UiD 1) (FileName "a.s") (Position 0 0 0) (QDirection 0 0 0 1))
  (Forest (UiD 2) (TreeTexture "t.ace") (Position 1 0 0) (Population 10))
  (Transfer (UiD 3) (FileName "x.ace") (Position 2 0 0) (Width 4) (Height 5) (QDirection 0 0 0 1))
  (Tr_Watermark 7)
  (Static (UiD 4) (FileName "b.s") (Position 3 0 0) (QDirection 0 0 0 1))
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 4);
        assert_eq!(world.items[0].static_detail_level(), 5);
        assert_eq!(world.items[1].static_detail_level(), 5);
        assert_eq!(world.items[2].static_detail_level(), 5);
        assert_eq!(world.items[3].static_detail_level(), 7);
        assert_eq!(world.items[0].kind(), "Static");
        assert_eq!(world.items[1].kind(), "Forest");
        assert_eq!(world.items[2].kind(), "Transfer");
    }

    #[test]
    fn dyntrack_parses_section_idx_and_track_sections() {
        // Canonical S-expressions (bypass Name-normalize quirks for TrackSections nesting).
        let src = r#"
(Tr_Worldfile
  (Dyntrack
    (UiD 9)
    (SectionIdx 3)
    (Position 10 1 20)
    (QDirection 0 0 0 1)
    (TrackSections
      (TrackSection (SectionCurve 1) 40002 -0.3 120.0)
    )
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 1, "items={:?}", world.items);
        let WorldItem::Dyntrack {
            section_idx,
            track_sections,
            ..
        } = &world.items[0]
        else {
            panic!("expected Dyntrack, got {:?}", world.items[0].kind());
        };
        assert_eq!(*section_idx, Some(3));
        assert_eq!(track_sections.len(), 1, "{track_sections:?}");
        let sec = track_sections[0];
        assert_eq!(sec.uid, 40002);
        assert_eq!(sec.is_curved, 1);
        assert!((sec.param1.abs() - 0.3).abs() < 1e-3);
        assert!((sec.param2 - 120.0).abs() < 1e-3);
        assert!(sec.is_curve());
    }

    #[test]
    fn dyntrack_section_travel_length_helpers() {
        let straight = DyntrackSection {
            is_curved: 0,
            uid: 1,
            param1: 25.0,
            param2: 0.0,
        };
        assert!((straight.travel_length_m() - 25.0).abs() < 1e-6);
        let curve = DyntrackSection {
            is_curved: 1,
            uid: 2,
            param1: -0.3,
            param2: 120.0,
        };
        assert!((curve.travel_length_m() - 36.0).abs() < 1e-3);
    }
}

#[cfg(test)]
mod last_wins_scalar_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn last_position_and_filename_win_tr_item_ids_accumulate() {
        let src = r#"
(Tr_Worldfile
  (Static
    (UiD 1)
    (FileName "first.s")
    (FileName "second.s")
    (Position 1 2 3)
    (Position 10 20 30)
    (QDirection 0 0 0 1)
  )
  (Pickup
    (UiD 2)
    (PickupType 5 0)
    (TrItemId 0 100)
    (TrItemId 0 200)
    (FileName "p.s")
    (Position 0 0 0)
    (QDirection 0 0 0 1)
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        let stat = world
            .items
            .iter()
            .find(|i| i.kind() == "Static")
            .expect("Static");
        assert_eq!(stat.file_name(), Some("second.s"));
        let pos = stat.position().expect("pos");
        assert!((pos.x - 10.0).abs() < 1e-9);
        assert!((pos.y - 20.0).abs() < 1e-9);
        assert!((pos.z - 30.0).abs() < 1e-9);
        let pickup = world
            .items
            .iter()
            .find(|i| i.kind() == "Pickup")
            .expect("Pickup");
        assert_eq!(pickup.tr_item_ids(), vec![100, 200]);
    }
}

#[cfg(test)]
mod platform_siding_other_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn five_tags_platform_siding_levelcr_collide_gantry() {
        let src = r#"
(Tr_Worldfile
  (Platform
    (UiD 1)
    (FileName "plat.s")
    (PlatformData 00000002)
    (TrItemId 0 10)
    (TrItemId 0 11)
    (Position 1 0 0)
    (QDirection 0 0 0 1)
  )
  (Siding
    (UiD 2)
    (TrItemId 0 20)
    (Position 2 0 0)
    (QDirection 0 0 0 1)
  )
  (LevelCr (UiD 3) (Position 3 0 0) (FileName "lc.s"))
  (CollideObject (UiD 4) (Position 4 0 0) (FileName "col.s"))
  (Gantry (UiD 5) (Position 5 0 0) (FileName "gan.s"))
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 5);
        assert_eq!(world.items[0].kind(), "Platform");
        assert_eq!(world.items[1].kind(), "Siding");
        match &world.items[2] {
            WorldItem::Other { tag, .. } => assert_eq!(tag, "LevelCr"),
            other => panic!("expected Other LevelCr, got {other:?}"),
        }
        match &world.items[3] {
            WorldItem::Other { tag, .. } => assert_eq!(tag, "CollideObject"),
            other => panic!("expected Other CollideObject, got {other:?}"),
        }
        match &world.items[4] {
            WorldItem::Other { tag, .. } => assert_eq!(tag, "Gantry"),
            other => panic!("expected Other Gantry, got {other:?}"),
        }
        let WorldItem::Platform {
            platform_data,
            tr_item_refs,
            file_name,
            ..
        } = &world.items[0]
        else {
            panic!("Platform");
        };
        assert_eq!(*platform_data, Some(2));
        assert_eq!(file_name.as_deref(), Some("plat.s"));
        assert_eq!(
            tr_item_refs,
            &vec![
                WorldTrItemRef { db: 0, item_id: 10 },
                WorldTrItemRef { db: 0, item_id: 11 }
            ]
        );
        assert_eq!(world.items[0].tr_item_ids(), vec![10, 11]);
        let WorldItem::Siding { tr_item_refs, .. } = &world.items[1] else {
            panic!("Siding");
        };
        assert_eq!(tr_item_refs, &vec![WorldTrItemRef { db: 0, item_id: 20 }]);
    }
}

#[cfg(test)]
mod speedpost_tr_item_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn speedpost_accumulates_all_tr_item_ids() {
        let src = r#"
(Tr_Worldfile
  (Speedpost
    (UiD 9)
    (FileName "sp.s")
    (TrItemId 0 100)
    (TrItemId 0 200)
    (TrItemId 1 300)
    (Position 0 0 0)
    (QDirection 0 0 0 1)
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        let WorldItem::Speedpost { tr_item_refs, .. } = &world.items[0] else {
            panic!("expected Speedpost, got {:?}", world.items[0].kind());
        };
        assert_eq!(
            tr_item_refs,
            &vec![
                WorldTrItemRef {
                    db: 0,
                    item_id: 100
                },
                WorldTrItemRef {
                    db: 0,
                    item_id: 200
                },
                WorldTrItemRef {
                    db: 1,
                    item_id: 300
                },
            ]
        );
        assert_eq!(world.items[0].tr_item_ids(), vec![100, 200]);
    }
}

#[cfg(test)]
mod hazard_db_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn hazard_ignores_non_tdb_tr_item_id() {
        let src = r#"
(Tr_Worldfile
  (Hazard
    (UiD 1)
    (TrItemId 1 42)
    (FileName "crow.haz")
    (Position 0 0 0)
    (QDirection 0 0 0 1)
  )
  (Hazard
    (UiD 2)
    (TrItemId 0 42)
    (FileName "crow.haz")
    (Position 1 0 0)
    (QDirection 0 0 0 1)
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 2);
        let WorldItem::Hazard {
            tr_item_id: id1, ..
        } = &world.items[0]
        else {
            panic!("Hazard");
        };
        let WorldItem::Hazard {
            tr_item_id: id2, ..
        } = &world.items[1]
        else {
            panic!("Hazard");
        };
        assert_eq!(*id1, None);
        assert_eq!(world.items[0].tr_item_ids(), Vec::<u32>::new());
        assert_eq!(*id2, Some(42));
        assert_eq!(world.items[1].tr_item_ids(), vec![42]);
    }
}

#[cfg(test)]
mod invalid_pose_skip_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn static_without_position_or_orientation_is_skipped() {
        let src = r#"
(Tr_Worldfile
  (Static (UiD 1) (FileName "a.s"))
  (Static (UiD 2) (FileName "b.s") (Position 1 0 0))
  (Static (UiD 3) (FileName "c.s") (Position 2 0 0) (QDirection 0 0 0 1))
  (Forest (UiD 4) (TreeTexture "t.ace"))
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 1);
        assert_eq!(world.items[0].uid(), Some(3));
        assert_eq!(world.skipped_invalid_pose, 3);
    }
}

#[cfg(test)]
mod jinx_transfer_sibling_tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn jinx_transfer_keeps_uid_bag_and_trackobj_sibling() {
        // Untyped Transfer wrapper: JINX flat UiD bag + typed TrackObj sibling (#92).
        let src = r#"
(Tr_Worldfile
  (Transfer
    (UiD (75) Width (30) Height (12) Position (1 2 3) QDirection (0 0 0 1))
    (TrackObj (UiD 99) (SectionIdx 7) (Position 4 5 6) (FileName "rail.s") (QDirection 0 0 0 1))
  )
)
"#;
        let ast = parse_from_first_paren(src).expect("parse");
        let Ast::List(root) = &ast else {
            panic!("list");
        };
        let flat = flatten_world_entries(root);
        assert_eq!(
            flat.len(),
            2,
            "expected UiD bag + TrackObj, got {} entries",
            flat.len()
        );
        let world = WorldFile::from_ast(&ast, 0, 0);
        assert_eq!(world.items.len(), 2, "items={:?}", world.items);
        assert_eq!(world.items[0].kind(), "Transfer");
        assert_eq!(world.items[0].uid(), Some(75));
        assert_eq!(world.items[1].kind(), "TrackObj");
        assert_eq!(world.items[1].uid(), Some(99));
    }
}

#[cfg(test)]
mod signal_unit_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn chiltern_signal_tr_item_ids_populated() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Some(home) = home else { return };
        let path = home
            .join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/WORLD/w-006080+014925.w");
        if !path.is_file() {
            return;
        }
        let world = WorldFile::from_path(&path).expect("world");
        let signals: Vec<_> = world
            .items
            .iter()
            .filter(|i| i.kind() == "Signal")
            .collect();
        assert!(!signals.is_empty());
        let with_ids = signals
            .iter()
            .filter(|i| !i.tr_item_ids().is_empty())
            .count();
        assert!(
            with_ids > signals.len() / 2,
            "expected most signals to have TrItemIds, got {with_ids}/{}",
            signals.len()
        );
        let theatre = signals
            .iter()
            .find(|i| matches!(i.file_name(), Some(n) if n.eq_ignore_ascii_case("TheatreBoxSQ.s")));
        if let Some(s) = theatre {
            eprintln!(
                "theatre units={:?} mask={:?}",
                s.signal_units(),
                s.signal_sub_obj_mask()
            );
            assert_eq!(s.tr_item_ids(), vec![11481, 11482]);
            assert_eq!(s.signal_units().len(), 2);
            assert_eq!(s.signal_sub_obj_mask(), Some(7));
        }
    }
}
