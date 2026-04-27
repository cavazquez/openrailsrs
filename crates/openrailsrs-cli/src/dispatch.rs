//! Dispatch panel (Fase 12 / A-live-multi) — real-time TUI with multi-train support.
//!
//! Uses `LiveMultiSim` for frame-by-frame stepping; renders a per-train table with
//! speed, energy, regen, block status, and progress.
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

use openrailsrs_sim::{LiveMultiSim, LiveTrainSnapshot, TrainStatus};

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run_dispatch(scenario_path: &Path, speed_mul: f64) -> anyhow::Result<()> {
    let mut sim = LiveMultiSim::new(scenario_path)
        .map_err(|e| anyhow::anyhow!("Error al cargar el escenario: {e}"))?;

    let dt = 0.1_f64; // scenario dt (LiveMultiSim uses its own internal dt)
    let real_frame = Duration::from_millis(80); // ~12 FPS
    let mut current_speed = speed_mul;
    let mut paused = false;
    let mut events: Vec<String> = Vec::new();
    let scenario_name = scenario_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("escenario")
        .to_string();

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
        let mut prev_snapshots: Vec<LiveTrainSnapshot> = Vec::new();

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
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
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
                            current_speed = (current_speed + 5.0).min(200.0);
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
            let snapshots = if !paused && !sim.all_arrived() {
                let sim_per_frame = current_speed * real_frame.as_secs_f64();
                let steps = ((sim_per_frame / dt).ceil() as u32).max(1);
                let snaps = sim.step_frame(steps);

                // Detect arrivals / block waits for the event log
                for snap in &snaps {
                    let already_arrived = prev_snapshots
                        .iter()
                        .find(|s| s.id == snap.id)
                        .map(|s| s.status == TrainStatus::Arrived)
                        .unwrap_or(false);
                    if snap.status == TrainStatus::Arrived && !already_arrived {
                        push_event(
                            &mut events,
                            format!(
                                "✓ [{}] LLEGÓ — t={:.0}s  dist={:.1}km  E_net={:.2}kWh",
                                snap.id,
                                snap.time_s,
                                snap.odometer_m / 1000.0,
                                snap.cumulative_energy_j / 3_600_000.0,
                            ),
                        );
                    }
                    let prev_wait = prev_snapshots
                        .iter()
                        .find(|s| s.id == snap.id)
                        .map(|s| s.status == TrainStatus::WaitingBlock)
                        .unwrap_or(false);
                    if snap.status == TrainStatus::WaitingBlock && !prev_wait {
                        push_event(
                            &mut events,
                            format!("⏸ [{}] esperando bloque libre", snap.id),
                        );
                    }
                    if snap.status == TrainStatus::Running && prev_wait {
                        push_event(
                            &mut events,
                            format!("▶ [{}] bloque libre — reanuda", snap.id),
                        );
                    }
                }
                prev_snapshots = snaps.clone();
                snaps
            } else {
                prev_snapshots.clone()
            };

            // ── Draw ──────────────────────────────────────────────────────────
            let sim_time = sim.sim_time();
            let all_done = sim.all_arrived();
            let speed_now = current_speed;
            let events_snapshot = events.clone();
            let name_clone = scenario_name.clone();

            terminal.draw(move |frame| {
                let size = frame.area();

                let row_count = (snapshots.len() + 2) as u16; // header + rows + border
                let table_h = row_count.max(5);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),           // title
                        Constraint::Length(table_h + 2), // train table
                        Constraint::Min(4),              // event log
                        Constraint::Length(3),           // controls
                    ])
                    .split(size);

                frame.render_widget(Clear, size);

                // Title
                let status_str = if all_done {
                    "TODOS LLEGARON ✓"
                } else if paused {
                    "PAUSA"
                } else {
                    "EN SERVICIO"
                };
                let title = Paragraph::new(format!(
                    " openrailsrs DISPATCH  •  {}  •  t={:.0}s  •  {:.0}×  •  {} ",
                    name_clone, sim_time, speed_now, status_str
                ))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .alignment(Alignment::Center);
                frame.render_widget(title, chunks[0]);

                // Train table
                let header = Row::new(vec![
                    Cell::from("Tren").style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Cell::from("Estado"),
                    Cell::from("v km/h"),
                    Cell::from("Odóm"),
                    Cell::from("Progreso"),
                    Cell::from("E_net kWh"),
                    Cell::from("Regen kWh"),
                    Cell::from("Arista"),
                ])
                .style(Style::default().bg(Color::DarkGray));

                let rows: Vec<Row> = snapshots
                    .iter()
                    .enumerate()
                    .map(|(i, snap)| {
                        let v_kmh = snap.velocity_mps * 3.6;
                        let progress = (snap.odometer_m / snap.total_dist_m).clamp(0.0, 1.0);
                        let (status_str, status_color) = match snap.status {
                            TrainStatus::Arrived => ("LLEGÓ ✓", Color::Green),
                            TrainStatus::WaitingBlock => ("ESPERA BLOQUE", Color::Yellow),
                            TrainStatus::WaitingToDepart => ("ESPERANDO", Color::DarkGray),
                            TrainStatus::Running => ("EN SERVICIO", Color::Cyan),
                        };
                        let prog_bar = {
                            let filled = (progress * 16.0).round() as usize;
                            format!(
                                "[{}{}]{:.0}%",
                                "█".repeat(filled),
                                "░".repeat(16usize.saturating_sub(filled)),
                                progress * 100.0
                            )
                        };
                        let edge_short = snap
                            .current_edge_id
                            .as_deref()
                            .map(|e| {
                                if e.len() > 12 {
                                    format!("{}…", &e[..12])
                                } else {
                                    e.to_string()
                                }
                            })
                            .unwrap_or_default();
                        let regen_kwh = snap.regen_energy_j / 3_600_000.0;
                        let bg = if i % 2 == 0 {
                            Color::Reset
                        } else {
                            Color::DarkGray
                        };
                        Row::new(vec![
                            Cell::from(snap.id.clone()),
                            Cell::from(status_str).style(Style::default().fg(status_color)),
                            Cell::from(format!("{v_kmh:5.1}")),
                            Cell::from(format!("{:.0}m", snap.odometer_m)),
                            Cell::from(prog_bar),
                            Cell::from(format!("{:.3}", snap.cumulative_energy_j / 3_600_000.0)),
                            Cell::from(if regen_kwh > 0.001 {
                                format!("{:.3}", regen_kwh)
                            } else {
                                "—".to_string()
                            }),
                            Cell::from(edge_short),
                        ])
                        .style(Style::default().bg(bg))
                    })
                    .collect();

                let mut all_rows: Vec<Row> = vec![header];
                all_rows.extend(rows);

                let table = Table::new(
                    all_rows,
                    [
                        Constraint::Length(14),
                        Constraint::Length(14),
                        Constraint::Length(8),
                        Constraint::Length(9),
                        Constraint::Length(22),
                        Constraint::Length(10),
                        Constraint::Length(10),
                        Constraint::Min(12),
                    ],
                )
                .block(
                    Block::default()
                        .title(" Trenes en servicio ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
                frame.render_widget(table, chunks[1]);

                // Event log
                let log_h = chunks[2].height.saturating_sub(2) as usize;
                let visible: Vec<Line> = events_snapshot
                    .iter()
                    .rev()
                    .take(log_h)
                    .rev()
                    .map(|e| {
                        let color = if e.contains('✓') || e.contains("reanuda") {
                            Color::Green
                        } else if e.contains("PAUSA")
                            || e.contains("Velocidad")
                            || e.contains("espera")
                        {
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

                let log = Paragraph::new(visible).block(
                    Block::default()
                        .title(" Log de eventos ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );
                frame.render_widget(log, chunks[2]);

                // Controls
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
