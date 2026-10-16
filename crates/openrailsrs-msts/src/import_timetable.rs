//! Native timetable service chains. Reused stock remains one physical formation.
use crate::{
    MstsError,
    path_placement::{
        native_path_polylines, pat_edge_path_with_offset, placement_for_pat_with_consist,
        snap_world_to_edge, switches_for_edges,
    },
};
use openrailsrs_formats::{
    ActivityFile, ConsistEntry, ConsistFile, PathFile, TrItemKind, TrackDbFile, TrackNodeKind,
    timetable::{NativeTimetable, timetable_commands, timetable_time_s},
};
use openrailsrs_scenarios::{
    ScenarioFile,
    model::{RouteLeg, TimetableAction, TimetableOperation},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

fn error(message: impl Into<String>) -> MstsError {
    MstsError::msg(message)
}
fn original(root: &Path, folder: &str, name: &str, extension: &str) -> Result<PathBuf, MstsError> {
    if name.is_empty()
        || name.len() > 256
        || name.contains(['/', '\\', ':'])
        || name == "."
        || name == ".."
    {
        return Err(error("Timetable resource must be an original filename"));
    }
    let filename = if Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        name.to_string()
    } else {
        format!("{name}.{extension}")
    };
    let path =
        openrailsrs_formats::resolve_path_case_insensitive(&root.join(folder).join(filename))
            .ok_or_else(|| error(format!("Original {folder} resource missing: {name}")))?;
    let canonical = path.canonicalize().map_err(|e| error(e.to_string()))?;
    let boundary = root.canonicalize().map_err(|e| error(e.to_string()))?;
    if !canonical.starts_with(boundary)
        || std::fs::metadata(&canonical)
            .map_err(|e| error(e.to_string()))?
            .len()
            > 8 * 1024 * 1024
    {
        return Err(error(
            "Timetable resource outside its package or over 8 MiB",
        ));
    }
    Ok(canonical)
}
fn stf_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
fn composition_parts(text: &str) -> Result<Vec<&str>, MstsError> {
    let mut depth = 0;
    let mut first = 0;
    let mut parts = vec![];
    for (index, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return Err(error("Invalid timetable consist brackets"));
                }
            }
            '+' if depth == 0 => {
                parts.push(text[first..index].trim());
                first = index + 1;
            }
            _ => (),
        }
    }
    if depth != 0 {
        return Err(error("Invalid timetable consist brackets"));
    }
    parts.push(text[first..].trim());
    Ok(parts)
}
/// Materialise a small CON descriptor referencing the user's original files.
/// Models, textures, cab views and sounds are never copied or modified.
pub fn prepare_timetable_consist(
    route: &Path,
    timetable: &Path,
    service: &str,
    output: &Path,
) -> Result<PathBuf, MstsError> {
    let table = NativeTimetable::from_path(timetable)?;
    let selected = table
        .services
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(service))
        .ok_or_else(|| error(format!("Native timetable service absent: {service}")))?;
    let content = route
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| error("No native content installation"))?;
    let stock = openrailsrs_formats::resolve_path_case_insensitive(&content.join("TRAINS"))
        .ok_or_else(|| error("Native TRAINS folder missing"))?;
    let boundary = stock.canonicalize().map_err(|e| error(e.to_string()))?;
    let mut entries = vec![];
    for part in composition_parts(&selected.consist)? {
        let name = part
            .split('$')
            .next()
            .unwrap()
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        let commands = timetable_commands(part)?;
        if commands
            .iter()
            .any(|c| c.name != "reverse" || c.value.is_some() || !c.qualifiers.is_empty())
        {
            return Err(error("Unsupported native consist command"));
        }
        let reverse = commands.iter().any(|c| c.name == "reverse");
        let path = original(&stock, "CONSISTS", name, "con")?;
        let text = openrailsrs_formats::read_msts_file_to_string(&path)?;
        let consist = ConsistFile::from_ast(&openrailsrs_formats::parse_vehicle_text(&text)?)?;
        let mut part_entries = vec![];
        for entry in consist.entries {
            let original = openrailsrs_train::from_ast::resolve_consist_entry_path(
                openrailsrs_train::consist_asset_root(&path),
                entry.path(),
            )
            .canonicalize()
            .map_err(|e| error(format!("{}: {e}", entry.path())))?;
            if !original.starts_with(&boundary) {
                return Err(error(
                    "Native stock reference leaves the original TRAINS installation",
                ));
            }
            part_entries.push((
                matches!(entry, ConsistEntry::Engine { .. }),
                original,
                entry.flipped() ^ reverse,
            ));
        }
        if reverse {
            part_entries.reverse();
        }
        entries.extend(part_entries);
    }
    if entries.is_empty() || entries.len() > 256 {
        return Err(error("Timetable formation must contain 1–256 vehicles"));
    }
    let mut descriptor = "(Train (TrainCfg (Name \"Native timetable formation\")\n".to_string();
    for (index, (engine, path, flip)) in entries.into_iter().enumerate() {
        descriptor += &format!(
            "({} {} (UiD {}) {})\n",
            if engine { "Engine" } else { "Wagon" },
            stf_string(&path.to_string_lossy()),
            index + 1,
            if flip { "(Flip)" } else { "" }
        );
    }
    descriptor += "))\n";
    std::fs::create_dir_all(output).map_err(|e| error(e.to_string()))?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in timetable
        .to_string_lossy()
        .bytes()
        .chain(service.bytes())
        .chain(descriptor.bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    let label = service
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let generated = output.join(format!("{label}-native-{hash:016x}.con"));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&generated)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(descriptor.as_bytes())
                .map_err(|e| error(e.to_string()))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata =
                std::fs::symlink_metadata(&generated).map_err(|e| error(e.to_string()))?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() != descriptor.len() as u64
                || std::fs::read(&generated).map_err(|e| error(e.to_string()))?
                    != descriptor.as_bytes()
            {
                return Err(error(
                    "Cached timetable formation changed; remove it and retry",
                ));
            }
        }
        Err(e) => return Err(error(e.to_string())),
    }
    generated.canonicalize().map_err(|e| error(e.to_string()))
}

fn append_station_operations(
    operations: &mut Vec<TimetableOperation>,
    commands: &[openrailsrs_formats::timetable::TimetableCommand],
    chainage_m: f64,
    reversed: bool,
    coupled: &mut usize,
    total: usize,
    parked: &mut Option<String>,
) -> Result<(), MstsError> {
    for command in commands {
        match command.name.as_str() {
            "hold" | "forcehold" if command.value.is_none() && command.qualifiers.is_empty() => (),
            "detach" if command.value.is_none() => {
                if parked.is_some()
                    || command
                        .qualifiers
                        .iter()
                        .any(|(q, _)| !matches!(q.as_str(), "units" | "static" | "manual" | "auto"))
                {
                    return Err(error(
                        "Native detach needs one static portion; formed trains, front portions and pools require another executor",
                    ));
                }
                let value = |key: &str| {
                    command
                        .qualifiers
                        .iter()
                        .find(|(q, _)| q == key)
                        .and_then(|(_, v)| v.as_deref())
                };
                let units = value("units")
                    .and_then(|s| s.parse::<i32>().ok())
                    .ok_or_else(|| error("Native static detach requires /units=N"))?;
                let count = units.unsigned_abs() as usize;
                let section = value("static")
                    .filter(|s| !s.is_empty() && s.len() <= 256)
                    .ok_or_else(|| error("Native static detach requires /static=NAME"))?;
                if count == 0 || count >= *coupled || (units > 0) != reversed {
                    return Err(error(
                        "Native detach must leave a driven prefix and a trailing static portion",
                    ));
                }
                if command
                    .qualifiers
                    .iter()
                    .filter(|(q, _)| q == "units")
                    .count()
                    != 1
                    || command
                        .qualifiers
                        .iter()
                        .filter(|(q, _)| q == "static")
                        .count()
                        != 1
                    || command
                        .qualifiers
                        .iter()
                        .any(|(q, v)| matches!(q.as_str(), "manual" | "auto") && v.is_some())
                {
                    return Err(error("Invalid or duplicate native detach qualifier"));
                }
                *coupled -= count;
                *parked = Some(section.into());
                operations.push(TimetableOperation {
                    chainage_m,
                    section: section.into(),
                    manual: command.qualifiers.iter().any(|(q, _)| q == "manual"),
                    action: TimetableAction::Detach { keep: *coupled },
                });
            }
            "pickup"
                if command.qualifiers.is_empty()
                    && command.value.as_ref().is_some_and(|target| {
                        parked
                            .as_ref()
                            .is_some_and(|p| p.eq_ignore_ascii_case(target))
                    }) =>
            {
                operations.push(TimetableOperation {
                    chainage_m,
                    section: parked.take().unwrap(),
                    manual: false,
                    action: TimetableAction::PickUp { count: total },
                });
                *coupled = total;
            }
            _ => {
                return Err(error(format!(
                    "Native station command ${} needs unsupported actors or qualifiers",
                    command.name
                )));
            }
        }
    }
    Ok(())
}

/// Import a selected original service and its `$forms` successors. Commands
/// requiring absent trains/pools remain explicit errors, never ignored logic.
pub fn import_native_timetable(
    route: &Path,
    timetable: &Path,
    service: &str,
    imported: &Path,
) -> Result<ScenarioFile, MstsError> {
    let consist = prepare_timetable_consist(route, timetable, service, imported)?;
    import_native_timetable_with_consist(route, timetable, service, imported, &consist)
}

/// Resolve a name in this original timetable. Foreign timetable files remain
/// explicit errors rather than silently choosing a similarly named service.
fn resolve_service<'a>(
    table: &'a NativeTimetable,
    reference: &str,
) -> Result<&'a openrailsrs_formats::timetable::NativeTimetableService, MstsError> {
    let name = reference
        .split_once(':')
        .map_or(reference, |(name, scope)| {
            if scope.eq_ignore_ascii_case(&table.description) {
                name
            } else {
                reference
            }
        });
    table
        .services
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(name))
        .ok_or_else(|| error(format!("Original timetable service not found: {reference}")))
}
// This is the import boundary for one native row and its consist/portion state.
#[allow(clippy::too_many_arguments)]
fn append_service_operations(
    operations: &mut Vec<TimetableOperation>,
    commands: &[openrailsrs_formats::timetable::TimetableCommand],
    chainage_m: f64,
    reversed: bool,
    coupled: &mut usize,
    total: usize,
    parked: &mut Option<String>,
    joined: &mut HashMap<String, usize>,
    mut resolve: impl FnMut(&str) -> Result<(String, usize), MstsError>,
) -> Result<(), MstsError> {
    for command in commands {
        if command.name == "pickup"
            && command.qualifiers.is_empty()
            && let Some(reference) = &command.value
            && !parked
                .as_ref()
                .is_some_and(|p| p.eq_ignore_ascii_case(reference))
        {
            let (service, count) = resolve(reference)?;
            if joined.contains_key(&service) || *coupled + count > 256 {
                return Err(error("Duplicate or oversized foreign timetable portion"));
            }
            joined.insert(service.clone(), count);
            *coupled += count;
            operations.push(TimetableOperation {
                chainage_m,
                section: service.clone(),
                manual: false,
                action: TimetableAction::JoinService {
                    service,
                    front: None,
                },
            });
        } else if command.name == "detach"
            && command.value.is_none()
            && command.qualifiers.iter().any(|(q, _)| q == "forms")
        {
            if command.qualifiers.iter().any(|(q, v)| {
                !matches!(q.as_str(), "forms" | "units" | "manual" | "auto")
                    || matches!(q.as_str(), "manual" | "auto") && v.is_some()
            }) {
                return Err(error("Unsupported foreign portion detach qualifier"));
            }
            let value = |key: &str| {
                command
                    .qualifiers
                    .iter()
                    .find(|(q, _)| q == key)
                    .and_then(|(_, v)| v.as_deref())
            };
            let reference =
                value("forms").ok_or_else(|| error("Foreign detach requires /forms=SERVICE"))?;
            let (service, count) = resolve(reference)?;
            let units = value("units")
                .and_then(|s| s.parse::<i32>().ok())
                .ok_or_else(|| error("Foreign detach requires signed /units=N"))?;
            if joined.get(&service) != Some(&count)
                || units.unsigned_abs() as usize != count
                || count >= *coupled
                || (units > 0) != reversed
            {
                return Err(error(
                    "Only a complete previously picked-up foreign end portion can form its original service; other splits require a new formed-service definition",
                ));
            }
            for key in ["forms", "units"] {
                if command.qualifiers.iter().filter(|(q, _)| q == key).count() != 1 {
                    return Err(error("Duplicate foreign detach qualifier"));
                }
            }
            joined.remove(&service);
            *coupled -= count;
            operations.push(TimetableOperation {
                chainage_m,
                section: service.clone(),
                manual: command.qualifiers.iter().any(|(q, _)| q == "manual"),
                action: TimetableAction::SplitService { service },
            });
        } else {
            append_station_operations(
                operations,
                std::slice::from_ref(command),
                chainage_m,
                reversed,
                coupled,
                total,
                parked,
            )?;
        }
    }
    Ok(())
}

pub fn import_native_timetable_with_consist(
    route: &Path,
    timetable: &Path,
    service: &str,
    imported: &Path,
    consist: &Path,
) -> Result<ScenarioFile, MstsError> {
    let mut scenario =
        import_one_native_timetable_with_consist(route, timetable, service, imported, consist)?;
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut seen: HashSet<String> = scenario
        .route
        .legs
        .iter()
        .filter_map(|l| l.service_id.as_ref().map(|id| id.to_ascii_lowercase()))
        .collect();
    let collect = |s: &ScenarioFile| {
        s.route
            .legs
            .iter()
            .flat_map(|l| &l.operations)
            .filter_map(|op| match &op.action {
                TimetableAction::JoinService { service, .. }
                | TimetableAction::SplitService { service } => Some(service.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    queue.extend(collect(&scenario));
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.to_ascii_lowercase()) {
            continue;
        }
        if seen.len() > 128 {
            return Err(error("Native timetable actor graph exceeds 128 services"));
        }
        let stock = prepare_timetable_consist(route, timetable, &id, imported)?;
        let actor =
            import_one_native_timetable_with_consist(route, timetable, &id, imported, &stock)?;
        queue.extend(collect(&actor));
        let start_clock = actor.scenario.start_time_s.unwrap_or(0.);
        let initial = scenario.scenario.start_time_s.unwrap_or(0.);
        let mut signed_departure = start_clock - initial;
        if signed_departure < -43200. {
            signed_departure += 86400.;
        }
        if signed_departure > 43200. {
            signed_departure -= 86400.;
        }
        let pre_roll = (-signed_departure).max(0.);
        if pre_roll > 21600. {
            return Err(error(
                "Foreign timetable service needs more than six hours of pre-run",
            ));
        }
        let departure = signed_departure.max(0.);
        scenario
            .extra_trains
            .push(openrailsrs_scenarios::TrainEntryDef {
                id: id.clone(),
                consist: actor.train.consist,
                start: actor.route.start,
                destination: actor.route.destination,
                start_time_s: departure,
                start_offset_m: actor.route.start_offset_m.unwrap_or(0.),
                relative_stop_times: true,
                waypoints: actor.route.waypoints,
                stops: actor.route.stops,
                switches: actor.route.switches,
                legs: actor.route.legs,
                systems: actor.train.systems,
                freight: actor.train.freight,
                cab_equipment: actor.train.cab_equipment,
                presentation: actor.train.presentation,
                electric_pickups: actor.train.electric_pickups,
                bogie_dynamics: actor.train.bogie_dynamics,
                rack: actor.train.rack,
                activity_uids: vec![],
                pre_roll_s: pre_roll,
                parked: false,
                davis: actor.train.davis,
                output_csv: format!("timetable-{}.csv", scenario.extra_trains.len()),
            });
    }
    install_mobile_rendezvous(&mut scenario, imported)?;
    scenario.validate().map_err(|e| error(e.to_string()))?;
    Ok(scenario)
}

fn import_one_native_timetable_with_consist(
    route: &Path,
    timetable: &Path,
    service: &str,
    imported: &Path,
    consist: &Path,
) -> Result<ScenarioFile, MstsError> {
    let table = NativeTimetable::from_path(timetable)?;
    let selected = table
        .services
        .iter()
        .find(|s| s.id.eq_ignore_ascii_case(service))
        .ok_or_else(|| error(format!("Native timetable service absent: {service}")))?;
    let consist = consist.canonicalize().map_err(|e| error(e.to_string()))?;
    let formation =
        openrailsrs_train::load_consist_with_asset_root(&consist, consist.parent().unwrap())
            .map_err(|e| error(e.to_string()))?;
    let length = formation.vehicle_lengths_m().iter().sum();
    let loaded =
        openrailsrs_route::load_route_from_dir(imported).map_err(|e| error(e.to_string()))?;
    let graph = &loaded.graph;
    let tdb = std::fs::read_dir(route)
        .map_err(|e| error(e.to_string()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tdb")))
        .ok_or_else(|| error("Original timetable TDB absent"))?;
    let tdb = TrackDbFile::from_path(tdb)?;
    let hosts: HashMap<_, _> = tdb
        .nodes
        .iter()
        .filter_map(|node| {
            if let TrackNodeKind::Vector { item_ids, .. } = &node.kind {
                Some(item_ids.iter().map(move |id| (*id, node.id)))
            } else {
                None
            }
        })
        .flatten()
        .collect();
    let initial_clock = timetable_time_s(selected.start.split('$').next().unwrap().trim())?;
    let first_path = original(
        route,
        "PATHS",
        selected.path.split('$').next().unwrap().trim(),
        "pat",
    )?;
    let activity = ActivityFile {
        name: selected.id.clone(),
        player_consist: consist.to_string_lossy().into_owned(),
        player_path: first_path.to_string_lossy().into_owned(),
        start_time_s: initial_clock,
        ..Default::default()
    };
    let (text, _, _) = crate::import_activity::import_activity_definition(
        route,
        timetable,
        Some(imported),
        Some(length),
        activity,
    )?;
    let mut scenario: ScenarioFile = toml::from_str(&text).map_err(|e| error(e.to_string()))?;
    let mut current = selected;
    let mut seen = HashSet::new();
    let mut legs: Vec<RouteLeg> = vec![];
    let mut coupled_count = formation.vehicles.len();
    let mut parked_section = None;
    let mut joined_services = HashMap::new();
    loop {
        if legs.len() >= 256 || !seen.insert(current.id.to_ascii_lowercase()) {
            return Err(error("Cyclic or oversized native timetable service chain"));
        }
        for (name, value) in &current.properties {
            let tag = name
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let commands = if tag == "#consist" {
                vec![]
            } else {
                timetable_commands(value)?
            };
            let supported = match tag.as_str() {
                "#consist" => true, // A $forms successor uses the incoming stock, as in OR.
                "#path" | "#start" => commands.is_empty(),
                "#dispose" => {
                    commands.len() <= 1
                        && commands.iter().all(|c| {
                            (c.name == "forms"
                                && c.value.is_some()
                                && c.qualifiers
                                    .iter()
                                    .all(|(q, v)| q == "atstation" && v.is_none()))
                                || (c.name == "static"
                                    && c.value.is_none()
                                    && c.qualifiers.is_empty())
                        })
                }
                _ => false,
            };
            if !supported {
                return Err(error(format!(
                    "Native service {} requires an unsupported property {name}: {value}",
                    current.id
                )));
            }
        }
        let path = original(
            route,
            "PATHS",
            current.path.split('$').next().unwrap().trim(),
            "pat",
        )?;
        let active_length = formation
            .vehicle_lengths_m()
            .iter()
            .take(coupled_count)
            .sum();
        let hints = placement_for_pat_with_consist(
            graph,
            &loaded.msts_aliases,
            &path,
            0.0,
            Some(active_length),
        )?;
        let file = PathFile::from_path(&path)?;
        if file.node_flags.iter().any(|flags| flags & 3 != 0) {
            return Err(error(format!(
                "Service {} has internal PAT reversals/waits; choose its ACT itinerary until timetable subpaths are supported",
                current.id
            )));
        }
        let mut directed = graph.clone();
        for switch in &hints.switches {
            directed
                .set_switch(
                    &switch.node,
                    match switch.position {
                        openrailsrs_scenarios::model::SwitchPositionDef::Straight => {
                            openrailsrs_track::SwitchPosition::Straight
                        }
                        openrailsrs_scenarios::model::SwitchPositionDef::Diverging => {
                            openrailsrs_track::SwitchPosition::Diverging
                        }
                    },
                )
                .map_err(|e| error(e.to_string()))?;
        }
        let edges = pat_edge_path_with_offset(
            &directed,
            &loaded.msts_aliases,
            &file,
            &hints.start,
            &hints.destination,
            hints.start_offset_m,
        )?;
        let mut end = edges
            .iter()
            .filter_map(|id| graph.edge(id))
            .map(|e| e.length_m)
            .sum();
        if file.pdps.len() >= 2
            && let Some(last) = file.pdps.last().and_then(|p| p.world)
        {
            let previous = file.pdps[file.pdps.len() - 2].world;
            let direction = previous.map(|p| {
                (
                    last.graph_x_m() - p.graph_x_m(),
                    last.graph_z_m() - p.graph_z_m(),
                )
            });
            let lines = native_path_polylines(graph, &loaded.msts_aliases, &path)?;
            let (from, offset, to) = snap_world_to_edge(
                graph,
                last.graph_x_m(),
                last.graph_z_m(),
                direction,
                lines.as_ref(),
            )?;
            let mut before = 0.0;
            for id in &edges {
                let edge = graph.edge(id).unwrap();
                if edge.from.0 == from && edge.to.0 == to {
                    end = before + offset;
                }
                before += edge.length_m;
            }
        }
        let reversed = legs.last().is_some_and(|previous| {
            let a = previous.edges.iter().rev().find_map(|id| graph.edge(id));
            let b = edges.iter().find_map(|id| graph.edge(id));
            let direction = |e: &openrailsrs_track::Edge| {
                let a = graph.node(&e.from.0).unwrap();
                let b = graph.node(&e.to.0).unwrap();
                (b.x_m - a.x_m, b.y_m - a.y_m)
            };
            let opposite = a.zip(b).is_some_and(|(a, b)| {
                let a = direction(a);
                let b = direction(b);
                a.0 * b.0 + a.1 * b.1 < 0.0
            });
            previous.reversed ^ opposite
        });
        let start = timetable_time_s(current.start.split('$').next().unwrap().trim())?;
        let elapsed = |clock: f64| {
            let delta = clock - initial_clock;
            if delta < -43200.0 {
                delta + 86400.0
            } else {
                delta.max(0.0)
            }
        };
        let mut records = vec![];
        for stop in &current.stops {
            let station = stop.station.split('$').next().unwrap().trim();
            let commands = timetable_commands(&stop.station)?
                .into_iter()
                .chain(timetable_commands(&stop.timing)?)
                .collect::<Vec<_>>();
            let timing = stop.timing.split('$').next().unwrap().trim();
            let (arrival, departure) = timing.split_once('-').unwrap_or((timing, timing));
            let arrival = timetable_time_s(arrival)?;
            let departure = timetable_time_s(departure)?;
            let mut candidates = vec![];
            let mut before = 0.0;
            for edge_id in &edges {
                let edge = graph.edge(edge_id).unwrap();
                for item in &tdb.items {
                    let TrItemKind::Platform {
                        station: name,
                        pair_id,
                        ..
                    } = &item.kind
                    else {
                        continue;
                    };
                    if !name.eq_ignore_ascii_case(station)
                        || hosts
                            .get(&item.id)
                            .is_none_or(|host| format!("e{host}") != edge_id.trim_end_matches("_r"))
                    {
                        continue;
                    }
                    let Some(pair) = tdb.items.iter().find(|i| i.id == *pair_id) else {
                        continue;
                    };
                    let pos = if edge_id.ends_with("_r") {
                        edge.length_m - item.distance_m.min(pair.distance_m)
                    } else {
                        item.distance_m.max(pair.distance_m)
                    };
                    let chainage = before + pos;
                    if chainage >= hints.start_offset_m - 1.0 {
                        candidates.push((chainage, item.id));
                    }
                }
                before += edge.length_m;
            }
            let (chainage, id) = candidates
                .into_iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .ok_or_else(|| {
                    error(format!(
                        "Native station {station} is absent from service {}'s PAT",
                        current.id
                    ))
                })?;
            records.push((
                chainage,
                openrailsrs_formats::typed::PlayerStationStop {
                    platform_start_id: id,
                    arrival_time_s: arrival,
                    departure_time_s: departure,
                    distance_down_path_m: chainage,
                },
                commands,
            ));
        }
        records.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut operations = vec![];
        for (chainage, _, commands) in &records {
            append_service_operations(
                &mut operations,
                commands,
                *chainage,
                reversed,
                &mut coupled_count,
                formation.vehicles.len(),
                &mut parked_section,
                &mut joined_services,
                |reference| {
                    let target = resolve_service(&table, reference)?;
                    let consist =
                        prepare_timetable_consist(route, timetable, &target.id, imported)?;
                    let stock = openrailsrs_train::load_consist_with_asset_root(
                        &consist,
                        consist.parent().unwrap(),
                    )
                    .map_err(|e| error(e.to_string()))?;
                    Ok((target.id.clone(), stock.vehicles.len()))
                },
            )?;
        }
        let activity = ActivityFile {
            start_time_s: initial_clock,
            player_stops: records.into_iter().map(|(_, s, _)| s).collect(),
            ..Default::default()
        };
        let mut sub = scenario.route.clone();
        sub.legs.clear();
        sub.start = hints.start.clone();
        sub.destination = hints.destination.clone();
        sub.start_offset_m = Some(hints.start_offset_m);
        sub.switches = switches_for_edges(graph, &edges);
        sub.waypoints = std::iter::once(hints.start.clone())
            .chain(edges.iter().map(|id| graph.edge(id).unwrap().to.0.clone()))
            .collect();
        sub.stops.clear();
        crate::native_timetable::apply(&activity, &tdb, graph, &mut sub)?;
        let dispose = timetable_commands(&current.dispose)?;
        let forms = dispose.iter().find(|c| c.name == "forms");
        if forms.is_some_and(|c| c.qualifiers.iter().any(|(q, _)| q == "atstation"))
            && let Some(last) = sub.stops.last()
        {
            let mut before = 0.0;
            for id in &edges {
                let edge = graph.edge(id).unwrap();
                before += edge.length_m;
                if edge.to.0 == last.node {
                    end = before + last.offset_m;
                    break;
                }
            }
        }
        legs.push(RouteLeg {
            operations,
            service_id: Some(current.id.clone()),
            not_before_s: Some(elapsed(start)),
            start: hints.start,
            destination: hints.destination,
            edges: edges.clone(),
            start_offset_m: hints.start_offset_m,
            end_chainage_m: end,
            reversed,
            wait_s: 0.0,
            stops: sub.stops,
            switches: switches_for_edges(graph, &edges),
        });
        let Some(forms) = forms else {
            break;
        };
        let target = forms
            .value
            .as_deref()
            .ok_or_else(|| error("Native $forms lacks a target"))?;
        current = table
            .services
            .iter()
            .find(|s| s.id.eq_ignore_ascii_case(target))
            .ok_or_else(|| error(format!("Original $forms successor missing: {target}")))?;
    }
    scenario.scenario.description = format!(
        "Horario original · {} · {} servicios enlazados; la formación conserva combustible, frenos y posición.",
        table.description,
        legs.len()
    );
    scenario.simulation.duration = 7200.0_f64.max(
        legs.iter()
            .filter_map(|leg| leg.not_before_s)
            .fold(0.0, f64::max)
            + 14400.0,
    );
    scenario.route.start.clone_from(&legs[0].start);
    scenario.route.destination.clone_from(&legs[0].destination);
    scenario.route.start_offset_m = Some(legs[0].start_offset_m);
    scenario.route.switches.clone_from(&legs[0].switches);
    scenario.route.waypoints.clear();
    scenario.route.stops.clone_from(&legs[0].stops);
    scenario.route.legs = legs;
    scenario.route.path = imported
        .canonicalize()
        .map_err(|e| error(e.to_string()))?
        .to_string_lossy()
        .into_owned();
    scenario.train.consist = consist.to_string_lossy().into_owned();
    scenario.extra_trains.clear();
    scenario.validate().map_err(|e| error(e.to_string()))?;
    Ok(scenario)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_detach_and_named_pickup_retain_authored_counts_and_reject_foreign_trains() {
        let mut operations = vec![];
        let mut count = 5;
        let mut parked = None;
        append_station_operations(
            &mut operations,
            &timetable_commands("09:00 $detach/units=-2/static=portion/manual").unwrap(),
            1000.0,
            false,
            &mut count,
            5,
            &mut parked,
        )
        .unwrap();
        assert_eq!(count, 3);
        assert!(operations[0].manual);
        assert!(
            append_station_operations(
                &mut operations,
                &timetable_commands("$pickup=another").unwrap(),
                1000.0,
                false,
                &mut count,
                5,
                &mut parked
            )
            .is_err()
        );
        append_station_operations(
            &mut operations,
            &timetable_commands("$pickup=PORTION").unwrap(),
            1000.0,
            false,
            &mut count,
            5,
            &mut parked,
        )
        .unwrap();
        assert_eq!(count, 5);
        assert!(parked.is_none());
        assert!(matches!(
            operations[1].action,
            TimetableAction::PickUp { count: 5 }
        ));
        assert!(
            append_station_operations(
                &mut operations,
                &timetable_commands("$detach/units=-2/forms=moving").unwrap(),
                2000.0,
                false,
                &mut count,
                5,
                &mut parked
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod portion_tests {
    use super::*;
    #[test]
    fn original_pickup_and_signed_forms_resolve_canonical_foreign_identity() {
        let mut operations = vec![];
        let mut count = 3;
        let mut parked = None;
        let mut joined = HashMap::new();
        append_service_operations(
            &mut operations,
            &timetable_commands("$pickup=sUr").unwrap(),
            240.,
            false,
            &mut count,
            3,
            &mut parked,
            &mut joined,
            |name| {
                assert!(name.eq_ignore_ascii_case("sur"));
                Ok(("Sur".into(), 3))
            },
        )
        .unwrap();
        assert_eq!(count, 6);
        assert!(
            matches!(&operations[0].action,TimetableAction::JoinService{service,front:None}if service=="Sur")
        );
        append_service_operations(
            &mut operations,
            &timetable_commands("$detach/units=-3/forms=sur/manual").unwrap(),
            240.,
            false,
            &mut count,
            3,
            &mut parked,
            &mut joined,
            |_| Ok(("Sur".into(), 3)),
        )
        .unwrap();
        assert_eq!(count, 3);
        assert!(joined.is_empty());
        assert!(operations[1].manual);
        assert!(
            matches!(&operations[1].action,TimetableAction::SplitService{service}if service=="Sur")
        );
    }
    #[test]
    fn missing_duplicate_and_partial_foreign_services_are_explicit_errors() {
        let mut operations = vec![];
        let mut count = 3;
        let mut parked = None;
        let mut joined = HashMap::new();
        assert!(
            append_service_operations(
                &mut operations,
                &timetable_commands("$pickup=missing").unwrap(),
                240.,
                false,
                &mut count,
                3,
                &mut parked,
                &mut joined,
                |_| Err(error("Original service missing"))
            )
            .is_err()
        );
        let pickup = timetable_commands("$pickup=Sur").unwrap();
        append_service_operations(
            &mut operations,
            &pickup,
            240.,
            false,
            &mut count,
            3,
            &mut parked,
            &mut joined,
            |_| Ok(("Sur".into(), 3)),
        )
        .unwrap();
        assert!(
            append_service_operations(
                &mut operations,
                &pickup,
                240.,
                false,
                &mut count,
                3,
                &mut parked,
                &mut joined,
                |_| Ok(("Sur".into(), 3))
            )
            .is_err()
        );
        assert!(
            append_service_operations(
                &mut operations,
                &timetable_commands("$detach/units=-2/forms=Sur").unwrap(),
                240.,
                false,
                &mut count,
                3,
                &mut parked,
                &mut joined,
                |_| Ok(("Sur".into(), 3))
            )
            .is_err()
        );
        assert!(
            append_service_operations(
                &mut operations,
                &timetable_commands("$attach=Sur").unwrap(),
                240.,
                false,
                &mut count,
                3,
                &mut parked,
                &mut joined,
                |_| Ok(("Sur".into(), 3))
            )
            .is_err()
        );
    }
}

/// Convert original foreign `$pickup` operations into physical, timed crew
/// handshakes for both the selected train and its imported AI actors. The
/// original CON/PAT/TT files and operation rows remain unchanged.
fn install_mobile_rendezvous(
    scenario: &mut ScenarioFile,
    imported: &Path,
) -> Result<(), MstsError> {
    use openrailsrs_scenarios::operational_yards::RendezvousDef;
    let loaded =
        openrailsrs_route::load_route_from_dir(imported).map_err(|e| error(e.to_string()))?;
    let actors = std::iter::once(("Jugador".to_string(), scenario.route.legs.clone(), 0.))
        .chain(
            scenario
                .extra_trains
                .iter()
                .map(|a| (a.id.clone(), a.legs.clone(), a.start_time_s - a.pre_roll_s)),
        )
        .collect::<Vec<_>>();
    for (receiver, legs, offset) in actors {
        for (leg_i, leg) in legs.iter().enumerate() {
            for (op_i, op) in leg.operations.iter().enumerate() {
                let TimetableAction::JoinService {
                    service: donor,
                    front,
                } = &op.action
                else {
                    continue;
                };
                let mut before = 0.;
                let mut placement = None;
                for id in &leg.edges {
                    let edge = loaded.graph.edge(id).ok_or_else(|| {
                        error("Foreign pickup has a missing original track vector")
                    })?;
                    if op.chainage_m <= before + edge.length_m + 0.001 {
                        let p = op.chainage_m - before;
                        placement = Some((
                            id.trim_end_matches("_r").to_string(),
                            if id.ends_with("_r") {
                                edge.length_m - p
                            } else {
                                p
                            },
                        ));
                        break;
                    }
                    before += edge.length_m;
                }
                let (edge, position_m) = placement
                    .ok_or_else(|| error("Foreign pickup marker is beyond its original path"))?;
                let not_before_s = (offset + leg.not_before_s.unwrap_or(0.)).max(0.);
                scenario
                    .simulation
                    .yard_operations
                    .rendezvous
                    .push(RendezvousDef {
                        id: format!(
                            "tt-{}-{leg_i}-{op_i}",
                            scenario.simulation.yard_operations.rendezvous.len()
                        ),
                        receiver: receiver.clone(),
                        donor: donor.clone(),
                        edge,
                        position_m,
                        not_before_s,
                        max_wait_s: 21600.,
                        front: front.unwrap_or(false),
                        receiver_leg: Some(leg_i),
                    });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod mobile_handshake_import_tests {
    use super::*;
    #[test]
    fn native_pickup_commands_produce_physical_handshakes_for_player_and_ai() {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/operational_yards");
        let mut scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario_meeting.toml")).unwrap();
        let native=NativeTimetable::from_text(";Receiver;Incoming\n#path;main;main\n#consist;loco;loco\n#start;09:00;09:00\nMeeting;09:04 $pickup=Incoming;09:04\n#dispose;$static;$static\n").unwrap();
        let mut operations = vec![];
        let mut coupled = 1;
        let mut parked = None;
        let mut joined = HashMap::new();
        append_service_operations(
            &mut operations,
            &timetable_commands(&native.services[0].stops[0].timing).unwrap(),
            450.,
            false,
            &mut coupled,
            1,
            &mut parked,
            &mut joined,
            |reference| Ok((reference.to_lowercase(), 1)),
        )
        .unwrap();
        scenario.extra_trains[0].legs[0].operations = operations;
        scenario.simulation.yard_operations.rendezvous.clear();
        install_mobile_rendezvous(&mut scenario, &directory.join("meeting")).unwrap();
        let handshakes = &scenario.simulation.yard_operations.rendezvous;
        assert_eq!(handshakes.len(), 1);
        let meeting = &handshakes[0];
        assert_eq!(
            (&meeting.receiver, &meeting.donor),
            (&"receiver".to_string(), &"incoming".to_string())
        );
        assert_eq!(meeting.edge, "approach");
        assert_eq!(meeting.position_m, 450.);
        assert_eq!(meeting.receiver_leg, Some(0));
        assert!(scenario.validate().is_ok());
    }
}
