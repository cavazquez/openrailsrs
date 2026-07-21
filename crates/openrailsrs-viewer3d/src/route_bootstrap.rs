//! Bootstrap async de ruta: ventana primero, parse en background (#55).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};

use bevy::prelude::*;

use crate::HudTitle;
use crate::launch::{RunCorridorPath, ViewerLaunchOpts, ViewerSceneryMode};
use crate::overhead_wire::RouteWireConfig;
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::RouteAssets;
use crate::terrain::{TerrainElevation, TerrainScene, TerrainTileStream};
use crate::track::TrackScene;
use crate::train::ReplayState;
use crate::tr_item_index::TrItemWorldIndex;
use crate::view_window::ViewWindow;
use crate::world::{RouteFocus, RouteWorldOffset, WorldScene, WorldTileStream};
use crate::{live::LiveDrive, view_radius_m};

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
    pub started: std::time::Instant,
}

#[derive(Resource)]
pub struct ViewerLoadingScreen {
    pub root: Entity,
    pub status: Entity,
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
            clear_color: ClearColorConfig::Custom(Color::srgb(0.08, 0.10, 0.14)),
            ..default()
        },
    ));
    commands.insert_resource(ViewerLoadingScreen { root, status });
}

pub fn poll_route_load(
    mut commands: Commands,
    pending: Option<ResMut<PendingRouteLoad>>,
    screen: Option<Res<ViewerLoadingScreen>>,
    mut texts: Query<&mut Text>,
    mut next: ResMut<NextState<ViewerAppState>>,
    loading_cams: Query<Entity, With<Camera2d>>,
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
            if std::env::var_os("OPENRAILSRS_PERF_DEBUG").is_some() {
                eprintln!("[PERF] time_to_ready_ms={elapsed_ms:.1}");
            }
            crate::viewer_log!(
                "openrailsrs-viewer3d: route ready in {elapsed_ms:.0} ms — inserting scenes"
            );
            insert_route_bundle(&mut commands, bundle);
            if let Some(screen) = screen {
                commands.entity(screen.root).despawn();
                commands.remove_resource::<ViewerLoadingScreen>();
            }
            for e in &loading_cams {
                commands.entity(e).despawn();
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
