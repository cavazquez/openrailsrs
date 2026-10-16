//! CPU-only replay of the user's installed MidEast activity. Original author
//! files are read in place; generated route/scenario/report stay in a fresh QA directory.
use openrailsrs_sim::{CarOperation, ServicePhase, sections::CouplingEnd};
use openrailsrs_sim::{LiveDriveSession, LiveTraffic};
use serde_json::json;
use std::io::{BufWriter, Write};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--drive") {
        return drive(
            Path::new(args.get(2).ok_or("Missing imported scenario")?),
            Path::new(args.get(3).ok_or("Missing new drive output")?),
        );
    }
    if args.len() < 3 {
        return Err("mideast_portions_qa ORIGINAL_ROUTE NEW_QA_DIRECTORY [drive]".into());
    }
    let native = Path::new(&args[1]);
    let out = PathBuf::from(&args[2]);
    if out.exists() {
        return Err("The QA output directory must be new".into());
    }
    fs::create_dir_all(&out)?;
    let out = out.canonicalize()?;
    let act = native.join("Activities/Firsby to Spilsby Return.act");
    let track = openrailsrs_msts::import_route_with_activity(native, &act)?;
    fs::write(out.join("track.toml"), track)?;
    let initial = openrailsrs_msts::import_activity_with_track(native, &act, Some(&out))?;
    let first: openrailsrs_scenarios::ScenarioFile = toml::from_str(&initial)?;
    fs::write(out.join("initial-import.toml"), &initial)?;
    let requested = PathBuf::from(&first.train.consist);
    let folder = native
        .parent()
        .and_then(Path::parent)
        .ok_or("Missing native installation")?
        .join("TRAINS/CONSISTS");
    let con = if requested.is_absolute() {
        requested
    } else {
        folder.join(requested.file_name().ok_or("Original CON name missing")?)
    };
    let con = if con.extension().is_none() {
        con.with_extension("con")
    } else {
        con
    };
    let con = openrailsrs_formats::resolve_path_case_insensitive(&con)
        .ok_or_else(|| format!("Original CON not found: {}", con.display()))?;
    let consist = openrailsrs_train::load_consist_with_asset_root(
        &con,
        openrailsrs_train::consist_asset_root(&con),
    )?;
    let length: f64 = consist.vehicle_lengths_m().iter().sum();
    let imported =
        openrailsrs_msts::import_activity_with_consist_length(native, &act, &out, length)?;
    let mut scenario: openrailsrs_scenarios::ScenarioFile = toml::from_str(&imported)?;
    scenario.route.path = out.to_string_lossy().into_owned();
    scenario.train.consist = con.to_string_lossy().into_owned();
    scenario.simulation.multi_body = true;
    fs::write(
        out.join("scenario.toml"),
        toml::to_string_pretty(&scenario)?,
    )?;
    let session = LiveDriveSession::from_scenario(&out, &scenario)?;
    let traffic = LiveTraffic::from_scenario(&out, &scenario).map_err(std::io::Error::other)?;
    let report = json!({"mode":"import_only_no_completion_claim","original_activity":act,"original_consist":con,"vehicles":session.formation.cars,"length_m":length,"head_m":session.head_chainage_m(),"legs":scenario.route.legs,"native_activity":scenario.native_activity,"extra_trains":traffic.services.iter().map(|s|&s.id).collect::<Vec<_>>(),"boiler":session.state.boiler_state,"state":session.snapshot()});
    fs::write(
        out.join("inspection.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "{}",
        serde_json::to_string(
            &json!({"inspection":out.join("inspection.json"),"vehicles":session.formation.cars.len(),"legs":scenario.route.legs.len(),"length_m":length,"head_m":session.head_chainage_m()})
        )?
    );
    Ok(())
}

fn physical_positions(
    s: &LiveDriveSession,
    ids: &[String],
) -> Vec<(String, Option<(String, f64)>)> {
    let inventory = s.vehicle_identities();
    ids.iter()
        .filter_map(|id| {
            inventory.iter().position(|i| i == id).map(|i| {
                let offset = s.formation.center_offset_m(i);
                (
                    id.clone(),
                    s.presentation_position_for_car(offset, offset, 0.),
                )
            })
        })
        .collect()
}
fn chainage_for_position(s: &LiveDriveSession, p: &(String, f64)) -> Option<f64> {
    let base = p.0.strip_suffix("_r").unwrap_or(&p.0);
    let length = s.graph.edge(&p.0)?.length_m;
    let at = if p.0.ends_with("_r") {
        length - p.1
    } else {
        p.1
    };
    s.state.path_edges.iter().enumerate().find_map(|(i, id)| {
        if id.strip_suffix("_r").unwrap_or(id) == base {
            Some(
                s.path_data.chainage_at_edge_position(
                    i,
                    if id.ends_with("_r") { length - at } else { at },
                ),
            )
        } else {
            None
        }
    })
}
fn checkpoint(
    s: &mut LiveDriveSession,
    t: &mut LiveTraffic,
    dir: &Path,
    scenario: &openrailsrs_scenarios::ScenarioFile,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let player = s.snapshot();
    let traffic = t.snapshot();
    s.validate_snapshot(&player)?;
    t.validate_snapshot(&traffic)?;
    fs::write(
        dir.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&json!({"player":player,"traffic":traffic}))?,
    )?;
    let mut next = LiveDriveSession::from_scenario(Path::new(&scenario.route.path), scenario)?;
    let mut others = LiveTraffic::from_scenario(Path::new(&scenario.route.path), scenario)
        .map_err(std::io::Error::other)?;
    others.restore_shared_snapshots(&mut next, player, traffic)?;
    next.traffic_brake_assistance = s.traffic_brake_assistance;
    *s = next;
    *t = others;
    Ok(())
}
fn drive(path: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if out.exists() {
        return Err("The drive output directory must be new".into());
    }
    fs::create_dir_all(out)?;
    let definition = openrailsrs_scenarios::load_scenario(path)?;
    let directory = path.parent().ok_or("Missing scenario directory")?;
    let mut s = LiveDriveSession::from_scenario(directory, &definition)?;
    let mut traffic =
        LiveTraffic::from_scenario(directory, &definition).map_err(std::io::Error::other)?;
    // Match the normal manual player's default. The explicit demonstration driver
    // still obeys signals; the final 1 m contact is controlled by this QA driver.
    s.traffic_brake_assistance = false;
    let original = s.vehicle_identities();
    if original.len() != 3 || s.state.boiler_state.is_none() {
        return Err("Expected original steam locomotive and two Mk1".into());
    }
    let mut trace = BufWriter::new(fs::File::create(out.join("drive.csv"))?);
    writeln!(
        trace,
        "time_s,leg,chainage_m,odometer_m,speed_mps,throttle,brake,stops,coupled,door,phase,status"
    )?;
    let mut actions = Vec::new();
    let mut detached = false;
    let mut rejoined = false;
    let mut parked = vec![];
    let mut leg_history = vec![0usize];
    let mut failed = None;
    let mut last_log = -1.;
    let mut last_progress = 0.;
    let mut previous_head = s.head_chainage_m();
    let start = std::time::Instant::now();
    for _ in 0..140_000 {
        let leg = s.operational_leg_index();
        if leg_history.last() != Some(&leg) {
            leg_history.push(leg);
            actions.push(json!({"action":"original_PAT_leg","leg":leg,"time_s":s.time_s(),"position":s.position_at_head_offset(0.)}));
        }
        if !detached
            && s.gameplay.next_stop_idx >= 3
            && s.gameplay.phase == ServicePhase::Approaching
            && s.velocity_mps().abs() <= 0.1
        {
            s.driver_throttle = 0.;
            s.driver_brake = 1.;
            for i in 1..3 {
                if !s.formation.cars[i].handbrake {
                    s.operate_car(i, CarOperation::Handbrake)?;
                }
            }
            s.uncouple_after_named(0, "Spilsby-Mk1")?;
            parked = physical_positions(&s, &original[1..]);
            detached = true;
            actions.push(json!({"action":"detach_two_original_Mk1","time_s":s.time_s(),"parked":parked,"occupancy":s.section_track_occupancy("Spilsby-Mk1")?,"boiler":s.state.boiler_state}));
            checkpoint(&mut s, &mut traffic, out, &definition, "after-detach")?;
            actions.push(json!({"action":"save_restore_after_detach","time_s":s.time_s(),"parked":physical_positions(&s,&original[1..])}));
        }
        let contact = detached && !rejoined && leg == 2;
        if contact {
            let portion = s
                .detached_sections()
                .into_iter()
                .find(|p| p.id == "Spilsby-Mk1")
                .ok_or("Missing parked coaches")?;
            let target =
                chainage_for_position(&s, portion.rear.as_ref().ok_or("Missing coach rear")?)
                    .ok_or("Coach rear absent from return approach")?;
            let distance = target - s.head_chainage_m();
            let v = s.velocity_mps().abs();
            s.driver_direction = 1.;
            if v <= 0.005 && distance.abs() < 0.008 {
                s.driver_throttle = 0.;
                s.driver_brake = 1.;
                s.recouple_section("Spilsby-Mk1", CouplingEnd::Front)?;
                for i in 0..s.formation.coupled_count {
                    if s.formation.cars[i].handbrake {
                        s.operate_car(i, CarOperation::Handbrake)?;
                    }
                    if i > 0 && !s.formation.cars[i].hose_connected {
                        s.operate_car(i, CarOperation::Hose)?;
                    }
                    if i > 0 && !s.formation.cars[i].front_cock_open {
                        s.operate_car(i, CarOperation::FrontCock)?;
                    }
                    if i + 1 < s.formation.coupled_count && !s.formation.cars[i].rear_cock_open {
                        s.operate_car(i, CarOperation::RearCock)?;
                    }
                }
                let after = physical_positions(&s, &original[1..]);
                let adjustment = parked
                    .iter()
                    .zip(&after)
                    .map(|((id, a), (next, b))| match (a, b) {
                        (Some((ea, pa)), Some((eb, pb))) if id == next && ea == eb => {
                            (pa - pb).abs()
                        }
                        _ => f64::INFINITY,
                    })
                    .fold(0.0_f64, f64::max);
                if adjustment > 0.01 {
                    failed = Some(format!(
                        "Join adjustment exceeded declared 1 cm tolerance: {adjustment} m"
                    ));
                    break;
                }
                actions.push(json!({"action":"join_geometry_check","maximum_adjustment_m":adjustment,"declared_tolerance_m":0.01}));
                rejoined = true;
                actions.push(json!({"action":"front_pickup_after_driven_loop","time_s":s.time_s(),"distance_m":distance,"vehicles":s.vehicle_identities(),"positions":physical_positions(&s,&original),"boiler":s.state.boiler_state}));
                checkpoint(&mut s, &mut traffic, out, &definition, "after-front-pickup")?;
                actions
                    .push(json!({"action":"save_restore_after_front_pickup","time_s":s.time_s()}));
            } else {
                if distance < -0.01 {
                    failed = Some(format!("Passed coach contact by {} m", -distance));
                    break;
                }
                let cap = (2. * 0.08 * (distance - 0.001).max(0.)).sqrt().min(1.2);
                let band = (cap * 0.1).min(0.002);
                if v > cap + band {
                    s.driver_throttle = 0.;
                    s.driver_brake = if cap < 0.01 { 0.6 } else { 0.25 };
                } else if v < cap - band {
                    s.driver_throttle = if distance < 0.5 {
                        0.012
                    } else if distance < 5. {
                        0.04
                    } else {
                        0.12
                    };
                    s.driver_brake = 0.;
                } else {
                    s.driver_throttle = 0.;
                    s.driver_brake = 0.;
                }
            }
        }
        let precision_reversal =
            detached && !rejoined && leg < 2 && s.operational_distance_m().is_some_and(|d| d < 18.);
        if precision_reversal {
            let target = definition.route.legs[leg].end_chainage_m + 1.;
            let distance = target - s.head_chainage_m();
            let cap = (2. * 0.15 * (distance - 0.12).max(0.)).sqrt().min(1.2);
            let v = s.velocity_mps().abs();
            s.driver_direction = if s.state.path_reversed { 0. } else { 1. };
            if distance < 0.3 && v < 0.09 {
                s.driver_throttle = 0.;
                s.driver_brake = 1.;
                s.driver_direction = if definition.route.legs[leg + 1].reversed {
                    0.
                } else {
                    1.
                };
            } else if v > cap + 0.025 {
                s.driver_throttle = 0.;
                s.driver_brake = if cap < 0.15 { 1. } else { 0.35 };
            } else if v < cap - 0.04 {
                s.driver_throttle = 0.12;
                s.driver_brake = 0.;
            } else {
                s.driver_throttle = 0.;
                s.driver_brake = 0.;
            }
        }
        traffic.advance(
            &mut s,
            0.05,
            if contact && !rejoined || precision_reversal {
                None
            } else {
                Some(0.3)
            },
            |_| {},
        );
        if detached && !rejoined {
            let current = physical_positions(&s, &original[1..]);
            if current != parked {
                failed = Some("Parked Mk1 position changed during the run-around".into());
                break;
            }
        }
        if s.time_s() - last_log >= 5. {
            let status = s
                .operational_status()
                .unwrap_or_default()
                .replace([',', '\n', '\r'], " ");
            writeln!(
                trace,
                "{:.3},{leg},{:.6},{:.6},{:.6},{:.3},{:.3},{},{},{:?},{:?},{}",
                s.time_s(),
                s.head_chainage_m(),
                s.state.odometer_m,
                s.velocity_mps(),
                s.driver_throttle,
                s.driver_brake,
                s.gameplay.next_stop_idx,
                s.formation.coupled_count,
                s.exterior.door,
                s.gameplay.phase,
                status
            )?;
            trace.flush()?;
            last_log = s.time_s();
            fs::write(
                out.join("progress.json"),
                serde_json::to_vec_pretty(
                    &json!({"time_s":s.time_s(),"leg":leg,"head_m":s.head_chainage_m(),"speed_mps":s.velocity_mps(),"next_stop":s.gameplay.next_stop_idx,"phase":s.gameplay.phase,"next_signal":s.next_signal_ahead(),"occupied_distance":s.distance_to_occupied_block_m(),"status":s.operational_status(),"detached":detached,"rejoined":rejoined,"traction":s.traction_diagnosis(),"state":s.snapshot()}),
                )?,
            )?;
        }
        if (s.head_chainage_m() - previous_head).abs() > 0.1 {
            last_progress = s.time_s();
            previous_head = s.head_chainage_m();
        }
        if s.time_s() - last_progress > 300. && s.gameplay.phase == ServicePhase::Approaching {
            failed = Some("No physical progress for 300 simulated seconds".into());
            break;
        }
        if s.gameplay.phase == ServicePhase::Failed {
            failed = s.gameplay.failure.clone();
            break;
        }
        if s.state.activity.as_ref().is_some_and(|a| a.successful) {
            break;
        }
    }
    trace.flush()?;
    let success = failed.is_none()
        && detached
        && rejoined
        && leg_history == vec![0, 1, 2, 3]
        && s.formation.coupled_count == 3
        && s.gameplay.missed_stops.is_empty()
        && s.state.activity.as_ref().is_some_and(|a| a.successful);
    fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":success,"original_scenario":path,"original_vehicle_ids":original,"detached":detached,"rejoined":rejoined,"leg_history":leg_history,"failure":failed,"wall_time_s":start.elapsed().as_secs_f64(),"actions":actions,"final":s.snapshot(),"traffic":traffic.snapshot()}),
        )?,
    )?;
    if !success {
        return Err(format!(
            "MidEast incomplete; see {}",
            out.join("report.json").display()
        )
        .into());
    }
    println!("MidEast original activity completed: {}", out.display());
    Ok(())
}
