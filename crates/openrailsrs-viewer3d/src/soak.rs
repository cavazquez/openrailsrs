//! Opt-in real-time endurance checks. Never enabled by a saved player setting.
use bevy::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
#[derive(Resource)]
pub struct Soak {
    routes: Vec<SoakRoute>,
    next_route: f64,
    route_index: usize,
    route_changes: u32,
    route_interval: f64,
    start: Instant,
    duration: f64,
    next_sample: f64,
    next_cycle: f64,
    rows: u32,
    cycles: u32,
    snapshots: u32,
    memory_failures: u32,
    memory_last_error: Option<String>,
    disk_path: PathBuf,
    disk_attempts: u32,
    disk_cycles: u32,
    disk_failures: u32,
    disk_last_bytes: Option<u64>,
    disk_last_sha256: Option<String>,
    disk_last_error: Option<String>,
    path: PathBuf,
}
#[derive(Clone, serde::Deserialize)]
struct SoakRoute {
    path: PathBuf,
    route_root: Option<PathBuf>,
}
fn next_deadline(age: f64, period: f64) -> f64 {
    // An absolute cadence prevents loading delays from accumulating on every
    // round trip. Missed slots are skipped, never counted as completed work.
    (age / period).floor().mul_add(period, period)
}
impl Soak {
    pub fn finished(&self) -> bool {
        self.start.elapsed().as_secs_f64() >= self.duration
    }
}
pub fn install(app: &mut App) {
    let Some(seconds) = std::env::var("OPENRAILSRS_SOAK_SECONDS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && (30. ..=21600.).contains(s))
    else {
        return;
    };
    let Some(path) = std::env::var_os("OPENRAILSRS_SOAK_OUT").map(PathBuf::from) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&path, "").is_err() {
        return;
    }
    // A fresh QA directory keeps this test outside the player's save slots
    // and prevents the normal save-history mechanism from accumulating files.
    let disk_dir = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            "soak-disk-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
    if let Err(error) = std::fs::create_dir(&disk_dir) {
        eprintln!("Cannot create private soak save directory: {error}");
        return;
    }
    let route_interval = std::env::var("OPENRAILSRS_SOAK_ROUTE_INTERVAL_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && (30. ..=600.).contains(s))
        .unwrap_or(180.);
    app.insert_resource(Soak {
        routes: std::env::var("OPENRAILSRS_SOAK_ROUTES")
            .ok()
            .filter(|s| s.len() <= 8192)
            .and_then(|s| serde_json::from_str::<Vec<SoakRoute>>(&s).ok())
            .filter(|routes| {
                routes.len() <= 8
                    && routes.iter().all(|r| {
                        r.path.is_absolute()
                            && r.path.is_file()
                            && r.route_root
                                .as_ref()
                                .is_none_or(|p| p.is_absolute() && p.is_dir())
                    })
            })
            .unwrap_or_default(),
        route_interval,
        next_route: route_interval,
        route_index: 0,
        route_changes: 0,
        start: Instant::now(),
        duration: seconds,
        next_sample: 30.,
        next_cycle: 60.,
        rows: 0,
        cycles: 0,
        snapshots: 0,
        memory_failures: 0,
        memory_last_error: None,
        disk_path: disk_dir.join("round-trip.json"),
        disk_attempts: 0,
        disk_cycles: 0,
        disk_failures: 0,
        disk_last_bytes: None,
        disk_last_sha256: None,
        disk_last_error: None,
        path,
    })
    .add_systems(
        Last,
        sample.run_if(in_state(crate::ViewerAppState::Playing)),
    );
}
fn json_world_round_trip(live: &mut crate::live::LiveDrive) -> Result<(), String> {
    let player = live.session.snapshot_with_scripts()?;
    let traffic = live.traffic.snapshot_with_scripts()?;
    let before = serde_json::to_value((&player, &traffic)).map_err(|error| error.to_string())?;
    let encoded = serde_json::to_vec(&(&player, &traffic)).map_err(|error| error.to_string())?;
    let (player, traffic) = serde_json::from_slice(&encoded).map_err(|error| error.to_string())?;
    live.traffic
        .restore_shared_snapshots(&mut live.session, player, traffic)?;
    let after = serde_json::to_value((
        live.session.snapshot_with_scripts()?,
        live.traffic.snapshot_with_scripts()?,
    ))
    .map_err(|error| error.to_string())?;
    if before != after {
        return Err("JSON world round trip changed player, traffic or interlocking".into());
    }
    Ok(())
}

fn saved_game_round_trip(
    live: &mut crate::live::LiveDrive,
    content: &crate::player_launch::ActivePlayerContent,
    camera: crate::saved_game::SavedCamera,
    path: &Path,
    weather: Option<&crate::weather_state::WeatherState>,
    settings: Option<&crate::player_settings::PlayerSettings>,
) -> Result<(crate::saved_game::SavedCamera, u64, String), String> {
    let before = serde_json::to_value((live.session.snapshot(), live.traffic.snapshot()))
        .map_err(|error| error.to_string())?;
    let camera_before = serde_json::to_value(&camera).map_err(|error| error.to_string())?;
    let paused = live.paused;
    let start_clock = live.start_clock_s;
    let season = live.season.clone();
    crate::saved_game::SavedGame::save(live, content, camera, path, weather, settings)?;
    let saved = crate::saved_game::SavedGame::read(path)?;
    let camera = saved.restore(live)?;
    // Player resume pauses; the endurance driver preserves its prior state
    // so it cannot quietly stop travelling after the first disk restore.
    live.paused = paused;
    let after = serde_json::to_value((live.session.snapshot(), live.traffic.snapshot()))
        .map_err(|error| error.to_string())?;
    if before != after || live.start_clock_s != start_clock || live.season != season {
        return Err("Disk round trip changed session, traffic, clock or season".into());
    }
    if serde_json::to_value(&camera).map_err(|error| error.to_string())? != camera_before {
        return Err("Disk round trip changed the saved camera".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    Ok((
        camera,
        bytes.len() as u64,
        format!("{:x}", Sha256::digest(&bytes)),
    ))
}
fn sample(world: &mut World) {
    let age = world.resource::<Soak>().start.elapsed().as_secs_f64();
    // Freeze travel at the requested wall time, letting streaming and GPU
    // publication finish before the final readiness-gated capture.
    if world.resource::<Soak>().finished()
        && let Some(mut live) = world.get_resource_mut::<crate::live::LiveDrive>()
    {
        live.paused = true;
    }
    let cycle = age >= world.resource::<Soak>().next_cycle;
    if cycle {
        let count = world.resource::<Soak>().cycles;
        world
            .resource_mut::<crate::player_settings::PlayerSettings>()
            .weather_profile = match count % 4 {
            0 => crate::weather_state::WeatherProfile::Downpour,
            1 => crate::weather_state::WeatherProfile::HeavySnow,
            2 => crate::weather_state::WeatherProfile::StormCycle,
            _ => crate::weather_state::WeatherProfile::Automatic,
        };
        *world.resource_mut::<crate::camera::CameraFollowMode>() = if count.is_multiple_of(2) {
            crate::camera::CameraFollowMode::ChaseCam
        } else {
            crate::camera::CameraFollowMode::DriverCam
        };
        let memory_result = world.resource_scope(|_, mut live: Mut<crate::live::LiveDrive>| {
            json_world_round_trip(&mut live)
        });
        match memory_result {
            Ok(()) => {
                let mut state = world.resource_mut::<Soak>();
                state.snapshots += 1;
                state.memory_last_error = None;
            }
            Err(error) => {
                let mut state = world.resource_mut::<Soak>();
                state.memory_failures += 1;
                state.memory_last_error = Some(error);
            }
        }
        let disk_path = world.resource::<Soak>().disk_path.clone();
        let orbit = *world
            .query_filtered::<
                &crate::camera::OrbitState,
                (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
            >()
            .single(world)
            .expect("one player orbit camera during soak");
        let camera = crate::saved_game::SavedCamera::capture(
            *world.resource::<crate::camera::CameraFollowMode>(),
            &orbit,
            world.resource::<crate::camera::DriverLookOffset>(),
            world.resource::<crate::floating_origin::FloatingOrigin>(),
        );
        let result = world.resource_scope(|world, mut live: Mut<crate::live::LiveDrive>| {
            saved_game_round_trip(
                &mut live,
                world.resource::<crate::player_launch::ActivePlayerContent>(),
                camera,
                &disk_path,
                world.get_resource::<crate::weather_state::WeatherState>(),
                world.get_resource::<crate::player_settings::PlayerSettings>(),
            )
        });
        let camera = match result {
            Ok((camera, bytes, sha256)) => {
                let mut state = world.resource_mut::<Soak>();
                state.disk_cycles += 1;
                state.disk_last_bytes = Some(bytes);
                state.disk_last_sha256 = Some(sha256);
                state.disk_last_error = None;
                Some(camera)
            }
            Err(error) => {
                let mut state = world.resource_mut::<Soak>();
                state.disk_failures += 1;
                state.disk_last_error = Some(error);
                None
            }
        };
        if let Some(camera) = camera {
            world.insert_resource(crate::saved_game::PendingSavedCamera(camera));
        }
        let mut state = world.resource_mut::<Soak>();
        state.disk_attempts += 1;
        state.cycles += 1;
        state.next_cycle = next_deadline(age, 60.);
    }
    if age < world.resource::<Soak>().next_sample {
        return;
    }
    let state = world.resource::<Soak>();
    let path = state.path.clone();
    let weather_state = world
        .get_resource::<crate::weather_state::WeatherState>()
        .map(|weather| {
            serde_json::json!({
                "profile": weather.profile,
                "seed": weather.seed,
                "live": weather.live,
                "journey": weather.journey,
                "timeline_elapsed_s": weather.timeline_elapsed_s(),
                "atmosphere": weather.atmosphere,
            })
        });
    let camera_follow = format!("{:?}", world.resource::<crate::camera::CameraFollowMode>());
    let row = serde_json::json!({"wall_s":age,"cycles":state.cycles,"save_restore_cycles":state.snapshots,"route_changes":state.route_changes,"scenario":world.resource::<crate::live::LiveDrive>().session.scenario_name,
        "schema":3,"memory_restore_failures":state.memory_failures,"memory_restore_last_error":state.memory_last_error,"camera_follow":camera_follow,"weather_state":weather_state,"disk_save_attempts":state.disk_attempts,"disk_save_restore_cycles":state.disk_cycles,"disk_save_failures":state.disk_failures,"disk_save_last_bytes":state.disk_last_bytes,"disk_save_last_sha256":state.disk_last_sha256,"disk_save_last_error":state.disk_last_error,
        "entities":world.entities().count_spawned(),"entity_index_capacity":world.entities().len(),"meshes":world.resource::<Assets<Mesh>>().len(),"images":world.resource::<Assets<Image>>().len(),
        "fonts":world.resource::<Assets<Font>>().len(),
        "font_atlases":world.resource::<bevy::text::FontAtlasSet>().values().flatten().count(),
        "shaders":world.resource::<Assets<bevy::shader::Shader>>().len(),
        "effect_assets":world.resource::<Assets<bevy_hanabi::EffectAsset>>().len(),
        "train_effects":world.get_resource::<crate::train_effects::TrainEffects>().map(|effects|effects.report()),
        "weather_particles":world.get_resource::<crate::weather_particles::WeatherParticles>().map(|particles|particles.report()),
        "render_resources":world.resource::<crate::performance::ScenePipelineStatus>().render_resources(),
        "native_audio":world.resource::<crate::native_audio::NativeAudio>().engine.as_ref().map(|e|e.report()),
        "graphics_memory":world.resource::<crate::gpu_memory::GraphicsMemory>().0,
        "performance":world.resource::<crate::performance::JourneyPerformance>().report(),
        "heap_allocator":crate::performance::heap_allocator_report(),
        "geometry_cache":crate::prepared_geometry::report(),
        "shape_stages":crate::performance::shape_stages_report(),
        "scene_cleanup":world.resource::<crate::scene_cleanup::SceneCleanup>().report(),
        "render_budget":world.resource::<crate::render_budget::RenderBudget>(),
        "residency":{ "pressured":world.resource::<crate::gpu_memory::ResidencyPolicy>().pressured,"transitions":world.resource::<crate::gpu_memory::ResidencyPolicy>().transitions,"budget_mib":world.resource::<crate::gpu_memory::ResidencyPolicy>().budget_mib,"basis":world.resource::<crate::gpu_memory::ResidencyPolicy>().basis},
        "renderer":world.resource::<crate::performance::ScenePipelineStatus>().device(),
        "pending_uploads":world.resource::<crate::performance::ScenePipelineStatus>().pending_uploads()});
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) {
        let _ = writeln!(f, "{row}");
    }
    let mut state = world.resource_mut::<Soak>();
    state.rows += 1;
    state.next_sample = next_deadline(age, 30.);
    if age >= state.next_route && !state.routes.is_empty() && !state.finished() {
        let route = state.routes[state.route_index % state.routes.len()].clone();
        state.route_index += 1;
        state.route_changes += 1;
        state.next_route = next_deadline(age, state.route_interval);
        let environment = world
            .resource::<crate::player_settings::PlayerSettings>()
            .environment;
        world
            .resource_mut::<crate::player_launch::PlayerLaunchQueue>()
            .0 = Some(crate::player_launch::QueuedPlayerLaunch {
            path: route.path,
            route_root: route.route_root,
            weather: environment.manual_weather,
            environment,
            resume: None,
            weather_checkpoint: None,
            player_options: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> crate::saved_game::SavedCamera {
        crate::saved_game::SavedCamera::capture(
            crate::camera::CameraFollowMode::ChaseCam,
            &crate::camera::OrbitState::default(),
            &crate::camera::DriverLookOffset::default(),
            &crate::floating_origin::FloatingOrigin::default(),
        )
    }

    fn unequal_paths(
        a: &serde_json::Value,
        b: &serde_json::Value,
        path: &str,
        out: &mut Vec<String>,
    ) {
        if a == b || out.len() >= 32 {
            return;
        }
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
                for key in keys {
                    unequal_paths(
                        &a.get(key).cloned().unwrap_or_default(),
                        &b.get(key).cloned().unwrap_or_default(),
                        &format!("{path}/{key}"),
                        out,
                    );
                }
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) if a.len() == b.len() => {
                for (index, (a, b)) in a.iter().zip(b).enumerate() {
                    unequal_paths(a, b, &format!("{path}/{index}"), out);
                }
            }
            _ => out.push(format!("{path}: {a} -> {b}")),
        }
    }

    fn assert_disk_round_trip(relative: &str, with_traffic: bool) {
        let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
        let fixture = || {
            let mut live = crate::live::LiveDrive::from_scenario_path(&scenario).unwrap();
            live.session.driver_direction = 1.;
            live.session.driver_throttle = 0.7;
            if with_traffic {
                assert!(!live.traffic.services.is_empty());
                // The complete equality assertion covers every saved AI field,
                // including inputs changed before a real-world traffic tick.
                live.traffic.services[0].session.driver_brake = 0.35;
                live.traffic.services[0].session.set_direction(0.0).unwrap();
            }
            live.traffic.advance(&mut live.session, 3., None, |_| {});
            live
        };
        let mut live = fixture();
        let mut control = fixture();
        let content = crate::player_launch::ActivePlayerContent::default();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("qa-round-trip.json");
        for paused in [false, true] {
            live.paused = paused;
            control.paused = paused;
            let before =
                serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap();
            let result = saved_game_round_trip(&mut live, &content, camera(), &path, None, None);
            if result.is_err() {
                let after =
                    serde_json::to_value((live.session.snapshot(), live.traffic.snapshot()))
                        .unwrap();
                let mut changed = Vec::new();
                unequal_paths(&before, &after, "", &mut changed);
                eprintln!("SavedGame round-trip differences: {changed:?}");
            }
            let (_, bytes, hash) = result.unwrap();
            assert_eq!(live.paused, paused);
            assert_eq!(bytes, std::fs::metadata(&path).unwrap().len());
            assert_eq!(hash.len(), 64);
            let saved = crate::saved_game::SavedGame::read(&path).unwrap();
            assert_eq!(
                serde_json::to_value(saved.session).unwrap(),
                serde_json::to_value(live.session.snapshot()).unwrap()
            );
            assert_eq!(
                serde_json::to_value(saved.traffic).unwrap(),
                serde_json::to_value(live.traffic.snapshot()).unwrap()
            );
            assert!(toml::from_str::<toml::Value>(&saved.scenario_toml).is_ok());
            // Resume a paused drive exactly as the user does. The uninterrupted
            // control never serializes/restores. A non-integral wall delta
            // exercises the saved substep remainder as well as dispatch,
            // signal decisions, brake/fuel physics and AI on the next tick.
            live.paused = false;
            control.paused = false;
            live.traffic.advance(&mut live.session, 0.17, None, |_| {});
            control
                .traffic
                .advance(&mut control.session, 0.17, None, |_| {});
            let continued =
                serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap();
            let expected =
                serde_json::to_value((control.session.snapshot(), control.traffic.snapshot()))
                    .unwrap();
            let mut changed = Vec::new();
            unequal_paths(&expected, &continued, "", &mut changed);
            assert!(
                changed.is_empty(),
                "Next traffic tick changed decisions: {changed:?}"
            );
        }
    }

    #[test]
    fn disk_round_trip_restores_player_envelope_and_preserves_running_or_paused_state() {
        assert_disk_round_trip("../../examples/smoke/scenario.toml", false);
    }

    #[test]
    fn disk_round_trip_preserves_nonempty_traffic_and_dispatch_state() {
        assert_disk_round_trip("../../examples/traffic_passing/scenario_control.toml", true);
        assert_disk_round_trip("../../examples/traffic_passing/scenario_meet.toml", true);
    }

    #[test]
    fn pre_run_points_are_shared_before_first_tick_and_both_round_trips() {
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let directory = tempfile::tempdir().unwrap();
        let track = "[route]\nid='warm-points'\n[[nodes]]\nid='A'\nx_m=0.0\ny_m=0.0\n[[nodes]]\nid='B'\nx_m=1000.0\ny_m=0.0\nkind={ switch={ stem_edge='main', diverging_edge='loop', default_position='straight' } }\n[[nodes]]\nid='C'\nx_m=1500.0\ny_m=0.0\n[[nodes]]\nid='D'\nx_m=1500.0\ny_m=100.0\n[[edges]]\nid='approach'\nfrom='A'\nto='B'\nlength_m=1000.0\nspeed_limit_kmh=36.0\n[[edges]]\nid='approach_r'\nfrom='B'\nto='A'\nlength_m=1000.0\nspeed_limit_kmh=36.0\n[[edges]]\nid='main'\nfrom='B'\nto='C'\nlength_m=500.0\nspeed_limit_kmh=36.0\n[[edges]]\nid='main_r'\nfrom='C'\nto='B'\nlength_m=500.0\nspeed_limit_kmh=36.0\n[[edges]]\nid='loop'\nfrom='B'\nto='D'\nlength_m=510.0\nspeed_limit_kmh=36.0\n";
        std::fs::write(directory.path().join("track.toml"), track).unwrap();
        let mut scenario =
            openrailsrs_scenarios::load_scenario(project.join("examples/smoke/scenario.toml"))
                .unwrap();
        scenario.route.path = ".".into();
        scenario.route.start = "C".into();
        scenario.route.destination = "A".into();
        scenario.route.stops.clear();
        scenario.route.waypoints.clear();
        scenario.route.switches.clear();
        scenario.sound_regions.clear();
        scenario.train.consist = project
            .join("examples/smoke/consists/freight.con")
            .to_string_lossy()
            .into();
        scenario.extra_trains = vec![
            serde_json::from_value(serde_json::json!({
                "id":"A_ai", "consist":scenario.train.consist,
                "start":"A", "destination":"D", "start_offset_m":980.0,
                "pre_roll_s":1.0, "output_csv":"ai.csv",
                "switches":[{"node":"B","position":"diverging"}]
            }))
            .unwrap(),
        ];
        let path = directory.path().join("scenario.toml");
        std::fs::write(&path, toml::to_string(&scenario).unwrap()).unwrap();
        let mut live = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        assert_eq!(live.session.time_s(), 0.0);
        // Shifting the pre-run clock subtracts accumulated floating-point steps.
        assert!(live.traffic.services[0].session.time_s().abs() < 1e-10);
        assert_eq!(
            live.session.graph.switch_position("B"),
            Some(openrailsrs_track::SwitchPosition::Diverging)
        );
        assert_eq!(
            live.traffic.services[0].session.graph.switch_position("B"),
            Some(openrailsrs_track::SwitchPosition::Diverging)
        );
        assert_eq!(
            live.traffic.services[0]
                .session
                .snapshot()
                .dispatcher
                .own_locks[0]
                .node,
            "B"
        );
        let before =
            serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap();
        json_world_round_trip(&mut live).unwrap();
        let save = directory.path().join("before-first-tick.json");
        saved_game_round_trip(&mut live, &default(), camera(), &save, None, None).unwrap();
        assert_eq!(
            before,
            serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap()
        );
        // A genuine incompatible position still fails, before changing state.
        let mut bad = crate::saved_game::SavedGame::read(&save).unwrap();
        bad.session
            .switches
            .insert("B".into(), openrailsrs_track::SwitchPosition::Straight);
        assert!(bad.restore(&mut live).is_err());
        assert_eq!(
            before,
            serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap()
        );
    }

    fn native_scenario(with_traffic: bool) -> (tempfile::TempDir, PathBuf) {
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let directory = tempfile::tempdir().unwrap();
        let mut track = "[route]\nid='saved-native'\n[[nodes]]\nid='A'\nx_m=0.0\ny_m=0.0\n[[nodes]]\nid='B'\nx_m=1000.0\ny_m=0.0\n".to_owned();
        track +=
            "[[nodes]]\nid='X'\nx_m=0.0\ny_m=100.0\n[[nodes]]\nid='Y'\nx_m=1000.0\ny_m=100.0\n";
        for (id, from, to) in [
            ("e1", "A", "B"),
            ("e1_r", "B", "A"),
            ("p1", "X", "Y"),
            ("p1_r", "Y", "X"),
        ] {
            track += &format!(
                "[[edges]]\nid='{id}'\nfrom='{from}'\nto='{to}'\nlength_m=1000.0\nspeed_limit_kmh=36.0\n"
            );
        }
        for (id, edge, position) in [
            ("f_entry", "e1", 100),
            ("f_boundary", "e1", 300),
            ("f_second", "e1", 500),
            ("f_second_exit", "e1", 700),
            ("f_exit", "e1", 900),
            ("r_entry", "e1_r", 100),
            ("r_exit", "e1_r", 900),
            ("p_entry", "p1", 100),
            ("p_exit", "p1", 900),
        ] {
            track += &format!(
                "[[signals]]\nid='{id}'\nedge_id='{edge}'\nposition_m={position}.0\naspect='stop'\n[signals.script.native]\nname='own-fixture-home'\nfunction='NORMAL'\nsource='if (!enabled || block_state()!=BLOCK_CLEAR || !route_set()) {{state=SIGASP_STOP;}} else {{state=SIGASP_CLEAR_2;}}'\n"
            );
        }
        std::fs::write(directory.path().join("track.toml"), track).unwrap();
        let mut scenario =
            openrailsrs_scenarios::load_scenario(project.join("examples/smoke/scenario.toml"))
                .unwrap();
        scenario.route.path = ".".into();
        scenario.route.start = "A".into();
        scenario.route.destination = "B".into();
        scenario.route.stops.clear();
        scenario.route.waypoints.clear();
        scenario.route.switches.clear();
        scenario.route.assume_signals_clear = false;
        scenario.sound_regions.clear();
        scenario.extra_trains.clear();
        scenario.train.consist = project
            .join("examples/smoke/consists/freight.con")
            .to_string_lossy()
            .into();
        if with_traffic {
            scenario.extra_trains.push(
                serde_json::from_value(serde_json::json!({
                    "id":"opposite", "consist":scenario.train.consist,
                    "start":"B", "destination":"A", "output_csv":"opposite.csv"
                }))
                .unwrap(),
            );
            // Its request sorts before Jugador, while the service vector has
            // opposite first. Rebinding foreign grants must not interpret
            // that different storage order as a changed authority.
            scenario.extra_trains.push(
                serde_json::from_value(serde_json::json!({
                    "id":"A_ahead", "consist":scenario.train.consist,
                    "start":"X", "destination":"Y", "parked":true,
                    "output_csv":"ahead.csv"
                }))
                .unwrap(),
            );
        }
        let path = directory.path().join("scenario.toml");
        std::fs::write(&path, toml::to_string(&scenario).unwrap()).unwrap();
        (directory, path)
    }

    fn native_drive(path: &Path, pending_refresh: bool) -> crate::live::LiveDrive {
        let mut live = crate::live::LiveDrive::from_scenario_path(path).unwrap();
        live.session.set_direction(1.).unwrap();
        live.session.driver_throttle = 0.2;
        live.traffic.synchronize_occupancy(&mut live.session);
        live.traffic.advance(&mut live.session, 0.37, None, |_| {});
        live.traffic.synchronize_occupancy(&mut live.session);
        assert!(live.session.native_signals.count() > 0);
        if !live.traffic.services.is_empty() {
            assert_eq!(live.traffic.services.len(), 2);
            assert!(!live.session.snapshot().track_reservations.is_empty());
            assert!(
                !live.traffic.services[1]
                    .session
                    .snapshot()
                    .track_reservations
                    .is_empty()
            );
            assert_eq!(
                live.traffic.services[0]
                    .session
                    .native_signal_aspect("r_entry"),
                Some(0)
            );
        }
        if pending_refresh {
            live.session.authorize_shunting("f_entry", 150.).unwrap();
        }
        assert_eq!(
            live.session
                .snapshot()
                .native_signal_cache
                .as_ref()
                .unwrap()
                .needs_refresh,
            pending_refresh
        );
        live
    }

    #[test]
    fn native_cache_preserves_pending_and_completed_refresh_with_and_without_ai() {
        for with_traffic in [false, true] {
            let (_directory, scenario) = native_scenario(with_traffic);
            for pending_refresh in [false, true] {
                let mut live = native_drive(&scenario, pending_refresh);
                let mut control = native_drive(&scenario, pending_refresh);
                let temp = tempfile::tempdir().unwrap();
                let save = temp.path().join("native-round-trip.json");
                saved_game_round_trip(&mut live, &default(), camera(), &save, None, None).unwrap();
                assert_eq!(
                    live.session.native_signals.aspects,
                    control.session.native_signals.aspects
                );
                assert_eq!(
                    live.session.native_signals.errors,
                    control.session.native_signals.errors
                );
                assert_eq!(
                    live.session.snapshot().native_signal_cache,
                    control.session.snapshot().native_signal_cache
                );
                // Cross several ordinary quanta and a full signal evaluation
                // period. All snapshots include cadence and raw native aspects.
                for seconds in [0.17, 1.13] {
                    live.traffic
                        .advance(&mut live.session, seconds, None, |_| {});
                    control
                        .traffic
                        .advance(&mut control.session, seconds, None, |_| {});
                    let after =
                        serde_json::to_value((live.session.snapshot(), live.traffic.snapshot()))
                            .unwrap();
                    let expected = serde_json::to_value((
                        control.session.snapshot(),
                        control.traffic.snapshot(),
                    ))
                    .unwrap();
                    let mut changed = Vec::new();
                    unequal_paths(&expected, &after, "", &mut changed);
                    assert!(
                        changed.is_empty(),
                        "Native next-step decisions changed (AI={with_traffic}, pending={pending_refresh}): {changed:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn invalid_native_cache_cannot_change_live_state_or_free_a_blocked_ai() {
        let (_directory, scenario) = native_scenario(true);
        let mut live = native_drive(&scenario, false);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("native-round-trip.json");
        crate::saved_game::SavedGame::save(&mut live, &default(), camera(), &path, None, None)
            .unwrap();
        for mutation in 0..5 {
            let mut saved = crate::saved_game::SavedGame::read(&path).unwrap();
            let cache = saved.traffic[0]
                .session
                .native_signal_cache
                .as_mut()
                .unwrap();
            match mutation {
                0 => cache.version = 2,
                1 => cache.aspects.push(cache.aspects[0].clone()),
                2 => cache.aspects[0].0 = "unknown-native-head".into(),
                3 => cache.aspects[0].1 = 8,
                _ => {
                    cache
                        .aspects
                        .iter_mut()
                        .find(|(id, _)| id == "r_entry")
                        .unwrap()
                        .1 = 7
                }
            }
            let before =
                serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap();
            assert!(
                saved.restore(&mut live).is_err(),
                "Cache mutation {mutation} accepted"
            );
            assert_eq!(
                before,
                serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap()
            );
            assert_eq!(
                live.traffic.services[0]
                    .session
                    .native_signal_aspect("r_entry"),
                Some(0)
            );
        }
    }

    #[test]
    fn older_version_one_save_without_native_cache_rebuilds_conservatively() {
        let (_directory, scenario) = native_scenario(true);
        let mut live = native_drive(&scenario, false);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("legacy-native.json");
        crate::saved_game::SavedGame::save(&mut live, &default(), camera(), &path, None, None)
            .unwrap();
        let mut old: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        old["session"]
            .as_object_mut()
            .unwrap()
            .remove("native_signal_cache");
        for service in old["traffic"].as_array_mut().unwrap() {
            service["session"]
                .as_object_mut()
                .unwrap()
                .remove("native_signal_cache");
        }
        let saved =
            crate::saved_game::SavedGame::from_bytes(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(saved.version, 1);
        let signal_steps = saved.session.signal_steps;
        saved.restore(&mut live).unwrap();
        let after = live.session.snapshot();
        assert_eq!(after.signal_steps, signal_steps);
        assert!(after.native_signal_cache.unwrap().needs_refresh);
        assert_eq!(
            live.traffic.services[0]
                .session
                .native_signal_aspect("r_entry"),
            Some(0)
        );
        live.traffic.advance(&mut live.session, 0.17, None, |_| {});
        assert!(live.session.native_signals.errors.is_empty());
        assert!(
            live.traffic.services[0]
                .session
                .native_signals
                .errors
                .is_empty()
        );
    }

    #[test]
    fn forged_foreign_lock_is_rejected_before_mutating_live_state() {
        let scenario = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traffic_passing/scenario_meet.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&scenario).unwrap();
        live.traffic.advance(&mut live.session, 3., None, |_| {});
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("qa-round-trip.json");
        crate::saved_game::SavedGame::save(
            &mut live,
            &crate::player_launch::ActivePlayerContent::default(),
            camera(),
            &path,
            None,
            None,
        )
        .unwrap();
        let mut saved = crate::saved_game::SavedGame::read(&path).unwrap();
        saved
            .session
            .dispatcher
            .other_locks
            .push(openrailsrs_sim::SwitchLock {
                node: "junction".into(),
                position: openrailsrs_track::SwitchPosition::Diverging,
                owner: "service-not-present".into(),
            });
        let before =
            serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap();
        assert!(saved.restore(&mut live).is_err());
        assert_eq!(
            before,
            serde_json::to_value((live.session.snapshot(), live.traffic.snapshot())).unwrap()
        );
    }

    #[test]
    fn failed_disk_save_preserves_existing_file_and_live_state() {
        let scenario =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&scenario).unwrap();
        let before = serde_json::to_value(live.session.snapshot()).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("file-not-directory");
        std::fs::write(&parent, b"preserve me").unwrap();
        assert!(
            saved_game_round_trip(
                &mut live,
                &crate::player_launch::ActivePlayerContent::default(),
                camera(),
                &parent.join("slot.json"),
                None,
                None
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&parent).unwrap(), b"preserve me");
        assert_eq!(
            serde_json::to_value(live.session.snapshot()).unwrap(),
            before
        );
    }
}
