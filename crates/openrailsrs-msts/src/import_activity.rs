//! Convert an MSTS Activity (`.act`) + Path (`.pat`) into an `openrailsrs` `scenario.toml`.

use std::collections::HashMap;
use std::path::Path;

use openrailsrs_formats::{
    ActivityFile, ActivityObjectDef, PathFile, SoundRegionOverride, TrItemKind, TrackDbFile,
    TrackNodeKind, TrafficServiceDef,
};
use openrailsrs_scenarios::model::{
    GameplaySection, ObjectiveKind, OutputSection, RouteSection, ScenarioFile, ScenarioMeta,
    SimulationSection, SoundRegionDef, StopDef, SwitchDef, TrainEntryDef, TrainSection,
};

use crate::error::MstsError;
use crate::path_placement::{placement_from_imported_route, read_distance_down_path};

/// Parse an MSTS `.act` file (and the `.pat` it references) and produce a
/// `scenario.toml` TOML string compatible with `openrailsrs-scenarios`.
///
/// `route_dir` is used to resolve the `.pat` path found inside the `.act`.
pub fn import_activity(route_dir: &Path, act_path: &Path) -> Result<String, MstsError> {
    import_activity_with_track(route_dir, act_path, None)
}

/// Import activity using optional imported `track.toml` directory for route placement.
pub fn import_activity_with_track(
    route_dir: &Path,
    act_path: &Path,
    imported_route_dir: Option<&Path>,
) -> Result<String, MstsError> {
    let (toml, _) = import_activity_with_summary(route_dir, act_path, imported_route_dir)?;
    Ok(toml)
}

/// Same as `import_activity` but also returns the activity name.
pub fn import_activity_with_summary(
    route_dir: &Path,
    act_path: &Path,
    imported_route_dir: Option<&Path>,
) -> Result<(String, String), MstsError> {
    let activity = ActivityFile::from_path(act_path)?;
    let pat_path = resolve_player_pat(route_dir, &activity);
    let path_file = PathFile::from_path(&pat_path)?;

    let service_id = service_id_for_player(&activity);
    let start_offset_m = service_id
        .as_deref()
        .and_then(|id| read_distance_down_path(route_dir, id))
        .unwrap_or(0.0);

    let (start_node, destination_node, route_switches, start_offset_m) =
        if let Some(track_dir) = imported_route_dir {
            match placement_from_imported_route(track_dir, &pat_path, start_offset_m) {
                Ok(hints) => (
                    hints.start,
                    hints.destination,
                    hints.switches,
                    hints.start_offset_m,
                ),
                Err(_) => {
                    let (s, d, sw) = fallback_route_nodes(&path_file);
                    (s, d, sw, start_offset_m)
                }
            }
        } else {
            let (s, d, sw) = fallback_route_nodes(&path_file);
            (s, d, sw, 0.0)
        };

    let start_offset_m = if imported_route_dir.is_some() {
        start_offset_m
    } else {
        0.0
    };

    // Use the consist path as-is; fall back to the player service `.srv` Train_Config name.
    let player_consist_str = resolve_player_consist(route_dir, &activity);

    // Duration: use the activity's duration, fallback to 2 hours.
    let duration_s = if activity.duration_s > 0.0 {
        activity.duration_s
    } else {
        7200.0
    };

    let extra_trains = build_extra_trains(route_dir, &activity, &player_consist_str);
    let stops = build_stops_from_objects(route_dir, &activity);
    let sound_regions = build_sound_regions(route_dir, &activity);

    let scenario = ScenarioFile {
        scenario: ScenarioMeta {
            name: activity.name.clone(),
            description: format!("Imported from MSTS activity: {}", act_path.display()),
            start_time_s: if activity.start_time_s > 0.0 {
                Some(activity.start_time_s)
            } else {
                None
            },
            season: activity.season.as_ref().map(|s| s.to_ascii_lowercase()),
        },
        route: RouteSection {
            path: ".".to_string(),
            start: start_node,
            destination: destination_node,
            start_offset_m: if start_offset_m > 0.0 {
                Some(start_offset_m)
            } else {
                None
            },
            stops,
            switches: route_switches,
        },
        train: TrainSection {
            consist: player_consist_str,
            davis: None,
            max_capacity: None,
        },
        gameplay: GameplaySection {
            objective: ObjectiveKind::Arrive,
            time_limit_seconds: None,
            difficulty: openrailsrs_scenarios::model::Difficulty::Normal,
            penalty_per_second_late: 0.0,
        },
        simulation: SimulationSection {
            duration: duration_s,
            time_step: 1.0,
            seed: 42,
        },
        output: OutputSection {
            csv: "run.csv".to_string(),
            metadata: "run.json".to_string(),
        },
        extra_trains,
        sound_regions,
        validate: None,
    };

    let toml = toml::to_string_pretty(&scenario)?;
    Ok((toml, activity.name))
}

fn fallback_route_nodes(path_file: &PathFile) -> (String, String, Vec<SwitchDef>) {
    let start = path_file
        .start_node()
        .map(|n| format!("n{n}"))
        .unwrap_or_else(|| "start".to_string());
    let destination = path_file
        .end_node()
        .map(|n| format!("n{n}"))
        .unwrap_or_else(|| "end".to_string());
    (start, destination, Vec::new())
}

/// Convert every parseable `Service_Definition` into a `[[extra_trains]]` entry.
/// Services whose `.pat` reference cannot be resolved are skipped silently.
fn build_extra_trains(
    route_dir: &Path,
    activity: &ActivityFile,
    fallback_consist: &str,
) -> Vec<TrainEntryDef> {
    let mut out = Vec::new();
    for (idx, svc) in activity.services.iter().enumerate() {
        let Some(entry) = build_one_extra_train(route_dir, svc, fallback_consist, idx) else {
            continue;
        };
        out.push(entry);
    }
    out
}

fn build_one_extra_train(
    route_dir: &Path,
    svc: &TrafficServiceDef,
    fallback_consist: &str,
    idx: usize,
) -> Option<TrainEntryDef> {
    let pat_path = resolve_asset_path(route_dir, &svc.path_file);
    let path_file = PathFile::from_path(&pat_path).ok()?;

    let start = path_file
        .start_node()
        .map(|n| format!("n{n}"))
        .unwrap_or_else(|| "start".to_string());
    let destination = path_file
        .end_node()
        .map(|n| format!("n{n}"))
        .unwrap_or_else(|| "end".to_string());

    let consist = svc
        .consist
        .as_deref()
        .map(sanitize_path)
        .unwrap_or_else(|| fallback_consist.to_string());

    let raw_id = if !svc.name.is_empty() {
        sanitize_id(&svc.name)
    } else {
        String::new()
    };
    let id = if raw_id.is_empty() {
        format!("svc{}", idx + 1)
    } else {
        raw_id
    };

    Some(TrainEntryDef {
        id: id.clone(),
        consist,
        start,
        destination,
        start_time_s: svc.start_time_s,
        stops: Vec::new(),
        davis: None,
        switches: Vec::new(),
        output_csv: format!("run_{id}.csv"),
    })
}

/// Build a TOML-friendly identifier from an arbitrary service name.
fn sanitize_id(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    mapped.trim_matches('_').to_string()
}

/// Resolve the player `.pat` from `Player_Path`, `PathID`, or `Player_Service_Definition`.
fn resolve_player_pat(route_dir: &Path, activity: &ActivityFile) -> std::path::PathBuf {
    if !activity.player_path.trim().is_empty() {
        return resolve_asset_path(route_dir, &activity.player_path);
    }
    if let Some(id) = service_id_for_player(activity) {
        return resolve_asset_path(route_dir, &format!("PATHS/{id}.pat"));
    }
    route_dir.to_path_buf()
}

/// Player consist path, or a sanitized `consists/<Train_Config>.con` from the service file.
fn resolve_player_consist(route_dir: &Path, activity: &ActivityFile) -> String {
    let direct = sanitize_path(&activity.player_consist);
    if !direct.is_empty() {
        return direct;
    }
    if let Some(name) = train_config_from_service(route_dir, activity) {
        return format!("consists/{}.con", sanitize_id(&name));
    }
    String::new()
}

fn service_id_for_player(activity: &ActivityFile) -> Option<String> {
    if let Some(id) = &activity.player_service_id {
        if !id.trim().is_empty() {
            return Some(id.trim().to_string());
        }
    }
    let path = activity.player_path.trim();
    if path.is_empty() {
        return None;
    }
    let normalized = path.replace('\\', "/");
    let stem = normalized
        .strip_prefix("PATHS/")
        .or_else(|| normalized.strip_prefix("paths/"))
        .unwrap_or(normalized.as_str());
    let stem = stem.strip_suffix(".pat").unwrap_or(stem);
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

fn train_config_from_service(route_dir: &Path, activity: &ActivityFile) -> Option<String> {
    let id = service_id_for_player(activity)?;
    ActivityFile::train_config_from_service(route_dir, &id)
}

/// Resolve an asset path that may use Windows backslashes and may be relative
/// to `route_dir`.
fn resolve_asset_path(base: &Path, asset: &str) -> std::path::PathBuf {
    let normalized = asset.trim().replace('\\', "/");
    base.join(&normalized)
}

/// Strip leading path separators and replace backslashes to make the consist
/// path suitable as a TOML string.
fn sanitize_path(s: &str) -> String {
    s.trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

/// Try to map every `ActivityObject` to a `[[route.stops]]` entry by resolving
/// the `TrItemId` to one of the endpoint nodes of its parent vector node.
///
/// If no `.tdb` is found in `route_dir`, or the item cannot be resolved, the
/// object is silently skipped (the user can edit `scenario.toml` manually).
fn build_stops_from_objects(route_dir: &Path, activity: &ActivityFile) -> Vec<StopDef> {
    if activity.activity_objects.is_empty() {
        return Vec::new();
    }
    let Some(tdb) = load_first_tdb(route_dir) else {
        return Vec::new();
    };

    let item_to_node = build_item_to_endpoint(&tdb);

    let mut out = Vec::new();
    let mut next_arrive = 600.0_f64;
    for (idx, obj) in activity.activity_objects.iter().enumerate() {
        let Some(node_id) = item_to_node.get(&obj.item_id) else {
            continue;
        };
        let arrive = next_arrive + (idx as f64) * 300.0;
        let depart = arrive + 60.0;
        out.push(stop_from_object(obj, node_id, arrive, depart));
        next_arrive = depart + 240.0;
    }
    out
}

fn stop_from_object(obj: &ActivityObjectDef, node_id: &str, arrive: f64, depart: f64) -> StopDef {
    let (passengers_on, passengers_off) =
        if obj.kind.eq_ignore_ascii_case("DropOffWagon") || obj.kind.eq_ignore_ascii_case("Drop") {
            (0u32, obj.workers)
        } else {
            (obj.workers, 0u32)
        };
    StopDef {
        node: node_id.to_string(),
        arrive_s: arrive,
        depart_s: depart,
        dwell_s: 60.0,
        passengers_on,
        passengers_off,
    }
}

/// Default radius applied to TDB sound sources that lack an activity override.
const DEFAULT_SOUND_REGION_RADIUS_M: f64 = 50.0;
/// Default base volume for ambient regions (matches a quiet idle layer).
const DEFAULT_SOUND_REGION_VOLUME: f32 = 0.4;

/// Combine `TDB SoundSourceItem`s with any `.act` overrides into the
/// `[[sound_regions]]` section of `scenario.toml`.
///
/// Items whose parent vector node never produced an edge (orphan refs) are
/// silently skipped, mirroring `build_stops_from_objects`.
fn build_sound_regions(route_dir: &Path, activity: &ActivityFile) -> Vec<SoundRegionDef> {
    let Some(tdb) = load_first_tdb(route_dir) else {
        return Vec::new();
    };

    let item_to_edge = build_item_to_edge(&tdb);
    let mut regions: Vec<SoundRegionDef> = Vec::new();

    for it in &tdb.items {
        if !matches!(it.kind, TrItemKind::SoundSource { .. }) {
            continue;
        }
        let Some(edge_id) = item_to_edge.get(&it.id) else {
            continue;
        };
        regions.push(SoundRegionDef {
            id: format!("sr{}", it.id),
            edge_id: edge_id.clone(),
            position_m: it.distance_m,
            radius_m: DEFAULT_SOUND_REGION_RADIUS_M,
            kind: "ambient".to_string(),
            base_volume: DEFAULT_SOUND_REGION_VOLUME,
        });
    }

    apply_sound_region_overrides(&mut regions, &activity.sound_regions);

    regions
}

/// Apply per-`TrItemId` activity overrides on top of TDB-defined sound regions.
fn apply_sound_region_overrides(regions: &mut [SoundRegionDef], overrides: &[SoundRegionOverride]) {
    if overrides.is_empty() {
        return;
    }
    for ov in overrides {
        let target_id = format!("sr{}", ov.tr_item_id);
        for region in regions.iter_mut() {
            if region.id != target_id {
                continue;
            }
            if !ov.kind.is_empty() {
                region.kind = ov.kind.to_ascii_lowercase();
            }
            if ov.volume > 0.0 {
                region.base_volume = ov.volume.clamp(0.0, 1.0) as f32;
            }
            if let Some(r) = ov.radius_m {
                if r > 0.0 {
                    region.radius_m = r;
                }
            }
        }
    }
}

/// Build a `TrItemId → "e{vector_node_id}"` map by replicating the edge id
/// scheme used by `import_route::convert_tdb_to_toml`.
fn build_item_to_edge(tdb: &TrackDbFile) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    for node in &tdb.nodes {
        let TrackNodeKind::Vector { item_ids, .. } = &node.kind else {
            continue;
        };
        let edge_id = format!("e{}", node.id);
        for item_id in item_ids {
            out.insert(*item_id, edge_id.clone());
        }
    }
    out
}

fn load_first_tdb(route_dir: &Path) -> Option<TrackDbFile> {
    let read = std::fs::read_dir(route_dir).ok()?;
    for entry in read {
        let Ok(e) = entry else { continue };
        let p = e.path();
        if p.extension()
            .map(|x| x.eq_ignore_ascii_case("tdb"))
            .unwrap_or(false)
        {
            return TrackDbFile::from_path(&p).ok();
        }
    }
    None
}

/// Build a `TrItemId → "n{end_node_id}"` map by walking every vector node and
/// projecting each referenced item onto the nearest endpoint pin.
fn build_item_to_endpoint(tdb: &TrackDbFile) -> HashMap<u32, String> {
    let mut item_distance: HashMap<u32, f64> = HashMap::new();
    for it in &tdb.items {
        item_distance.insert(it.id, it.distance_m);
    }

    let mut out = HashMap::new();
    for node in &tdb.nodes {
        let TrackNodeKind::Vector {
            length_m,
            pins,
            item_ids,
            ..
        } = &node.kind
        else {
            continue;
        };
        for item_id in item_ids {
            let dist = item_distance.get(item_id).copied().unwrap_or(0.0);
            let pin = if dist < length_m / 2.0 {
                pins.0
            } else {
                pins.1
            };
            out.insert(*item_id, format!("n{pin}"));
        }
    }
    out
}
