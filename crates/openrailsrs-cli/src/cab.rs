//! Interactive cab (driver's cab) mode — Fase 11.
//!
//! Controls
//! ─────────
//!   W / Up    → increase throttle (+10 %)
//!   S / Down  → decrease throttle / apply brake
//!   Space     → emergency brake (full stop)
//!   H         → sound horn
//!   Q / Esc   → quit
//!
//! The simulation runs at `speed_mul` × real time so you feel the inertia of a real train.

use std::path::Path;
use std::time::{Duration, Instant};

use openrailsrs_audio::{AudioCmd, AudioEngine};
use openrailsrs_cli::sound_regions::{RegionTracker, RegionTransition};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, ClearType},
};
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::load_scenario;
use openrailsrs_sim::{
    path::edge_path,
    path_data::PathData,
    physics::{TrainPhysics, step},
    state::TrainSimState,
};
use openrailsrs_train::{DavisCoefficients, TractiveCurve, load_consist_with_asset_root};

pub fn run_cab(scenario_path: &Path, speed_mul: f64) -> anyhow::Result<()> {
    // ── Load assets ──────────────────────────────────────────────────────────
    let scenario_dir = scenario_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("scenario has no parent dir"))?;
    let scenario = load_scenario(scenario_path)?;
    let route_dir = scenario_dir.join(&scenario.route.path);
    let graph =
        load_track_graph_from_route_dir(&route_dir).map_err(|e| anyhow::anyhow!("route: {e}"))?;

    let path_edges = edge_path(&graph, &scenario.route.start, &scenario.route.destination)
        .map_err(|e| anyhow::anyhow!("path: {e}"))?;
    let path_data = PathData::from_path(&path_edges, &graph);

    let consist_path = scenario_dir.join(&scenario.train.consist);
    let asset_root = consist_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(scenario_dir);
    let consist = load_consist_with_asset_root(&consist_path, asset_root)
        .map_err(|e| anyhow::anyhow!("consist: {e}"))?;
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
    let diesel_engines = consist.diesel_traction_models();
    let raw_curve = consist.aggregate_tractive_curve();
    let tractive = if !diesel_engines.is_empty() {
        TractiveCurve::default()
    } else if raw_curve.points.is_empty() {
        TractiveCurve::from_power_and_effort(
            consist.total_max_power_w(),
            consist.total_max_tractive_effort_n(),
        )
    } else {
        raw_curve
    };
    let train_physics = TrainPhysics {
        mass_kg: consist.total_mass_kg(),
        max_power_w: consist.total_max_power_w(),
        max_tractive_effort_n: consist.total_max_tractive_effort_n(),
        max_brake_n: consist.total_max_brake_n(),
        davis,
        tractive,
        diesel_engines,
        regen_factor: consist.regen_factor(),
        diesel_sfc_g_per_kwh: consist.diesel_sfc_g_per_kwh(),
        steam_params: consist.aggregate_steam_params(),
    };

    let total_dist_m: f64 = path_edges
        .iter()
        .filter_map(|eid| graph.edge(eid))
        .map(|e| e.length_m)
        .sum();

    let max_speed_mps = path_edges
        .iter()
        .filter_map(|eid| graph.edge(eid))
        .map(|e| e.speed_limit_mps)
        .fold(f64::NAN, f64::max);

    // ── Pre-compute cumulative distance to each stop node ────────────────────
    let penalty_per_late = scenario.gameplay.penalty_per_second_late;
    let stops = scenario.route.stops.clone();
    // Map stop node id → (cumulative_dist_m, arrive_s, depart_s, name)
    let stop_targets: Vec<(f64, f64, f64, String)> = {
        let mut cum = 0.0;
        let mut result = Vec::new();
        for eid in &path_edges {
            if let Some(edge) = graph.edge(eid) {
                cum += edge.length_m;
                // Check if the destination node is a stop
                let to_id = &edge.to.0;
                if let Some(stop) = stops.iter().find(|s| &s.node == to_id) {
                    let name = graph
                        .node(to_id)
                        .and_then(|n| {
                            if let openrailsrs_track::NodeKind::Station { name } = &n.kind {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| to_id.clone());
                    result.push((cum, stop.arrive_s, stop.depart_s, name));
                }
            }
        }
        result
    };

    let route_name = scenario.scenario.name.clone();
    let dt = scenario.simulation.time_step;
    let real_frame = Duration::from_millis(50); // 20 FPS
    let sim_per_frame = speed_mul * real_frame.as_secs_f64();
    let frames_per_dt = (dt / sim_per_frame).max(1.0);
    let dt_per_frame = sim_per_frame.min(dt);

    // ── State ────────────────────────────────────────────────────────────────
    let mut state = TrainSimState::new(path_edges.clone());
    state.boiler_state = consist
        .aggregate_steam_params()
        .map(|p| openrailsrs_sim::steam::BoilerState::from_params(&p));
    let mut throttle: f64 = 0.0;
    let mut brake: f64 = 0.0;
    let mut emergency = false;
    let mut arrived = false;
    let mut next_stop_idx: usize = 0;
    let mut accrued_penalty: f64 = 0.0;
    let mut passed_stops: Vec<(String, f64)> = Vec::new(); // (name, delay_s)

    // ── Audio engine (CI-safe: None when no audio device) ────────────────────
    let audio = AudioEngine::try_start();
    let mut region_tracker = RegionTracker::new(scenario.sound_regions.clone());

    // ── Terminal setup ───────────────────────────────────────────────────────
    terminal::enable_raw_mode().map_err(|e| {
        anyhow::anyhow!(
            "El modo cabina requiere una terminal interactiva (TTY). \
             Ejecutá el comando directamente desde la terminal, no desde un pipe.\n\
             Error interno: {e}"
        )
    })?;
    let mut stdout = std::io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let result: anyhow::Result<()> = (|| {
        let mut last_frame = Instant::now();

        loop {
            // ── Poll keyboard (non-blocking) ─────────────────────────────────
            while event::poll(Duration::ZERO)? {
                if let Event::Key(KeyEvent {
                    code, modifiers, ..
                }) = event::read()?
                {
                    match code {
                        KeyCode::Char('w') | KeyCode::Up => {
                            emergency = false;
                            brake = 0.0;
                            throttle = (throttle + 0.1).min(1.0);
                        }
                        KeyCode::Char('s') | KeyCode::Down => {
                            if throttle > 0.0 {
                                throttle = (throttle - 0.1).max(0.0);
                            } else {
                                emergency = false;
                                brake = (brake + 0.15).min(1.0);
                            }
                        }
                        KeyCode::Char(' ') => {
                            throttle = 0.0;
                            brake = 1.0;
                            emergency = true;
                        }
                        KeyCode::Char('h') | KeyCode::Char('H') => {
                            if let Some(ref a) = audio {
                                a.send(AudioCmd::Horn);
                            }
                        }
                        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('c')
                            if modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }

            // ── Send audio updates once per frame ────────────────────────────
            if let Some(ref a) = audio {
                a.send(AudioCmd::SetVelocity(state.velocity_mps));
                a.send(AudioCmd::SetBraking(brake));
            }

            // ── Advance simulation by one frame worth of sim-time ────────────
            if !arrived {
                let steps_this_frame = (sim_per_frame / dt).ceil() as u32;
                for _ in 0..steps_this_frame {
                    state.throttle = throttle;
                    state.brake = brake;
                    let res = step(&mut state, &path_data, &train_physics, dt);
                    if res.arrived {
                        arrived = true;
                        break;
                    }
                    // Check if we passed the next scheduled stop
                    if next_stop_idx < stop_targets.len() {
                        let (stop_dist, arrive_s, _depart_s, ref stop_name) =
                            stop_targets[next_stop_idx];
                        if state.odometer_m >= stop_dist {
                            let delay = (state.time_s() - arrive_s).max(0.0);
                            accrued_penalty += delay * penalty_per_late;
                            passed_stops.push((stop_name.clone(), state.time_s() - arrive_s));
                            next_stop_idx += 1;
                        }
                    }

                    // Sound region transitions after each physics step (headless-safe:
                    // tracker always runs; audio commands only if a device exists).
                    if let Some(edge_id) = state.current_edge() {
                        let transitions = region_tracker.step(edge_id, state.pos_on_edge_m);
                        if let Some(ref a) = audio {
                            for t in transitions {
                                match t {
                                    RegionTransition::Enter {
                                        id,
                                        kind,
                                        base_volume,
                                    } => {
                                        a.send(AudioCmd::EnterRegion {
                                            id,
                                            kind,
                                            base_volume,
                                        });
                                    }
                                    RegionTransition::Leave { id } => {
                                        a.send(AudioCmd::LeaveRegion { id });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── Render ───────────────────────────────────────────────────────
            let v_kmh = state.velocity_mps * 3.6;
            let progress = (state.odometer_m / total_dist_m).clamp(0.0, 1.0);
            let cur_edge = state
                .current_edge()
                .and_then(|eid| graph.edge(eid))
                .map(|e| e.speed_limit_mps * 3.6)
                .unwrap_or(max_speed_mps * 3.6);
            let overspeed = v_kmh > cur_edge * 1.05;

            execute!(
                stdout,
                cursor::MoveTo(0, 0),
                terminal::Clear(ClearType::All),
            )?;

            // Title bar
            execute!(
                stdout,
                SetAttribute(Attribute::Bold),
                Print(format!(" openrailsrs — MODO CABINA — {route_name}\n")),
                ResetColor,
            )?;

            execute!(
                stdout,
                Print(" ─────────────────────────────────────────────\n")
            )?;

            // Speed
            let speed_color = if overspeed {
                Color::Red
            } else if v_kmh > cur_edge * 0.92 {
                Color::Yellow
            } else {
                Color::Green
            };
            execute!(
                stdout,
                Print(" Velocidad   "),
                SetForegroundColor(speed_color),
                SetAttribute(Attribute::Bold),
                Print(format!("{:6.1} km/h", v_kmh)),
                ResetColor,
                Print(format!(
                    "   límite {:5.0} km/h{}\n",
                    cur_edge,
                    if overspeed { "  ⚠ EXCESO" } else { "" }
                )),
            )?;

            // Throttle bar
            let thr_bar = bar(throttle, 20);
            let brk_bar = bar(brake, 20);
            execute!(
                stdout,
                Print(format!(
                    " Acelerador  [{thr_bar}] {:3.0}%\n",
                    throttle * 100.0
                )),
                Print(format!(
                    " Freno       [{brk_bar}] {:3.0}%{}\n",
                    brake * 100.0,
                    if emergency { "  ⚠ EMERGENCIA" } else { "" }
                )),
            )?;

            // Progress
            let prog_bar = progress_bar(progress, 40);
            execute!(
                stdout,
                Print(format!(
                    " Recorrido   [{prog_bar}] {:5.1} km / {:5.1} km  ({:4.1}%)\n",
                    state.odometer_m / 1000.0,
                    total_dist_m / 1000.0,
                    progress * 100.0
                )),
            )?;

            // Time + energy
            let regen_kwh = state.regen_energy_j / 3_600_000.0;
            let energy_str = if regen_kwh > 0.01 {
                format!(
                    " Tiempo sim  {:6.0} s       Energía {:6.3} kWh  (regen {:5.3} kWh)\n",
                    state.time_s(),
                    state.cumulative_energy_j / 3_600_000.0,
                    regen_kwh,
                )
            } else {
                format!(
                    " Tiempo sim  {:6.0} s       Energía {:6.3} kWh\n",
                    state.time_s(),
                    state.cumulative_energy_j / 3_600_000.0,
                )
            };
            execute!(stdout, Print(energy_str))?;

            // ── Puntuality HUD ───────────────────────────────────────────────
            if !stop_targets.is_empty() {
                execute!(
                    stdout,
                    Print(" ─────────────────────────────────────────────\n")
                )?;

                // Next stop info
                if next_stop_idx < stop_targets.len() {
                    let (stop_dist, arrive_s, _depart_s, ref stop_name) =
                        stop_targets[next_stop_idx];
                    let dist_remaining = (stop_dist - state.odometer_m).max(0.0);
                    let time_to_sched = arrive_s - state.time_s();
                    let (label, color) = if time_to_sched >= 0.0 {
                        (
                            format!(
                                "en {:.0}s (faltan {:.0}s horario)",
                                time_to_sched, time_to_sched
                            ),
                            Color::Green,
                        )
                    } else {
                        (format!("{:.0}s DE RETRASO", -time_to_sched), Color::Red)
                    };
                    execute!(
                        stdout,
                        Print(format!(" Próxima parada  {:<18}", stop_name)),
                        SetForegroundColor(color),
                        Print(format!(" {label}  ({:.0}m)\n", dist_remaining)),
                        ResetColor,
                    )?;
                } else {
                    execute!(stdout, Print(" Próxima parada  —  (recorrido libre)\n"))?;
                }

                // Penalties
                let penalty_color = if accrued_penalty > 50.0 {
                    Color::Red
                } else if accrued_penalty > 10.0 {
                    Color::Yellow
                } else {
                    Color::Green
                };
                execute!(
                    stdout,
                    Print(" Penalizaciones  "),
                    SetForegroundColor(penalty_color),
                    Print(format!("{:.0} pts", accrued_penalty)),
                    ResetColor,
                    Print(format!("  ({} paradas pasadas)\n", next_stop_idx)),
                )?;

                // Passenger count (only if scenario has boarding data)
                if state.passengers > 0 || state.extra_mass_kg > 0.0 {
                    let cap_str = scenario
                        .train
                        .max_capacity
                        .map(|c| format!(" / {c}"))
                        .unwrap_or_default();
                    let pax_color = scenario
                        .train
                        .max_capacity
                        .map(|c| {
                            if state.passengers as f64 > c as f64 * 0.9 {
                                Color::Yellow
                            } else {
                                Color::Green
                            }
                        })
                        .unwrap_or(Color::Green);
                    execute!(
                        stdout,
                        Print(" Pasajeros       "),
                        SetForegroundColor(pax_color),
                        Print(format!("{}{}", state.passengers, cap_str)),
                        ResetColor,
                        Print(format!("  (+{:.0} kg)\n", state.extra_mass_kg)),
                    )?;
                }
            }

            execute!(
                stdout,
                Print(" ─────────────────────────────────────────────\n")
            )?;

            if arrived {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Green),
                    SetAttribute(Attribute::Bold),
                    Print(" ¡DESTINO ALCANZADO! Presioná Q para salir.\n"),
                    ResetColor,
                )?;
            } else {
                execute!(
                    stdout,
                    Print(" W/↑ acelerar   S/↓ freno   Espacio=freno emergencia   Q=salir\n"),
                )?;
            }

            // FPS throttle
            let elapsed = last_frame.elapsed();
            if elapsed < real_frame {
                std::thread::sleep(real_frame - elapsed);
            }
            last_frame = Instant::now();
            let _ = frames_per_dt; // used conceptually above
            let _ = dt_per_frame;
        }
    })();

    // ── Restore terminal ─────────────────────────────────────────────────────
    let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();

    result
}

fn bar(value: f64, width: usize) -> String {
    let filled = (value * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), " ".repeat(empty))
}

fn progress_bar(value: f64, width: usize) -> String {
    let filled = (value * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "▓".repeat(filled), "░".repeat(empty))
}
