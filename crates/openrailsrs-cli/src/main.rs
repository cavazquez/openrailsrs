use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use openrailsrs_export::{
    textual_replay_from_csv, track_graph_to_ascii, track_graph_to_dot, track_graph_to_geojson,
};
use openrailsrs_formats::parse_from_first_paren;
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_sim::run_from_scenario_file;
use openrailsrs_validate::compare_csv_files;

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
    Sim { scenario: PathBuf },
    /// Run simulation and evaluate game rules (writes outcome.toml).
    PlayHeadless { scenario: PathBuf },
    /// Compare two run CSV files (velocity, position, energy).
    Compare { run_a: PathBuf, run_b: PathBuf },
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
    /// Print a short textual replay of a run CSV.
    Replay {
        csv: PathBuf,
        #[arg(long, default_value_t = 25)]
        lines: usize,
    },
    /// Run several scenarios in parallel (rayon).
    Batch {
        #[arg(required = true)]
        scenarios: Vec<PathBuf>,
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
        Commands::Sim { scenario } => {
            let r = run_from_scenario_file(&scenario)
                .map_err(|e| anyhow::anyhow!("sim {}: {e}", scenario.display()))?;
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
            println!(
                "success={} score={:.1} penalties={:?}",
                o.success, o.score, o.penalties
            );
        }
        Commands::Compare { run_a, run_b } => {
            let rep =
                compare_csv_files(&run_a, &run_b).map_err(|e| anyhow::anyhow!("compare: {e}"))?;
            println!("{}", toml::to_string_pretty(&rep)?);
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
        Commands::Replay { csv, lines } => {
            let t =
                textual_replay_from_csv(&csv, lines).map_err(|e| anyhow::anyhow!("replay: {e}"))?;
            print!("{t}");
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
