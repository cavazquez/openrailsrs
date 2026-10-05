//! Headless execution of the same station service and driver used in Bevy.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use openrailsrs_sim::{LiveDriveSession, ServicePhase, csv_out::RunCsvWriter};
use serde::Serialize;

#[derive(Serialize)]
struct ServiceOutcome<'a> {
    success: bool,
    phase: ServicePhase,
    scenario: &'a str,
    time_s: f64,
    distance_m: f64,
    score: f64,
    failure: Option<&'a str>,
    stops: &'a [openrailsrs_sim::ServiceStopResult],
}

pub fn run(scenario_path: &Path, out_dir: &Path, frame_dt: f64) -> Result<bool> {
    anyhow::ensure!(
        frame_dt.is_finite() && (0.001..=1.0).contains(&frame_dt),
        "frame dt must be finite, in [0.001,1] s"
    );
    let scenario_dir = scenario_path
        .parent()
        .context("scenario has no directory")?;
    let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;
    let mut session = LiveDriveSession::from_scenario(scenario_dir, &scenario)?;
    anyhow::ensure!(
        session
            .gameplay
            .stop_targets
            .last()
            .is_some_and(|s| s.is_terminal),
        "a station service needs a declared terminal stop"
    );
    std::fs::create_dir_all(out_dir)?;
    let mut csv = RunCsvWriter::new(File::create(out_dir.join("run.csv"))?)?;
    csv.write_sample(&session.state)?;
    let mut diagnostic = csv::Writer::from_path(out_dir.join("service-diagnostics.csv"))?;
    diagnostic.write_record([
        "time_s",
        "head_chainage_m",
        "route_limit_mps",
        "effective_limit_mps",
        "next_signal_distance_m",
        "next_signal_aspect",
        "next_stop_distance_m",
        "phase",
    ])?;
    while !session.arrived && session.time_s() < scenario.simulation.duration {
        let before = session.time_s();
        session.step_autodrive(frame_dt, 0.75, |_| {});
        if session.time_s() > before {
            csv.write_sample(&session.state)?;
            let signal = session.next_signal_ahead();
            diagnostic.write_record([
                session.time_s().to_string(),
                session.head_chainage_m().to_string(),
                session.speed_limit_mps().to_string(),
                session.effective_speed_limit_mps().to_string(),
                signal.map(|s| s.0.to_string()).unwrap_or_default(),
                signal.map(|s| format!("{:?}", s.1)).unwrap_or_default(),
                session
                    .distance_to_next_stop_m()
                    .map(|d| d.to_string())
                    .unwrap_or_default(),
                format!("{:?}", session.gameplay.phase),
            ])?;
        }
    }
    csv.flush()?;
    diagnostic.flush()?;
    if !session.arrived {
        session.gameplay.fail("Tiempo máximo del servicio agotado");
    }
    let success = session.gameplay.phase == ServicePhase::Completed
        && session.gameplay.stop_results.len() == session.gameplay.stop_targets.len();
    let outcome = ServiceOutcome {
        success,
        phase: session.gameplay.phase,
        scenario: &session.scenario_name,
        time_s: session.time_s(),
        distance_m: session.state.odometer_m,
        score: (1000.0 - session.gameplay.accrued_penalty).max(0.0),
        failure: session.gameplay.failure.as_deref(),
        stops: &session.gameplay.stop_results,
    };
    let json = serde_json::to_string_pretty(&outcome)?;
    std::fs::write(out_dir.join("outcome.json"), &json)?;
    println!("{json}");
    Ok(success)
}
