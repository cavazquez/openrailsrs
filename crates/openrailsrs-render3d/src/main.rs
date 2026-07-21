//! Render 3D nuevo, desde cero. Hito 1: mostrar UN tile de terreno de una ruta
//! MSTS/Open Rails (por defecto Chiltern), bien posicionado y con relieve real.
//!
//! Crecemos una capa a la vez (terreno → vía → objetos → tren), validando cada
//! una contra Open Rails antes de seguir. El viewer3d viejo queda como referencia.
//!
//! Uso:
//!   cargo run -p openrailsrs-render3d -- [--route DIR] [--activity ACT.act] [--tile-x N --tile-z N] [--radius R]
//!
//! Rutas OR recomendadas (scripts): ver [`docs/RENDER3D.md`](../../docs/RENDER3D.md) y `./scripts/run_render3d_*.sh`.
//!
//! Controles:
//!   W/A/S/D  mover    Q/E  bajar/subir    Shift  más rápido
//!   Botón derecho + mover el mouse  mirar    F3 HUD    F4-F8 VSM    F9 preset debug    Esc  salir
//!
//! #55: la ventana abre de inmediato; el parse de tiles corre en `AsyncComputeTaskPool`.

//! CLI entry for `openrailsrs-render3d`. Core logic in [`openrailsrs_render3d`].

use std::path::PathBuf;
use std::time::Instant;

use bevy::prelude::*;
use bevy::state::condition::in_state;
use clap::Parser;

use openrailsrs_bevy_scenery::MstsLoadDiagnostics;
use openrailsrs_render3d::{
    DebugHudEnabled, FlySpeed, MstsRootDir, PlayerStartPoseResource, Render3dPlugin, RouteDir,
    SceneDebugContext, SceneExtent, TdbTrackResource, TileCatalog, TileParseRequest,
    TileStreamConfig, TilesToRender, activity, fly_camera, objects, quit_on_esc, scenery, sky,
    stream, terrain, tile_bundle, toggle_debug_hud, track, update_debug_hud, update_window_title,
};

#[derive(Parser, Debug)]
#[command(
    about = "Render 3D mínimo: tiles de terreno (desde cero)",
    allow_negative_numbers = true
)]
struct Cli {
    /// Carpeta de la ruta (con TILES/ y WORLD/).
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/chiltern"))]
    route: PathBuf,
    /// Raíz de la instalación de MSTS/Open Rails (con GLOBAL/). Si se omite, se deduce subiendo dos niveles.
    #[arg(long)]
    msts_root: Option<PathBuf>,
    /// Tile X central (interno, con signo). Si se omite, se elige el centroide de WORLD/.
    #[arg(long)]
    tile_x: Option<i32>,
    /// Tile Z central (interno, con signo).
    #[arg(long)]
    tile_z: Option<i32>,
    /// Radio del grid de tiles a cargar. 0=solo el tile central (1×1),
    /// 1=3×3=9 tiles, 2=5×5=25 tiles, etc. Todo el grid se spawnea al arrancar.
    #[arg(long, default_value_t = 2)]
    radius: u32,
    /// Actividad MSTS (`.act`): estación y hora de inicio para texturas/sol.
    #[arg(long)]
    activity: Option<PathBuf>,
    /// Path del jugador (`.pat`) sin `.act` — rutas OR-only como New Forest.
    #[arg(long)]
    player_path: Option<PathBuf>,
    /// Metros desde el inicio del `.pat` (con `--player-path`).
    #[arg(long, default_value_t = 0.0)]
    path_offset_m: f64,
    /// Override de estación (`Spring`, `Summer`, `Autumn`, `Winter`).
    #[arg(long)]
    season: Option<String>,
    /// Override de clima: `clear` o `snow`.
    #[arg(long)]
    weather: Option<String>,
    /// Forzar modo noche (texturas Night/ + sub-objetos nocturnos).
    #[arg(long, action = clap::ArgAction::SetTrue)]
    night: bool,
    /// Forzar modo día (ignora hora nocturna del `.act`).
    #[arg(long, action = clap::ArgAction::SetTrue, conflicts_with = "night")]
    day: bool,
    /// Ocultar HUD de depuración (posición, tile, FPS).
    #[arg(long, action = clap::ArgAction::SetTrue)]
    no_hud: bool,
    /// Consist del jugador (`.con`); si se omite y hay `--activity`, se usa el del `.act`.
    #[arg(long)]
    consist: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let boot = Instant::now();
    let cli = Cli::parse();

    // Deducir msts_root si no se especificó.
    let msts_root = cli.msts_root.unwrap_or_else(|| {
        let route_canonical =
            std::fs::canonicalize(&cli.route).unwrap_or_else(|_| cli.route.clone());
        if let Some(parent) = route_canonical.parent() {
            if let Some(grandparent) = parent.parent() {
                return grandparent.to_path_buf();
            }
        }
        PathBuf::from(".")
    });

    let center_tile = match (cli.tile_x, cli.tile_z) {
        (Some(x), Some(z)) => Some((x, z)),
        (None, None) => None,
        _ => anyhow::bail!("pasá --tile-x y --tile-z juntos, o ninguno"),
    };
    let graph = track::load_graph(&cli.route);

    // Elegir el tile central (descubrimiento ligero; sin parsear geometría).
    let (cx, cz) = if let Some(t) = center_tile {
        t
    } else if let Some(t) = objects::busiest_world_tile(&cli.route) {
        t
    } else if let Some(t) = graph.as_ref().and_then(track::graph_centroid_tile) {
        t
    } else {
        terrain::resolve_tile(&cli.route, None)?
    };

    let activity_session = cli
        .activity
        .as_ref()
        .and_then(|p| activity::load_activity_session(&cli.route, p));
    if cli.activity.is_some() && activity_session.is_none() {
        anyhow::bail!(
            "no se pudo cargar --activity {:?} bajo {}",
            cli.activity,
            cli.route.display()
        );
    }
    let night_override = if cli.night {
        Some(true)
    } else if cli.day {
        Some(false)
    } else {
        None
    };
    let texture_env = activity::build_texture_environment(
        activity_session.as_ref(),
        cli.season.as_deref(),
        cli.weather.as_deref(),
        night_override,
    );

    let tdb_ctx = track::load_tdb_context(&cli.route);
    if let Some(ctx) = &tdb_ctx {
        println!(
            "render3d: .tdb — {} nodos, {} secciones, {} formas tsection",
            ctx.track_db.nodes.len(),
            ctx.tsection.sections.len(),
            ctx.tsection.shapes.len()
        );
    }

    let r = cli.radius;
    println!("render3d: abriendo ventana — tile ({cx}, {cz}) r={r} (parse async #55)");
    println!(
        "controles: WASD mover · Q/E bajar/subir · Shift rápido · click derecho + mouse mirar · Esc salir"
    );
    if let Some(act) = &activity_session {
        let (h, m, s) = act.start_time_hms();
        println!(
            "actividad: \"{}\" — inicio {:02}:{:02}:{:02}, estación={}",
            act.name,
            h,
            m,
            s,
            act.season.label()
        );
    }
    println!(
        "texturas: estación={} clima={} noche={}",
        texture_env.season.label(),
        if texture_env.snow_weather {
            "snow"
        } else {
            "clear"
        },
        texture_env.night
    );

    let activity_consist = activity_session.as_ref().and_then(|s| {
        if s.player_consist.is_empty() {
            None
        } else {
            Some(s.player_consist.clone())
        }
    });
    let activity_path_for_pose = activity_session.as_ref().map(|s| s.path.clone());

    let parse_request = TileParseRequest {
        route: cli.route.clone(),
        center: (cx, cz),
        radius: r,
        player_path: cli.player_path.clone(),
        path_offset_m: cli.path_offset_m,
        consist: cli.consist.clone(),
        activity_consist,
        activity_path_for_pose,
        graph,
        tdb: tdb_ctx.clone(),
    };

    let night_mode = texture_env.night;
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(openrailsrs_bevy_scenery::shared_asset_plugin())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!("openrailsrs-render3d — tile ({cx}, {cz}) r={r}"),
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(Render3dPlugin)
    .insert_resource(ClearColor(sky::sky_clear_color(night_mode)))
    .insert_resource(FlySpeed(120.0))
    .insert_resource(SceneDebugContext {
        center_tile: (cx, cz),
        radius: r,
        tile_count: 0,
        object_count: 0,
    })
    .insert_resource(DebugHudEnabled(!cli.no_hud))
    .insert_resource(RouteDir(cli.route.clone()))
    .insert_resource(MstsRootDir(msts_root))
    .insert_resource(texture_env)
    .insert_resource(parse_request)
    // Placeholders until async tile parse completes.
    .insert_resource(TileCatalog {
        entries: Vec::new(),
    })
    .insert_resource(TileStreamConfig::new((cx, cz), r))
    .insert_resource(TilesToRender(Vec::new()))
    .insert_resource(SceneExtent { side_m: 2048.0 })
    .insert_resource(PlayerStartPoseResource(None))
    .insert_resource(MstsLoadDiagnostics::default());
    if let Some(session) = activity_session {
        app.insert_resource(session);
    }
    if let Some(ctx) = tdb_ctx {
        app.insert_resource(TdbTrackResource {
            ctx,
            grid_radius: r,
        });
    }
    if openrailsrs_render3d::loading::perf_debug() {
        eprintln!(
            "[PERF] time_to_app_ms={:.1}",
            boot.elapsed().as_secs_f64() * 1000.0
        );
    }
    app.add_systems(
        Update,
        (
            fly_camera.run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            update_debug_hud.run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            update_window_title.run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            toggle_debug_hud.run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            scenery::update_water_surfaces
                .run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            tile_bundle::materialize_tile_bundle_system
                .run_if(in_state(openrailsrs_render3d::AppState::Playing))
                .before(stream::tile_stream_system),
            stream::tile_stream_system.run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            quit_on_esc,
        ),
    )
    .run();

    Ok(())
}
