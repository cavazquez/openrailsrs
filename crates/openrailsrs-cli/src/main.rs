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
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_sim::{
    ScriptedDriver, run_from_scenario_file, run_from_scenario_file_with_driver,
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
