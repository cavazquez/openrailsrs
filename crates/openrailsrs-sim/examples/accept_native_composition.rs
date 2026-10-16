//! Real BNSF ACT metadata plus controlled condition inputs; no journey certificate.
use std::path::{Path, PathBuf};

use openrailsrs_core::activity::{
    ActivityCondition, ActivityDefinition, ActivityEvent, ActivitySiding,
};
use openrailsrs_formats::ActivityFile;
use openrailsrs_sim::{LiveDriveSession, LiveTraffic};
use sha2::{Digest, Sha256};

fn sha(path: &Path) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).map_err(|e| e.to_string())?)
    ))
}
fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn controlled(
    event: ActivityEvent,
    ending: ActivityEvent,
    ids: Vec<u32>,
    siding: bool,
) -> Result<LiveDriveSession, String> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/activity_operations/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&file).map_err(|e| e.to_string())?;
    scenario.native_activity = Some(ActivityDefinition {
        vehicle_uids: ids,
        events: vec![event, ending],
        sidings: if siding {
            vec![ActivitySiding {
                id: 491,
                edge: "e1".into(),
                start_m: 90.,
                end_m: 110.,
            }]
        } else {
            vec![]
        },
        ..Default::default()
    });
    LiveDriveSession::from_scenario(file.parent().unwrap(), &scenario).map_err(|e| e.to_string())
}
fn fired(session: &LiveDriveSession, id: i32) -> u32 {
    session.state.activity.as_ref().unwrap().progress[&id].times_triggered
}
fn verify_actors(act: &Path, activity: &ActivityFile, out: &Path) -> Result<usize, String> {
    let trace = openrailsrs_formats::read_trace::ReadTrace::default();
    let route = act
        .parent()
        .and_then(Path::parent)
        .ok_or("Original route missing")?;
    let cache = out.join("native-actors-import");
    std::fs::create_dir(&cache).map_err(|e| e.to_string())?;
    std::fs::write(
        cache.join("track.toml"),
        openrailsrs_msts::import_route_with_activity(route, act).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let imported = openrailsrs_msts::import_activity_with_track(route, act, Some(&cache))
        .map_err(|e| e.to_string())?;
    std::fs::write(out.join("native-actors-original-import.toml"), &imported)
        .map_err(|e| e.to_string())?;
    let mut scenario: openrailsrs_scenarios::ScenarioFile =
        toml::from_str(&imported).map_err(|e| e.to_string())?;
    scenario.route.path = cache
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let filename = Path::new(&scenario.train.consist)
        .file_name()
        .ok_or("Original player CON missing")?;
    scenario.train.consist = route
        .ancestors()
        .take(5)
        .find_map(|root| {
            openrailsrs_formats::resolve_path_case_insensitive(
                &root.join("TRAINS/CONSISTS").join(filename),
            )
        })
        .ok_or("Original player CON missing")?
        .to_string_lossy()
        .into_owned();
    let mut player =
        LiveDriveSession::from_scenario(&cache, &scenario).map_err(|e| e.to_string())?;
    let mut traffic = LiveTraffic::from_scenario(&cache, &scenario)?;
    if traffic.services.len() != 6 || traffic.services.iter().any(|s| !s.parked) {
        return Err("Expected the six original static actors without omitted services".into());
    }
    let footprints = |traffic: &LiveTraffic| {
        traffic
            .services
            .iter()
            .map(|service| (service.id.clone(), service.session.own_track_occupancy()))
            .collect::<Vec<_>>()
    };
    for object in &activity.static_trains {
        let id = format!("act-static-{}", object.id);
        let actor = &traffic
            .services
            .iter()
            .find(|service| service.id == id)
            .ok_or("Original actor missing")?
            .session;
        let expected = object
            .entries
            .iter()
            .map(|e| {
                e.uid()
                    .map(|uid| object.id << 16 | uid)
                    .ok_or("Original UiD missing")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let actual = actor
            .formation
            .cars
            .iter()
            .filter_map(|c| c.activity_uid)
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(format!("Actor {id} changed original UID/order"));
        }
        let length = actor
            .own_track_occupancy()
            .iter()
            .map(|part| part.end_m - part.start_m)
            .sum::<f64>();
        if (length - actor.formation.length_m()).abs() > 0.01 {
            return Err(format!("Actor {id} has incomplete track occupancy"));
        }
    }
    let original = footprints(&traffic);
    traffic.advance(&mut player, 1., None, |_| {});
    if original != footprints(&traffic) {
        return Err("Original parked actors moved".into());
    }
    let player_saved = player.snapshot();
    let traffic_saved = traffic.snapshot();
    write(
        &out.join("native-actors-checkpoint.json"),
        &serde_json::json!({"player":player_saved,"traffic":traffic_saved}),
    )?;
    let mut restored_player =
        LiveDriveSession::from_scenario(&cache, &scenario).map_err(|e| e.to_string())?;
    let mut restored_traffic = LiveTraffic::from_scenario(&cache, &scenario)?;
    restored_traffic.restore_shared_snapshots(&mut restored_player, player_saved, traffic_saved)?;
    if original != footprints(&restored_traffic) {
        return Err("Native actor occupancy changed during restore".into());
    }
    write(&out.join("native-actors-occupancy.json"), &original)?;
    let content = route
        .parent()
        .and_then(Path::parent)
        .ok_or("Original content root missing")?;
    let mut sources = vec![];
    for (path, before) in trace.fingerprints() {
        if path.starts_with(content) {
            let after = sha(&path)?;
            if before.as_ref().is_some_and(|v| v != &after) {
                return Err("Original actor resource bytes changed".into());
            }
            sources
                .push(serde_json::json!({"path":path,"read_sha256":before,"after_sha256":after}));
        }
    }
    if !trace.consistent() {
        return Err("Original actor resource reads changed".into());
    }
    write(
        &out.join("native-actors-source-preservation.json"),
        &sources,
    )?;
    Ok(sources.len())
}
fn run(act: &Path, out: &Path, actors: bool) -> Result<(), String> {
    if out.exists() {
        return Err("Choose a fresh evidence directory".into());
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let before = sha(act)?;
    write(
        &out.join("protocol-before.json"),
        &serde_json::json!({
            "schema": 1, "act_sha256": before, "reference": "BNSF Scenic 4_lumberrun.act and Open Rails 1.6.1 Activity.cs source semantics",
            "dt_s": 0.05, "exact_event_order": [1,2,3,4,5,6,7,8,9,11],
            "pickup_event": 5, "pickup_uids": [2148007936u32,2148007937u32],
            "drop_event": 9, "drop_uids": [2148073472u32,2148073473u32], "drop_siding": 491,
            "verify_native_actors": actors, "actor_occupancy_tolerance_m":0.01,
            "scope": "Original parsing and outcomes, controlled Rust condition inputs and persistence. Own four-car exercise supplies physics; prescribed siding geometry is not the original TDB. No original BNSF journey, visual assets or executed OR DLL is certified."
        }),
    )?;
    let activity = ActivityFile::from_path(act).map_err(|e| e.to_string())?;
    if activity.events.iter().map(|e| e.id).collect::<Vec<_>>() != [1, 2, 3, 4, 5, 6, 7, 8, 9, 11] {
        return Err("Original event order differs".into());
    }
    let event = |id| {
        activity
            .events
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| format!("Missing original event {id}"))
    };
    let pickup = event(5)?;
    let ending = event(11)?;
    let drop = event(9)?;
    if !matches!(&pickup.condition, ActivityCondition::PickUpWagons {uids} if uids == &[2148007936,2148007937])
        || pickup.outcomes.increment != [11]
    {
        return Err("Original pickup list/outcome differs".into());
    }
    if !matches!(&drop.condition, ActivityCondition::DropOffWagons {uids,siding:Some(491)} if uids == &[2148073472,2148073473])
    {
        return Err(
            "Original drop UID/siding differs; per-car UINT_MAX must not become the destination"
                .into(),
        );
    }
    for (id, expected) in [(4, 553), (7, 493)] {
        if !matches!(event(id)?.condition, ActivityCondition::DropOffWagons {siding:Some(s),..} if s == expected)
        {
            return Err(format!("Original siding differs in event {id}"));
        }
    }
    if activity.static_trains.len() != 6 {
        return Err("Original loose formations differ".into());
    }
    let mut good = controlled(
        pickup.clone(),
        ending.clone(),
        vec![0, 2148007936, 2148007937, 1],
        false,
    )?;
    good.step_realtime(0.05, |_| {});
    if fired(&good, 5) != 1
        || good.state.activity.as_ref().unwrap().progress[&11].activation != -6
        || good.arrived
    {
        return Err("Native pickup/outcome was not evaluated".into());
    }
    let mut resumed = controlled(
        pickup.clone(),
        ending.clone(),
        vec![0, 2148007936, 2148007937, 1],
        false,
    )?;
    resumed.restore_snapshot(
        serde_json::from_slice(&serde_json::to_vec(&good.snapshot()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    )?;
    resumed.step_realtime(0.1, |_| {});
    if fired(&resumed, 5) != 1
        || resumed.state.activity.as_ref().unwrap().progress[&11].activation != -6
    {
        return Err("Native pickup repeated on resume".into());
    }
    let mut missing = controlled(
        pickup.clone(),
        ending.clone(),
        vec![0, 2148007936, 99, 1],
        false,
    )?;
    missing.step_realtime(0.05, |_| {});
    if fired(&missing, 5) != 0 {
        return Err("Missing original wagon was accepted".into());
    }
    let mut moving = controlled(
        pickup,
        ending.clone(),
        vec![0, 2148007936, 2148007937, 1],
        false,
    )?;
    moving.state.velocity_mps = 2.;
    moving.step_realtime(0.05, |_| {});
    if fired(&moving, 5) != 0 {
        return Err("Moving pickup was accepted above the native speed threshold".into());
    }
    let mut dropped = controlled(
        drop.clone(),
        ending.clone(),
        vec![0, 2148073472, 2, 3],
        true,
    )?;
    dropped.step_realtime(0.05, |_| {});
    if fired(&dropped, 9) != 1 || dropped.arrived {
        return Err("Native drop matching peculiarity/outcome was not preserved".into());
    }
    let mut elsewhere = controlled(drop, ending, vec![0, 2148073472, 2, 3], true)?;
    elsewhere.state.pos_on_edge_m = 200.;
    elsewhere.step_realtime(0.05, |_| {});
    if fired(&elsewhere, 9) != 0 {
        return Err("Drop at the wrong siding was accepted".into());
    }
    let native_actor_sources = if actors {
        Some(verify_actors(act, &activity, out)?)
    } else {
        None
    };
    let after = sha(act)?;
    if before != after {
        return Err("Original ACT bytes changed".into());
    }
    write(&out.join("parsed-original-events.json"), &activity.events)?;
    write(
        &out.join("result.json"),
        &serde_json::json!({"status":"passed","source_sha256":before,"after_sha256":after,"original_events":10,"original_loose_formations":6,"controlled_runtime_cases":6,"resumed_without_retrigger":true,"missing_uid_rejected":true,"moving_pickup_rejected":true,"wrong_siding_rejected":true,"native_drop_peculiarity_preserved":true,"native_actor_sources_checked":native_actor_sources,"native_actor_occupancy_and_resume":actors,"scope":"Controlled source-semantic acceptance with optional original actor import/persistence; not an OR DLL capture or complete BNSF journey"}),
    )?;
    Ok(())
}
fn main() {
    let args = std::env::args()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let actors = args.get(2).is_some_and(|p| p == "--actors");
    if args.len() != 2 && !(args.len() == 3 && actors) {
        eprintln!(
            "usage: accept_native_composition ORIGINAL_BNSF_LUMBERRUN_ACT FRESH_OUTPUT [--actors]"
        );
        std::process::exit(2);
    }
    if let Err(error) = run(&args[0], &args[1], actors) {
        let _ = write(
            &args[1].join("failed.json"),
            &serde_json::json!({"status":"failed","error":error}),
        );
        eprintln!("{error}");
        std::process::exit(1);
    }
}
