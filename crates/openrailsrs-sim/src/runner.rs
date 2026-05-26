use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::{ScenarioFile, SwitchPositionDef};
use openrailsrs_track::{SignalAspect, SwitchPosition};
use openrailsrs_train::{DavisCoefficients, TractiveCurve, load_consist_with_asset_root};
use serde::Serialize;

use crate::SimError;
use crate::brake::BrakeSystem;
use crate::csv_out::RunCsvWriter;
use crate::path::edge_path;
use crate::path_data::PathData;
use crate::physics::{TrainPhysics, step};
use crate::state::TrainSimState;

/// Average mass per passenger including luggage (kg).
const KG_PER_PASSENGER: f64 = 70.0;

/// Pipe propagation speed for the Westinghouse brake system (m/s).
const BRAKE_PIPE_SPEED_MPS: f64 = 200.0;

/// Build a [`BrakeSystem`] from a consist's vehicle list.
///
/// Each vehicle gets a cylinder whose position is the cumulative length of
/// the vehicles ahead of it.  When exact lengths are unavailable a default of
/// 15 m per vehicle is assumed.
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
    /// Emitted once when the train stops at a red (Stop) signal.
    SignalStop {
        time_s: f64,
        signal_id: String,
    },
    /// Emitted when a Stop signal clears and the train resumes.
    SignalClear {
        time_s: f64,
        signal_id: String,
    },
    /// Emitted (multi-train only) when a train is blocked waiting for an occupied edge.
    BlockWait {
        time_s: f64,
        train_id: String,
        edge_id: String,
    },
    /// Emitted (multi-train only) when the blocking edge clears and the train resumes.
    BlockClear {
        time_s: f64,
        train_id: String,
        edge_id: String,
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
/// Determine the asset root for resolving engine/wagon files inside a consist.
///
/// The consist file itself lives inside a `consists/` (or similar) directory.  
/// Paths stored in the `.con` file (e.g. `consists/foo.eng`) are relative to
/// the *parent* of that directory, not to the `.con` file itself.
fn consist_root(consist_path: &Path) -> &Path {
    consist_path
        .parent() // …/consists
        .and_then(|p| p.parent()) // parent of consists/ == route root
        .unwrap_or(consist_path)
}

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
    let mut graph = load_track_graph_from_route_dir(&route_dir)?;

    // Apply per-scenario switch overrides (take precedence over track.toml defaults).
    for sw in &scenario.route.switches {
        let pos = match sw.position {
            SwitchPositionDef::Straight => SwitchPosition::Straight,
            SwitchPositionDef::Diverging => SwitchPosition::Diverging,
        };
        graph.set_switch(&sw.node, pos)?;
    }

    let path_edges = edge_path(&graph, &scenario.route.start, &scenario.route.destination)?;
    let consist_path = scenario_dir.join(&scenario.train.consist);
    let consist = load_consist_with_asset_root(&consist_path, consist_root(&consist_path))?;
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
    // Build the aggregate traction curve; if the consist has no explicit curves, build a
    // synthetic one from P and F_te so that `step` always has a non-empty curve.
    let diesel_traction = consist.aggregate_diesel_traction();
    let raw_curve = consist.aggregate_tractive_curve();
    let tractive = if diesel_traction.is_some() {
        TractiveCurve::default()
    } else if raw_curve.points.is_empty() {
        TractiveCurve::from_power_and_effort(
            consist.total_max_power_w(),
            consist.total_max_tractive_effort_n(),
        )
    } else {
        raw_curve
    };
    let steam_params = consist.aggregate_steam_params();
    let train_physics = TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: consist.total_max_power_w(),
        max_tractive_effort_n: consist.total_max_tractive_effort_n(),
        max_brake_n: consist.total_max_brake_n(),
        davis,
        tractive,
        diesel_traction,
        regen_factor: consist.regen_factor(),
        diesel_sfc_g_per_kwh: consist.diesel_sfc_g_per_kwh(),
        steam_params,
    };

    let stop_nodes: HashSet<&str> = scenario
        .route
        .stops
        .iter()
        .map(|s| s.node.as_str())
        .collect();

    // Map node_id -> dwell_s for quick lookup.
    let stop_dwell: HashMap<String, f64> = scenario
        .route
        .stops
        .iter()
        .filter(|s| s.dwell_s > 0.0)
        .map(|s| (s.node.clone(), s.dwell_s))
        .collect();

    // Map node_id -> (passengers_off, passengers_on) for boarding/alighting.
    let stop_passengers: HashMap<String, (u32, u32)> = scenario
        .route
        .stops
        .iter()
        .filter(|s| s.passengers_on > 0 || s.passengers_off > 0)
        .map(|s| (s.node.clone(), (s.passengers_off, s.passengers_on)))
        .collect();
    let max_capacity = scenario.train.max_capacity;

    let path_data = PathData::from_path(&path_edges, &graph);
    let mut state = TrainSimState::new(path_edges);
    if let Some(offset) = scenario.route.start_offset_m {
        apply_start_offset(&mut state, &path_data, offset);
    }
    state.brake_system = build_brake_system(&consist);
    state.boiler_state = consist
        .aggregate_steam_params()
        .map(|p| crate::steam::BoilerState::from_params(&p));
    let dt = scenario.simulation.time_step;
    let duration = scenario.simulation.duration;
    let seed = scenario.simulation.seed;

    let csv_path: PathBuf = scenario_dir.join(&scenario.output.csv);
    let meta_path = scenario_dir.join(&scenario.output.metadata);
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let csv_file = File::create(&csv_path)?;
    let has_steam = train_physics.steam_params.is_some();
    let mut csv_writer = RunCsvWriter::new_with_steam(csv_file, has_steam)?;
    let mut events = Vec::new();

    // Distance ahead (on the current edge) at which the train starts braking for a dwell stop.
    // Calculated dynamically per-step so it adapts to current speed.
    const BRAKE_DECEL_MPS2: f64 = 0.7; // conservative deceleration estimate
    const BRAKE_MARGIN_M: f64 = 50.0; // extra safety margin beyond computed distance
    /// Caution signals halve the effective speed limit on the signalled edge.
    const CAUTION_SPEED_FACTOR: f64 = 0.5;

    // Mutable signal aspects: start from the static aspects in the graph.
    // The runner updates aspects when clear_after_s elapses.
    let mut signal_runtime: HashMap<String, SignalAspect> =
        graph.signals().map(|s| (s.id.clone(), s.aspect)).collect();

    /// Run state machine: Normal driving, approach/dwell at stops, or awaiting a signal.
    enum RunPhase {
        Normal,
        /// Approaching a stop with dwell; brake until v ≈ 0.
        Approaching {
            node: String,
            dwell_s: f64,
        },
        /// Dwelling at a stop; hold brakes for `remaining` seconds.
        Dwelling {
            node: String,
            remaining: f64,
        },
        /// Stopped at a red signal; waiting for it to clear.
        AwaitingSignal {
            signal_id: String,
        },
    }

    let mut phase = RunPhase::Normal;

    let mut steps = 0_u64;
    while state.time_s() < duration {
        match phase {
            // ── Approaching phase: brake hard until v ≈ 0, then snap to node and dwell. ──
            RunPhase::Approaching { ref node, dwell_s } => {
                state.throttle = 0.0;
                state.brake = 0.9;
                let step_res = step(&mut state, &path_data, &train_physics, dt);
                csv_writer.write_sample(&state)?;
                steps += 1;

                if state.velocity_mps < 0.2 {
                    // Train has effectively stopped — snap to the next edge boundary
                    // (simulate rolling exactly to the platform).
                    let node_id = node.clone();
                    if let Some(eid) = state.current_edge() {
                        if let Some(edge) = graph.edge(eid) {
                            if edge.to.0 == node_id {
                                state.edge_index += 1;
                                state.pos_on_edge_m = 0.0;
                            }
                        }
                    }
                    events.push(SimEvent::StationArrival {
                        time_s: state.time_s(),
                        node_id: node_id.clone(),
                    });
                    if dwell_s > 0.0 {
                        phase = RunPhase::Dwelling {
                            node: node_id,
                            remaining: dwell_s,
                        };
                    } else {
                        update_passengers(&mut state, &node_id, &stop_passengers, max_capacity);
                        events.push(SimEvent::StationDeparture {
                            time_s: state.time_s(),
                            node_id,
                        });
                        phase = RunPhase::Normal;
                    }
                }

                if step_res.arrived {
                    break;
                }
                if steps > 10_000_000 {
                    return Err(SimError::Msg("simulation step limit exceeded".into()));
                }
            }

            // ── Dwelling phase: hold brakes until dwell expires, then depart. ──
            RunPhase::Dwelling {
                ref node,
                ref mut remaining,
            } => {
                state.throttle = 0.0;
                state.brake = 1.0;
                let step_res = step(&mut state, &path_data, &train_physics, dt);
                csv_writer.write_sample(&state)?;
                steps += 1;
                *remaining -= dt;

                if *remaining <= 0.0 {
                    let node_id = node.clone();
                    update_passengers(&mut state, &node_id, &stop_passengers, max_capacity);
                    events.push(SimEvent::StationDeparture {
                        time_s: state.time_s(),
                        node_id,
                    });
                    phase = RunPhase::Normal;
                }

                if step_res.arrived {
                    break;
                }
                if steps > 10_000_000 {
                    return Err(SimError::Msg("simulation step limit exceeded".into()));
                }
            }

            // ── Awaiting signal phase: braked at red; resume when signal clears. ──
            RunPhase::AwaitingSignal { ref signal_id } => {
                // Auto-clear logic: if clear_after_s has elapsed, update the runtime aspect.
                let should_clear = {
                    let current_asp = signal_runtime
                        .get(signal_id.as_str())
                        .copied()
                        .unwrap_or(SignalAspect::Stop);
                    if current_asp == SignalAspect::Clear {
                        true
                    } else if let Some(sig) = graph.signal(signal_id.as_str()) {
                        sig.clear_after_s
                            .map(|t| state.time_s() >= t)
                            .unwrap_or(false)
                    } else {
                        false
                    }
                };

                if should_clear {
                    signal_runtime.insert(signal_id.clone(), SignalAspect::Clear);
                    let cleared_id = signal_id.clone();
                    events.push(SimEvent::SignalClear {
                        time_s: state.time_s(),
                        signal_id: cleared_id,
                    });
                    phase = RunPhase::Normal;
                    // Don't step this tick; let Normal handle it next iteration.
                    continue;
                }

                // Hold brakes while waiting.
                state.throttle = 0.0;
                state.brake = 0.9;
                let step_res = step(&mut state, &path_data, &train_physics, dt);
                csv_writer.write_sample(&state)?;
                steps += 1;

                if step_res.arrived {
                    break;
                }
                if steps > 10_000_000 {
                    return Err(SimError::Msg("simulation step limit exceeded".into()));
                }
            }

            // ── Normal driving phase ──
            RunPhase::Normal => {
                let edge_id = state.current_edge().map(str::to_string);
                let base_speed_limit = edge_id
                    .as_deref()
                    .and_then(|e| graph.edge(e))
                    .map(|e| e.speed_limit_mps)
                    .unwrap_or(55.0 / 3.6);

                // Apply Caution signals on the current edge: halve effective speed limit.
                let speed_limit = if let Some(eid) = edge_id.as_deref() {
                    let has_caution = graph.signals_on_edge(eid).any(|s| {
                        let asp = signal_runtime.get(&s.id).copied().unwrap_or(s.aspect);
                        asp == SignalAspect::Caution
                    });
                    if has_caution {
                        base_speed_limit * CAUTION_SPEED_FACTOR
                    } else {
                        base_speed_limit
                    }
                } else {
                    base_speed_limit
                };

                // Check the NEXT path edge for a Stop signal near its entry.
                // If within dynamic braking distance, enter AwaitingSignal immediately.
                let upcoming_stop_signal: Option<String> = {
                    let next_idx = state.edge_index + 1;
                    if next_idx < state.path_edges.len() {
                        let next_eid = &state.path_edges[next_idx];
                        graph
                            .signals_on_edge(next_eid)
                            .find(|s| {
                                let asp = signal_runtime.get(&s.id).copied().unwrap_or(s.aspect);
                                asp == SignalAspect::Stop
                            })
                            .and_then(|s| {
                                // Only trigger if within braking distance of the current edge end.
                                edge_id
                                    .as_deref()
                                    .and_then(|eid| graph.edge(eid))
                                    .and_then(|e| {
                                        let v = state.velocity_mps;
                                        let dist_needed =
                                            v * v / (2.0 * BRAKE_DECEL_MPS2) + BRAKE_MARGIN_M;
                                        let remaining_m = e.length_m - state.pos_on_edge_m;
                                        if remaining_m <= dist_needed {
                                            Some(s.id.clone())
                                        } else {
                                            None
                                        }
                                    })
                            })
                    } else {
                        None
                    }
                };

                // Also detect Stop signals on the CURRENT edge that the train hasn't yet passed.
                let current_stop_signal: Option<String> = edge_id.as_deref().and_then(|eid| {
                    graph
                        .signals_on_edge(eid)
                        .find(|s| {
                            let asp = signal_runtime.get(&s.id).copied().unwrap_or(s.aspect);
                            asp == SignalAspect::Stop && s.position_m > state.pos_on_edge_m
                        })
                        .and_then(|s| {
                            let v = state.velocity_mps;
                            let dist_needed = v * v / (2.0 * BRAKE_DECEL_MPS2) + BRAKE_MARGIN_M;
                            let dist_to_signal = s.position_m - state.pos_on_edge_m;
                            if dist_to_signal <= dist_needed {
                                Some(s.id.clone())
                            } else {
                                None
                            }
                        })
                });

                if let Some(sig_id) = upcoming_stop_signal.or(current_stop_signal) {
                    events.push(SimEvent::SignalStop {
                        time_s: state.time_s(),
                        signal_id: sig_id.clone(),
                    });
                    phase = RunPhase::AwaitingSignal { signal_id: sig_id };
                    continue;
                }

                // Detect whether we should start braking for an upcoming dwell stop.
                let upcoming_dwell: Option<(String, f64)> = edge_id
                    .as_deref()
                    .and_then(|eid| graph.edge(eid))
                    .and_then(|e| {
                        let to_node = e.to.0.as_str();
                        stop_dwell.get(to_node).map(|&d| {
                            // Dynamic braking distance: v²/(2a) + margin.
                            let v = state.velocity_mps;
                            let dist_needed = v * v / (2.0 * BRAKE_DECEL_MPS2) + BRAKE_MARGIN_M;
                            let remaining_m = e.length_m - state.pos_on_edge_m;
                            (to_node.to_string(), d, remaining_m, dist_needed)
                        })
                    })
                    .and_then(|(n, d, rem, need)| if rem <= need { Some((n, d)) } else { None });

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
                let step_res = step(&mut state, &path_data, &train_physics, dt);

                if let Some((dwell_node, dwell_s)) = upcoming_dwell {
                    // Start approach braking on next iteration.
                    phase = RunPhase::Approaching {
                        node: dwell_node,
                        dwell_s,
                    };
                } else {
                    // Detect pass-through station crossings (no dwell).
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
                }

                csv_writer.write_sample(&state)?;
                steps += 1;

                // Evaluate scripted signals every ~1 s of simulation time.
                // Build a single-entry block_map for this train's current edge.
                if steps % ((1.0 / dt).round() as u64).max(1) == 0 {
                    let mut block_map = HashMap::new();
                    if let Some(eid) = state.current_edge() {
                        block_map.insert(eid.to_string(), "player".to_string());
                    }
                    graph.evaluate_signals(&block_map);
                    // Sync runtime map with updated aspects.
                    for sig in graph.signals() {
                        signal_runtime.insert(sig.id.clone(), sig.aspect);
                    }
                }

                if step_res.arrived {
                    break;
                }
                if steps > 10_000_000 {
                    return Err(SimError::Msg("simulation step limit exceeded".into()));
                }
            }
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

/// Update passenger count and extra_mass_kg on departure from a stop.
fn update_passengers(
    state: &mut TrainSimState,
    node_id: &str,
    stop_passengers: &HashMap<String, (u32, u32)>,
    max_capacity: Option<u32>,
) {
    if let Some(&(off, on)) = stop_passengers.get(node_id) {
        let alighted = off.min(state.passengers);
        let boarded = if let Some(cap) = max_capacity {
            on.min(cap.saturating_sub(state.passengers - alighted))
        } else {
            on
        };
        state.passengers = state.passengers - alighted + boarded;
        state.extra_mass_kg = state.passengers as f64 * KG_PER_PASSENGER;
    }
}

/// Convenience: load `scenario.toml` from `scenario_path` (file), resolve sibling directory.
pub fn run_from_scenario_file(scenario_path: &Path) -> Result<SimRunResult, SimError> {
    let scenario_dir = scenario_path
        .parent()
        .ok_or_else(|| SimError::Msg("scenario path has no parent directory".into()))?;
    let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;
    run_scenario_headless(scenario_dir, &scenario)
}

/// Convenience: like `run_from_scenario_file` but accepts an explicit driver (e.g. `ScriptedDriver`).
pub fn run_from_scenario_file_with_driver(
    scenario_path: &Path,
    driver: &mut dyn Driver,
) -> Result<SimRunResult, SimError> {
    let scenario_dir = scenario_path
        .parent()
        .ok_or_else(|| SimError::Msg("scenario path has no parent directory".into()))?;
    let scenario = openrailsrs_scenarios::load_scenario(scenario_path)?;
    run_scenario_headless_with_driver(scenario_dir, &scenario, driver)
}

fn apply_start_offset(state: &mut TrainSimState, path_data: &PathData, offset_m: f64) {
    let mut remaining = offset_m.max(0.0);
    state.pos_on_edge_m = 0.0;
    state.odometer_m = 0.0;
    state.edge_index = 0;
    while remaining > 0.0 && state.edge_index < state.path_edges.len() {
        let Some(edge) = path_data.get(state.edge_index) else {
            break;
        };
        if edge.length_m <= 0.0 {
            state.edge_index += 1;
            continue;
        }
        if remaining <= edge.length_m {
            state.pos_on_edge_m = remaining;
            return;
        }
        remaining -= edge.length_m;
        state.edge_index += 1;
    }
}
