mod cab;
mod dispatch;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use openrailsrs_export::{
    animated_replay_from_csv, textual_replay_from_csv, track_graph_to_ascii, track_graph_to_dot,
    track_graph_to_geojson,
};
use openrailsrs_formats::parse_from_first_paren;
use openrailsrs_msts::{import_activity_with_summary, import_route_with_summary};
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_sim::{
    LiveMultiSim, ScriptedDriver, run_from_scenario_file, run_from_scenario_file_with_driver,
    run_multi_train_from_scenario_file,
};
use openrailsrs_validate::{ValidationConfig, compare_csv_files_with_config};

#[derive(Parser)]
#[command(
    name = "openrailsrs",
    version,
    about = "Headless-first railway simulation (openrailsrs)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable `tracing` logs (RUST_LOG still applies).
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse an MSTS-style file and print the generic AST.
    Inspect { file: PathBuf },
    /// Export the route track graph as Graphviz DOT.
    Graph {
        route: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Run headless simulation from a scenario TOML.
    Sim {
        scenario: PathBuf,
        /// Path to a ScriptedDriver CSV (time_s,throttle,brake). Uses AutoDriver if omitted.
        #[arg(long)]
        driver: Option<PathBuf>,
    },
    /// Run simulation and evaluate game rules (writes outcome.toml).
    PlayHeadless { scenario: PathBuf },
    /// Compare two run CSV files (velocity, position, energy) with optional tolerances.
    Compare {
        run_a: PathBuf,
        run_b: PathBuf,
        /// Max RMS tolerance for velocity_mps (m/s). Omit to skip velocity check.
        #[arg(long)]
        max_velocity_rms: Option<f64>,
        /// Max peak absolute tolerance for velocity_mps (m/s).
        #[arg(long)]
        max_velocity_max: Option<f64>,
        /// Max RMS tolerance for odometer_m (m).
        #[arg(long)]
        max_position_rms: Option<f64>,
        /// Max peak absolute tolerance for odometer_m (m).
        #[arg(long)]
        max_position_max: Option<f64>,
        /// Max RMS tolerance for cumulative_energy_kwh (kWh).
        #[arg(long)]
        max_energy_rms: Option<f64>,
        /// Max peak absolute tolerance for cumulative_energy_kwh (kWh).
        #[arg(long)]
        max_energy_max: Option<f64>,
    },
    /// Export GeoJSON for the route graph.
    ExportGeojson {
        route: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Print a small ASCII map of the route to stdout.
    AsciiMap {
        route: PathBuf,
        #[arg(long, default_value_t = 48)]
        width: usize,
        #[arg(long, default_value_t = 12)]
        height: usize,
    },
    /// Print a short textual replay of a run CSV (or animate it with --watch).
    Replay {
        csv: PathBuf,
        /// Max rows printed in static mode (ignored when --watch is active).
        #[arg(long, default_value_t = 25)]
        lines: usize,
        /// Animate the replay in the terminal, refreshing each row in place.
        #[arg(long)]
        watch: bool,
        /// Time acceleration factor for --watch mode (e.g. 10 = 10× faster than real-time).
        #[arg(long, default_value_t = 10.0)]
        speed: f64,
    },
    /// Run several scenarios in parallel (rayon).
    Batch {
        #[arg(required = true)]
        scenarios: Vec<PathBuf>,
    },
    /// Run multi-train simulation (block-occupancy aware, interleaved clock).
    SimMulti { scenario: PathBuf },
    /// Interactive cab mode: drive the train in real time with keyboard controls.
    Cab {
        scenario: PathBuf,
        /// Simulation speed multiplier (default 10× = 10 sim-seconds per real second).
        #[arg(long, default_value_t = 10.0)]
        speed: f64,
    },
    /// Real-time dispatch panel: monitor simulation with ratatui TUI.
    Dispatch {
        scenario: PathBuf,
        /// Simulation speed multiplier.
        #[arg(long, default_value_t = 10.0)]
        speed: f64,
    },
    /// Campaign management commands.
    Campaign {
        #[command(subcommand)]
        cmd: CampaignCmd,
    },
    /// Run a timetable (multi-train, non-interactive) and print per-train results.
    Timetable {
        #[command(subcommand)]
        cmd: TimetableCmd,
    },
    /// Import railway topology from an Overpass JSON file and write track.toml.
    ImportOsm {
        /// Path to the Overpass JSON file (see examples/osm/overpass_query.txt).
        input: PathBuf,
        /// Destination track.toml file (parent directory is created if needed).
        #[arg(long)]
        out: PathBuf,
        /// Route id written into [route] id in the output TOML.
        #[arg(long, default_value = "imported")]
        route_id: String,
        /// Default speed limit (km/h) for ways without a maxspeed tag.
        #[arg(long, default_value_t = 80.0)]
        default_speed: f64,
        /// Disable bidirectional edges (by default railway edges are added in both directions).
        #[arg(long)]
        one_way: bool,
    },
    /// Inspect an MSTS ASCII `.s` shape file: prints LOD / mesh / texture stats.
    ShapeDump {
        file: PathBuf,
        /// Emit structured stats as JSON instead of a human-readable summary.
        #[arg(long)]
        json: bool,
    },
    /// Inspect an MSTS `.w` world tile file: prints item counts per kind.
    WorldDump {
        file: PathBuf,
        /// Optional path to write a CSV with one row per item (kind,uid,file_name,x,y,z).
        #[arg(long)]
        csv: Option<PathBuf>,
    },
    /// Decode an MSTS `.ace` texture and write its mip 0 as a PNG.
    AceDecode {
        file: PathBuf,
        /// Output PNG file.
        out: PathBuf,
    },
    /// Inspect an MSTS terrain `.y` tile: sample grid stats and optional mesh counts.
    TerrainDump {
        file: PathBuf,
        /// Emit structured stats as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Import a Microsoft Train Simulator route / activity.
    ImportMsts {
        /// Path to the MSTS route directory (must contain a *.tdb file).
        route_dir: PathBuf,
        /// Output directory for generated track.toml / scenario.toml files.
        /// Defaults to the current directory.
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Path to an MSTS activity file (*.act).  If omitted, only track.toml is generated.
        #[arg(long)]
        activity: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum TimetableCmd {
    /// Run all services in a timetable.toml and print a summary table.
    Run {
        /// Path to the timetable.toml file.
        timetable: PathBuf,
        /// Simulation speed multiplier (steps per second; higher = faster run).
        #[arg(long, default_value_t = 100.0)]
        speed: f64,
    },
}

#[derive(Subcommand)]
enum CampaignCmd {
    /// Show mission list and progress for a campaign.
    Status {
        /// Path to the campaign.toml file.
        campaign: PathBuf,
        /// Path to the progress.json file (created if missing).
        #[arg(long, default_value = "progress.json")]
        progress: PathBuf,
    },
    /// Run a mission from a campaign (headless simulation).
    Play {
        /// Path to the campaign.toml file.
        campaign: PathBuf,
        /// Mission id to play.
        mission: String,
        /// Path to the progress.json file.
        #[arg(long, default_value = "progress.json")]
        progress: PathBuf,
    },
    /// Reset all progress for a campaign.
    Reset {
        campaign: PathBuf,
        #[arg(long, default_value = "progress.json")]
        progress: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.verbose {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .try_init();
    }

    match cli.command {
        Commands::Inspect { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("read {}", file.display()))?;
            let ast = parse_from_first_paren(&text)
                .map_err(|e| anyhow::anyhow!("parse {}: {e}", file.display()))?;
            println!("{ast}");
        }
        Commands::Graph { route, out } => {
            let g = load_track_graph_from_route_dir(&route)
                .map_err(|e| anyhow::anyhow!("load route {}: {e}", route.display()))?;
            let dot = track_graph_to_dot(&g);
            std::fs::write(&out, dot).with_context(|| format!("write {}", out.display()))?;
            tracing::info!(path = %out.display(), "wrote DOT");
        }
        Commands::Sim { scenario, driver } => {
            let r = if let Some(driver_csv) = driver {
                let mut d = ScriptedDriver::from_csv(&driver_csv)
                    .map_err(|e| anyhow::anyhow!("load driver {}: {e}", driver_csv.display()))?;
                run_from_scenario_file_with_driver(&scenario, &mut d)
                    .map_err(|e| anyhow::anyhow!("sim {}: {e}", scenario.display()))?
            } else {
                run_from_scenario_file(&scenario)
                    .map_err(|e| anyhow::anyhow!("sim {}: {e}", scenario.display()))?
            };
            println!(
                "done: reached={} t={:.3}s odometer={:.1}m energy_kwh={:.4}",
                r.metadata.reached_destination,
                r.metadata.final_time_s,
                r.metadata.final_odometer_m,
                r.metadata.cumulative_energy_kwh
            );
        }
        Commands::PlayHeadless { scenario } => {
            let o = openrailsrs_game::play_headless_from_scenario_file(&scenario)
                .map_err(|e| anyhow::anyhow!("play {}: {e}", scenario.display()))?;
            println!("=== PlayHeadless: {} ===", scenario.display());
            println!("success={} score={:.1}", o.success, o.score);
            if o.penalties.is_empty() {
                println!("penalties: none");
            } else {
                println!("penalties:");
                for p in &o.penalties {
                    println!("  - {p}");
                }
            }
            println!("\n--- timeline ---");
            for ev in &o.timeline {
                println!("  [{:>8.1}s] {:16} {}", ev.time_s, ev.kind, ev.detail);
            }
            if !o.stops.is_empty() {
                println!("\n--- stops ---");
                for s in &o.stops {
                    let arrive = s
                        .actual_arrive_s
                        .map(|t| format!("{t:.0}s"))
                        .unwrap_or_else(|| "MISSED".into());
                    let depart = s
                        .actual_depart_s
                        .map(|t| format!("{t:.0}s"))
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "  {} arrive={} depart={} on_time={} early_dep={}",
                        s.node, arrive, depart, s.on_time, s.early_departure
                    );
                }
            }
            println!(
                "\nreached={} overspeed_events={} final_time={:.1}s",
                o.reached_destination, o.overspeed_events, o.final_time_s
            );
            println!("(outcome.toml written next to scenario)");
        }
        Commands::Compare {
            run_a,
            run_b,
            max_velocity_rms,
            max_velocity_max,
            max_position_rms,
            max_position_max,
            max_energy_rms,
            max_energy_max,
        } => {
            let config = ValidationConfig {
                max_velocity_rms,
                max_velocity_max,
                max_position_rms,
                max_position_max,
                max_energy_rms,
                max_energy_max,
            };
            let rep = compare_csv_files_with_config(&run_a, &run_b, &config)
                .map_err(|e| anyhow::anyhow!("compare: {e}"))?;

            // Human-readable summary with pass/fail.
            let status = |p: bool| if p { "PASS ✓" } else { "FAIL ✗" };
            println!(
                "=== Compare: {} vs {} ===",
                run_a.display(),
                run_b.display()
            );
            println!(
                "  velocity  rms={:.6}  max={:.6}  mean={:.6}  n={}  {}",
                rep.velocity.rms_diff,
                rep.velocity.max_abs_diff,
                rep.velocity.mean_abs_diff,
                rep.velocity.samples,
                status(rep.velocity_pass)
            );
            println!(
                "  position  rms={:.3}  max={:.3}  mean={:.3}  n={}  {}",
                rep.position.rms_diff,
                rep.position.max_abs_diff,
                rep.position.mean_abs_diff,
                rep.position.samples,
                status(rep.position_pass)
            );
            println!(
                "  energy    rms={:.6}  max={:.6}  mean={:.6}  n={}  {}",
                rep.energy.rms_diff,
                rep.energy.max_abs_diff,
                rep.energy.mean_abs_diff,
                rep.energy.samples,
                status(rep.energy_pass)
            );
            println!("overall: {}", if rep.pass { "PASS" } else { "FAIL" });

            // Also print full TOML report.
            println!("\n--- full report (TOML) ---");
            println!("{}", toml::to_string_pretty(&rep)?);

            if !rep.pass {
                std::process::exit(1);
            }
        }
        Commands::ExportGeojson { route, out } => {
            let g = load_track_graph_from_route_dir(&route)
                .map_err(|e| anyhow::anyhow!("load route: {e}"))?;
            let v = track_graph_to_geojson(&g);
            let s = serde_json::to_string_pretty(&v)?;
            std::fs::write(&out, s).with_context(|| format!("write {}", out.display()))?;
        }
        Commands::AsciiMap {
            route,
            width,
            height,
        } => {
            let g = load_track_graph_from_route_dir(&route)
                .map_err(|e| anyhow::anyhow!("load route: {e}"))?;
            print!("{}", track_graph_to_ascii(&g, width, height));
        }
        Commands::Replay {
            csv,
            lines,
            watch,
            speed,
        } => {
            if watch {
                animated_replay_from_csv(&csv, speed)
                    .map_err(|e| anyhow::anyhow!("replay: {e}"))?;
            } else {
                let t = textual_replay_from_csv(&csv, lines)
                    .map_err(|e| anyhow::anyhow!("replay: {e}"))?;
                print!("{t}");
            }
        }
        Commands::SimMulti { scenario } => {
            let result = run_multi_train_from_scenario_file(&scenario)
                .map_err(|e| anyhow::anyhow!("sim-multi {}: {e}", scenario.display()))?;
            println!("=== SimMulti: {} ===", scenario.display());
            for train in &result.results {
                let m = &train.sim_result.metadata;
                let block_waits = train
                    .sim_result
                    .events
                    .iter()
                    .filter(|e| matches!(e, openrailsrs_sim::SimEvent::BlockWait { .. }))
                    .count();
                println!(
                    "  [{}] reached={} t={:.1}s odometer={:.0}m energy={:.3}kwh block_waits={}",
                    train.id,
                    m.reached_destination,
                    m.final_time_s,
                    m.final_odometer_m,
                    m.cumulative_energy_kwh,
                    block_waits,
                );
            }
        }
        Commands::Cab { scenario, speed } => {
            cab::run_cab(&scenario, speed)?;
        }
        Commands::Dispatch { scenario, speed } => {
            dispatch::run_dispatch(&scenario, speed)?;
        }
        Commands::Campaign { cmd } => {
            use openrailsrs_campaign::{
                MissionState, load_campaign, load_progress, mission_statuses, record_result,
                save_progress,
            };
            use openrailsrs_game::evaluate::play_headless_from_scenario_file;
            match cmd {
                CampaignCmd::Status { campaign, progress } => {
                    let camp = load_campaign(&campaign)?;
                    let prog = load_progress(&progress)?;
                    let statuses = mission_statuses(&camp, &prog);

                    println!(
                        "\n  🚆  {}  —  {}\n",
                        camp.campaign.name, camp.campaign.description
                    );
                    println!(
                        "  {:<4}  {:<28}  {:<10}  {:<6}  Dificultad",
                        "ID", "Nombre", "Estado", "Score"
                    );
                    println!("  {}", "─".repeat(72));
                    for ms in &statuses {
                        let state_label = match ms.state {
                            MissionState::Locked => "🔒 bloqueada",
                            MissionState::Available => "▶ disponible",
                            MissionState::Completed => "✅ completada",
                        };
                        let score_str = ms
                            .best_score
                            .map(|s| format!("{s:3}/100{}", if ms.bonus { " ⭐" } else { "" }))
                            .unwrap_or_else(|| "  —".into());
                        println!(
                            "  {:<4}  {:<28}  {:<14}  {:<10}  {:?}",
                            ms.def.id, ms.def.name, state_label, score_str, ms.def.difficulty
                        );
                    }
                    println!();
                }
                CampaignCmd::Play {
                    campaign,
                    mission,
                    progress: progress_path,
                } => {
                    let camp = load_campaign(&campaign)?;
                    let mut prog = load_progress(&progress_path)?;
                    let statuses = mission_statuses(&camp, &prog);

                    let ms = statuses
                        .iter()
                        .find(|s| s.def.id == mission)
                        .ok_or_else(|| anyhow::anyhow!("misión no encontrada: {mission}"))?;

                    if ms.state == MissionState::Locked {
                        anyhow::bail!(
                            "misión bloqueada: {} — requiere: {:?}",
                            mission,
                            ms.def.requires
                        );
                    }

                    let camp_dir = campaign
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("campaign path has no parent"))?;
                    let scenario_path = camp_dir.join(&ms.def.scenario);

                    println!("▶  {} — {}", ms.def.name, ms.def.description);
                    let outcome = play_headless_from_scenario_file(&scenario_path)
                        .map_err(|e| anyhow::anyhow!("sim: {e}"))?;
                    let pct = (outcome.score.clamp(0.0, 100.0)) as u32;

                    println!(
                        "   Puntuación: {:.1}/100 → {}%  {} {}",
                        outcome.score,
                        pct,
                        if pct >= ms.def.bonus_threshold {
                            "⭐ BONUS"
                        } else {
                            ""
                        },
                        if pct >= ms.def.min_pass_score {
                            "✅ APROBADA"
                        } else {
                            "❌ No aprobada"
                        },
                    );

                    record_result(&mut prog, &mission, pct, ms.def.bonus_threshold);
                    save_progress(&progress_path, &prog)?;
                    println!("   Progreso guardado en {}", progress_path.display());
                }
                CampaignCmd::Reset {
                    campaign: _,
                    progress,
                } => {
                    if progress.exists() {
                        std::fs::remove_file(&progress)?;
                        println!("Progreso eliminado: {}", progress.display());
                    } else {
                        println!("No existe archivo de progreso: {}", progress.display());
                    }
                }
            }
        }
        Commands::ImportOsm {
            input,
            out,
            route_id,
            default_speed,
            one_way,
        } => {
            use openrailsrs_import::{OsmImportOptions, import_osm_file};
            let opts = OsmImportOptions {
                route_id: route_id.clone(),
                default_speed_kmh: default_speed,
                bidirectional: !one_way,
            };
            let toml_str = import_osm_file(&input, &opts)
                .map_err(|e| anyhow::anyhow!("import-osm {}: {e}", input.display()))?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, &toml_str)?;
            // Report summary.
            let nodes = toml_str
                .lines()
                .filter(|l| l.starts_with("[[nodes]]"))
                .count();
            let edges = toml_str
                .lines()
                .filter(|l| l.starts_with("[[edges]]"))
                .count();
            println!(
                "import-osm: wrote {} ({} nodes, {} edges)",
                out.display(),
                nodes,
                edges
            );
        }
        Commands::Timetable { cmd } => match cmd {
            TimetableCmd::Run { timetable, speed } => {
                let mut sim = LiveMultiSim::from_timetable(&timetable)
                    .map_err(|e| anyhow::anyhow!("timetable {}: {e}", timetable.display()))?;

                // Steps per frame sized so we finish without taking forever.
                let steps_per_frame = (speed as u32).max(1);
                let mut block_wait_total: u32 = 0;

                loop {
                    let snapshots = sim.step_frame(steps_per_frame);
                    // Count block-wait events (status transitions).
                    for snap in &snapshots {
                        if matches!(snap.status, openrailsrs_sim::TrainStatus::WaitingBlock) {
                            block_wait_total += 1;
                        }
                    }
                    if sim.all_arrived() || sim.sim_time() >= sim.duration() {
                        break;
                    }
                }

                let snapshots = sim.step_frame(0); // Final snapshot
                println!(
                    "\n  {:<8}  {:<10}  {:<8}  {:<8}  {:<10}  {:<12}  {:<10}",
                    "ID", "Destino", "Salida", "Llegada", "Distancia", "Energía", "Estado"
                );
                println!("  {}", "─".repeat(76));
                let mut trains_arrived: u32 = 0;
                let mut total_energy_kwh: f64 = 0.0;
                for snap in &snapshots {
                    let state_label = match snap.status {
                        openrailsrs_sim::TrainStatus::Running => "en marcha",
                        openrailsrs_sim::TrainStatus::WaitingBlock => "bloqueado",
                        openrailsrs_sim::TrainStatus::Arrived => {
                            trains_arrived += 1;
                            "LLEGÓ ✓"
                        }
                        openrailsrs_sim::TrainStatus::WaitingToDepart => "esperando",
                    };
                    let energy_kwh = snap.cumulative_energy_j / 3.6e6;
                    total_energy_kwh += energy_kwh;
                    println!(
                        "  {:<8}  {:<10}  {:<8.0}  {:<8.0}  {:<10.1}  {:<12.2}  {:<10}",
                        snap.id,
                        snap.id,
                        0.0_f64, // depart_s not stored in snapshot; use placeholder
                        snap.time_s,
                        snap.total_dist_m / 1000.0,
                        energy_kwh,
                        state_label,
                    );
                }
                let total = snapshots.len() as u32;
                let pct = (100 * trains_arrived).checked_div(total).unwrap_or(0);
                let mean_kwh = total_energy_kwh / total.max(1) as f64;
                println!();
                println!(
                    "  Red: {} trenes | Puntualidad {}% | Bloqueos totales: {} | Tiempo total {:.0} s | Energía media {:.2} kWh",
                    total,
                    pct,
                    block_wait_total,
                    sim.sim_time(),
                    mean_kwh,
                );
            }
        },
        Commands::ImportMsts {
            route_dir,
            out_dir,
            activity,
        } => {
            let out = out_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            std::fs::create_dir_all(&out)
                .with_context(|| format!("create output dir {}", out.display()))?;

            // 1. Import route: TDB → track.toml
            let (track_toml, n_nodes, n_edges) = import_route_with_summary(&route_dir)
                .map_err(|e| anyhow::anyhow!("import route {}: {e}", route_dir.display()))?;
            let track_out = out.join("track.toml");
            std::fs::write(&track_out, &track_toml)
                .with_context(|| format!("write {}", track_out.display()))?;
            println!(
                "✓ track.toml  — {} nodos, {} edges → {}",
                n_nodes,
                n_edges,
                track_out.display()
            );

            // 2. If an activity is given, import it: ACT + PAT → scenario.toml
            if let Some(act_path) = activity {
                let (scenario_toml, act_name) = import_activity_with_summary(&route_dir, &act_path)
                    .map_err(|e| anyhow::anyhow!("import activity {}: {e}", act_path.display()))?;
                let scenario_out = out.join("scenario.toml");
                std::fs::write(&scenario_out, &scenario_toml)
                    .with_context(|| format!("write {}", scenario_out.display()))?;
                println!(
                    "✓ scenario.toml  — \"{}\" → {}",
                    act_name,
                    scenario_out.display()
                );
            } else {
                // Auto-discover any *.act files in route_dir
                let acts: Vec<_> = std::fs::read_dir(&route_dir)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|x| x.eq_ignore_ascii_case("act"))
                            .unwrap_or(false)
                    })
                    .collect();

                for (i, act_entry) in acts.iter().enumerate() {
                    let act_path = act_entry.path();
                    match import_activity_with_summary(&route_dir, &act_path) {
                        Ok((scenario_toml, act_name)) => {
                            let fname = if i == 0 {
                                "scenario.toml".to_string()
                            } else {
                                format!("scenario_{i}.toml")
                            };
                            let scenario_out = out.join(&fname);
                            std::fs::write(&scenario_out, &scenario_toml)
                                .with_context(|| format!("write {}", scenario_out.display()))?;
                            println!(
                                "✓ {}  — \"{}\" → {}",
                                fname,
                                act_name,
                                scenario_out.display()
                            );
                        }
                        Err(e) => {
                            eprintln!("  warn: skipping {}: {e}", act_path.display());
                        }
                    }
                }
            }
        }
        Commands::ShapeDump { file, json } => {
            run_shape_dump(&file, json)?;
        }
        Commands::WorldDump { file, csv } => {
            run_world_dump(&file, csv.as_deref())?;
        }
        Commands::AceDecode { file, out } => {
            run_ace_decode(&file, &out)?;
        }
        Commands::TerrainDump { file, json } => {
            run_terrain_dump(&file, json)?;
        }
        Commands::Batch { scenarios } => {
            use rayon::prelude::*;
            let results: Vec<_> = scenarios
                .par_iter()
                .map(|p| {
                    let res = run_from_scenario_file(p);
                    (p.clone(), res)
                })
                .collect();
            for (p, res) in results {
                match res {
                    Ok(r) => println!(
                        "OK {} reached={}",
                        p.display(),
                        r.metadata.reached_destination
                    ),
                    Err(e) => println!("ERR {}: {e}", p.display()),
                }
            }
        }
    }
    Ok(())
}

fn run_shape_dump(file: &std::path::Path, json: bool) -> anyhow::Result<()> {
    use openrailsrs_formats::ShapeFile;

    let shape = ShapeFile::from_path(file)
        .map_err(|e| anyhow::anyhow!("parse shape {}: {e}", file.display()))?;

    let lod_count = shape.lod_controls.len();
    let distance_levels: usize = shape
        .lod_controls
        .iter()
        .map(|c| c.distance_levels.len())
        .sum();
    let primitive_count: usize = shape
        .lod_controls
        .iter()
        .flat_map(|c| &c.distance_levels)
        .flat_map(|dl| &dl.sub_objects)
        .map(|so| so.primitives.len())
        .sum();
    let triangle_count: usize = shape
        .lod_controls
        .iter()
        .flat_map(|c| &c.distance_levels)
        .flat_map(|dl| &dl.sub_objects)
        .flat_map(|so| &so.primitives)
        .map(|p| p.triangle_count())
        .sum();
    let texture_count = shape.texture_filenames.len();
    let prim_state_count = shape.prim_states.len();
    let matrix_count = shape.matrices.len();
    let point_count = shape.points.len();
    let normal_count = shape.normals.len();
    let uv_count = shape.uvs.len();

    if json {
        let value = serde_json::json!({
            "file": file.display().to_string(),
            "lod_controls": lod_count,
            "distance_levels": distance_levels,
            "primitives": primitive_count,
            "triangles": triangle_count,
            "points": point_count,
            "normals": normal_count,
            "uvs": uv_count,
            "prim_states": prim_state_count,
            "textures": texture_count,
            "texture_filenames": shape.texture_filenames,
            "matrices": matrix_count,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("=== shape-dump: {} ===", file.display());
        println!("  lod_controls       : {lod_count}");
        println!("  distance_levels    : {distance_levels}");
        println!("  primitives         : {primitive_count}");
        println!("  triangles          : {triangle_count}");
        println!("  points/normals/uvs : {point_count}/{normal_count}/{uv_count}");
        println!("  prim_states        : {prim_state_count}");
        println!("  matrices           : {matrix_count}");
        println!("  textures           : {texture_count}");
        for (i, name) in shape.texture_filenames.iter().enumerate() {
            println!("    [{i}] {name}");
        }
    }
    Ok(())
}

fn run_world_dump(file: &std::path::Path, csv: Option<&std::path::Path>) -> anyhow::Result<()> {
    use openrailsrs_formats::WorldFile;
    use std::collections::BTreeMap;

    let world = WorldFile::from_path(file)
        .map_err(|e| anyhow::anyhow!("parse world {}: {e}", file.display()))?;

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for item in &world.items {
        *counts.entry(item.kind()).or_insert(0) += 1;
    }

    println!(
        "=== world-dump: {} (tile {},{}) ===",
        file.display(),
        world.tile_x,
        world.tile_z
    );
    if counts.is_empty() {
        println!("  (no items)");
    } else {
        for (kind, n) in &counts {
            println!("  {kind:<10} = {n}");
        }
    }
    println!("  total = {}", world.items.len());

    if let Some(csv_path) = csv {
        if let Some(parent) = csv_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut writer = csv::Writer::from_path(csv_path)
            .map_err(|e| anyhow::anyhow!("open csv {}: {e}", csv_path.display()))?;
        writer.write_record(["kind", "uid", "file_name", "x", "y", "z"])?;
        for item in &world.items {
            let kind = item.kind();
            let uid = item.uid().map(|u| u.to_string()).unwrap_or_default();
            let file_name = item.file_name().unwrap_or("").to_string();
            let pos = item.position().unwrap_or_default();
            writer.write_record([
                kind,
                &uid,
                &file_name,
                &format!("{:.6}", pos.x),
                &format!("{:.6}", pos.y),
                &format!("{:.6}", pos.z),
            ])?;
        }
        writer.flush()?;
        println!("  csv → {}", csv_path.display());
    }
    Ok(())
}

fn run_ace_decode(file: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let ace = openrailsrs_ace::read_ace(file)
        .map_err(|e| anyhow::anyhow!("decode ace {}: {e}", file.display()))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    openrailsrs_ace::write_png(&ace, out)
        .map_err(|e| anyhow::anyhow!("write png {}: {e}", out.display()))?;
    println!(
        "=== ace-decode: {} ({}x{}, {}, {} mips) → {} ===",
        file.display(),
        ace.width,
        ace.height,
        ace.format.as_str(),
        ace.mips_count,
        out.display()
    );
    Ok(())
}

fn run_terrain_dump(file: &std::path::Path, json: bool) -> anyhow::Result<()> {
    use openrailsrs_formats::{TerrainFile, build_tile_mesh_data, read_f_raw, read_y_raw};

    let tile = TerrainFile::from_path(file)
        .map_err(|e| anyhow::anyhow!("parse terrain {}: {e}", file.display()))?;
    let raw_path = tile.y_raw_path(file);
    let grid = read_y_raw(&raw_path, &tile.samples)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", raw_path.display()))?;

    let min_h = grid
        .elevations
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let max_h = grid
        .elevations
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let mesh = build_tile_mesh_data(&grid, tile.samples.sample_size);
    let patch_set = tile.primary_patch_set();
    let patch_count = patch_set.map(|s| s.patches.len()).unwrap_or(0);
    let f_raw_path = tile.f_raw_path(file);
    let hidden = if tile.samples.f_buffer_file.trim().is_empty() {
        None
    } else {
        read_f_raw(&f_raw_path, &tile.samples).ok()
    };
    let hidden_vertices = hidden.as_ref().map(|f| f.hidden_count()).unwrap_or(0);
    let textures: Vec<String> = tile
        .shaders
        .iter()
        .flat_map(|s| s.texslots.iter().map(|t| t.filename.clone()))
        .collect();

    if json {
        let payload = serde_json::json!({
            "tile_x": tile.tile_x,
            "tile_z": tile.tile_z,
            "nsamples": tile.samples.nsamples,
            "sample_size_m": tile.samples.sample_size,
            "sample_floor": tile.samples.sample_floor,
            "sample_scale": tile.samples.sample_scale,
            "y_raw": raw_path.display().to_string(),
            "f_raw": if tile.samples.f_buffer_file.is_empty() { None } else { Some(f_raw_path.display().to_string()) },
            "elevation_min_m": min_h,
            "elevation_max_m": max_h,
            "vertices": mesh.positions.len(),
            "triangles": mesh.indices.len() / 3,
            "shader_count": tile.shaders.len(),
            "patch_count": patch_count,
            "hidden_vertices": hidden_vertices,
            "textures": textures,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "=== terrain-dump: {} (tile {},{}; {}×{} samples @ {} m) ===",
            file.display(),
            tile.tile_x,
            tile.tile_z,
            tile.samples.nsamples,
            tile.samples.nsamples,
            tile.samples.sample_size
        );
        println!("  y_raw      : {}", raw_path.display());
        if !tile.samples.f_buffer_file.is_empty() {
            println!("  f_raw      : {}", f_raw_path.display());
        }
        println!("  elevation  : {min_h:.2} .. {max_h:.2} m");
        println!(
            "  mesh       : {} vertices, {} triangles (legacy merged tile)",
            mesh.positions.len(),
            mesh.indices.len() / 3
        );
        println!("  shaders    : {}", tile.shaders.len());
        println!("  patches    : {patch_count}");
        println!("  hidden vtx : {hidden_vertices}");
        if !textures.is_empty() {
            println!("  textures   : {}", textures.join(", "));
        }
    }
    Ok(())
}
