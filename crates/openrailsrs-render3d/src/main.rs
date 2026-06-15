//! Render 3D nuevo, desde cero. Hito 1: mostrar UN tile de terreno de una ruta
//! MSTS/Open Rails (por defecto Chiltern), bien posicionado y con relieve real.
//!
//! Crecemos una capa a la vez (terreno → vía → objetos → tren), validando cada
//! una contra Open Rails antes de seguir. El viewer3d viejo queda como referencia.
//!
//! Uso:
//!   cargo run -p openrailsrs-render3d -- [--route DIR] [--tile-x N --tile-z N] [--radius R]
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
    /// 1=3×3=9 tiles, 2=5×5=25 tiles, etc.
    #[arg(long, default_value_t = 1)]
    radius: u32,
}

/// Información de un tile (geometría + offset en el espacio world).
pub struct TileEntry {
    pub geometry: TileGeometry,
    /// Desplazamiento del origen del tile en el espacio world (metros).
    pub world_offset: Vec3,
    /// Vía del tile.
    pub track: track::TrackRibbon,
    /// Marcadores de objetos del tile.
    pub objects: Vec<objects::ObjectMarker>,
}

/// Lista de tiles a renderizar.
#[derive(Resource)]
pub struct TilesToRender(pub Vec<TileEntry>);

/// Carpeta de la ruta (para resolver texturas TERRTEX y SHAPES locales).
#[derive(Resource)]
pub struct RouteDir(pub PathBuf);

/// Carpeta base de MSTS/OR (para resolver GLOBAL/SHAPES y GLOBAL/TEXTURES).
#[derive(Resource)]
pub struct MstsRootDir(pub PathBuf);

/// Velocidad base de la cámara (m/s).
#[derive(Resource)]
struct FlySpeed(f32);

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

    for &(tx, tz) in &tile_coords {
        let world_offset = Vec3::new(
            (tx - cx) as f32 * tile_size_m,
            0.0,
            (tz - cz) as f32 * tile_size_m,
        );
        match terrain::load_tile_geometry(&cli.route, tx, tz) {
            Ok(geom) => {
                let base_y = geom.height.base_y();
                let ribbon = graph
                    .as_ref()
                    .map(|g| track::build_track_ribbon(g, tx, tz, &geom.height))
                    .unwrap_or_default();
                let objs = objects::load_objects(&cli.route, tx, tz, base_y);
                entries.push(TileEntry {
                    geometry: geom,
                    world_offset,
                    track: ribbon,
                    objects: objs,
                });
            }
            Err(_) => {
                skipped += 1;
            }
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

    // Ventana centrada en el tile principal.
    let central_geom = &entries[0].geometry;
    let side = central_geom.side_m;

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!(
                    "openrailsrs-render3d — tile ({cx}, {cz}) r={r} [{} tiles]",
                    entries.len()
                ),
                ..default()
            }),
            ..default()
        }))
        .init_state::<AppState>()
        .insert_resource(ClearColor(Color::srgb(0.53, 0.81, 0.92))) // Sky blue
        .insert_resource(FlySpeed(120.0))
        .insert_resource(RouteDir(cli.route.clone()))
        .insert_resource(MstsRootDir(msts_root))
        .insert_resource(TilesToRender(entries))
        .insert_resource(SceneExtent { side_m: side })
        .add_systems(
            Startup,
            (setup_loading_screen, begin_load_stage, spawn_sun).chain(),
        )
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

/// Lado del tile central en metros — para posicionar la cámara inicial.
#[derive(Resource)]
pub struct SceneExtent {
    pub side_m: f32,
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.98, 0.9),
            illuminance: 10000.0, // Lux
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            std::f32::consts::PI / 4.0,  // Yaw
            -std::f32::consts::PI / 4.0, // Pitch (downwards)
            0.0,
        )),
    ));
    commands.spawn(AmbientLight {
        color: Color::srgb(0.6, 0.7, 0.9),
        brightness: 200.0,
        affects_lightmapped_meshes: false,
    });
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
