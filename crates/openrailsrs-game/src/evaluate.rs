use std::path::Path;

use openrailsrs_scenarios::ScenarioFile;
use openrailsrs_scenarios::model::ObjectiveKind;
use openrailsrs_sim::runner::{SimEvent, run_scenario_headless};
use serde::Serialize;

use crate::GameError;

/// Grace window (seconds) for stop timing before a penalty is applied.
const STOP_GRACE_S: f64 = 30.0;

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEvent {
    pub time_s: f64,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopResult {
    pub node: String,
    pub scheduled_arrive_s: f64,
    pub actual_arrive_s: Option<f64>,
    pub actual_depart_s: Option<f64>,
    pub on_time: bool,
    pub missed: bool,
    /// True when the train departed before `scheduled_depart_s - STOP_GRACE_S`.
    pub early_departure: bool,
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
    pub stops: Vec<StopResult>,
}

/// Run simulation then evaluate rules; writes `outcome.toml` next to scenario.
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
    let mut timeline = Vec::new();
    let mut stop_results = Vec::new();

    timeline.push(TimelineEvent {
        time_s: 0.0,
        kind: "start".into(),
        detail: scenario.scenario.name.clone(),
    });

    if overspeed > 0 {
        penalties.push(format!("overspeed_samples:{overspeed}"));
    }

    // Evaluate each declared intermediate stop.
    for stop_def in &scenario.route.stops {
        let actual_arrive = sim.events.iter().find_map(|e| match e {
            SimEvent::StationArrival { node_id, time_s } if node_id == &stop_def.node => {
                Some(*time_s)
            }
            _ => None,
        });

        let actual_depart = sim.events.iter().find_map(|e| match e {
            SimEvent::StationDeparture { node_id, time_s } if node_id == &stop_def.node => {
                Some(*time_s)
            }
            _ => None,
        });

        match actual_arrive {
            None => {
                penalties.push(format!("missed_stop:{}", stop_def.node));
                timeline.push(TimelineEvent {
                    time_s: stop_def.arrive_s,
                    kind: "missed_stop".into(),
                    detail: format!(
                        "node={} scheduled_arrive={:.0}s",
                        stop_def.node, stop_def.arrive_s
                    ),
                });
                stop_results.push(StopResult {
                    node: stop_def.node.clone(),
                    scheduled_arrive_s: stop_def.arrive_s,
                    actual_arrive_s: None,
                    actual_depart_s: None,
                    on_time: false,
                    missed: true,
                    early_departure: false,
                });
            }
            Some(t) => {
                let delay = t - stop_def.arrive_s;
                let on_time = delay <= STOP_GRACE_S;
                if !on_time {
                    penalties.push(format!(
                        "late_stop:{}:{:.0}s_over",
                        stop_def.node,
                        delay - STOP_GRACE_S
                    ));
                }
                timeline.push(TimelineEvent {
                    time_s: t,
                    kind: "station_arrival".into(),
                    detail: format!(
                        "node={} scheduled={:.0}s actual={:.0}s",
                        stop_def.node, stop_def.arrive_s, t
                    ),
                });

                // Check for early departure.
                let early_departure = actual_depart
                    .map(|td| td < stop_def.depart_s - STOP_GRACE_S)
                    .unwrap_or(false);
                if early_departure {
                    let ahead = stop_def.depart_s - actual_depart.unwrap_or(t);
                    penalties.push(format!(
                        "early_departure:{}:{:.0}s_early",
                        stop_def.node, ahead
                    ));
                    if let Some(td) = actual_depart {
                        timeline.push(TimelineEvent {
                            time_s: td,
                            kind: "early_departure".into(),
                            detail: format!(
                                "node={} scheduled_depart={:.0}s actual_depart={:.0}s",
                                stop_def.node, stop_def.depart_s, td
                            ),
                        });
                    }
                }

                stop_results.push(StopResult {
                    node: stop_def.node.clone(),
                    scheduled_arrive_s: stop_def.arrive_s,
                    actual_arrive_s: Some(t),
                    actual_depart_s: actual_depart,
                    on_time,
                    missed: false,
                    early_departure,
                });
            }
        }
    }

    if let Some(limit) = scenario.gameplay.time_limit_seconds {
        if sim.metadata.final_time_s > limit as f64 {
            penalties.push(format!(
                "late_arrival:{:.0}s_over",
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
    let stops_ok = stop_results
        .iter()
        .all(|s| s.on_time && !s.missed && !s.early_departure);

    let success = match scenario.gameplay.objective {
        ObjectiveKind::ArriveOnTime => reached && on_time && overspeed == 0 && stops_ok,
        ObjectiveKind::Arrive => reached,
    };

    let mut score = if reached { 1000.0 } else { 0.0 };
    score -= overspeed as f64 * 5.0;
    score -= penalties.len() as f64 * 50.0;
    if score < 0.0 {
        score = 0.0;
    }

    timeline.push(TimelineEvent {
        time_s: sim.metadata.final_time_s,
        kind: if reached {
            "arrived".into()
        } else {
            "end".into()
        },
        detail: format!("odometer_m={:.0}", sim.metadata.final_odometer_m),
    });

    PlayOutcome {
        success,
        score,
        penalties,
        timeline,
        reached_destination: reached,
        final_time_s: sim.metadata.final_time_s,
        overspeed_events: overspeed,
        stops: stop_results,
    }
}
