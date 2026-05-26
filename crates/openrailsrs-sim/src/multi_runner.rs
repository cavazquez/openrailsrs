//! Multi-train simulation with block-level occupancy.
//!
//! One edge = one block.  Only one train may occupy a given edge at a time.
//! When a train's next edge is occupied by another train it enters
//! `AgentPhase::WaitingForBlock`, applies full brakes, and emits a
//! `SimEvent::BlockWait`.  Once the blocking train moves away the waiting
//! train receives a `SimEvent::BlockClear` and resumes normal driving.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::{ScenarioFile, SwitchPositionDef, load_timetable};
use openrailsrs_track::{SignalAspect, SwitchPosition};
use openrailsrs_train::{DavisCoefficients, TractiveCurve, load_consist_with_asset_root};

use crate::SimError;
use crate::brake::BrakeSystem;
use crate::csv_out::RunCsvWriter;
use crate::path::edge_path;
use crate::path_data::PathData;
use crate::physics::{TrainPhysics, step};
use crate::runner::{RunMetadata, SimEvent, SimRunResult};
use crate::state::TrainSimState;

// ── Result types ─────────────────────────────────────────────────────────────

pub struct TrainRunResult {
    pub id: String,
    pub sim_result: SimRunResult,
}

pub struct MultiTrainResult {
    pub results: Vec<TrainRunResult>,
}

// ── Internal per-agent state ──────────────────────────────────────────────────

enum AgentPhase {
    Normal,
    /// The next edge in the path is occupied; hold brakes until it clears.
    WaitingForBlock {
        /// The edge we are waiting to enter.
        edge_id: String,
        /// Prevent emitting BlockWait more than once per block event.
        emitted: bool,
    },
}

struct TrainAgent {
    id: String,
    state: TrainSimState,
    physics: TrainPhysics,
    path_data: PathData,
    phase: AgentPhase,
    events: Vec<SimEvent>,
    csv_writer: RunCsvWriter<File>,
    stop_nodes: HashSet<String>,
    signal_runtime: HashMap<String, SignalAspect>,
    start_time_s: f64,
    arrived: bool,
    scenario_name: String,
    seed: u64,
    duration_requested_s: f64,
    time_step_s: f64,
}

// ── Simple AutoDriver (duplicated to avoid coupling) ─────────────────────────

fn auto_decide(vel_mps: f64, speed_limit_mps: f64) -> (f64, f64) {
    if vel_mps > speed_limit_mps * 1.02 {
        (0.0, 0.35)
    } else if vel_mps < speed_limit_mps * 0.92 {
        (1.0, 0.0)
    } else {
        (0.2, 0.0)
    }
}

// ── Build physics from a TrainSection-like triple ────────────────────────────

/// Asset root for resolving .eng/.wag paths inside a consist file.
/// Paths in the .con reference `consists/foo.eng` which are relative to the route root.
const BRAKE_PIPE_SPEED_MPS: f64 = 200.0;

fn build_brake_system(consist: &openrailsrs_train::Consist) -> BrakeSystem {
    const DEFAULT_VEHICLE_LENGTH_M: f64 = 15.0;
    let mut pos = 0.0_f64;
    let pairs: Vec<(f64, f64)> = consist
        .vehicles
        .iter()
        .map(|v| {
            let cylinder_pos = pos;
            let length_m = match v {
                openrailsrs_train::Vehicle::Loco(l) => {
                    if l.length_m > 0.0 {
                        l.length_m
                    } else {
                        DEFAULT_VEHICLE_LENGTH_M
                    }
                }
                openrailsrs_train::Vehicle::Wagon(w) => {
                    if w.length_m > 0.0 {
                        w.length_m
                    } else {
                        DEFAULT_VEHICLE_LENGTH_M
                    }
                }
            };
            pos += length_m;
            let force_n = match v {
                openrailsrs_train::Vehicle::Loco(l) => l.max_brake_force_n,
                openrailsrs_train::Vehicle::Wagon(w) => w.max_brake_force_n,
            };
            (cylinder_pos, force_n)
        })
        .collect();
    BrakeSystem::from_vehicles(&pairs, BRAKE_PIPE_SPEED_MPS)
}

fn build_brake_from_path(consist_path: &Path) -> BrakeSystem {
    load_consist_with_asset_root(consist_path, consist_root(consist_path))
        .map(|c| build_brake_system(&c))
        .unwrap_or_default()
}

fn consist_root(consist_path: &Path) -> &Path {
    consist_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(consist_path)
}

fn build_physics(
    consist_path: &Path,
    _scenario_dir: &Path,
    davis_override: Option<&openrailsrs_scenarios::DavisSection>,
) -> Result<TrainPhysics, SimError> {
    let consist = load_consist_with_asset_root(consist_path, consist_root(consist_path))?;
    let davis = davis_override
        .map(|d| DavisCoefficients {
            a_n: d.a_n,
            b_n_per_mps: d.b_n_per_mps,
            c_n_per_mps2: d.c_n_per_mps2,
        })
        .unwrap_or_else(|| consist.davis.clone());
    let raw_curve = consist.aggregate_tractive_curve();
    let tractive = if raw_curve.points.is_empty() {
        TractiveCurve::from_power_and_effort(
            consist.total_max_power_w(),
            consist.total_max_tractive_effort_n(),
        )
    } else {
        raw_curve
    };
    Ok(TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: consist.total_max_power_w(),
        max_tractive_effort_n: consist.total_max_tractive_effort_n(),
        max_brake_n: consist.total_max_brake_n(),
        davis,
        tractive,
        regen_factor: consist.regen_factor(),
        diesel_sfc_g_per_kwh: consist.diesel_sfc_g_per_kwh(),
        steam_params: consist.aggregate_steam_params(),
    })
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run all trains in `scenario` (primary + `extra_trains`) on a single shared
/// clock with block-level occupancy enforcement.
pub fn run_scenario_multi_train(
    scenario_dir: &Path,
    scenario: &ScenarioFile,
) -> Result<MultiTrainResult, SimError> {
    // ── Load shared route graph ───────────────────────────────────────────────
    let route_dir = scenario_dir.join(&scenario.route.path);
    let mut graph = load_track_graph_from_route_dir(&route_dir)?;

    // Apply primary-train switch overrides.
    for sw in &scenario.route.switches {
        let pos = match sw.position {
            SwitchPositionDef::Straight => SwitchPosition::Straight,
            SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        graph.set_switch(&sw.node, pos)?;
    }

    let dt = scenario.simulation.time_step;
    let duration = scenario.simulation.duration;
    let seed = scenario.simulation.seed;

    // ── Build agent list ──────────────────────────────────────────────────────
    let mut agents: Vec<TrainAgent> = Vec::new();

    // Primary train (id = scenario name or "primary").
    {
        let path_edges = edge_path(&graph, &scenario.route.start, &scenario.route.destination)?;
        let consist_path = scenario_dir.join(&scenario.train.consist);
        let physics = build_physics(&consist_path, scenario_dir, scenario.train.davis.as_ref())?;
        let path_data = PathData::from_path(&path_edges, &graph);
        let mut state = TrainSimState::new(path_edges);
        state.brake_system = build_brake_from_path(&consist_path);
        state.boiler_state =
            load_consist_with_asset_root(&consist_path, consist_root(&consist_path))
                .ok()
                .and_then(|c| c.aggregate_steam_params())
                .map(|p| crate::steam::BoilerState::from_params(&p));
        // Primary train starts at t=0; shift its internal clock to 0.
        state.time = openrailsrs_core::SimTime(0.0);
        let csv_path = scenario_dir.join(&scenario.output.csv);
        if let Some(p) = csv_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let csv_file = File::create(&csv_path)?;
        let stop_nodes: HashSet<String> = scenario
            .route
            .stops
            .iter()
            .map(|s| s.node.clone())
            .collect();
        let signal_runtime: HashMap<String, SignalAspect> =
            graph.signals().map(|s| (s.id.clone(), s.aspect)).collect();
        agents.push(TrainAgent {
            id: "primary".to_string(),
            state,
            physics,
            path_data,
            phase: AgentPhase::Normal,
            events: Vec::new(),
            csv_writer: RunCsvWriter::new(csv_file)?,
            stop_nodes,
            signal_runtime,
            start_time_s: 0.0,
            arrived: false,
            scenario_name: scenario.scenario.name.clone(),
            seed,
            duration_requested_s: duration,
            time_step_s: dt,
        });
    }

    // Extra trains.
    for entry in &scenario.extra_trains {
        // Apply this entry's switch overrides on a clone-compatible graph.
        // NOTE: extra trains share the same graph topology but may request
        // different switch positions; we apply their overrides per-path
        // computation only (graph is reset between path calculations here).
        let mut g2 = graph.clone();
        for sw in &entry.switches {
            let pos = match sw.position {
                SwitchPositionDef::Straight => SwitchPosition::Straight,
                SwitchPositionDef::Diverging => SwitchPosition::Diverging,
            };
            g2.set_switch(&sw.node, pos)?;
        }
        let path_edges = edge_path(&g2, &entry.start, &entry.destination)?;
        let consist_path = scenario_dir.join(&entry.consist);
        let physics = build_physics(&consist_path, scenario_dir, entry.davis.as_ref())?;
        let path_data = PathData::from_path(&path_edges, &g2);
        let mut state = TrainSimState::new(path_edges);
        state.brake_system = build_brake_from_path(&consist_path);
        state.boiler_state =
            load_consist_with_asset_root(&consist_path, consist_root(&consist_path))
                .ok()
                .and_then(|c| c.aggregate_steam_params())
                .map(|p| crate::steam::BoilerState::from_params(&p));
        state.time = openrailsrs_core::SimTime(entry.start_time_s);
        let csv_path = scenario_dir.join(&entry.output_csv);
        if let Some(p) = csv_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let csv_file = File::create(&csv_path)?;
        let stop_nodes: HashSet<String> = entry.stops.iter().map(|s| s.node.clone()).collect();
        let signal_runtime: HashMap<String, SignalAspect> =
            graph.signals().map(|s| (s.id.clone(), s.aspect)).collect();
        agents.push(TrainAgent {
            id: entry.id.clone(),
            state,
            physics,
            path_data,
            phase: AgentPhase::Normal,
            events: Vec::new(),
            csv_writer: RunCsvWriter::new(csv_file)?,
            stop_nodes,
            signal_runtime,
            start_time_s: entry.start_time_s,
            arrived: false,
            scenario_name: format!("{} ({})", scenario.scenario.name, entry.id),
            seed,
            duration_requested_s: duration,
            time_step_s: dt,
        });
    }

    // ── Initialise block map: claim the starting edge of each active agent ────
    // block_map: edge_id → train_id that currently occupies it.
    let mut block_map: HashMap<String, String> = HashMap::new();
    for agent in &agents {
        if agent.start_time_s <= 0.0 {
            if let Some(eid) = agent.state.current_edge() {
                block_map.insert(eid.to_string(), agent.id.clone());
            }
        }
    }

    // ── Main simulation loop ──────────────────────────────────────────────────
    const MAX_STEPS: u64 = 20_000_000;
    let mut global_steps: u64 = 0;

    let mut t = 0.0_f64;
    while t < duration {
        for agent in agents.iter_mut() {
            if agent.arrived {
                continue;
            }
            // Train hasn't departed yet.
            if t < agent.start_time_s {
                continue;
            }

            // Claim starting edge on first active tick.
            if (t - agent.start_time_s).abs() < dt * 0.5 && t >= agent.start_time_s {
                if let Some(eid) = agent.state.current_edge() {
                    block_map
                        .entry(eid.to_string())
                        .or_insert_with(|| agent.id.clone());
                }
            }

            match agent.phase {
                // ── Waiting for block to clear ─────────────────────────────
                AgentPhase::WaitingForBlock {
                    ref edge_id,
                    ref mut emitted,
                } => {
                    let blocker = block_map.get(edge_id.as_str()).cloned();
                    let is_free = blocker.as_deref().map(|id| id == agent.id).unwrap_or(true);

                    if is_free {
                        // Claim the newly free edge.
                        let freed_id = edge_id.clone();
                        if let Some(old) = agent.state.current_edge() {
                            block_map.remove(old);
                        }
                        block_map.insert(freed_id.clone(), agent.id.clone());
                        agent.events.push(SimEvent::BlockClear {
                            time_s: agent.state.time_s(),
                            train_id: agent.id.clone(),
                            edge_id: freed_id,
                        });
                        agent.phase = AgentPhase::Normal;
                        // Don't step this tick; resume next tick.
                        continue;
                    }

                    // Still blocked — emit once then hold brakes.
                    if !*emitted {
                        agent.events.push(SimEvent::BlockWait {
                            time_s: agent.state.time_s(),
                            train_id: agent.id.clone(),
                            edge_id: edge_id.clone(),
                        });
                        *emitted = true;
                    }
                    agent.state.throttle = 0.0;
                    agent.state.brake = 1.0;
                    let step_res = step(&mut agent.state, &agent.path_data, &agent.physics, dt);
                    agent.csv_writer.write_sample(&agent.state)?;
                    global_steps += 1;
                    if step_res.arrived {
                        agent.arrived = true;
                        block_map.retain(|_, v| v != &agent.id);
                    }
                }

                // ── Normal driving ─────────────────────────────────────────
                AgentPhase::Normal => {
                    // Determine speed limit from current edge.
                    let edge_id_opt = agent.state.current_edge().map(str::to_string);
                    let base_speed_limit = edge_id_opt
                        .as_deref()
                        .and_then(|e| graph.edge(e))
                        .map(|e| e.speed_limit_mps)
                        .unwrap_or(55.0 / 3.6);

                    // Apply Caution signals on current edge.
                    const CAUTION_FACTOR: f64 = 0.5;
                    let speed_limit = if let Some(eid) = edge_id_opt.as_deref() {
                        let has_caution = graph.signals_on_edge(eid).any(|s| {
                            let asp = agent.signal_runtime.get(&s.id).copied().unwrap_or(s.aspect);
                            asp == SignalAspect::Caution
                        });
                        if has_caution {
                            base_speed_limit * CAUTION_FACTOR
                        } else {
                            base_speed_limit
                        }
                    } else {
                        base_speed_limit
                    };

                    // Check if the next path edge is occupied by another train.
                    let next_idx = agent.state.edge_index + 1;
                    if next_idx < agent.state.path_edges.len() {
                        let next_eid = agent.state.path_edges[next_idx].clone();
                        let blocker = block_map.get(&next_eid).cloned();
                        let is_blocked =
                            blocker.as_deref().map(|id| id != agent.id).unwrap_or(false);

                        // Trigger when within braking distance.
                        const BRAKE_DECEL: f64 = 0.7;
                        const BRAKE_MARGIN: f64 = 50.0;
                        let v = agent.state.velocity_mps;
                        let dist_needed = v * v / (2.0 * BRAKE_DECEL) + BRAKE_MARGIN;
                        let remaining = edge_id_opt
                            .as_deref()
                            .and_then(|e| graph.edge(e))
                            .map(|e| e.length_m - agent.state.pos_on_edge_m)
                            .unwrap_or(f64::MAX);

                        if is_blocked && remaining <= dist_needed {
                            agent.phase = AgentPhase::WaitingForBlock {
                                edge_id: next_eid,
                                emitted: false,
                            };
                            // Apply brakes this tick.
                            agent.state.throttle = 0.0;
                            agent.state.brake = 0.9;
                            let step_res =
                                step(&mut agent.state, &agent.path_data, &agent.physics, dt);
                            agent.csv_writer.write_sample(&agent.state)?;
                            global_steps += 1;
                            if step_res.arrived {
                                agent.arrived = true;
                                block_map.retain(|_, v| v != &agent.id);
                            }
                            continue;
                        }
                    }

                    // AutoDriver decides.
                    let (throttle, brake) = auto_decide(agent.state.velocity_mps, speed_limit);
                    agent.state.throttle = throttle;
                    agent.state.brake = brake;

                    let prev_edge_index = agent.state.edge_index;
                    let step_res = step(&mut agent.state, &agent.path_data, &agent.physics, dt);
                    agent.csv_writer.write_sample(&agent.state)?;
                    global_steps += 1;

                    // Detect pass-through stations.
                    for idx in
                        prev_edge_index..agent.state.edge_index.min(agent.state.path_edges.len())
                    {
                        if let Some(edge) = graph.edge(&agent.state.path_edges[idx]) {
                            let to_node = edge.to.0.as_str();
                            if agent.stop_nodes.contains(to_node) {
                                let t_ev = agent.state.time_s();
                                agent.events.push(SimEvent::StationArrival {
                                    time_s: t_ev,
                                    node_id: to_node.to_string(),
                                });
                                agent.events.push(SimEvent::StationDeparture {
                                    time_s: t_ev,
                                    node_id: to_node.to_string(),
                                });
                            }
                        }
                    }

                    // Update block map on edge transitions.
                    if agent.state.edge_index > prev_edge_index {
                        for idx in prev_edge_index..agent.state.edge_index {
                            if idx < agent.state.path_edges.len() {
                                let old_eid = &agent.state.path_edges[idx];
                                if block_map
                                    .get(old_eid)
                                    .map(|id| id == &agent.id)
                                    .unwrap_or(false)
                                {
                                    block_map.remove(old_eid);
                                }
                            }
                        }
                        if let Some(new_eid) = agent.state.current_edge() {
                            block_map
                                .entry(new_eid.to_string())
                                .or_insert_with(|| agent.id.clone());
                        }
                    }

                    if step_res.arrived {
                        agent.arrived = true;
                        block_map.retain(|_, v| v != &agent.id);
                    }
                }
            }
        }

        t += dt;
        global_steps += 1;
        if global_steps > MAX_STEPS {
            return Err(SimError::Msg(
                "multi-train simulation step limit exceeded".into(),
            ));
        }

        // Evaluate scripted signals every ~1 s of simulation time.
        let eval_period = (1.0 / dt).round() as u64;
        if global_steps % eval_period.max(1) == 0 {
            graph.evaluate_signals(&block_map);
            // Sync each agent's signal_runtime map with the updated aspects.
            for agent in agents.iter_mut() {
                for sig in graph.signals() {
                    agent.signal_runtime.insert(sig.id.clone(), sig.aspect);
                }
            }
        }

        // Early exit once all trains have arrived.
        if agents.iter().all(|a| a.arrived) {
            break;
        }
    }

    // ── Flush and build results ───────────────────────────────────────────────
    let mut results: Vec<TrainRunResult> = Vec::new();
    for agent in agents.iter_mut() {
        agent.csv_writer.flush()?;
        let reached = agent.arrived || agent.state.edge_index >= agent.state.path_edges.len();
        let metadata = RunMetadata {
            scenario_name: agent.scenario_name.clone(),
            seed: agent.seed,
            duration_requested_s: agent.duration_requested_s,
            time_step_s: agent.time_step_s,
            final_time_s: agent.state.time_s(),
            final_odometer_m: agent.state.odometer_m,
            final_velocity_mps: agent.state.velocity_mps,
            cumulative_energy_kwh: agent.state.cumulative_energy_j / 3_600_000.0,
            reached_destination: reached,
        };
        results.push(TrainRunResult {
            id: agent.id.clone(),
            sim_result: SimRunResult {
                metadata,
                final_state: agent.state.clone(),
                events: agent.events.clone(),
            },
        });
    }

    Ok(MultiTrainResult { results })
}

/// Convenience: load scenario from file and run multi-train simulation.
pub fn run_multi_train_from_scenario_file(
    scenario_path: &Path,
) -> Result<MultiTrainResult, SimError> {
    let scenario_dir = scenario_path
        .parent()
        .ok_or_else(|| SimError::Msg("scenario path has no parent directory".into()))?;
    let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;
    run_scenario_multi_train(scenario_dir, &scenario)
}

// ── LiveMultiSim ─────────────────────────────────────────────────────────────
//
// Frame-by-frame multi-train simulation for interactive dispatch panels.
// Unlike `run_scenario_multi_train`, no CSV files are written; callers receive
// `LiveTrainSnapshot` structs every call to `step_frame`.

/// Status of an individual train inside `LiveMultiSim`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrainStatus {
    /// Waiting for its scheduled departure time.
    WaitingToDepart,
    /// Moving normally.
    Running,
    /// Stopped because the next block is occupied by another train.
    WaitingBlock,
    /// Has reached its destination.
    Arrived,
}

/// Lightweight snapshot of a single train's state, returned each frame.
#[derive(Clone, Debug)]
pub struct LiveTrainSnapshot {
    pub id: String,
    pub velocity_mps: f64,
    pub odometer_m: f64,
    pub cumulative_energy_j: f64,
    pub regen_energy_j: f64,
    pub fuel_consumption_g: f64,
    pub time_s: f64,
    pub status: TrainStatus,
    pub current_edge_id: Option<String>,
    pub total_dist_m: f64,
}

struct LiveAgent {
    id: String,
    state: TrainSimState,
    physics: TrainPhysics,
    path_data: PathData,
    phase: AgentPhase,
    start_time_s: f64,
    arrived: bool,
    total_dist_m: f64,
}

/// Interactive multi-train simulation that advances frame-by-frame.
pub struct LiveMultiSim {
    agents: Vec<LiveAgent>,
    block_map: HashMap<String, String>,
    graph: openrailsrs_track::TrackGraph,
    dt: f64,
    t: f64,
    duration: f64,
    _scenario_dir: PathBuf,
}

impl LiveMultiSim {
    /// Load and initialise from a scenario file.
    pub fn new(scenario_path: &Path) -> Result<Self, SimError> {
        let scenario_dir = scenario_path
            .parent()
            .ok_or_else(|| SimError::Msg("scenario path has no parent directory".into()))?;
        let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;

        let route_dir = scenario_dir.join(&scenario.route.path);
        let mut graph = load_track_graph_from_route_dir(&route_dir)?;

        for sw in &scenario.route.switches {
            let pos = match sw.position {
                SwitchPositionDef::Straight => SwitchPosition::Straight,
                SwitchPositionDef::Diverging => SwitchPosition::Diverging,
            };
            graph.set_switch(&sw.node, pos)?;
        }

        let dt = scenario.simulation.time_step;
        let duration = scenario.simulation.duration;
        let mut agents: Vec<LiveAgent> = Vec::new();

        // Primary train
        {
            let path_edges =
                crate::path::edge_path(&graph, &scenario.route.start, &scenario.route.destination)?;
            let consist_path = scenario_dir.join(&scenario.train.consist);
            let physics =
                build_physics(&consist_path, scenario_dir, scenario.train.davis.as_ref())?;
            let total_dist_m: f64 = path_edges
                .iter()
                .filter_map(|eid| graph.edge(eid))
                .map(|e| e.length_m)
                .sum();
            let path_data = PathData::from_path(&path_edges, &graph);
            let mut state = TrainSimState::new(path_edges);
            state.brake_system = build_brake_from_path(&consist_path);
            state.boiler_state =
                load_consist_with_asset_root(&consist_path, consist_root(&consist_path))
                    .ok()
                    .and_then(|c| c.aggregate_steam_params())
                    .map(|p| crate::steam::BoilerState::from_params(&p));
            state.time = openrailsrs_core::SimTime(0.0);
            agents.push(LiveAgent {
                id: "primary".to_string(),
                state,
                physics,
                path_data,
                phase: AgentPhase::Normal,
                start_time_s: 0.0,
                arrived: false,
                total_dist_m,
            });
        }

        // Extra trains
        for entry in &scenario.extra_trains {
            let mut g2 = graph.clone();
            for sw in &entry.switches {
                let pos = match sw.position {
                    SwitchPositionDef::Straight => SwitchPosition::Straight,
                    SwitchPositionDef::Diverging => SwitchPosition::Diverging,
                };
                g2.set_switch(&sw.node, pos)?;
            }
            let path_edges = crate::path::edge_path(&g2, &entry.start, &entry.destination)?;
            let consist_path = scenario_dir.join(&entry.consist);
            let physics = build_physics(&consist_path, scenario_dir, entry.davis.as_ref())?;
            let total_dist_m: f64 = path_edges
                .iter()
                .filter_map(|eid| g2.edge(eid))
                .map(|e| e.length_m)
                .sum();
            let path_data = PathData::from_path(&path_edges, &g2);
            let mut state = TrainSimState::new(path_edges);
            state.brake_system = build_brake_from_path(&consist_path);
            state.boiler_state =
                load_consist_with_asset_root(&consist_path, consist_root(&consist_path))
                    .ok()
                    .and_then(|c| c.aggregate_steam_params())
                    .map(|p| crate::steam::BoilerState::from_params(&p));
            state.time = openrailsrs_core::SimTime(entry.start_time_s);
            agents.push(LiveAgent {
                id: entry.id.clone(),
                state,
                physics,
                path_data,
                phase: AgentPhase::Normal,
                start_time_s: entry.start_time_s,
                arrived: false,
                total_dist_m,
            });
        }

        // Initial block map
        let mut block_map: HashMap<String, String> = HashMap::new();
        for agent in &agents {
            if agent.start_time_s <= 0.0 {
                if let Some(eid) = agent.state.current_edge() {
                    block_map.insert(eid.to_string(), agent.id.clone());
                }
            }
        }

        Ok(Self {
            agents,
            block_map,
            graph,
            dt,
            t: 0.0,
            duration,
            _scenario_dir: scenario_dir.to_path_buf(),
        })
    }

    /// Load and initialise from a timetable file (`timetable.toml`).
    ///
    /// All trains share the same route graph; the consist path in each entry is
    /// resolved relative to the route directory specified in `[timetable].route`.
    pub fn from_timetable(timetable_path: &Path) -> Result<Self, SimError> {
        let tt = load_timetable(timetable_path)?;
        let timetable_dir = timetable_path
            .parent()
            .ok_or_else(|| SimError::Msg("timetable path has no parent directory".into()))?;

        let route_dir = timetable_dir.join(&tt.timetable.route);
        let graph = load_track_graph_from_route_dir(&route_dir)?;

        let dt = tt.timetable.time_step_s;
        let duration = tt.timetable.duration_s;
        let mut agents: Vec<LiveAgent> = Vec::new();

        for entry in &tt.trains {
            let path_edges = crate::path::edge_path(&graph, &entry.start, &entry.destination)?;
            let consist_path = route_dir.join(&entry.consist);
            let physics = build_physics(&consist_path, &route_dir, None)?;
            let total_dist_m: f64 = path_edges
                .iter()
                .filter_map(|eid| graph.edge(eid))
                .map(|e| e.length_m)
                .sum();
            let path_data = PathData::from_path(&path_edges, &graph);
            let mut state = TrainSimState::new(path_edges);
            state.brake_system = build_brake_from_path(&consist_path);
            state.boiler_state =
                load_consist_with_asset_root(&consist_path, consist_root(&consist_path))
                    .ok()
                    .and_then(|c| c.aggregate_steam_params())
                    .map(|p| crate::steam::BoilerState::from_params(&p));
            state.time = openrailsrs_core::SimTime(entry.depart_s);
            agents.push(LiveAgent {
                id: entry.id.clone(),
                state,
                physics,
                path_data,
                phase: AgentPhase::Normal,
                start_time_s: entry.depart_s,
                arrived: false,
                total_dist_m,
            });
        }

        let mut block_map: HashMap<String, String> = HashMap::new();
        for agent in &agents {
            if agent.start_time_s <= 0.0 {
                if let Some(eid) = agent.state.current_edge() {
                    block_map.insert(eid.to_string(), agent.id.clone());
                }
            }
        }

        Ok(Self {
            agents,
            block_map,
            graph,
            dt,
            t: 0.0,
            duration,
            _scenario_dir: timetable_dir.to_path_buf(),
        })
    }

    /// Advance simulation by `steps` time steps (each step = `dt` seconds from the scenario).
    /// Returns one `LiveTrainSnapshot` per train.
    pub fn step_frame(&mut self, steps: u32) -> Vec<LiveTrainSnapshot> {
        let dt = self.dt;
        for _ in 0..steps {
            if self.t >= self.duration {
                break;
            }
            for agent in self.agents.iter_mut() {
                if agent.arrived {
                    continue;
                }
                if self.t < agent.start_time_s {
                    continue;
                }

                // Claim starting edge on first active tick.
                if (self.t - agent.start_time_s).abs() < dt * 0.5 {
                    if let Some(eid) = agent.state.current_edge() {
                        self.block_map
                            .entry(eid.to_string())
                            .or_insert_with(|| agent.id.clone());
                    }
                }

                match agent.phase {
                    AgentPhase::WaitingForBlock {
                        ref edge_id,
                        ref mut emitted,
                    } => {
                        let is_free = self
                            .block_map
                            .get(edge_id.as_str())
                            .map(|id| id == &agent.id)
                            .unwrap_or(true);
                        if is_free {
                            let freed_id = edge_id.clone();
                            if let Some(old) = agent.state.current_edge() {
                                self.block_map.remove(old);
                            }
                            self.block_map.insert(freed_id, agent.id.clone());
                            agent.phase = AgentPhase::Normal;
                            continue;
                        }
                        *emitted = true;
                        agent.state.throttle = 0.0;
                        agent.state.brake = 1.0;
                        let res = crate::physics::step(
                            &mut agent.state,
                            &agent.path_data,
                            &agent.physics,
                            dt,
                        );
                        if res.arrived {
                            agent.arrived = true;
                            self.block_map.retain(|_, v| *v != agent.id);
                        }
                    }
                    AgentPhase::Normal => {
                        let edge_id_opt = agent.state.current_edge().map(str::to_string);
                        let speed_limit = edge_id_opt
                            .as_deref()
                            .and_then(|e| self.graph.edge(e))
                            .map(|e| e.speed_limit_mps)
                            .unwrap_or(55.0 / 3.6);

                        let next_idx = agent.state.edge_index + 1;
                        if next_idx < agent.state.path_edges.len() {
                            let next_eid = agent.state.path_edges[next_idx].clone();
                            let is_blocked = self
                                .block_map
                                .get(&next_eid)
                                .map(|id| id != &agent.id)
                                .unwrap_or(false);
                            const BRAKE_DECEL: f64 = 0.7;
                            const BRAKE_MARGIN: f64 = 50.0;
                            let v = agent.state.velocity_mps;
                            let dist_needed = v * v / (2.0 * BRAKE_DECEL) + BRAKE_MARGIN;
                            let remaining = edge_id_opt
                                .as_deref()
                                .and_then(|e| self.graph.edge(e))
                                .map(|e| e.length_m - agent.state.pos_on_edge_m)
                                .unwrap_or(f64::MAX);
                            if is_blocked && remaining <= dist_needed {
                                agent.phase = AgentPhase::WaitingForBlock {
                                    edge_id: next_eid,
                                    emitted: false,
                                };
                                agent.state.throttle = 0.0;
                                agent.state.brake = 0.9;
                                let res = crate::physics::step(
                                    &mut agent.state,
                                    &agent.path_data,
                                    &agent.physics,
                                    dt,
                                );
                                if res.arrived {
                                    agent.arrived = true;
                                    self.block_map.retain(|_, v| *v != agent.id);
                                }
                                continue;
                            }
                        }

                        let (throttle, brake) = auto_decide(agent.state.velocity_mps, speed_limit);
                        agent.state.throttle = throttle;
                        agent.state.brake = brake;
                        let prev_idx = agent.state.edge_index;
                        let res = crate::physics::step(
                            &mut agent.state,
                            &agent.path_data,
                            &agent.physics,
                            dt,
                        );

                        if agent.state.edge_index > prev_idx {
                            for idx in prev_idx..agent.state.edge_index {
                                if idx < agent.state.path_edges.len() {
                                    let old_eid = &agent.state.path_edges[idx];
                                    if self
                                        .block_map
                                        .get(old_eid)
                                        .map(|id| id == &agent.id)
                                        .unwrap_or(false)
                                    {
                                        self.block_map.remove(old_eid);
                                    }
                                }
                            }
                            if let Some(new_eid) = agent.state.current_edge() {
                                self.block_map
                                    .entry(new_eid.to_string())
                                    .or_insert_with(|| agent.id.clone());
                            }
                        }

                        if res.arrived {
                            agent.arrived = true;
                            self.block_map.retain(|_, v| *v != agent.id);
                        }
                    }
                }
            }
            self.t += dt;
        }

        // Build snapshots
        self.agents
            .iter()
            .map(|a| {
                let status = if a.arrived {
                    TrainStatus::Arrived
                } else if self.t < a.start_time_s {
                    TrainStatus::WaitingToDepart
                } else {
                    match &a.phase {
                        AgentPhase::WaitingForBlock { .. } => TrainStatus::WaitingBlock,
                        AgentPhase::Normal => TrainStatus::Running,
                    }
                };
                LiveTrainSnapshot {
                    id: a.id.clone(),
                    velocity_mps: a.state.velocity_mps,
                    odometer_m: a.state.odometer_m,
                    cumulative_energy_j: a.state.cumulative_energy_j,
                    regen_energy_j: a.state.regen_energy_j,
                    fuel_consumption_g: a.state.fuel_consumption_g,
                    time_s: a.state.time_s(),
                    status,
                    current_edge_id: a.state.current_edge().map(str::to_string),
                    total_dist_m: a.total_dist_m,
                }
            })
            .collect()
    }

    /// Returns true if all trains have reached their destination.
    pub fn all_arrived(&self) -> bool {
        self.agents.iter().all(|a| a.arrived)
    }

    /// Elapsed simulation time (seconds).
    pub fn sim_time(&self) -> f64 {
        self.t
    }

    /// Configured total simulation duration (seconds).
    pub fn duration(&self) -> f64 {
        self.duration
    }
}
