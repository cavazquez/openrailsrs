//! Parser for MSTS Activity (`.act`) files.
//!
//! An activity file drives a simulation session: it names the player's consist
//! (`.con`), the path the player should follow (`.pat`), and the start time.
//!
//! Relevant section (simplified):
//! ```text
//! (Tr_Activity
//!     (Tr_Activity_Header
//!         (Name "Retiro to Bartolomé Mitre" )
//!         (Player_Train_Init
//!             (Player_Train_Init_TD  0  1  1 )
//!             (Player_Train_Init_Cons "PATHS\\Retiro-Victoria.con" )
//!         )
//!         (Player_Path "PATHS\\Retiro-Victoria.pat" )
//!         (Season Summer )
//!         (StartTime  8  0  0 )
//!         (Duration   1  30 )
//!     )
//! )
//! ```

use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::parser::parse_from_first_paren;

use std::path::Path;

use super::{atom_to_number, atom_to_string};

/// One AI traffic service declared inside `Tr_Activity_Service_Definition` /
/// `Traffic_Definition` blocks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrafficServiceDef {
    /// Service name as declared in `Service_Definition "<name>"`.
    pub name: String,
    /// Relative path to the AI `.pat` file.
    pub path_file: String,
    /// Optional override consist; falls back to the player's consist when absent.
    pub consist: Option<String>,
    /// Departure time in seconds (from `Service_Init_Time`).
    pub start_time_s: f64,
}

/// One restricted-speed zone declared inside `RestrictedSpeedZones`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RestrictedZone {
    /// `TrItemId` marking the start of the zone.
    pub item_id_start: u32,
    /// `TrItemId` marking the end of the zone (may equal `item_id_start`).
    pub item_id_end: u32,
    /// Maximum speed in metres per second to apply over the zone.
    pub max_speed_mps: f64,
    /// MSTS world tile position `(tileX, height, tileZ, pos)` when declared via `StartPosition`.
    pub position_start: Option<[f64; 4]>,
    /// MSTS world tile position for `EndPosition`.
    pub position_end: Option<[f64; 4]>,
}

/// One activity-level override for a `TrItemTable` `SoundSourceItem`.
///
/// MSTS spells these blocks as `SoundRegion` (or `ActivitySoundRegion`) inside
/// a `SoundRegions` container.  Each override binds extra metadata to a
/// `TrItemId` that already exists in the route's `.tdb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundRegionOverride {
    /// `TrItemId` of the `SoundSourceItem` this override applies to.
    pub tr_item_id: u32,
    /// Region kind tag (free-form; `tunnel`, `depot`, `urban`, ...).
    pub kind: String,
    /// Base playback volume in `[0.0, 1.0]`.
    pub volume: f64,
    /// Optional radius in metres; `None` lets the importer pick a default.
    pub radius_m: Option<f64>,
}

/// One pickup/dropoff event declared inside `ActivityObjects`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActivityObjectDef {
    /// `TrItemId` of the track item at which the event occurs.
    pub item_id: u32,
    /// Crew/passengers/freight quantity (`Workers` or `Population` field).
    pub workers: u32,
    /// Activity object kind (`PickupWagon`, `DropOffWagon`, etc.) — free-form.
    pub kind: String,
}

/// Parsed representation of a `.act` file.
#[derive(Clone, Debug, Default)]
pub struct ActivityFile {
    /// Human-readable activity name.
    pub name: String,
    /// Relative path to the player consist (`.con`).
    pub player_consist: String,
    /// Relative path to the player path (`.pat`).
    pub player_path: String,
    /// Service / path id when the activity uses `PathID` or `Player_Service_Definition`.
    pub player_service_id: Option<String>,
    /// Start time in seconds from midnight.
    pub start_time_s: f64,
    /// Duration in seconds.
    pub duration_s: f64,
    /// Optional season tag (`Spring` / `Summer` / `Autumn` / `Winter`).
    pub season: Option<String>,
    /// AI traffic services parsed from `Service_Definition` blocks.
    pub services: Vec<TrafficServiceDef>,
    /// `TrItemId`s of signals listed under `FailedSignals` (force `Stop` aspect).
    pub failed_signals: Vec<u32>,
    /// Speed restrictions parsed from `RestrictedSpeedZones`.
    pub restricted_zones: Vec<RestrictedZone>,
    /// Pickup/dropoff event metadata from `ActivityObjects`.
    pub activity_objects: Vec<ActivityObjectDef>,
    /// Activity-level overrides for sound regions defined in the TDB.
    pub sound_regions: Vec<SoundRegionOverride>,
}

impl ActivityFile {
    /// Parse from a pre-built AST.
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let name = find_string_field(ast, &["Name"]).unwrap_or_default();
        let player_consist = find_string_field(ast, &["Player_Train_Init_Cons"])
            .or_else(|| find_string_field(ast, &["Player_Consist"]))
            .unwrap_or_default();
        let player_service_id = find_string_field(ast, &["PathID"])
            .or_else(|| find_header_name(ast))
            .or_else(|| {
                find_string_field(ast, &["Player_Service_Definition"])
                    .filter(|s| valid_service_name(s))
            })
            .or_else(|| {
                if valid_service_name(&name) {
                    Some(name.clone())
                } else {
                    None
                }
            });
        let player_path = find_string_field(ast, &["Player_Path"])
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                player_service_id
                    .as_ref()
                    .map(|id| pat_path_from_service_id(id))
            })
            .unwrap_or_default();
        let start_time_s = parse_start_time(ast);
        let duration_s = parse_duration(ast);
        let season = find_string_field(ast, &["Season"]);
        let mut services = Vec::new();
        collect_service_defs(ast, &mut services);
        let failed_signals = collect_failed_signals(ast);
        let restricted_zones = collect_restricted_zones(ast);
        let activity_objects = collect_activity_objects(ast);
        let sound_regions = collect_sound_regions(ast);

        Ok(Self {
            name,
            player_consist,
            player_path,
            player_service_id,
            start_time_s,
            duration_s,
            season,
            services,
            failed_signals,
            restricted_zones,
            activity_objects,
            sound_regions,
        })
    }

    /// Convenience: read and parse a `.act` file from disk.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, FormatError> {
        let text = crate::encoding::read_msts_file_to_string(path.as_ref())?;
        let ast = parse_from_first_paren(&text)?;
        let mut file = Self::from_ast(&ast)?;
        if file.name.is_empty() {
            file.name = extract_activity_name_from_text(&text).unwrap_or_default();
        }
        if file.player_service_id.is_none() {
            file.player_service_id = extract_activity_name_from_text(&text);
        }
        if file.player_path.is_empty() {
            if let Some(id) = file
                .player_service_id
                .clone()
                .or_else(|| Some(file.name.clone()))
            {
                if valid_service_name(&id) {
                    file.player_path = pat_path_from_service_id(&id);
                    file.player_service_id = Some(id);
                }
            }
        }
        Ok(file)
    }

    /// Read `Train_Config` from `SERVICES/<service_id>.srv` under `route_dir`.
    ///
    /// `PathID` values often carry a `(player)` suffix that is absent from the
    /// actual `.srv` filename.  Both the full id and the base name (without the
    /// trailing parenthetical, e.g. `"Foo(player)"` → `"Foo"`) are tried.
    pub fn train_config_from_service(route_dir: &Path, service_id: &str) -> Option<String> {
        let trimmed = service_id
            .rfind('(')
            .map(|i| service_id[..i].trim())
            .unwrap_or(service_id);
        let candidates: &[&str] = if trimmed != service_id {
            &[service_id, trimmed]
        } else {
            &[service_id]
        };
        for &id in candidates {
            let srv_path = route_dir.join("SERVICES").join(format!("{id}.srv"));
            if let Ok(text) = crate::encoding::read_msts_file_case_insensitive(&srv_path) {
                if let Ok(ast) = parse_from_first_paren(&text) {
                    if let Some(name) = find_string_field(&ast, &["Train_Config"]) {
                        return Some(name);
                    }
                }
                if let Some(name) = extract_train_config_from_text(&text) {
                    return Some(name);
                }
            }
        }
        None
    }
}

/// MSTS activities that use `Player_Service_Definition` / `PathID` instead of `Player_Path`.
fn pat_path_from_service_id(id: &str) -> String {
    let id = id.trim();
    if id.to_ascii_lowercase().ends_with(".pat") {
        id.replace('\\', "/")
    } else {
        format!("PATHS/{id}.pat")
    }
}

fn find_header_name(ast: &Ast) -> Option<String> {
    let Ast::List(items) = ast else {
        return None;
    };
    for item in items {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("Tr_Activity_Header") {
                return find_string_field(item, &["Name"]);
            }
        }
        if let Some(name) = find_header_name(item) {
            return Some(name);
        }
    }
    None
}

fn valid_service_name(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && !s.contains("Definition") && !s.chars().all(|c| c.is_ascii_digit())
}

fn extract_train_config_from_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if !line.contains("Train_Config") {
            continue;
        }
        if let Some(start) = line.find('"') {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn extract_activity_name_from_text(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("Name") {
            continue;
        }
        if let Some(start) = line.find('"') {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find('"') {
                let name = &rest[..end];
                if valid_service_name(name) {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Recursively find the first string value of a field with any of the given names.
///
/// Handles two MSTS layout conventions:
///
/// - **Nested** (standard S-expression): `(Name "value")` — a list whose first element is the key.
/// - **Flat** (common in MSTS headers): `Name ( "value" )` — the keyword symbol and its value
///   list appear as *adjacent siblings* inside a parent list, not as a nested `(key value)` pair.
pub(crate) fn find_string_field(ast: &Ast, names: &[&str]) -> Option<String> {
    let Ast::List(items) = ast else { return None };

    // Pattern 1: list starts with the key symbol — nested format `(Name "value")`.
    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        for n in names {
            if head.eq_ignore_ascii_case(n) {
                if let Some(s) = string_after_first(items) {
                    return Some(s);
                }
            }
        }
    }

    // Pattern 2: adjacent flat format — `Symbol(key)` followed by its value sibling.
    // This covers the common MSTS layout where a keyword is NOT the first element of the
    // enclosing list (e.g. `RouteID ( SCE ) Name ( "value" )` inside a header list).
    for i in 0..items.len().saturating_sub(1) {
        if let Ast::Atom(Atom::Symbol(sym)) = &items[i] {
            for n in names {
                if sym.eq_ignore_ascii_case(n) {
                    if let Some(s) = extract_any_string(&items[i + 1]) {
                        return Some(s);
                    }
                }
            }
        }
    }

    for child in items {
        if let Some(v) = find_string_field(child, names) {
            return Some(v);
        }
    }
    None
}

/// Extract the first string/symbol value after the keyword (items[0]) in a key-value list.
/// When the value is wrapped in a single-element list `( "string" )`, dig into it.
fn string_after_first(items: &[Ast]) -> Option<String> {
    for item in items.iter().skip(1) {
        if let Some(s) = extract_any_string(item) {
            return Some(s);
        }
    }
    None
}

/// Find the first string or symbol atom in any AST node (no skipping).
fn extract_any_string(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(a) => atom_to_string(a),
        Ast::List(sub) => sub.iter().find_map(extract_any_string),
    }
}

/// Parse `(StartTime <h> <m> <s>)` → seconds from midnight.
fn parse_start_time(ast: &Ast) -> f64 {
    if let Some(vals) = find_numeric_tuple(ast, "StartTime", 3) {
        return vals[0] * 3600.0 + vals[1] * 60.0 + vals[2];
    }
    0.0
}

/// Parse `(Duration <h> <m>)` → seconds.
fn parse_duration(ast: &Ast) -> f64 {
    if let Some(vals) = find_numeric_tuple(ast, "Duration", 2) {
        return vals[0] * 3600.0 + vals[1] * 60.0;
    }
    3600.0
}

fn find_numeric_tuple(ast: &Ast, name: &str, count: usize) -> Option<Vec<f64>> {
    let Ast::List(items) = ast else { return None };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case(name) {
            let vals: Vec<f64> = items
                .iter()
                .skip(1)
                .take(count)
                .filter_map(|a| match a {
                    Ast::Atom(Atom::Integer(i)) => Some(*i as f64),
                    Ast::Atom(Atom::Number(n)) => Some(*n),
                    _ => None,
                })
                .collect();
            if vals.len() == count {
                return Some(vals);
            }
        }
    }

    for child in items {
        if let Some(v) = find_numeric_tuple(child, name, count) {
            return Some(v);
        }
    }
    None
}

/// Recursively walk the AST collecting every `Service_Definition` block.
fn collect_service_defs(ast: &Ast, out: &mut Vec<TrafficServiceDef>) {
    let Ast::List(items) = ast else { return };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("Service_Definition") {
            if let Some(svc) = parse_one_service(items) {
                out.push(svc);
            }
            return;
        }
    }

    for child in items {
        collect_service_defs(child, out);
    }
}

/// Convert a `(Service_Definition "<name>" ...)` list into a `TrafficServiceDef`.
///
/// Returns `None` when no `.pat` reference can be found anywhere inside the
/// service body — those entries are unusable for an `extra_trains` import.
fn parse_one_service(items: &[Ast]) -> Option<TrafficServiceDef> {
    let name = items
        .iter()
        .skip(1)
        .find_map(|a| match a {
            Ast::Atom(at) => atom_to_string(at),
            _ => None,
        })
        .unwrap_or_default();
    let path_file = find_pat_path(items).unwrap_or_default();
    if path_file.is_empty() {
        return None;
    }
    let start_time_s = find_service_init_time(items).unwrap_or(0.0);
    let consist = find_service_consist(items);
    Some(TrafficServiceDef {
        name,
        path_file,
        consist,
        start_time_s,
    })
}

/// Search recursively for any string atom ending in `.pat`.
fn find_pat_path(items: &[Ast]) -> Option<String> {
    for item in items {
        match item {
            Ast::Atom(Atom::String(s)) if s.to_ascii_lowercase().ends_with(".pat") => {
                return Some(s.clone());
            }
            Ast::List(sub) => {
                if let Some(p) = find_pat_path(sub) {
                    return Some(p);
                }
            }
            _ => {}
        }
    }
    None
}

/// Recursively look for `(Service_Init_Time <seconds>)`.
fn find_service_init_time(items: &[Ast]) -> Option<f64> {
    for item in items {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("Service_Init_Time") {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(v) = atom_to_number(at) {
                        return Some(v);
                    }
                }
            }
        }
        if let Some(t) = find_service_init_time(sub) {
            return Some(t);
        }
    }
    None
}

/// Recursively look for `(Train_Config "...")` or `(Service_Train_Config "...")`.
fn find_service_consist(items: &[Ast]) -> Option<String> {
    for item in items {
        let Ast::List(sub) = item else { continue };
        if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
            if tag.eq_ignore_ascii_case("Train_Config")
                || tag.eq_ignore_ascii_case("Service_Train_Config")
            {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(s) = atom_to_string(at) {
                        return Some(s);
                    }
                }
            }
        }
        if let Some(c) = find_service_consist(sub) {
            return Some(c);
        }
    }
    None
}

/// Walk the AST and return every `TrItemId` listed under any `FailedSignals` block.
fn collect_failed_signals(ast: &Ast) -> Vec<u32> {
    let mut out = Vec::new();
    walk_failed_signals(ast, &mut out);
    out
}

fn walk_failed_signals(ast: &Ast, out: &mut Vec<u32>) {
    let Ast::List(items) = ast else { return };
    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("FailedSignals") {
            for child in items.iter().skip(1) {
                if let Ast::List(sub) = child {
                    if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                        if tag.eq_ignore_ascii_case("TrItemId") || tag.eq_ignore_ascii_case("UiD") {
                            if let Some(Ast::Atom(at)) = sub.get(1) {
                                if let Some(n) = atom_to_number(at) {
                                    if n >= 0.0 {
                                        out.push(n as u32);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return;
        }
    }
    for child in items {
        walk_failed_signals(child, out);
    }
}

/// Walk the AST collecting every `ActivityRestrictedSpeedZone` block.
fn collect_restricted_zones(ast: &Ast) -> Vec<RestrictedZone> {
    let mut out = Vec::new();
    walk_restricted_zones(ast, &mut out);
    out
}

fn walk_restricted_zones(ast: &Ast, out: &mut Vec<RestrictedZone>) {
    let Ast::List(items) = ast else { return };
    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("ActivityRestrictedSpeedZone")
            || head.eq_ignore_ascii_case("RestrictedSpeedZone")
        {
            if let Some(zone) = parse_restricted_zone(items) {
                out.push(zone);
            }
            return;
        }
    }
    for child in items {
        walk_restricted_zones(child, out);
    }
}

fn parse_restricted_zone(items: &[Ast]) -> Option<RestrictedZone> {
    let mut ids = Vec::with_capacity(2);
    let mut speed: Option<f64> = None;
    let mut position_start: Option<[f64; 4]> = None;
    let mut position_end: Option<[f64; 4]> = None;

    for child in items.iter().skip(1) {
        let Ast::List(sub) = child else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        let lower = tag.to_ascii_lowercase();
        match lower.as_str() {
            "zonestart" | "zoneend" | "tritemid" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        if n >= 0.0 {
                            ids.push(n as u32);
                        }
                    }
                }
            }
            "startposition" => {
                position_start = parse_position_tuple(sub);
            }
            "endposition" => {
                position_end = parse_position_tuple(sub);
            }
            "speedmps" | "maxspeedmps" | "maxspeed" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    speed = atom_to_number(at);
                }
            }
            "speedmph" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    speed = atom_to_number(at).map(|mph| mph * 0.44704);
                }
            }
            _ => {}
        }
    }

    let has_tr_items = !ids.is_empty();
    let has_positions = position_start.is_some() && position_end.is_some();
    if !has_tr_items && !has_positions {
        return None;
    }
    let item_id_start = ids.first().copied().unwrap_or(0);
    let item_id_end = ids.get(1).copied().unwrap_or(item_id_start);
    Some(RestrictedZone {
        item_id_start,
        item_id_end,
        max_speed_mps: speed.unwrap_or(0.0),
        position_start,
        position_end,
    })
}

fn parse_position_tuple(sub: &[Ast]) -> Option<[f64; 4]> {
    let coords = sub.get(1)?;
    let nums: Vec<f64> = match coords {
        Ast::List(inner) => inner
            .iter()
            .filter_map(|a| match a {
                Ast::Atom(at) => atom_to_number(at),
                _ => None,
            })
            .collect(),
        Ast::Atom(at) => {
            let mut out = vec![atom_to_number(at)?];
            out.extend(sub.iter().skip(2).filter_map(|a| match a {
                Ast::Atom(at) => atom_to_number(at),
                _ => None,
            }));
            out
        }
    };
    if nums.len() >= 4 {
        Some([nums[0], nums[1], nums[2], nums[3]])
    } else {
        None
    }
}

/// Walk the AST collecting every `ActivityObject` block.
fn collect_activity_objects(ast: &Ast) -> Vec<ActivityObjectDef> {
    let mut out = Vec::new();
    walk_activity_objects(ast, &mut out);
    out
}

fn walk_activity_objects(ast: &Ast, out: &mut Vec<ActivityObjectDef>) {
    let Ast::List(items) = ast else { return };
    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("ActivityObject") {
            if let Some(obj) = parse_activity_object(items) {
                out.push(obj);
            }
            return;
        }
    }
    for child in items {
        walk_activity_objects(child, out);
    }
}

fn parse_activity_object(items: &[Ast]) -> Option<ActivityObjectDef> {
    let mut item_id: Option<u32> = None;
    let mut workers: u32 = 0;
    let mut kind = String::new();

    for child in items.iter().skip(1) {
        let Ast::List(sub) = child else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        let lower = tag.to_ascii_lowercase();
        match lower.as_str() {
            "tritemid" | "uid" if item_id.is_none() => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        if n >= 0.0 {
                            item_id = Some(n as u32);
                        }
                    }
                }
            }
            "workers" | "population" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        if n >= 0.0 {
                            workers = n as u32;
                        }
                    }
                }
            }
            "objecttype" | "type" if kind.is_empty() => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(s) = atom_to_string(at) {
                        kind = s;
                    }
                }
            }
            "track_items" => {
                // Nested `( Track_Items <count> ( TrItemId <n> ) ... )`
                for inner in sub.iter().skip(1) {
                    let Ast::List(isub) = inner else { continue };
                    let Some(Ast::Atom(Atom::Symbol(itag))) = isub.first() else {
                        continue;
                    };
                    if itag.eq_ignore_ascii_case("TrItemId") && item_id.is_none() {
                        if let Some(Ast::Atom(at)) = isub.get(1) {
                            if let Some(n) = atom_to_number(at) {
                                if n >= 0.0 {
                                    item_id = Some(n as u32);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Some(ActivityObjectDef {
        item_id: item_id?,
        workers,
        kind,
    })
}

/// Walk the AST collecting every per-region override under `SoundRegions`.
fn collect_sound_regions(ast: &Ast) -> Vec<SoundRegionOverride> {
    let mut out = Vec::new();
    walk_sound_regions(ast, &mut out);
    out
}

fn walk_sound_regions(ast: &Ast, out: &mut Vec<SoundRegionOverride>) {
    let Ast::List(items) = ast else { return };
    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        if head.eq_ignore_ascii_case("SoundRegion")
            || head.eq_ignore_ascii_case("ActivitySoundRegion")
        {
            if let Some(region) = parse_sound_region(items) {
                out.push(region);
            }
            return;
        }
        if head.eq_ignore_ascii_case("SoundRegions") {
            for child in items.iter().skip(1) {
                if let Ast::List(sub) = child {
                    if let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() {
                        if tag.eq_ignore_ascii_case("SoundRegion")
                            || tag.eq_ignore_ascii_case("ActivitySoundRegion")
                        {
                            if let Some(region) = parse_sound_region(sub) {
                                out.push(region);
                            }
                        }
                    }
                }
            }
            return;
        }
    }
    for child in items {
        walk_sound_regions(child, out);
    }
}

fn parse_sound_region(items: &[Ast]) -> Option<SoundRegionOverride> {
    let mut tr_item_id: Option<u32> = None;
    let mut kind = String::new();
    let mut volume: f64 = 1.0;
    let mut radius_m: Option<f64> = None;

    for child in items.iter().skip(1) {
        let Ast::List(sub) = child else { continue };
        let Some(Ast::Atom(Atom::Symbol(tag))) = sub.first() else {
            continue;
        };
        let lower = tag.to_ascii_lowercase();
        match lower.as_str() {
            "tritemid" | "uid" if tr_item_id.is_none() => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        if n >= 0.0 {
                            tr_item_id = Some(n as u32);
                        }
                    }
                }
            }
            "soundregiontype" | "regiontype" | "kind" | "type" if kind.is_empty() => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(s) = atom_to_string(at) {
                        kind = s;
                    }
                }
            }
            "volume" | "basevolume" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        volume = n;
                    }
                }
            }
            "radiusm" | "radius" | "soundregionradius" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    if let Some(n) = atom_to_number(at) {
                        if n > 0.0 {
                            radius_m = Some(n);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Some(SoundRegionOverride {
        tr_item_id: tr_item_id?,
        kind,
        volume,
        radius_m,
    })
}
