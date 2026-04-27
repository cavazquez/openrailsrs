use std::path::Path;

use openrailsrs_scenarios::ScenarioFile;
use openrailsrs_scenarios::model::ObjectiveKind;
use openrailsrs_sim::runner::{SimEvent, run_scenario_headless};
use serde::Serialize;

use crate::GameError;

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    pub time_s: f64,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct PlayOutcome {
    pub success: bool,
    pub score: f64,
    pub penalties: Vec<String>,
    pub timeline: Vec<TimelineEvent>,
    pub reached_destination: bool,
    pub final_time_s: f64,
    pub overspeed_events: u32,
}

/// Run simulation then evaluate rules; writes `outcome.toml` next to scenario unless configured otherwise.
pub fn play_headless_from_scenario_file(scenario_path: &Path) -> Result<PlayOutcome, GameError> {
    let scenario_dir = scenario_path
        .parent()
        .ok_or(GameError::InvalidScenarioPath)?;
    let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;
    let sim_result = run_scenario_headless(scenario_dir, &scenario)?;
    let outcome = evaluate(&scenario, &sim_result);
    let out_path = scenario_dir.join("outcome.toml");
    let s = toml::to_string_pretty(&outcome)?;
    std::fs::write(&out_path, s)?;
    Ok(outcome)
}

fn evaluate(scenario: &ScenarioFile, sim: &openrailsrs_sim::SimRunResult) -> PlayOutcome {
    let overspeed = sim
        .events
        .iter()
        .filter(|e| matches!(e, SimEvent::OverspeedSample { .. }))
        .count() as u32;

    let mut penalties = Vec::new();
    if overspeed > 0 {
        penalties.push(format!("overspeed_samples:{overspeed}"));
    }
    if let Some(limit) = scenario.gameplay.time_limit_seconds {
        if sim.metadata.final_time_s > limit as f64 {
            penalties.push(format!(
                "late_arrival:{}s_over",
                sim.metadata.final_time_s - limit as f64
            ));
        }
    }

    let reached = sim.metadata.reached_destination;
    let on_time = scenario
        .gameplay
        .time_limit_seconds
        .map(|l| sim.metadata.final_time_s <= l as f64)
        .unwrap_or(true);

    let success = match scenario.gameplay.objective {
        ObjectiveKind::ArriveOnTime => reached && on_time && overspeed == 0,
        ObjectiveKind::Arrive => reached,
    };

    let mut score = if reached { 1000.0 } else { 0.0 };
    score -= overspeed as f64 * 5.0;
    score -= penalties.len() as f64 * 50.0;
    if score < 0.0 {
        score = 0.0;
    }

    let timeline = vec![
        TimelineEvent {
            time_s: 0.0,
            kind: "start".into(),
            detail: scenario.scenario.name.clone(),
        },
        TimelineEvent {
            time_s: sim.metadata.final_time_s,
            kind: if reached {
                "arrived".into()
            } else {
                "end".into()
            },
            detail: format!("odometer_m={}", sim.metadata.final_odometer_m),
        },
    ];

    PlayOutcome {
        success,
        score,
        penalties,
        timeline,
        reached_destination: reached,
        final_time_s: sim.metadata.final_time_s,
        overspeed_events: overspeed,
    }
}
