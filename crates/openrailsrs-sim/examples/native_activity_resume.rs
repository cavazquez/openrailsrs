//! Reproducible native-activity checkpoint validation, without a renderer.
//! Supply an imported scenario and an output directory. Original resources stay
//! in place; only the checkpoint and bounded receipt are written to that directory.
use openrailsrs_sim::{LiveDriveSession, LiveTraffic, SessionSnapshot, TrafficSnapshot};
use std::path::{Path, PathBuf};

fn states(player: &LiveDriveSession, traffic: &LiveTraffic) -> Vec<(String, f64, f64, f64)> {
    std::iter::once((
        "Jugador".into(),
        player.time_s(),
        player.head_chainage_m(),
        player.velocity_mps(),
    ))
    .chain(traffic.services.iter().map(|train| {
        (
            train.id.clone(),
            train.session.time_s(),
            train.session.head_chainage_m(),
            train.session.velocity_mps(),
        )
    }))
    .collect()
}
fn run(path: &Path, output: &Path) -> Result<(), String> {
    let scenario = openrailsrs_scenarios::load_scenario(path).map_err(|e| e.to_string())?;
    let directory = path.parent().ok_or("Scenario has no parent")?;
    let mut player =
        LiveDriveSession::from_scenario(directory, &scenario).map_err(|e| e.to_string())?;
    let mut traffic = LiveTraffic::from_scenario(directory, &scenario)?;
    for _ in 0..40 {
        traffic.advance(&mut player, 0.5, Some(0.75), |_| {});
    }
    let checkpoint = (player.snapshot(), traffic.snapshot());
    SessionSnapshot::validate_shared_dispatcher(
        std::iter::once(&checkpoint.0).chain(checkpoint.1.iter().map(|t| &t.session)),
    )?;
    let bytes = serde_json::to_vec_pretty(&checkpoint).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    std::fs::write(output.join("checkpoint.json"), &bytes).map_err(|e| e.to_string())?;
    let (saved_player, saved_traffic): (SessionSnapshot, Vec<TrafficSnapshot>) =
        serde_json::from_slice(
            &std::fs::read(output.join("checkpoint.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    let mut restored_player =
        LiveDriveSession::from_scenario(directory, &scenario).map_err(|e| e.to_string())?;
    let mut restored_traffic = LiveTraffic::from_scenario(directory, &scenario)?;
    restored_player.validate_snapshot(&saved_player)?;
    restored_traffic.validate_snapshot(&saved_traffic)?;
    // An invalid path in one actor must reject the whole traffic restore before
    // modifying any of the other actors in the newly constructed world.
    if !saved_traffic.is_empty() {
        let before = states(&restored_player, &restored_traffic);
        let mut corrupt = saved_traffic.clone();
        corrupt[0].session.state.path_edges[0] = "missing-native-vector".into();
        if restored_traffic.restore_snapshot(corrupt).is_ok()
            || before != states(&restored_player, &restored_traffic)
        {
            return Err("Malformed traffic checkpoint changed the world".into());
        }
    }
    restored_player.restore_snapshot(saved_player)?;
    restored_traffic.restore_snapshot(saved_traffic)?;
    restored_traffic.synchronize_occupancy(&mut restored_player);
    for _ in 0..10 {
        traffic.advance(&mut player, 0.5, Some(0.75), |_| {});
        restored_traffic.advance(&mut restored_player, 0.5, Some(0.75), |_| {});
    }
    let mut max_error = 0.0f64;
    for (a, b) in states(&player, &traffic)
        .iter()
        .zip(states(&restored_player, &restored_traffic))
    {
        if a.0 != b.0 {
            return Err("Restored actor identity changed".into());
        }
        max_error = max_error
            .max((a.1 - b.1).abs())
            .max((a.2 - b.2).abs())
            .max((a.3 - b.3).abs());
    }
    if max_error > 1e-8 {
        return Err(format!("Restored physics differs by {max_error}"));
    }
    let signals_equal = player.native_signals.aspects == restored_player.native_signals.aspects
        && traffic
            .services
            .iter()
            .zip(&restored_traffic.services)
            .all(|(a, b)| a.session.native_signals.aspects == b.session.native_signals.aspects);
    if !signals_equal {
        return Err("Restored native signal aspects differ".into());
    }
    let failures: Vec<_> = traffic
        .services
        .iter()
        .filter_map(|t| {
            t.session
                .gameplay
                .failure
                .as_ref()
                .map(|reason| (&t.id, reason))
        })
        .collect();
    if !failures.is_empty() {
        return Err(format!("Native traffic failed: {failures:?}"));
    }
    let receipt = serde_json::json!({
        "scope":"Native core player and traffic snapshots; renderer and camera use the separate SavedGame tests",
        "ai_count":traffic.services.len(),"checkpoint_bytes":bytes.len(),"checkpoint_time_s":20.,
        "after_resume_time_s":restored_player.time_s(),"max_clock_position_velocity_error":max_error,
        "native_signal_aspects_equal":signals_equal,"malformed_traffic_rejected_atomically":true,
        "assume_signals_clear":scenario.route.assume_signals_clear,"traffic_failures":failures,
    });
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("{receipt}");
    Ok(())
}
fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if arguments.len() != 2 {
        eprintln!("Usage: native_activity_resume <scenario.toml> <output-directory>");
        std::process::exit(2);
    }
    if let Err(error) = run(&arguments[0], &arguments[1]) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
