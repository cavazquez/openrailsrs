//! Bootstrap async de ruta: ventana primero, parse en background (#55).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::HudTitle;
use crate::launch::{RunCorridorPath, ViewerLaunchOpts, ViewerSceneryMode};
use crate::overhead_wire::RouteWireConfig;
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::RouteAssets;
use crate::terrain::{TerrainElevation, TerrainScene, TerrainTileStream};
use crate::tr_item_index::TrItemWorldIndex;
use crate::track::TrackScene;
use crate::train::ReplayState;
use crate::view_window::ViewWindow;
use crate::world::{RouteFocus, RouteWorldOffset, WorldScene, WorldTileStream};
use crate::{live::LiveDrive, view_radius_m};

fn perf_debug_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_PERF_DEBUG").is_some()
}

/// Estados de arranque del viewer (#55).
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ViewerAppState {
    #[default]
    Loading,
    Playing,
}

/// Resultado de la carga CPU pre-escena (hilo background).
pub struct RouteLoadBundle {
    pub title: String,
    pub route_dir: std::path::PathBuf,
    pub scene: TrackScene,
    pub world: WorldScene,
    pub terrain: TerrainScene,
    pub elevation: TerrainElevation,
    pub replay: ReplayState,
    pub consist: TrainConsistScene,
    pub live: Option<LiveDrive>,
    pub scenery_mode: ViewerSceneryMode,
    pub run_corridor_path: RunCorridorPath,
    pub route_focus: RouteFocus,
    pub route_offset: RouteWorldOffset,
    pub assets: RouteAssets,
    pub launch_opts: ViewerLaunchOpts,
}

#[derive(Resource)]
pub struct PendingRouteLoad {
    /// `Mutex` porque `mpsc::Receiver` no es `Sync` y Bevy exige `Resource: Sync`.
    pub rx: Mutex<Receiver<Result<RouteLoadBundle, String>>>,
    pub started: Instant,
}

/// Process boot clock for startup presentation metrics (#82).
///
/// Insert before [`App::run`]. [`log_time_to_first_presented_frame`] reports once
/// when the primary window has a non-zero size (first Update after Winit/GPU init).
#[derive(Resource, Debug)]
pub struct ViewerBootClock {
    pub started: Instant,
    pub first_presented_logged: bool,
}

impl ViewerBootClock {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            first_presented_logged: false,
        }
    }
}

/// True when the primary window is sized (proxy for “window actually presented”).
pub fn primary_window_is_presented(window: &Window) -> bool {
    window.physical_width() > 0 && window.physical_height() > 0
}

/// Log `[PERF] time_to_first_presented_ms` once (#82).
///
/// Includes Bevy/Winit/GPU plugin init through the first Update with a sized
/// primary window. Does **not** wait for route parse (`time_to_ready_ms`) or
/// scenery spawn. Historical “&lt;500 ms to window” claims are not enforced.
pub fn log_time_to_first_presented_frame(
    mut clock: ResMut<ViewerBootClock>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if clock.first_presented_logged {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    if !primary_window_is_presented(window) {
        return;
    }
    clock.first_presented_logged = true;
    if !perf_debug_enabled() {
        return;
    }
    let ms = clock.started.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "[PERF] time_to_first_presented_ms={ms:.1} (Bevy+Winit+GPU init → first sized primary window; excludes route parse #55)"
    );
}

#[derive(Resource)]
pub struct ViewerLoadingScreen {
    pub root: Entity,
    pub status: Entity,
    pub scenery_spawn_started: bool,
}

pub fn setup_viewer_loading_ui(mut commands: Commands) {
    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.10, 0.14)),
        ))
        .id();
    let title = commands
        .spawn((
            Text::new("openrailsrs-viewer3d"),
            TextFont {
                font_size: FontSize::Px(28.0),
                ..default()
            },
            TextColor(Color::srgb(0.92, 0.94, 0.98)),
        ))
        .id();
    let status = commands
        .spawn((
            Text::new("Cargando ruta…"),
            TextFont {
                font_size: FontSize::Px(17.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.80, 0.88)),
        ))
        .id();
    commands.entity(root).add_children(&[title, status]);
    commands.spawn((
        Camera2d,
        Camera {
            order: 10,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.08, 0.10, 0.14)),
            ..default()
        },
    ));
    commands.insert_resource(ViewerLoadingScreen {
        root,
        status,
        scenery_spawn_started: false,
    });
    crate::viewer_log!("openrailsrs-viewer3d: loading screen active — waiting for world progressive spawn");
}

pub fn poll_route_load(
    mut commands: Commands,
    pending: Option<ResMut<PendingRouteLoad>>,
    screen: Option<Res<ViewerLoadingScreen>>,
    mut texts: Query<&mut Text>,
    mut next: ResMut<NextState<ViewerAppState>>,
) {
    let Some(pending) = pending else {
        return;
    };
    let recv = match pending.rx.lock() {
        Ok(guard) => guard.try_recv(),
        Err(_) => {
            eprintln!("error: route load channel poisoned");
            commands.remove_resource::<PendingRouteLoad>();
            return;
        }
    };
    match recv {
        Ok(Ok(bundle)) => {
            let elapsed_ms = pending.started.elapsed().as_secs_f64() * 1000.0;
            if perf_debug_enabled() {
                eprintln!(
                    "[PERF] time_to_ready_ms={elapsed_ms:.1} (boot → route bundle ready; may be after first presented frame)"
                );
            }
            crate::viewer_log!(
                "openrailsrs-viewer3d: route ready in {elapsed_ms:.0} ms — inserting scenes"
            );
            insert_route_bundle(&mut commands, bundle);
            if let Some(screen) = screen.as_ref() {
                if let Ok(mut t) = texts.get_mut(screen.status) {
                    *t = Text::new("Generando escenografía y mallas 3D...".to_string());
                }
            }
            commands.remove_resource::<PendingRouteLoad>();
            next.set(ViewerAppState::Playing);
        }
        Ok(Err(err)) => {
            if let Some(screen) = screen.as_ref() {
                if let Ok(mut t) = texts.get_mut(screen.status) {
                    *t = Text::new(format!("Error: {err}"));
                }
            }
            eprintln!("error: {err}");
            commands.remove_resource::<PendingRouteLoad>();
        }
        Err(TryRecvError::Empty) => {
            if let Some(screen) = screen.as_ref() {
                if let Ok(mut t) = texts.get_mut(screen.status) {
                    let secs = pending.started.elapsed().as_secs();
                    *t = Text::new(format!("Cargando ruta… ({secs}s)"));
                }
            }
        }
        Err(TryRecvError::Disconnected) => {
            eprintln!("error: route load thread disconnected");
            commands.remove_resource::<PendingRouteLoad>();
        }
    }
}

pub fn update_loading_screen_progress(
    mut commands: Commands,
    screen: Option<ResMut<ViewerLoadingScreen>>,
    progress: Option<Res<crate::world::WorldSpawnProgress>>,
    mut texts: Query<&mut Text>,
    loading_cams: Query<Entity, With<Camera2d>>,
) {
    let Some(mut screen) = screen else {
        return;
    };

    if let Some(progress) = progress.as_ref() {
        screen.scenery_spawn_started = true;
        if let Ok(mut t) = texts.get_mut(screen.status) {
            *t = Text::new(progress.status_text());
        }
    } else if screen.scenery_spawn_started {
        // Progressive world spawn finished!
        crate::viewer_log!("openrailsrs-viewer3d: progressive spawn complete — entering simulation");
        commands.entity(screen.root).despawn();
        commands.remove_resource::<ViewerLoadingScreen>();
        for e in &loading_cams {
            commands.entity(e).despawn();
        }
    }
}

fn insert_route_bundle(commands: &mut Commands, bundle: RouteLoadBundle) {
    let RouteLoadBundle {
        title,
        route_dir,
        scene,
        world,
        terrain,
        elevation,
        replay,
        consist,
        live,
        scenery_mode,
        run_corridor_path,
        route_focus,
        route_offset,
        assets,
        launch_opts,
    } = bundle;

    let world_stream = if scenery_mode.is_tile_lab() {
        WorldTileStream::default()
    } else {
        WorldTileStream::new(&route_dir, &world, view_radius_m())
    };
    let terrain_stream =
        TerrainTileStream::new(&route_dir, &terrain, &route_focus, view_radius_m());
    let tr_index = TrItemWorldIndex::rebuild_from_scene(&world);
    let wire = RouteWireConfig::load_from_route_dir(&route_dir);

    commands.insert_resource(launch_opts);
    commands.insert_resource(scenery_mode);
    commands.insert_resource(run_corridor_path);
    commands.insert_resource(scene);
    commands.insert_resource(world_stream);
    commands.insert_resource(ViewWindow::from_route_focus(&route_focus));
    commands.insert_resource(terrain_stream);
    commands.insert_resource(assets);
    commands.insert_resource(wire);
    commands.insert_resource(route_focus);
    commands.insert_resource(route_offset);
    commands.insert_resource(world);
    commands.insert_resource(tr_index);
    commands.insert_resource(terrain);
    commands.insert_resource(elevation);
    commands.insert_resource(replay);
    commands.insert_resource(consist);
    commands.insert_resource(HudTitle(title));
    if let Some(live) = live {
        commands.insert_resource(live);
    }
}

/// `true` cuando la app ya está en Playing (spawn/stream activos).
pub fn viewer_playing(state: Res<State<ViewerAppState>>) -> bool {
    *state.get() == ViewerAppState::Playing
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::state::app::StatesPlugin;
    use bevy::state::condition::in_state;

    #[test]
    fn primary_window_presented_requires_nonzero_size() {
        let mut window = Window::default();
        window.resolution.set_physical_resolution(0, 0);
        assert!(!primary_window_is_presented(&window));
        window.resolution.set_physical_resolution(1280, 720);
        assert!(primary_window_is_presented(&window));
    }

    #[test]
    fn boot_clock_starts_unlogged() {
        let clock = ViewerBootClock::new(Instant::now());
        assert!(!clock.first_presented_logged);
    }

    #[derive(Resource, Default)]
    struct PlayingTicks(u32);

    fn tick_playing(mut c: ResMut<PlayingTicks>) {
        c.0 += 1;
    }

    #[test]
    fn update_systems_gated_to_playing_do_not_run_while_loading() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .init_state::<ViewerAppState>()
            .init_resource::<PlayingTicks>()
            .add_systems(
                Update,
                tick_playing.run_if(in_state(ViewerAppState::Playing)),
            );
        assert_eq!(
            *app.world().resource::<State<ViewerAppState>>().get(),
            ViewerAppState::Loading
        );
        app.update();
        app.update();
        assert_eq!(app.world().resource::<PlayingTicks>().0, 0);

        app.world_mut()
            .resource_mut::<NextState<ViewerAppState>>()
            .set(ViewerAppState::Playing);
        app.update();
        app.update();
        assert!(
            app.world().resource::<PlayingTicks>().0 >= 1,
            "Playing systems should run after state transition"
        );
    }
}
