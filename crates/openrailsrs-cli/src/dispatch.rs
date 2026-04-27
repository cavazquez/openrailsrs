//! Dispatch panel (Fase 12) — real-time TUI for monitoring and controlling a simulation.
//!
//! Runs a headless simulation at `speed_mul`× real time and draws:
//!   • Train table  — position, speed, edge, next stop, status
//!   • Event log    — last 20 simulation events
//!   • Controls bar — keyboard shortcuts
//!
//! Keyboard
//! ────────
//!   Q / Esc   quit
//!   Space     pause / resume
//!   + / -     increase / decrease simulation speed

use std::{
    path::Path,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
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

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run_dispatch(scenario_path: &Path, speed_mul: f64) -> anyhow::Result<()> {
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

    let total_dist_m: f64 = path_edges
        .iter()
        .filter_map(|eid| graph.edge(eid))
        .map(|e| e.length_m)
        .sum();

    let consist_path = scenario_dir.join(&scenario.train.consist);
    let consist = load_consist_with_asset_root(&consist_path, scenario_dir)
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
    let raw_curve = consist.aggregate_tractive_curve();
    let tractive = if raw_curve.points.is_empty() {
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
    };

    let route_name = scenario.scenario.name.clone();
    let dt = scenario.simulation.time_step;
    let real_frame = Duration::from_millis(80); // ~12 FPS — responsive but calm

    // ── Sim state ─────────────────────────────────────────────────────────────
    let mut sim_state = TrainSimState::new(path_edges.clone());
    let mut paused = false;
    let mut arrived = false;
    let mut current_speed = speed_mul;
    let mut events: Vec<String> = Vec::new();
    let mut prev_edge: Option<String> = None;

    // ── Terminal setup ────────────────────────────────────────────────────────
    terminal::enable_raw_mode().map_err(|e| {
        anyhow::anyhow!("El panel de despacho requiere una terminal interactiva (TTY).\nError: {e}")
    })?;
    let mut stdout = std::io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result: anyhow::Result<()> = (|| {
        let mut last_frame = Instant::now();

        loop {
            // ── Input ─────────────────────────────────────────────────────────
            while event::poll(Duration::ZERO)? {
                if let Event::Key(KeyEvent {
                    code, modifiers, ..
                }) = event::read()?
                {
                    match code {
                        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('c')
                            if modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(());
                        }
                        KeyCode::Char(' ') => {
                            paused = !paused;
                            let msg = if paused {
                                "▐▐ SIMULACIÓN PAUSADA".to_string()
                            } else {
                                "▶ Simulación reanudada".to_string()
                            };
                            push_event(&mut events, msg);
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            current_speed = (current_speed + 5.0).min(100.0);
                            push_event(
                                &mut events,
                                format!("Velocidad simulación: {:.0}×", current_speed),
                            );
                        }
                        KeyCode::Char('-') => {
                            current_speed = (current_speed - 5.0).max(1.0);
                            push_event(
                                &mut events,
                                format!("Velocidad simulación: {:.0}×", current_speed),
                            );
                        }
                        _ => {}
                    }
                }
            }

            // ── Simulation advance ────────────────────────────────────────────
            if !paused && !arrived {
                let sim_per_frame = current_speed * real_frame.as_secs_f64();
                let steps = (sim_per_frame / dt).ceil() as u32;
                for _ in 0..steps {
                    sim_state.throttle = 1.0;
                    sim_state.brake = 0.0;
                    let res = step(&mut sim_state, &path_data, &train_physics, dt);
                    if res.arrived {
                        arrived = true;
                        push_event(
                            &mut events,
                            format!(
                                "✓ LLEGADA — t={:.0}s  dist={:.1}km  E={:.2}kWh",
                                sim_state.time_s(),
                                sim_state.odometer_m / 1000.0,
                                sim_state.cumulative_energy_j / 3_600_000.0,
                            ),
                        );
                        break;
                    }
                }

                // Log edge transitions
                let cur_edge = sim_state.current_edge().map(|s| s.to_string());
                if cur_edge != prev_edge {
                    if let Some(eid) = cur_edge.clone() {
                        if let Some(edge) = graph.edge(&eid) {
                            let to_name = graph
                                .node(&edge.to.0)
                                .and_then(|n| {
                                    if let openrailsrs_track::NodeKind::Station { name } = &n.kind {
                                        Some(format!(" → {name}"))
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_default();
                            push_event(
                                &mut events,
                                format!(
                                    "Arista: {}  {:.0}m  lím {:.0}km/h{}",
                                    &eid[..eid.len().min(12)],
                                    edge.length_m,
                                    edge.speed_limit_mps * 3.6,
                                    to_name,
                                ),
                            );
                        }
                    }
                    prev_edge = cur_edge;
                }
            }

            // ── Draw ──────────────────────────────────────────────────────────
            let v_kmh = sim_state.velocity_mps * 3.6;
            let progress = (sim_state.odometer_m / total_dist_m).clamp(0.0, 1.0);
            let cur_limit = sim_state
                .current_edge()
                .and_then(|eid| graph.edge(eid))
                .map(|e| e.speed_limit_mps * 3.6)
                .unwrap_or(0.0);
            let cur_edge_id = sim_state
                .current_edge()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let sim_time = sim_state.time_s();
            let sim_odometer = sim_state.odometer_m;
            let sim_energy = sim_state.cumulative_energy_j;

            let overspeed = v_kmh > cur_limit * 1.05 && cur_limit > 0.0;
            let status_str = if arrived {
                "LLEGÓ ✓"
            } else if paused {
                "PAUSA"
            } else {
                "EN SERVICIO"
            };

            let events_snapshot = events.clone();
            let route_name_clone = route_name.clone();

            terminal.draw(move |frame| {
                let size = frame.area();

                // Layout: title | train-table | event-log | controls
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // title
                        Constraint::Length(7), // train table
                        Constraint::Min(5),    // event log
                        Constraint::Length(3), // controls
                    ])
                    .split(size);

                // ── Clear ─────────────────────────────────────────────────────
                frame.render_widget(Clear, size);

                // ── Title ─────────────────────────────────────────────────────
                let title = Paragraph::new(format!(
                    " openrailsrs DISPATCH  •  {}  •  t={:.0}s  •  {:.0}× ",
                    route_name_clone, sim_time, current_speed,
                ))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .alignment(Alignment::Center)
                .block(Block::default());
                frame.render_widget(title, chunks[0]);

                // ── Train table ───────────────────────────────────────────────
                let speed_color = if overspeed {
                    Color::Red
                } else if v_kmh > cur_limit * 0.92 {
                    Color::Yellow
                } else {
                    Color::Green
                };

                let header = Row::new(vec![
                    Cell::from("Tren").style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Cell::from("Estado"),
                    Cell::from("Velocidad"),
                    Cell::from("Límite"),
                    Cell::from("Odómetro"),
                    Cell::from("Progreso"),
                    Cell::from("Energía"),
                    Cell::from("Arista actual"),
                ])
                .style(Style::default().bg(Color::DarkGray));

                let prog_bar = {
                    let filled = (progress * 20.0).round() as usize;
                    format!(
                        "[{}{}] {:.0}%",
                        "█".repeat(filled),
                        "░".repeat(20usize.saturating_sub(filled)),
                        progress * 100.0
                    )
                };

                let row = Row::new(vec![
                    Cell::from("CAF-6000 #1"),
                    Cell::from(status_str).style(Style::default().fg(if arrived {
                        Color::Green
                    } else if paused {
                        Color::Yellow
                    } else {
                        Color::Cyan
                    })),
                    Cell::from(format!("{:6.1} km/h", v_kmh))
                        .style(Style::default().fg(speed_color)),
                    Cell::from(format!("{:5.0} km/h", cur_limit)),
                    Cell::from(format!("{:7.0} m", sim_odometer)),
                    Cell::from(prog_bar),
                    Cell::from(format!("{:.2} kWh", sim_energy / 3_600_000.0)),
                    Cell::from(if cur_edge_id.len() > 14 {
                        format!("{}…", &cur_edge_id[..14])
                    } else {
                        cur_edge_id.clone()
                    }),
                ]);

                let table = Table::new(
                    vec![header, row],
                    [
                        Constraint::Length(12),
                        Constraint::Length(12),
                        Constraint::Length(12),
                        Constraint::Length(12),
                        Constraint::Length(12),
                        Constraint::Length(26),
                        Constraint::Length(10),
                        Constraint::Min(16),
                    ],
                )
                .block(
                    Block::default()
                        .title(" Trenes en servicio ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
                frame.render_widget(table, chunks[1]);

                // ── Event log ─────────────────────────────────────────────────
                let log_height = chunks[2].height.saturating_sub(2) as usize;
                let visible: Vec<Line> = events_snapshot
                    .iter()
                    .rev()
                    .take(log_height)
                    .rev()
                    .map(|e| {
                        let color = if e.contains("✓") || e.contains("reanudada") {
                            Color::Green
                        } else if e.contains("PAUSA") || e.contains("Velocidad") {
                            Color::Yellow
                        } else {
                            Color::White
                        };
                        Line::from(vec![Span::styled(
                            format!(" {e}"),
                            Style::default().fg(color),
                        )])
                    })
                    .collect();

                let log = Paragraph::new(visible)
                    .block(
                        Block::default()
                            .title(" Log de eventos ")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    )
                    .style(Style::default().fg(Color::White));
                frame.render_widget(log, chunks[2]);

                // ── Controls ─────────────────────────────────────────────────
                let controls =
                    Paragraph::new("  Espacio=pausa/reanudar   +/-=velocidad   Q/Esc=salir")
                        .style(Style::default().fg(Color::DarkGray))
                        .alignment(Alignment::Center)
                        .block(
                            Block::default()
                                .borders(Borders::TOP)
                                .border_style(Style::default().fg(Color::DarkGray)),
                        );
                frame.render_widget(controls, chunks[3]);
            })?;

            // ── Frame rate ────────────────────────────────────────────────────
            let elapsed = last_frame.elapsed();
            if elapsed < real_frame {
                std::thread::sleep(real_frame - elapsed);
            }
            last_frame = Instant::now();
        }
    })();

    // ── Restore terminal ──────────────────────────────────────────────────────
    let _ = execute!(terminal.backend_mut(), terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();

    result
}

fn push_event(events: &mut Vec<String>, msg: String) {
    if events.len() >= 200 {
        events.remove(0);
    }
    events.push(msg);
}
