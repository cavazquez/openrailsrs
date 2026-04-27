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
        let player_path = find_string_field(ast, &["Player_Path"]).unwrap_or_default();
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
        Self::from_ast(&ast)
    }
}

/// Recursively find the first string value of a field with any of the given names.
fn find_string_field(ast: &Ast, names: &[&str]) -> Option<String> {
    let Ast::List(items) = ast else { return None };

    if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
        for n in names {
            if head.eq_ignore_ascii_case(n) {
                // Return first string/symbol child.
                if let Some(Ast::Atom(a)) = items.get(1) {
                    if let Some(s) = atom_to_string(a) {
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
            "speedmps" | "maxspeedmps" | "maxspeed" => {
                if let Some(Ast::Atom(at)) = sub.get(1) {
                    speed = atom_to_number(at);
                }
            }
            _ => {}
        }
    }

    if ids.is_empty() || speed.is_none() {
        return None;
    }
    let item_id_start = ids[0];
    let item_id_end = *ids.get(1).unwrap_or(&item_id_start);
    Some(RestrictedZone {
        item_id_start,
        item_id_end,
        max_speed_mps: speed.unwrap_or(0.0),
    })
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
