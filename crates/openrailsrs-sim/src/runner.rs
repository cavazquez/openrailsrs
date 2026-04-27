use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::ScenarioFile;
use openrailsrs_train::{DavisCoefficients, load_consist_with_asset_root};
use serde::Serialize;

use crate::SimError;
use crate::csv_out::RunCsvWriter;
use crate::path::edge_path;
use crate::physics::{TrainPhysics, step};
use crate::state::TrainSimState;

#[derive(Debug, Clone, Copy)]
pub struct DriverInput {
    pub throttle: f64,
    pub brake: f64,
}

pub trait Driver {
    fn decide(&mut self, state: &TrainSimState, speed_limit_mps: f64) -> DriverInput;
}

#[derive(Debug, Default)]
pub struct AutoDriver;

impl Driver for AutoDriver {
    fn decide(&mut self, state: &TrainSimState, speed_limit_mps: f64) -> DriverInput {
        if state.velocity_mps > speed_limit_mps * 1.02 {
            DriverInput {
                throttle: 0.0,
                brake: 0.35,
            }
        } else if state.velocity_mps < speed_limit_mps * 0.92 {
            DriverInput {
                throttle: 1.0,
                brake: 0.0,
            }
        } else {
            DriverInput {
                throttle: 0.2,
                brake: 0.0,
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum SimEvent {
    OverspeedSample {
        time_s: f64,
        edge_id: String,
        speed_mps: f64,
        limit_mps: f64,
    },
    StationArrival {
        time_s: f64,
        node_id: String,
    },
    StationDeparture {
        time_s: f64,
        node_id: String,
    },
}

#[derive(Debug, Serialize)]
pub struct RunMetadata {
    pub scenario_name: String,
    pub seed: u64,
    pub duration_requested_s: f64,
    pub time_step_s: f64,
    pub final_time_s: f64,
    pub final_odometer_m: f64,
    pub final_velocity_mps: f64,
    pub cumulative_energy_kwh: f64,
    pub reached_destination: bool,
}

pub struct SimRunResult {
    pub metadata: RunMetadata,
    pub final_state: TrainSimState,
    pub events: Vec<SimEvent>,
}

/// Run headless simulation from an already-loaded scenario; paths in the scenario are relative to `scenario_dir`.
pub fn run_scenario_headless(
    scenario_dir: &Path,
    scenario: &ScenarioFile,
) -> Result<SimRunResult, SimError> {
    let mut driver = AutoDriver;
    run_scenario_headless_with_driver(scenario_dir, scenario, &mut driver)
}

pub fn run_scenario_headless_with_driver(
    scenario_dir: &Path,
    scenario: &ScenarioFile,
    driver: &mut dyn Driver,
) -> Result<SimRunResult, SimError> {
    let route_dir = scenario_dir.join(&scenario.route.path);
    let graph = load_track_graph_from_route_dir(&route_dir)?;
    let path_edges = edge_path(&graph, &scenario.route.start, &scenario.route.destination)?;
    let consist_path = scenario_dir.join(&scenario.train.consist);
    let consist = load_consist_with_asset_root(&consist_path, scenario_dir)?;
    let davis = scenario
        .train
        .davis
        .as_ref()
        .map(|d| DavisCoefficients {
            a_n: d.a_n,
            b_n_per_mps: d.b_n_per_mps,
            c_n_per_mps2: d.c_n_per_mps2,
        })
        .unwrap_or_else(|| consist.davis.clone());
    let train_physics = TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: consist.total_max_power_w(),
        max_tractive_effort_n: consist.total_max_tractive_effort_n(),
        max_brake_n: consist.total_max_brake_n(),
        davis,
    };

    let stop_nodes: HashSet<&str> = scenario
        .route
        .stops
        .iter()
        .map(|s| s.node.as_str())
        .collect();

    let mut state = TrainSimState::new(path_edges);
    let dt = scenario.simulation.time_step;
    let duration = scenario.simulation.duration;
    let seed = scenario.simulation.seed;

    let csv_path: PathBuf = scenario_dir.join(&scenario.output.csv);
    let meta_path = scenario_dir.join(&scenario.output.metadata);
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let csv_file = File::create(&csv_path)?;
    let mut csv_writer = RunCsvWriter::new(csv_file)?;
    let mut events = Vec::new();

    let mut steps = 0_u64;
    while state.time_s() < duration {
        let edge_id = state.current_edge().map(str::to_string);
        let speed_limit = edge_id
            .as_deref()
            .and_then(|e| graph.edge(e))
            .map(|e| e.speed_limit_mps)
            .unwrap_or(55.0 / 3.6);
        let decision = driver.decide(&state, speed_limit);
        state.throttle = decision.throttle.clamp(0.0, 1.0);
        state.brake = decision.brake.clamp(0.0, 1.0);
        if state.velocity_mps > speed_limit * 1.05 && speed_limit.is_finite() {
            events.push(SimEvent::OverspeedSample {
                time_s: state.time_s(),
                edge_id: edge_id.clone().unwrap_or_default(),
                speed_mps: state.velocity_mps,
                limit_mps: speed_limit,
            });
        }

        let prev_edge_index = state.edge_index;
        let step_res = step(&mut state, &graph, &train_physics, dt);

        // Detect station crossings: for each completed edge boundary this step,
        // emit arrival + departure events for matching stop nodes.
        for idx in prev_edge_index..state.edge_index.min(state.path_edges.len()) {
            let crossed_edge_id = &state.path_edges[idx];
            if let Some(edge) = graph.edge(crossed_edge_id) {
                let to_node = edge.to.0.as_str();
                if stop_nodes.contains(to_node) {
                    let t = state.time_s();
                    events.push(SimEvent::StationArrival {
                        time_s: t,
                        node_id: to_node.to_string(),
                    });
                    events.push(SimEvent::StationDeparture {
                        time_s: t,
                        node_id: to_node.to_string(),
                    });
                }
            }
        }

        csv_writer.write_sample(&state)?;
        steps += 1;
        if step_res.arrived {
            break;
        }
        if steps > 10_000_000 {
            return Err(SimError::Msg("simulation step limit exceeded".into()));
        }
    }

    csv_writer.flush()?;

    let reached = state.edge_index >= state.path_edges.len();
    let metadata = RunMetadata {
        scenario_name: scenario.scenario.name.clone(),
        seed,
        duration_requested_s: duration,
        time_step_s: dt,
        final_time_s: state.time_s(),
        final_odometer_m: state.odometer_m,
        final_velocity_mps: state.velocity_mps,
        cumulative_energy_kwh: state.cumulative_energy_j / 3_600_000.0,
        reached_destination: reached,
    };

    let meta_toml = toml::to_string_pretty(&metadata)?;
    std::fs::write(&meta_path, meta_toml)?;

    Ok(SimRunResult {
        metadata,
        final_state: state,
        events,
    })
}

/// Convenience: load `scenario.toml` from `scenario_path` (file), resolve sibling directory.
pub fn run_from_scenario_file(scenario_path: &Path) -> Result<SimRunResult, SimError> {
    let scenario_dir = scenario_path
        .parent()
        .ok_or_else(|| SimError::Msg("scenario path has no parent directory".into()))?;
    let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;
    run_scenario_headless(scenario_dir, &scenario)
}
