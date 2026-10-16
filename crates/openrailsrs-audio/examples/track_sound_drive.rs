//! Read-only original SCE formation, real Rust motion and device-free PCM.
use openrailsrs_audio::mix::SoundGroup;
use openrailsrs_audio::native::{
    ConsistSoundSpec, SoundFrame, SoundState, TrainSoundFrame, render_oracle,
};
use openrailsrs_formats::{
    Ast, Atom, ConsistEntry, ConsistFile, typed::track_sound::TrackSoundTypes,
};
use openrailsrs_sim::{LiveDriveSession, path_data::PathData};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn hash(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}
fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

#[derive(Clone)]
struct Observation {
    source_time_s: f64,
    phase: usize,
    head_m: f64,
    speed_mps: f64,
    raw: Vec<Option<u32>>,
    effective: Vec<u32>,
    states: Vec<SoundState>,
}
struct Probe {
    at: f64,
    value: Option<u32>,
    reverse: bool,
}
fn raw_types(session: &LiveDriveSession) -> Result<Vec<Option<u32>>> {
    (0..session.formation.cars.len())
        .map(|i| {
            let offset = session.formation.center_offset_m(i);
            let (path, _) = session.presentation_path_for_car(offset);
            let at = session.presentation_chainage_for_car(offset, offset, 0.);
            Ok(session
                .graph
                .track_sound_type_at_chainage(path, at, session.driver_direction < 0.5))
        })
        .collect()
}
fn observe(session: &LiveDriveSession, phase: usize, held: &mut [u32]) -> Result<Observation> {
    // Native car audio retains its last type below this motion threshold.
    let raw = if session.velocity_mps() > 0.1 {
        raw_types(session)?
    } else {
        vec![None; held.len()]
    };
    let mut states = vec![];
    for (i, value) in raw.iter().enumerate() {
        if let Some(t) = value {
            held[i] = *t;
        }
        let car = &session.formation.cars[i];
        let diesel = session.state.diesel.car(i);
        let powered = car.powered
            && car.power_on
            && car.battery_on
            && (i == session.cab_vehicle() || car.mu_connected)
            && session.state.electric.power_available(i)
            && session.state.diesel.power_available(i);
        let rpm_fraction = session
            .physics
            .diesel
            .cars
            .iter()
            .find(|c| c.vehicle == i)
            .zip(diesel)
            .map_or(0., |(c, d)| {
                (d.rpm - c.governor.idle_rpm) / (c.governor.max_rpm - c.governor.idle_rpm).max(1.)
            });
        let cylinder = session.state.brake_system.cylinders.get(i);
        states.push(SoundState {
            speed: session.velocity_mps() as f32,
            distance: session.state.odometer_m as f32,
            track_type: *value,
            variable1: if powered {
                session.driver_throttle as f32
            } else {
                0.
            },
            variable2: rpm_fraction.clamp(0., 1.) as f32,
            throttle: if powered {
                session.driver_throttle as f32
            } else {
                0.
            },
            brake: session.driver_brake as f32,
            direction: session.driver_direction as f32,
            engine_on: diesel.map(|d| d.command_running && d.fuel_l > 0. && car.battery_on),
            brake_cylinder: cylinder.map_or(0., |c| c.pressure_bar() * 14.503774) as f32,
            brake_pipe: cylinder.and_then(|c| c.pipe_pressure_bar()).unwrap_or(0.) as f32,
            ..Default::default()
        });
    }
    Ok(Observation {
        source_time_s: session.time_s(),
        phase,
        head_m: session.head_chainage_m(),
        speed_mps: session.velocity_mps(),
        raw,
        effective: held.to_vec(),
        states,
    })
}
fn frame(row: &Observation, time: f64, paused: bool, seed: bool) -> SoundFrame {
    let mut states = row.states.clone();
    if seed {
        for (s, t) in states.iter_mut().zip(&row.effective) {
            s.track_type = Some(*t);
        }
    }
    SoundFrame {
        time_s: time,
        paused,
        volume: 0.35,
        presentation_dt_s: 0.05,
        trains: vec![TrainSoundFrame {
            id: 0,
            state: states[0],
            distance_m: 12.,
            vehicle_distances_m: vec![12.; states.len()],
            vehicle_states: states,
            vehicle_placement: vec![],
        }],
        ..Default::default()
    }
}
fn sounds(ast: &Ast, out: &mut Vec<String>) {
    if let Ast::List(v) = ast {
        if matches!(v.first(),Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case("Sound"))
            && let Some(Ast::Atom(Atom::String(s) | Atom::Symbol(s))) = v.get(1)
        {
            out.push(s.clone());
        }
        for child in v {
            sounds(child, out);
        }
    }
}
fn resolve(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .find_map(|d| {
            // Match Windows lexical parents when a local Sound folder is
            // absent but the authored program points to common.sound.
            let mut normalized = PathBuf::new();
            for part in d.join(name.replace('\\', "/")).components() {
                match part {
                    std::path::Component::CurDir => {}
                    std::path::Component::ParentDir => {
                        normalized.pop();
                    }
                    other => normalized.push(other),
                }
            }
            openrailsrs_formats::resolve_path_case_insensitive(&normalized).filter(|p| p.is_file())
        })
        .filter(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}
fn inventory(consist: &Path, route: &Path, catalog: &TrackSoundTypes) -> Result<Value> {
    let content = route
        .parent()
        .and_then(Path::parent)
        .ok_or("route content root")?;
    let ast = openrailsrs_formats::read_vehicle_ast(consist)?;
    let entries = ConsistFile::from_ast(&ast)?.entries;
    let mut files = BTreeMap::new();
    let mut missing = BTreeMap::<String, BTreeSet<String>>::new();
    let mut selected_programs = vec![];
    let mut selected_exterior_missing = 0;
    files.insert(consist.canonicalize()?, hash(consist)?);
    for (vehicle, entry) in entries.into_iter().enumerate() {
        let rel = match entry {
            ConsistEntry::Engine { path, .. } | ConsistEntry::Wagon { path, .. } => path,
        };
        let stock = openrailsrs_train::resolve_consist_entry_path(
            openrailsrs_train::consist_asset_root(consist),
            &rel,
        );
        files.insert(stock.canonicalize()?, hash(&stock)?);
        let root = stock.parent().ok_or("stock directory")?;
        let active_stock = stock
            .file_name()
            .and_then(|name| {
                openrailsrs_formats::resolve_path_case_insensitive(
                    &root.join("OpenRails").join(name),
                )
            })
            .filter(|p| p.is_file())
            .unwrap_or_else(|| stock.clone());
        files.insert(active_stock.canonicalize()?, hash(&active_stock)?);
        let dirs = vec![
            root.join("Sound"),
            root.into(),
            route.join("SOUND"),
            content.join("SOUND"),
        ];
        let mut stock_refs = vec![];
        sounds(
            &openrailsrs_formats::read_vehicle_ast(&active_stock)?,
            &mut stock_refs,
        );
        let mut refs: Vec<_> = stock_refs.into_iter().map(|s| (s, None, "stock")).collect();
        refs.extend(
            catalog
                .types
                .iter()
                .enumerate()
                .flat_map(|(t, s)| {
                    [
                        (s.inside.clone(), Some(t as u32), "inside"),
                        (s.outside.clone(), Some(t as u32), "outside"),
                    ]
                })
                .filter(|(s, _, _)| !s.is_empty()),
        );
        for (name, track_type, view) in refs {
            let selected_region = track_type.is_some_and(|t| [0, 3, 7].contains(&t));
            let selected_exterior = selected_region && view == "outside";
            let category = match (track_type, selected_region, view) {
                (None, _, _) => "stock_program_reference_not_all_events_selected",
                (_, false, _) => "unselected_region_catalog",
                (_, true, "inside") => "selected_region_interior_not_selected_by_exterior_clips",
                _ => "selected_region_exterior",
            };
            let context =
                format!("vehicle={vehicle}; category={category}; type={track_type:?}; view={view}");
            let Some(sms) = resolve(&dirs, &name) else {
                missing
                    .entry(format!("SMS: {name}"))
                    .or_default()
                    .insert(context);
                selected_exterior_missing += usize::from(selected_exterior);
                continue;
            };
            files.insert(sms.clone(), hash(&sms)?);
            let program = openrailsrs_audio::sms::SmsProgram::from_path(&sms)
                .map_err(|e| format!("{}: {e}", sms.display()))?;
            let mut resources = BTreeSet::new();
            for command in program
                .streams
                .iter()
                .flat_map(|s| &s.triggers)
                .flat_map(|t| &t.commands)
            {
                if let openrailsrs_audio::sms::Command::Play { files: names, .. } = command {
                    resources.extend(names.iter().cloned());
                }
            }
            let search = vec![
                sms.parent().unwrap().into(),
                root.join("Sound"),
                route.join("SOUND"),
                content.join("SOUND"),
            ];
            let mut wavs = vec![];
            for name in resources {
                if let Some(wav) = resolve(&search, &name) {
                    let sha = hash(&wav)?;
                    files.insert(wav.clone(), sha.clone());
                    wavs.push(json!({"resource":name,"path":wav,"sha256":sha,"missing":false}));
                } else {
                    missing
                        .entry(format!("WAV: {name} ({})", sms.display()))
                        .or_default()
                        .insert(context.clone());
                    selected_exterior_missing += usize::from(selected_exterior);
                    wavs.push(json!({"resource":name,"missing":true}));
                }
            }
            if selected_region {
                selected_programs.push(
                    json!({"vehicle":vehicle,"track_type":track_type,"view":view,
                    "sms_path":sms,"sms_sha256":files[&sms],"wavs":wavs}),
                );
            }
        }
    }
    Ok(
        json!({"files":files.iter().map(|(p,h)|json!({"path":p,"sha256":h})).collect::<Vec<_>>(),
            "missing_unique_count":missing.len(),"missing":missing.keys().collect::<Vec<_>>(),
            "missing_contexts":missing,"selected_region_programs":selected_programs,
            "selected_exterior_rolling_missing":selected_exterior_missing,
            "missing_scope":"All authored references are inventoried; absence in stock events or unselected catalog entries is not a missing selected rolling voice. Active exterior rolling voices are checked individually in each clip."}),
    )
}
fn pcm_stats(path: &Path) -> Result<Value> {
    let b = fs::read(path)?;
    if &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("Invalid PCM header".into());
    }
    let samples: Vec<i16> = b[44..]
        .chunks_exact(2)
        .map(|s| i16::from_le_bytes([s[0], s[1]]))
        .collect();
    let nonzero = samples.iter().filter(|s| **s != 0).count();
    Ok(
        json!({"sha256":hash(path)?,"bytes":b.len(),"format":"PCM signed16 stereo 44100Hz",
        "frames":samples.len()/2,"nonzero_samples":nonzero,"peak":samples.iter().map(|s|i32::from(*s).abs()).max().unwrap_or(0)}),
    )
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 2 {
        return Err("track_sound_drive ORIGINAL_CONTENT_ROOT FRESH_OUTPUT_DIRECTORY".into());
    }
    let content = args[0].canonicalize()?;
    let out = std::env::current_dir()?.join(&args[1]);
    if out.exists() || out.starts_with(&content) {
        return Err("Use a fresh output outside original content".into());
    }
    fs::create_dir_all(&out)?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let example = repo.join("crates/openrailsrs-audio/examples/track_sound_drive.rs");
    let source_before = hash(&example)?;
    let protocol = repo.join("docs/fixtures/compatibility/track-sound-drive/protocol.json");
    fs::copy(&protocol, out.join("protocol.json"))?;
    let fixture = repo.join("docs/fixtures/compatibility/track-sound-or161");
    let native = read_json(&fixture.join("reference.json"))?;
    let manifest = read_json(&fixture.join("manifest.json"))?;
    if hash(&fixture.join("reference.json"))? != manifest["output_sha256"] {
        return Err("Native reference changed".into());
    }
    let route = content.join("ROUTES/SCE");
    for (file, expected) in manifest["original_content"]
        .as_object()
        .ok_or("Native content hashes")?
    {
        if hash(&route.join(file))? != expected.as_str().ok_or("hash")? {
            return Err(format!("Original changed: {file}").into());
        }
    }
    let consist = content.join("TRAINS/CONSISTS/MT_MT_Class 47 & 6 mk2 PP.con");
    let loaded_consist = openrailsrs_train::load_consist_with_asset_root(
        &consist,
        openrailsrs_train::consist_asset_root(&consist),
    )?;
    let catalog = TrackSoundTypes::from_path(&route.join("TTYPE.DAT"))?;
    let derived = out.join("route");
    fs::create_dir(&derived)?;
    fs::write(
        derived.join("track.toml"),
        openrailsrs_msts::import_route(&route)?,
    )?;
    let graph = openrailsrs_route::load_track_graph_from_route_dir(&derived)?;
    let native_rows = native["rows"].as_array().ok_or("Native rows")?;
    let path: Vec<String> = serde_json::from_value(native_rows[0]["path"].clone())?;
    let pd = PathData::from_path(&path, &graph);
    // Keep comparisons inside constant native-probe intervals. Original OR's
    // only extra switch inside a boundary gap is its ten-metre hold threshold.
    let mut decision_points = vec![];
    let mut edge_start = 0.;
    for edge in &pd.edges {
        for b in &edge.profile.track_sound_regions {
            let at = edge_start + b.position_m;
            decision_points.extend([at - 10., at, at + 10.]);
        }
        edge_start += edge.length_m;
    }
    let mut probes = vec![];
    for r in native_rows
        .iter()
        .filter(|r| r["path"] == native_rows[0]["path"])
    {
        let index = path
            .iter()
            .position(|e| Some(e.as_str()) == r["edge"].as_str())
            .ok_or("Native edge")?;
        probes.push(Probe {
            at: pd.chainage_at_edge_position(
                index,
                r["position_m"].as_f64().ok_or("probe position")?,
            ),
            value: r["result"].as_u64().map(|n| n as u32),
            reverse: r["reverse"].as_bool().ok_or("direction")?,
        });
    }
    probes.sort_by(|a, b| a.at.total_cmp(&b.at));
    let min_probe = probes.first().ok_or("Probes")?.at;
    let max_probe = probes.last().unwrap().at;
    let length = loaded_consist.vehicle_lengths_m().iter().sum::<f64>();
    let start_head = min_probe - length - 80.;
    let forward_target = max_probe + length + 15.;
    let mut scenario = openrailsrs_scenarios::load_scenario(
        repo.join("examples/class47_reference/scenario.toml"),
    )?;
    scenario.scenario.name = "SCE original Class47: per-car rolling sound CPU acceptance".into();
    scenario.route.path = derived.to_string_lossy().into();
    scenario.route.start = graph.edge(&path[0]).unwrap().from.0.clone();
    scenario.route.destination = graph.edge(path.last().unwrap()).unwrap().to.0.clone();
    scenario.route.waypoints.clear();
    scenario.route.switches.clear();
    scenario.route.stops.clear();
    scenario.route.start_offset_m = Some(start_head);
    scenario.route.assume_signals_clear = true;
    scenario.route.legs = vec![openrailsrs_scenarios::model::RouteLeg {
        switches: vec![],
        operations: vec![],
        service_id: None,
        not_before_s: None,
        start: scenario.route.start.clone(),
        destination: scenario.route.destination.clone(),
        edges: path.clone(),
        start_offset_m: start_head,
        end_chainage_m: pd.edges.iter().map(|e| e.length_m).sum(),
        reversed: false,
        wait_s: 0.,
        stops: vec![],
    }];
    scenario.train.consist = consist.to_string_lossy().into();
    scenario.simulation.duration = 600.;
    scenario.simulation.multi_body = false;
    scenario.simulation.native_physics = Some(openrailsrs_scenarios::NativePhysicsEnvironment {
        simple_control_physics: true,
        ambient_c: 20.,
        adhesion_factor: 1.,
        paused_weather: true,
        first_slippery_spot: None,
    });
    scenario.simulation.brake_shoe_speed_factor = false;
    scenario.simulation.brake_skid_limit = false;
    fs::write(
        out.join("scenario.toml"),
        toml::to_string_pretty(&scenario)?,
    )?;
    let mut session = LiveDriveSession::from_scenario(&out, &scenario)?;
    session.driver_direction = 1.;
    session.driver_brake = 0.;
    let mut held = vec![0; loaded_consist.vehicles.len()];
    let mut trace = vec![];
    let mut checks = vec![0usize; held.len()];
    let mut checks_by_type = vec![BTreeMap::<String, usize>::new(); held.len()];
    let mut checks_by_direction_type = vec![BTreeMap::<String, usize>::new(); held.len()];
    let mut observed = vec![BTreeSet::new(); held.len()];
    let mut split_rows = vec![];
    let mut save_checked = false;
    let mut phase = 0;
    let mut stopped_forward = None;
    let mut pause_observation = None;
    for tick in 0..12_000 {
        match phase {
            0 | 2 => {
                session.driver_throttle = if session.velocity_mps() < 8. {
                    0.65
                } else {
                    0.05
                };
                session.driver_brake = 0.;
            }
            _ => {
                session.driver_throttle = 0.;
                session.driver_brake = 1.;
            }
        }
        session.step_realtime(0.05, |_| {});
        let row = observe(&session, phase, &mut held)?;
        for (i, value) in row.effective.iter().enumerate() {
            observed[i].insert(*value);
        }
        let mut types = BTreeSet::new();
        for value in &row.effective {
            types.insert(*value);
        }
        if types.len() > 1 && row.speed_mps > 0.5 {
            split_rows.push(trace.len());
        }
        for (i, offset) in (0..held.len()).map(|i| (i, session.formation.center_offset_m(i))) {
            if row.speed_mps <= 0.1 {
                continue;
            }
            let at = session.presentation_chainage_at_offset(offset, 0.);
            let reverse = session.driver_direction < 0.5;
            let same: Vec<_> = probes.iter().filter(|p| p.reverse == reverse).collect();
            if let Some(pair) = same.windows(2).find(|p| {
                p[0].at + 0.001 < at
                    && at < p[1].at - 0.001
                    && p[0].value == p[1].value
                    && !decision_points.iter().any(|x| p[0].at < *x && *x < p[1].at)
            }) {
                if row.raw[i] != pair[0].value {
                    return Err(format!(
                        "Per-car native interval mismatch at tick{tick} car{i}: {:?} != {:?}",
                        row.raw[i], pair[0].value
                    )
                    .into());
                }
                checks[i] += 1;
                *checks_by_type[i]
                    .entry(pair[0].value.map_or("hold".into(), |t| t.to_string()))
                    .or_default() += 1;
                let direction = if reverse { "reverse" } else { "forward" };
                let kind = pair[0].value.map_or("hold".into(), |t| t.to_string());
                *checks_by_direction_type[i]
                    .entry(format!("{direction}:{kind}"))
                    .or_default() += 1;
            }
        }
        if !save_checked && types.len() > 1 && row.speed_mps > 0.5 {
            let before = serde_json::to_value(session.snapshot())?;
            for _ in 0..20 {
                session.step_realtime(0., |_| {});
            }
            if before != serde_json::to_value(session.snapshot())? {
                return Err("Zero-delta paused drive changed state".into());
            }
            let mut restored = LiveDriveSession::from_scenario(&out, &scenario)?;
            restored.restore_snapshot(serde_json::from_value(before.clone())?)?;
            if raw_types(&restored)? != row.raw {
                return Err("Track types changed on restore".into());
            }
            write_json(&out.join("paused-snapshot.json"), &before)?;
            session = restored;
            save_checked = true;
            pause_observation = Some(row.clone());
        }
        trace.push(row);
        if phase == 0 && session.head_chainage_m() >= forward_target {
            phase = 1;
        } else if phase == 1 && session.velocity_mps() < 0.01 {
            stopped_forward = Some(session.head_chainage_m());
            session.set_direction(0.)?;
            phase = 2;
        } else if phase == 2 && session.head_chainage_m() < min_probe - 40. {
            phase = 3;
        } else if phase == 3 && session.velocity_mps() < 0.01 {
            phase = 4;
            break;
        }
    }
    let drive_ok = phase == 4
        && save_checked
        && checks.iter().all(|n| *n > 0)
        && observed.iter().all(|s| s.len() >= 3)
        && observed.iter().zip(&checks_by_type).all(|(types, checks)| {
            types
                .iter()
                .all(|t| checks.get(&t.to_string()).is_some_and(|n| *n > 0))
        })
        && checks_by_direction_type.iter().all(|counts| {
            ["forward", "reverse"].into_iter().all(|d| {
                [0, 3, 7]
                    .into_iter()
                    .all(|t| counts.get(&format!("{d}:{t}")).is_some_and(|n| *n > 0))
            })
        })
        && !split_rows.is_empty();
    let rows:Vec<_>=trace.iter().map(|r|json!({"time_s":r.source_time_s,"phase":r.phase,"head_m":r.head_m,"speed_mps":r.speed_mps,"raw_types":r.raw,"effective_types":r.effective})).collect();
    write_json(&out.join("drive.json"), &json!(rows))?;
    let spec = ConsistSoundSpec {
        id: 0,
        vehicle_offset: 0,
        vehicle_mapping: None,
        consist: consist.clone(),
        route: route.clone(),
    };
    let mut clips = vec![];
    let mut selected = BTreeSet::new();
    let assets = inventory(&consist, &route, &catalog)?;
    let mut audio_ok = assets["selected_exterior_rolling_missing"] == 0;
    for index in split_rows {
        let pattern = (
            trace[index].phase >= 2,
            trace[index]
                .effective
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
        );
        if selected.insert(pattern) && clips.len() < 6 {
            let start = index.saturating_sub(5);
            let end = (index + 20).min(trace.len());
            let mut frames: Vec<_> = trace[start..end]
                .iter()
                .enumerate()
                .map(|(i, r)| frame(r, i as f64 * 0.05, false, i == 0))
                .collect();
            // Ten time constants let the exponential presentation fade settle
            // below the unchanged 0.0001 active-voice audit threshold;
            // it does not extend or prescribe the train's physical trajectory.
            for _ in 0..36 {
                frames.push(frame(
                    &trace[end - 1],
                    frames.len() as f64 * 0.05,
                    false,
                    false,
                ));
            }
            let file = out.join(format!("transition-{}.wav", clips.len()));
            let report = render_oracle(std::slice::from_ref(&spec), &frames, &file)?;
            let expected = &trace[end - 1].effective;
            let direction = if trace[index].phase >= 2 {
                "reverse"
            } else {
                "forward"
            };
            let per_car:Vec<_>=expected.iter().enumerate().map(|(car,t)| {
                let voices:Vec<_>=report.voices.iter().filter(|v|v.vehicle==car&&v.gain>0.0001&&v.track_type.is_some()).collect();
                let actual:BTreeSet<_>=voices.iter().map(|v|v.program.to_ascii_lowercase()).collect();
                let wanted=openrailsrs_audio::mix::resource_name(&catalog.types[*t as usize].outside).to_ascii_lowercase();
                let program = assets["selected_region_programs"].as_array().unwrap().iter()
                    .find(|p|p["vehicle"]==car&&p["track_type"]==*t&&p["view"]=="outside");
                let mut resolved = true;
                let resolved_voices:Vec<_>=voices.iter().map(|v| {
                    let wav=program.and_then(|p|p["wavs"].as_array()).and_then(|wavs|wavs.iter().find(|w|
                        w["resource"].as_str().is_some_and(|r|
                            openrailsrs_audio::mix::resource_name(r).eq_ignore_ascii_case(&v.resource))));
                    let ok=v.track_type==Some(*t)&&v.group==SoundGroup::Rolling&&wav.is_some_and(|w|w["missing"]==false);
                    resolved&=ok;
                    json!({"bank_track_type":v.track_type,"program":v.program,"resource":v.resource,"gain":v.gain,
                        "group":v.group,"wav":wav,"resolved_original_rolling_voice":ok})
                }).collect();
                let passed=actual==BTreeSet::from([wanted.clone()])&&resolved&&!voices.is_empty();
                audio_ok&=passed;
                let stock_rolling=report.voices.iter().filter(|v|v.vehicle==car&&v.gain>0.0001&&v.track_type.is_none()&&v.group==SoundGroup::Rolling).count();
                json!({"vehicle":car,"track_type":t,"direction":direction,"expected_program":wanted,"active_track_programs":actual,
                    "active_track_region_voice_count":voices.len(),"active_stock_rolling_voice_count":stock_rolling,
                    "selected_region_program":program,"resolved_active_track_voices":resolved_voices,"passed":passed})
            }).collect();
            let stats = pcm_stats(&file)?;
            audio_ok &= stats["nonzero_samples"].as_u64().unwrap_or(0) > 0;
            clips.push(json!({"path":file.file_name().unwrap().to_string_lossy(),"direction":direction,"source_start_s":trace[start].source_time_s,"source_end_s":trace[end-1].source_time_s,
                "source_types":trace[start].effective,"final_types":expected,"pcm":stats,"report":report,"selection_checks":per_car}));
        }
    }
    let pause = pause_observation.ok_or("No split for pause")?;
    let pause_frames: Vec<_> = (0..=30)
        .map(|i| frame(&pause, i as f64 * 0.05, (10..20).contains(&i), i == 0))
        .collect();
    let pause_file = out.join("pause-resume.wav");
    let pause_report = render_oracle(std::slice::from_ref(&spec), &pause_frames, &pause_file)?;
    let data = fs::read(&pause_file)?;
    let from = 44 + (0.60 * 44100.) as usize * 4;
    let to = 44 + (0.95 * 44100.) as usize * 4;
    let paused_silent = data[from..to].iter().all(|b| *b == 0);
    let receipt = json!({"schema":1,"status":if drive_ok&&paused_silent&&audio_ok{"PASS"}else{"FAIL"},
        "scope":"Real Rust LiveDriveSession motion of original SCE Class47+six Mk2; native equivalence applies to frozen GetTType probes/intervals only. Offline PCM uses original SMS/WAV, without physical output device, listening judgment or acoustic parity claim.",
        "original_content_root":content,"original_consist":consist,"original_consist_sha256":hash(&consist)?,
        "native_reference_sha256":hash(&fixture.join("reference.json"))?,"protocol_sha256":hash(&out.join("protocol.json"))?,
        "source_example_sha256_observed_before_run":source_before,"source_example_sha256_observed_after_run":hash(&example)?,
        "tested_executable_sha256":hash(&std::env::current_exe()?)?,
        "step_s":0.05,"ticks":trace.len(),"vehicle_count":held.len(),"native_interval_checks_per_car":checks,
        "types_per_car":observed,"native_interval_checks_by_type_per_car":checks_by_type,
        "native_interval_checks_by_direction_type_per_car":checks_by_direction_type,"split_frames":selected,"save_restore_during_split":save_checked,
        "paused_zero_delta_checks":20,"forward_stopped_at_m":stopped_forward,"final_head_m":session.head_chainage_m(),"distance_m":session.state.odometer_m,
        "phase_completed":phase==4,"drive_trace_sha256":hash(&out.join("drive.json"))?,"assets":assets,
        "clips":clips,"audio_selection_passed":audio_ok,"pcm_editing":"Short real-drive windows, current effective-type seed at each cut and 1.8s (10 tau) held final state for crossfade inspection; not a continuous acoustic journey reference.",
        "pause_pcm":{"stats":pcm_stats(&pause_file)?,"silent_inner_interval_s":[0.60,0.95],"silent":paused_silent,"report":pause_report},
        "audio_device_opened":false,"gpu_or_wine_used":false});
    write_json(&out.join("receipt.json"), &receipt)?;
    println!(
        "{}",
        json!({"status":receipt["status"],"ticks":trace.len(),"checks":receipt["native_interval_checks_per_car"],"types":receipt["types_per_car"],"pause_silent":paused_silent})
    );
    if !drive_ok || !paused_silent || !audio_ok {
        return Err("Drive or PCM acceptance failed; receipt preserved".into());
    }
    Ok(())
}
