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

//! CLI entry for `openrailsrs-render3d`. Core logic in [`openrailsrs_render3d`].

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::state::condition::in_state;
use clap::Parser;

use openrailsrs_bevy_scenery::{MstsAssetKind, MstsLoadDiagnostics};
use openrailsrs_render3d::{
    DebugHudEnabled, FlySpeed, MstsRootDir, PlayerStartPoseResource, Render3dPlugin, RouteDir,
    SceneDebugContext, SceneExtent, StaticConsistPlan, TdbTrackResource, TileCatalog, TileEntry,
    TileStreamConfig, TilesToRender, activity, catalog_entries_for_initial_load,
    default_track_camera_pose, default_trackobj_camera_pose, fly_camera, load_consist_at_path,
    objects, quit_on_esc, resolve_pat_start_pose, resolve_player_consist_path,
    resolve_player_start_pose, scenery, sky, stream, tdb_track, terrain, toggle_debug_hud, track,
    update_debug_hud, update_window_title,
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

    // Elegir el tile central.
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
    let tdb_chords = tdb_ctx
        .as_ref()
        .map(|ctx| track::collect_tdb_chords(ctx, cx, cz, cli.radius));

    // Construir la lista de tiles (grid de radio R, el central primero).
    let r = cli.radius as i32;
    let mut tile_coords: Vec<(i32, i32)> = Vec::new();
    tile_coords.push((cx, cz)); // central siempre primero
    for dz in -r..=r {
        for dx in -r..=r {
            if dx == 0 && dz == 0 {
                continue; // ya está
            }
            tile_coords.push((cx + dx, cz + dz));
        }
    }

    // Cargar cada tile (los que existan).
    let tile_size_m = 2048.0_f32; // lado de un tile MSTS
    let mut entries: Vec<TileEntry> = Vec::new();
    let mut skipped = 0usize;
    let mut load_diag = MstsLoadDiagnostics::default();

    for &(tx, tz) in &tile_coords {
        let world_offset = Vec3::new(
            (tx - cx) as f32 * tile_size_m,
            0.0,
            (cz - tz) as f32 * tile_size_m,
        );
        match terrain::load_tile_geometry(&cli.route, tx, tz) {
            Ok(geom) => {
                load_diag.record_loaded(MstsAssetKind::Terrain);
                let base_y = geom.height.base_y();
                let ribbon = if tdb_chords.is_some() {
                    track::TrackRibbon::default()
                } else if let Some(g) = graph.as_ref() {
                    track::build_track_ribbon(g, tx, tz, &geom.height)
                } else {
                    track::TrackRibbon::default()
                };
                let objs = objects::load_objects_with_diag(
                    &cli.route,
                    tx,
                    tz,
                    base_y,
                    Some(&mut load_diag),
                );
                entries.push(TileEntry {
                    geometry: geom,
                    world_offset,
                    track: ribbon,
                    objects: objs,
                });
            }
            Err(_) => {
                // Grid holes (no `.t`) are normal; do not inflate failed counts.
                skipped += 1;
            }
        }
    }

    if let Some(chords) = &tdb_chords {
        let height_rows: Vec<_> = entries
            .iter()
            .map(|e| (e.geometry.tile_x, e.geometry.tile_z, &e.geometry.height))
            .collect();
        let height_index = tdb_track::TileHeightIndex::new(&height_rows, (cx, cz));
        let scene_ribbon = track::build_tdb_track_ribbon(chords, cx, cz, &height_index, cli.radius);
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.geometry.tile_x == cx && e.geometry.tile_z == cz)
        {
            entry.track = scene_ribbon;
        }
    }

    if entries.is_empty() {
        anyhow::bail!("no se pudo cargar ningún tile en el radio={r} alrededor de ({cx}, {cz})");
    }

    // Estadísticas de la escena.
    let total_patches: usize = entries.iter().map(|e| e.geometry.patches.len()).sum();
    let total_segments: usize = entries.iter().map(|e| e.track.segment_count()).sum();
    let total_objects: usize = entries.iter().map(|e| e.objects.len()).sum();
    println!(
        "render3d: {} tiles cargados ({} sin .t ignorados), {} patches, {} segmentos de vía, {} objetos",
        entries.len(),
        skipped,
        total_patches,
        total_segments,
        total_objects,
    );
    println!(
        "controles: WASD mover · Q/E bajar/subir · Shift rápido · click derecho + mouse mirar · Esc salir"
    );
    println!(
        "vía: OPENRAILSRS_TDB_UKFS=procedural fuerza rieles 3D (dyntrack); al cargar verás resumen vía:"
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

    // Posición inicial de cámara: `.act` → `.pat` directo → vía del tile central.
    let full_catalog = TileCatalog {
        entries: entries.clone(),
    };
    let stream_config = TileStreamConfig::new((cx, cz), cli.radius);
    let initial_entries = catalog_entries_for_initial_load(&full_catalog, &stream_config);
    let n_initial = initial_entries.len();
    if stream_config.streaming_enabled() {
        println!(
            "streaming: radio {} — carga inicial {n_initial}/{} tiles (resto bajo demanda)",
            stream_config.stream_radius,
            full_catalog.entries.len()
        );
    } else if stream_config.stream_radius > 0 {
        println!(
            "tiles: radio {} — {} tiles en escena",
            stream_config.stream_radius, n_initial
        );
    }
    let tiles_res = TilesToRender(initial_entries);
    let tdb = tdb_ctx.as_ref().map(|c| &c.track_db);
    let from_ribbon = default_track_camera_pose(&tiles_res);
    let from_scenery = default_trackobj_camera_pose(&tiles_res);
    let player_start = activity_session
        .as_ref()
        .and_then(|session| {
            resolve_player_start_pose(
                &cli.route,
                &session.path,
                graph.as_ref(),
                tdb,
                (cx, cz),
                &tiles_res,
            )
        })
        .or_else(|| {
            cli.player_path.as_ref().and_then(|pat| {
                resolve_pat_start_pose(
                    &cli.route,
                    pat,
                    cli.path_offset_m.max(0.0),
                    graph.as_ref(),
                    tdb,
                    (cx, cz),
                    &tiles_res,
                )
            })
        })
        .or(from_ribbon)
        .or(from_scenery);
    if let Some(pose) = &player_start {
        let src = if activity_session.is_some() {
            "jugador (.act)"
        } else if cli.player_path.is_some() {
            "jugador (.pat)"
        } else if Some(*pose) == from_ribbon {
            "vía (tile central)"
        } else {
            "escenario (TrackObj/túnel)"
        };
        println!(
            "cámara [{src}]: ({:.0}, {:.1}, {:.0}) yaw {:.0}°",
            pose.position.x,
            pose.position.y,
            pose.position.z,
            pose.yaw_rad.to_degrees()
        );
    } else {
        println!("cámara [overview]: vista cenital del tile (sin .act/.pat/vía ribbon)");
    }

    let activity_consist = activity_session.as_ref().and_then(|s| {
        if s.player_consist.is_empty() {
            None
        } else {
            Some(s.player_consist.as_str())
        }
    });
    let consist_plan =
        resolve_player_consist_path(&cli.route, cli.consist.as_deref(), activity_consist).and_then(
            |path| {
                load_consist_at_path(&path).map(|vehicles| {
                    println!(
                        "consist: {} vehículo(s) desde {}",
                        vehicles.len(),
                        path.display()
                    );
                    StaticConsistPlan { vehicles }
                })
            },
        );

    // Ventana centrada en el tile principal.
    let side = tiles_res
        .0
        .first()
        .map(|e| e.geometry.side_m)
        .unwrap_or(2048.0);
    let n_tiles = tiles_res.0.len();
    let _catalog_count = full_catalog.entries.len();
    let object_count: usize = tiles_res.0.iter().map(|e| e.objects.len()).sum();

    let mut app = App::new();
    let night_mode = texture_env.night;
    app.add_plugins(
        DefaultPlugins
            .set(openrailsrs_bevy_scenery::shared_asset_plugin())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: format!(
                        "openrailsrs-render3d — tile ({cx}, {cz}) r={r} [{n_tiles} tiles]",
                    ),
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
        radius: r as u32,
        tile_count: n_tiles,
        object_count,
    })
    .insert_resource(DebugHudEnabled(!cli.no_hud))
    .insert_resource(RouteDir(cli.route.clone()))
    .insert_resource(MstsRootDir(msts_root))
    .insert_resource(texture_env)
    .insert_resource(full_catalog)
    .insert_resource(stream_config)
    .insert_resource(tiles_res)
    .insert_resource(SceneExtent { side_m: side })
    .insert_resource(PlayerStartPoseResource(player_start))
    .insert_resource(load_diag);
    if let Some(session) = activity_session {
        app.insert_resource(session);
    }
    if let Some(plan) = consist_plan {
        app.insert_resource(plan);
    }
    if let Some(ctx) = tdb_ctx {
        app.insert_resource(TdbTrackResource {
            ctx,
            grid_radius: r as u32,
        });
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
            stream::tile_stream_system.run_if(in_state(openrailsrs_render3d::AppState::Playing)),
            quit_on_esc,
        ),
    )
    .run();

    Ok(())
}
