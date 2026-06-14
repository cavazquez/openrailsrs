//! Render 3D nuevo, desde cero. Hito 1: mostrar UN tile de terreno de una ruta
//! MSTS/Open Rails (por defecto Chiltern), bien posicionado y con relieve real.
//!
//! Crecemos una capa a la vez (terreno → vía → objetos → tren), validando cada
//! una contra Open Rails antes de seguir. El viewer3d viejo queda como referencia.
//!
//! Uso:
//!   cargo run -p openrailsrs-render3d -- [--route DIR] [--tile-x N --tile-z N]
//!
//! Controles:
//!   W/A/S/D  mover    Q/E  bajar/subir    Shift  más rápido
//!   Botón derecho + mover el mouse  mirar    Rueda  no usada
//!   Esc  salir

mod coords;
mod loading;
mod objects;
mod shapes;
mod terrain;
mod textures;
mod track;
mod world_spawn;

use std::path::PathBuf;

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::state::condition::in_state;
use bevy::window::PrimaryWindow;
use clap::Parser;

use loading::{
    AppState, begin_load_stage, progressive_world_load, setup_loading_screen, update_loading_ui,
};
use terrain::TileGeometry;

#[derive(Parser, Debug)]
#[command(
    about = "Render 3D mínimo: un tile de terreno (desde cero)",
    allow_negative_numbers = true
)]
struct Cli {
    /// Carpeta de la ruta (con TILES/ y WORLD/).
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/chiltern"))]
    route: PathBuf,
    /// Tile X (interno, con signo). Si se omite, se elige el centroide de WORLD/.
    #[arg(long)]
    tile_x: Option<i32>,
    /// Tile Z (interno, con signo).
    #[arg(long)]
    tile_z: Option<i32>,
}

/// El tile cargado, insertado como recurso para spawnearlo en Startup.
#[derive(Resource)]
pub struct TileToRender(pub TileGeometry);

/// La vía del tile, en coords locales, para spawnear en Startup.
#[derive(Resource)]
pub struct TrackToRender(pub track::TrackRibbon);

/// Marcadores de objetos del `.w`, para spawnear en Startup.
#[derive(Resource)]
pub struct ObjectsToRender(pub Vec<objects::ObjectMarker>);

/// Carpeta de la ruta (para resolver texturas TERRTEX).
#[derive(Resource)]
pub struct RouteDir(pub PathBuf);

/// Velocidad base de la cámara (m/s).
#[derive(Resource)]
struct FlySpeed(f32);

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let tile = match (cli.tile_x, cli.tile_z) {
        (Some(x), Some(z)) => Some((x, z)),
        (None, None) => None,
        _ => anyhow::bail!("pasá --tile-x y --tile-z juntos, o ninguno"),
    };
    let graph = track::load_graph(&cli.route);

    let (tx, tz) = if let Some(t) = tile {
        t
    } else if let Some(t) = objects::busiest_world_tile(&cli.route) {
        t
    } else if let Some(t) = graph.as_ref().and_then(track::graph_centroid_tile) {
        t
    } else {
        terrain::resolve_tile(&cli.route, None)?
    };

    let loaded = terrain::load_tile_geometry(&cli.route, tx, tz)?;
    let track_ribbon = graph
        .as_ref()
        .map(|g| track::build_track_ribbon(g, tx, tz, &loaded.height))
        .unwrap_or_default();
    let object_markers = objects::load_objects(&cli.route, tx, tz, loaded.height.base_y());

    let textured = loaded
        .patches
        .iter()
        .filter(|p| p.texture.is_some())
        .count();
    println!(
        "render3d: tile ({}, {}) — {} patches ({} con textura), {} segmentos de vía, {} objetos, lado {:.0} m, altura {:.1}..{:.1} m MSL",
        loaded.tile_x,
        loaded.tile_z,
        loaded.patches.len(),
        textured,
        track_ribbon.segment_count(),
        object_markers.len(),
        loaded.side_m,
        loaded.min_y,
        loaded.max_y,
    );
    println!(
        "controles: WASD mover · Q/E bajar/subir · Shift rápido · click derecho + mouse mirar · Esc salir"
    );

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("openrailsrs-render3d — tile ({tx}, {tz})"),
                ..default()
            }),
            ..default()
        }))
        .init_state::<AppState>()
        .insert_resource(ClearColor(Color::srgb(0.53, 0.70, 0.92)))
        .insert_resource(FlySpeed(120.0))
        .insert_resource(RouteDir(cli.route.clone()))
        .insert_resource(TileToRender(loaded))
        .insert_resource(TrackToRender(track_ribbon))
        .insert_resource(ObjectsToRender(object_markers))
        .add_systems(Startup, (setup_loading_screen, begin_load_stage).chain())
        .add_systems(
            Update,
            (
                update_loading_ui.run_if(in_state(AppState::Loading)),
                progressive_world_load.run_if(in_state(AppState::Loading)),
                fly_camera.run_if(in_state(AppState::Playing)),
                quit_on_esc,
            ),
        )
        .run();

    Ok(())
}

/// Cámara fly: WASD/QE para moverse, click derecho + mouse para mirar.
fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    speed: Res<FlySpeed>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut tf) = cam.single_mut() else {
        return;
    };

    if buttons.pressed(MouseButton::Right) {
        let mut delta = Vec2::ZERO;
        for ev in motion.read() {
            delta += ev.delta;
        }
        if delta != Vec2::ZERO {
            let sens = 0.003;
            let (mut yaw, mut pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
            yaw -= delta.x * sens;
            pitch = (pitch - delta.y * sens).clamp(-1.54, 1.54);
            tf.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        }
    } else {
        motion.clear();
    }

    let mut dir = Vec3::ZERO;
    let fwd = *tf.forward();
    let right = *tf.right();
    if keys.pressed(KeyCode::KeyW) {
        dir += fwd;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= fwd;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        dir -= Vec3::Y;
    }
    if dir != Vec3::ZERO {
        let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            4.0
        } else {
            1.0
        };
        tf.translation += dir.normalize() * speed.0 * boost * time.delta_secs();
    }
}

fn quit_on_esc(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
    _windows: Query<&Window, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
