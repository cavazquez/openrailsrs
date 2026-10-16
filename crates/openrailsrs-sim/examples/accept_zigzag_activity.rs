//! Read-only original ACT intake plus a controlled cargo transfer, not a journey certificate.
use openrailsrs_formats::{ActivityFile, WorldFile, WorldItem};
use openrailsrs_sim::{LiveDriveSession, LiveTraffic, refill::RefillStation};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
fn sha(p: &Path) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(std::fs::read(p).map_err(|e| e.to_string())?)
    ))
}
fn write(p: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    std::fs::write(
        p,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn run(route: &Path, act: &Path, out: &Path) -> Result<(), String> {
    if out.exists() {
        return Err("Choose a fresh evidence directory".into());
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let protocol = serde_json::json!({"schema":1,"reference":"Open Rails 1.6.1 source semantics and original author assets","act_sha256":sha(act)?,"dt_s":0.05,"expected_static_id":32768,"expected_vehicle_uids":[2147483648u32,2147483649u32,2147483650u32],"expected_empty_mass_kg":10160.5,"expected_capacity_per_wagon_kg":16256.8,"quantity_tolerance_kg":0.02,"scope":"ACT import, original identities/occupancy, authored capacities and controlled station transfer/save. Loader alignment is a controlled input here; no complete original journey, turntable operation or automatic quantity objective is certified."});
    write(&out.join("protocol-before.json"), &protocol)?;
    let trace = openrailsrs_formats::read_trace::ReadTrace::default();
    let activity = ActivityFile::from_path(act).map_err(|e| e.to_string())?;
    if activity.static_trains.len() != 1
        || activity.static_trains[0].id != 32768
        || activity.static_trains[0].entries.len() != 3
    {
        return Err("Original tutorial static declaration differs".into());
    }
    if activity.events.len() != 11 {
        return Err(format!(
            "Expected 11 original events, received {}",
            activity.events.len()
        ));
    }
    let cache = out.join("import");
    std::fs::create_dir(&cache).map_err(|e| e.to_string())?;
    std::fs::write(
        cache.join("track.toml"),
        openrailsrs_msts::import_route_with_activity(route, act).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let imported = openrailsrs_msts::import_activity_with_track(route, act, Some(&cache))
        .map_err(|e| e.to_string())?;
    std::fs::write(out.join("original-import.toml"), &imported).map_err(|e| e.to_string())?;
    let mut scenario: openrailsrs_scenarios::ScenarioFile =
        toml::from_str(&imported).map_err(|e| e.to_string())?;
    scenario.route.path = cache
        .canonicalize()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let filename = Path::new(&scenario.train.consist)
        .file_name()
        .ok_or("Original consist filename missing")?;
    scenario.train.consist = route
        .ancestors()
        .take(5)
        .find_map(|p| {
            openrailsrs_formats::resolve_path_case_insensitive(
                &p.join("TRAINS/CONSISTS").join(filename),
            )
        })
        .ok_or("Original player consist missing")?
        .to_string_lossy()
        .into_owned();
    // Preserve the complete imported file above. This acceptance exercises the
    // actor, not the six AI services' preceding 27 minutes or the complete trip.
    scenario.extra_trains.retain(|e| e.parked);
    scenario.simulation.time_step = 0.05;
    write(
        &out.join("controlled-scope.json"),
        &serde_json::json!({"ai_pre_run_exercised":false,"static_services":scenario.extra_trains.iter().map(|s|&s.id).collect::<Vec<_>>(),"geometry_alignment":"prescribed controlled input, checked separately by the menu WORLD target resolver"}),
    )?;
    let mut player =
        LiveDriveSession::from_scenario(&cache, &scenario).map_err(|e| e.to_string())?;
    let mut traffic = LiveTraffic::from_scenario(&cache, &scenario)?;
    if traffic.services.len() != 1 || !traffic.services[0].parked {
        return Err("Original static actor missing".into());
    }
    let actor = &traffic.services[0].session;
    let uids = actor
        .formation
        .cars
        .iter()
        .map(|c| c.activity_uid.ok_or("Static UID lost"))
        .collect::<Result<Vec<_>, _>>()?;
    if uids != [2147483648, 2147483649, 2147483650] {
        return Err(format!("Static UID/order differs: {uids:?}"));
    }
    let occupied = actor.own_track_occupancy();
    if occupied.is_empty() {
        return Err("Static actor does not occupy track".into());
    }
    write(&out.join("original-static-before.json"), &actor.snapshot())?;
    traffic.advance(&mut player, 5.05, None, |_| {});
    let runtime = player
        .state
        .activity
        .as_ref()
        .ok_or("ACT runtime missing")?;
    if runtime.successful
        || runtime.progress[&0].times_triggered != 0
        || runtime.progress[&1].times_triggered != 1
    {
        return Err("Empty AllStops succeeded or original time message did not trigger".into());
    }
    let world =
        openrailsrs_formats::resolve_path_case_insensitive(&route.join("WORLD/w+001451+010332.w"))
            .ok_or("Original colliery WORLD missing")?;
    let w = WorldFile::from_path(&world).map_err(|e| e.to_string())?;
    let mut pickups = vec![];
    for item in &w.items {
        if let WorldItem::Pickup {
            uid,
            pickup_type: Some(kind @ (2 | 6)),
            pickup_capacity: Some([capacity, feed]),
            pickup_anim_data,
            speed_range_mps,
            ..
        } = item
            && *capacity > 0.
            && *feed > 0.
        {
            pickups.push(RefillStation {
                id: format!("1451:10332:{uid}"),
                pickup_type: *kind,
                capacity_kg: capacity * 0.45359237,
                feed_kg_s: feed * 0.45359237,
                opening_time_s: pickup_anim_data.map_or(1., |a| a[1].max(0.)),
                speed_range_mps: speed_range_mps.unwrap_or([0., 0.]),
            });
        }
    }
    write(&out.join("original-world-pickups.json"), &pickups)?;
    let station = pickups
        .into_iter()
        .find(|p| p.capacity_kg >= 3. * 16256.8 && p.speed_range_mps[0] == 0.)
        .ok_or("No compatible original colliery loader with sufficient supply")?;
    let mut actor = traffic.services.remove(0).session;
    if actor.state.freight.len() != 3 {
        return Err("Original wagon freight profiles missing".into());
    }
    for c in &actor.state.freight {
        if (c.profile.capacity_kg - 16256.8).abs() > 0.02
            || (c.profile.empty_mass_kg - 10160.5).abs() > 0.02
            || c.load_kg != 0.
        {
            return Err("Original empty/capacity metadata differs".into());
        }
    }
    let initial_mass = actor.effective_mass_kg();
    let mut checkpoints = vec![];
    for index in 0..3 {
        let width = actor.intake_points[index]
            .iter()
            .find(|i| i.pickup_type == station.pickup_type)
            .ok_or("Original material intake missing")?
            .width_m;
        actor.begin_refill(station.clone(), index, 0., width)?;
        let half_time = station.opening_time_s + 16256.8 / station.feed_kg_s / 2.;
        actor.step_realtime(half_time, |_| {});
        if index == 0 {
            let snapshot = actor.snapshot();
            write(&out.join("half-load-checkpoint.json"), &snapshot)?;
            let mut fresh = LiveTraffic::from_scenario(&cache, &scenario)?
                .services
                .remove(0)
                .session;
            fresh.restore_snapshot(
                serde_json::from_slice(&serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )?;
            actor.step_realtime(1., |_| {});
            fresh.step_realtime(1., |_| {});
            if actor.state.freight != fresh.state.freight {
                return Err("Payload resume diverges".into());
            }
        }
        actor.step_realtime(
            station.opening_time_s + 16256.8 / station.feed_kg_s + 2.,
            |_| {},
        );
        let (load, capacity) = actor.cargo_load_kg(index).ok_or("Payload lost")?;
        if (load - capacity).abs() > 0.02 {
            return Err(format!(
                "Wagon {} failed to fill: {load}/{capacity}",
                index + 1
            ));
        }
        checkpoints.push(serde_json::json!({"vehicle":index,"uid":actor.formation.cars[index].activity_uid,"load_kg":load,"capacity_kg":capacity,"mass_kg":actor.effective_mass_kg()}));
    }
    let final_mass = actor.effective_mass_kg();
    if (final_mass - initial_mass - 3. * 16256.8).abs() > 0.02 {
        return Err("Three payloads did not change physical mass".into());
    }
    if occupied != actor.own_track_occupancy() {
        return Err("Controlled filling moved the static actor".into());
    }
    let content = route
        .parent()
        .and_then(Path::parent)
        .ok_or("Content root missing")?;
    let mut sources = vec![];
    for (path, before) in trace.fingerprints() {
        if path.starts_with(content) {
            let after = sha(&path)?;
            if before.as_ref().is_some_and(|hash| hash != &after) {
                return Err(format!("Original bytes changed: {}", path.display()));
            }
            sources
                .push(serde_json::json!({"path":path,"read_sha256":before,"after_sha256":after}));
        }
    }
    if !trace.consistent() {
        return Err("Original reads were inconsistent".into());
    }
    if sha(act)?
        != protocol["act_sha256"]
            .as_str()
            .ok_or("Original ACT hash missing")?
    {
        return Err("Original ACT bytes changed".into());
    }
    write(&out.join("source-preservation.json"), &sources)?;
    write(
        &out.join("result.json"),
        &serde_json::json!({"status":"passed","act_sha256":sha(act)?,"original_events":activity.events.len(),"original_static_vehicles":3,"uids":uids,"loads":checkpoints,"initial_mass_kg":initial_mass,"final_mass_kg":final_mass,"total_payload_kg":final_mass-initial_mass,"empty_allstops_triggered":false,"time_message_triggered":true,"checkpoint_resumed":true,"sources_checked":sources.len(),"scope":protocol["scope"]}),
    )?;
    Ok(())
}
fn main() {
    let a = std::env::args()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if a.len() != 3 {
        eprintln!("usage: accept_zigzag_activity ORIGINAL_ROUTE ORIGINAL_ACT FRESH_OUTPUT");
        std::process::exit(2);
    }
    if let Err(e) = run(&a[0], &a[1], &a[2]) {
        let _ = write(
            &a[2].join("failed.json"),
            &serde_json::json!({"status":"failed","error":e}),
        );
        eprintln!("{e}");
        std::process::exit(1);
    }
}
