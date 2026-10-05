//! Player menus and railway tools built with retained Bevy UI.
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use openrailsrs_sim::{CarOperation, LiveDriveSession, ServicePhase};
use openrailsrs_track::{NodeKind, SignalAspect};

use crate::camera::{CameraFollowMode, DriverLookOffset, OrbitState};
use crate::live::LiveDrive;
use crate::player_launch::{
    ActivePlayerContent, PlayerLaunchMenu, PlayerLaunchQueue, PlayerWeather, cycle,
};
use crate::player_settings::{PlayerAction, PlayerSettings, player_data_dir};
use crate::route_bootstrap::ViewerAppState;
use crate::saved_game::{PendingSavedCamera, SavedCamera, SavedGame, slot_path};

const BG: Color = Color::srgb(0.045, 0.065, 0.095);
const FIELD: Color = Color::srgb(0.10, 0.15, 0.21);
const TEXT: Color = Color::srgb(0.93, 0.95, 0.98);
const MUTED: Color = Color::srgb(0.66, 0.74, 0.83);
const ACCENT: Color = Color::srgb(0.32, 0.78, 0.94);
const GOOD: Color = Color::srgb(0.32, 0.9, 0.55);
const BAD: Color = Color::srgb(1.0, 0.37, 0.32);
const CAUTION: Color = Color::srgb(1.0, 0.77, 0.23);
const ADVANCED_PAGES: [&str; 10] = [
    "General",
    "Formación",
    "Locomotora",
    "Potencia distribuida",
    "Alimentación",
    "Frenos",
    "Fuerzas",
    "Despachador",
    "Clima",
    "Diagnóstico",
];

pub struct PlayerUiPlugin;
impl Plugin for PlayerUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerUiState>()
            .init_resource::<PlayerSettings>()
            .init_resource::<PlayerLaunchMenu>()
            .init_resource::<PlayerLaunchQueue>()
            .init_resource::<ActivePlayerContent>()
            .init_resource::<UiPointerCapture>()
            .add_systems(Startup, spawn_ui_camera)
            .add_systems(Update, target_screen_hud_camera)
            .add_systems(OnEnter(ViewerAppState::Menu), enter_menu)
            .add_systems(
                OnEnter(ViewerAppState::Playing),
                (spawn_toolbar, apply_settings)
                    .chain()
                    .after(crate::camera::spawn_camera)
                    .after(crate::scene::spawn_ground_and_lights),
            )
            .add_systems(OnEnter(ViewerAppState::Playing), apply_weather)
            .add_systems(
                Update,
                apply_weather
                    .run_if(resource_changed::<ActivePlayerContent>)
                    .run_if(in_state(ViewerAppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    player_keys,
                    handle_buttons,
                    capture_ui_pointer,
                    build_panel,
                    update_panel_text,
                    scroll_panel,
                    update_monitor,
                )
                    .chain()
                    .before(crate::live::live_driver_input)
                    .before(crate::camera::toggle_mode_system)
                    .before(crate::camera::cycle_follow_mode)
                    .before(crate::camera::orbit_camera_system)
                    .before(crate::camera::fly_camera_system)
                    .before(crate::cab_mouse::cab_mouse_controls),
            )
            .add_systems(
                Update,
                apply_settings.run_if(resource_changed::<PlayerSettings>),
            )
            .add_systems(
                Update,
                crate::saved_game::restore_camera
                    .after(crate::live::enable_live_defaults)
                    .before(crate::camera::follow_train_camera)
                    .run_if(in_state(ViewerAppState::Playing)),
            );
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayerPanel {
    #[default]
    None,
    Menu,
    Pause,
    Notebook,
    Formation,
    Map,
    Advanced,
    Settings,
    Help,
}
#[derive(Clone, Debug, PartialEq)]
enum MapSelection {
    Signal(String),
    Switch(String),
}
#[derive(Resource)]
pub struct PlayerUiState {
    pub panel: PlayerPanel,
    pub monitor: bool,
    pub awaiting_key: Option<PlayerAction>,
    notebook_tab: usize,
    advanced_page: usize,
    selected_car: usize,
    map_selection: Option<MapSelection>,
    map_center: Vec2,
    map_scale: f32,
    map_initialized: bool,
    pause_before: bool,
    return_to_menu: bool,
    pub(crate) notice: String,
    rebuild: bool,
    root: Option<Entity>,
    camera: Option<Entity>,
}
impl Default for PlayerUiState {
    fn default() -> Self {
        Self {
            panel: PlayerPanel::None,
            monitor: false,
            awaiting_key: None,
            notebook_tab: 1,
            advanced_page: 0,
            selected_car: 0,
            map_selection: None,
            map_center: Vec2::ZERO,
            map_scale: 0.05,
            map_initialized: false,
            pause_before: false,
            return_to_menu: false,
            notice: String::new(),
            rebuild: true,
            root: None,
            camera: None,
        }
    }
}
#[derive(Resource, Default)]
pub struct UiPointerCapture(pub bool);
pub fn world_input_available(ui: Option<Res<PlayerUiState>>) -> bool {
    ui.is_none_or(|u| u.panel == PlayerPanel::None)
}
pub fn camera_input_available(
    ui: Option<Res<PlayerUiState>>,
    pointer: Option<Res<UiPointerCapture>>,
    cab: Option<Res<crate::cab_mouse::CabMouseState>>,
) -> bool {
    world_input_available(ui)
        && pointer.is_none_or(|p| !p.0)
        && cab.is_none_or(|c| !c.pointer_captured)
}
#[derive(Component)]
struct PlayerUiCamera;
/// Screen information renders after cab/weather postprocessing.
#[derive(Component)]
pub(crate) struct ScreenHud;
#[derive(Component)]
struct PlayerToolbar;
#[derive(Component)]
struct PlayerScroll;
#[derive(Component, Clone, Copy)]
enum DynamicText {
    Status,
    Notebook,
    Advanced,
    Car,
    Map,
    Help,
}
#[derive(Component, Clone, Copy)]
struct TrackMonitorRoot;
#[derive(Component, Clone, Copy)]
struct TrackMonitorBody;
#[derive(Component, Clone, Debug)]
enum UiCommand {
    Open(PlayerPanel),
    Close,
    Start,
    Exit,
    ReturnMenu,
    Save(usize),
    Load(usize),
    Resume(usize),
    Cycle(MenuField, i32),
    NoteTab(usize),
    Advanced(usize),
    SelectCar(usize),
    Car(CarOperation),
    SecureTail,
    Uncouple,
    Recouple,
    MapPick(MapSelection),
    DispatchSignal(Option<SignalAspect>),
    DispatchSwitch,
    Zoom(f32),
    Pan(Vec2),
    FitMap,
    Setting(SettingField, f32),
    Rebind(PlayerAction),
    DefaultKeys,
    SaveSettings,
}
#[derive(Clone, Copy, Debug)]
enum MenuField {
    Route,
    Service,
    Consist,
    Path,
    Time,
    Season,
    Weather,
}
#[derive(Clone, Copy, Debug)]
enum SettingField {
    QuickStations,
    SeatHeight,
    SeatBack,
    WipeScale,
    CabReset,
    Distance,
    Fov,
    Scale,
    Shadows,
    Fog,
    FogQuality,
    WeatherExecution,
    Renderer,
    Audio,
    Volume,
    Units,
}

fn label(
    p: &mut ChildSpawnerCommands<'_>,
    value: impl Into<String>,
    size: f32,
    color: Color,
) -> Entity {
    p.spawn((
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    ))
    .id()
}
fn button(
    p: &mut ChildSpawnerCommands<'_>,
    value: impl Into<String>,
    command: UiCommand,
) -> Entity {
    p.spawn((
        Button,
        Node {
            padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
            min_height: Val::Px(32.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(FIELD),
        command,
    ))
    .with_children(|p| {
        label(p, value, 13.0, TEXT);
    })
    .id()
}
fn row(p: &mut ChildSpawnerCommands<'_>, f: impl FnOnce(&mut ChildSpawnerCommands<'_>)) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(8.0),
        row_gap: Val::Px(6.0),
        align_items: AlignItems::Center,
        flex_wrap: FlexWrap::Wrap,
        ..default()
    })
    .with_children(f);
}
fn dynamic(p: &mut ChildSpawnerCommands<'_>, kind: DynamicText, size: f32) -> Entity {
    p.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(TEXT),
        kind,
    ))
    .id()
}
fn spawn_ui_camera(mut commands: Commands, mut ui: ResMut<PlayerUiState>) {
    ui.camera = Some(
        commands
            .spawn((
                Camera2d,
                Msaa::Off,
                Camera {
                    order: 50,
                    clear_color: ClearColorConfig::None,
                    ..default()
                },
                PlayerUiCamera,
                Name::new("player-ui-camera"),
            ))
            .id(),
    );
}
fn target_screen_hud_camera(
    mut commands: Commands,
    ui: Res<PlayerUiState>,
    roots: Query<Entity, (With<ScreenHud>, Without<UiTargetCamera>)>,
) {
    let Some(camera) = ui.camera else {
        return;
    };
    for entity in &roots {
        commands.entity(entity).insert(UiTargetCamera(camera));
    }
}
fn enter_menu(mut ui: ResMut<PlayerUiState>) {
    ui.panel = PlayerPanel::Menu;
    ui.rebuild = true;
}
fn spawn_toolbar(
    mut commands: Commands,
    mut ui: ResMut<PlayerUiState>,
    live: Option<Res<LiveDrive>>,
    settings: Res<PlayerSettings>,
) {
    if live.is_none() {
        return;
    }
    ui.panel = if live.is_some_and(|l| l.paused) {
        PlayerPanel::Pause
    } else {
        PlayerPanel::None
    };
    ui.rebuild = true;
    let Some(camera) = ui.camera else { return };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(8.0),
                column_gap: Val::Px(6.0),
                ..default()
            },
            UiTargetCamera(camera),
            ZIndex(800),
            PlayerToolbar,
        ))
        .with_children(|p| {
            for (panel, action) in [
                (PlayerPanel::Pause, PlayerAction::Pause),
                (PlayerPanel::Notebook, PlayerAction::Notebook),
                (PlayerPanel::Formation, PlayerAction::Formation),
                (PlayerPanel::Map, PlayerAction::Map),
                (PlayerPanel::Settings, PlayerAction::Settings),
            ] {
                button(
                    p,
                    format!(
                        "{} · {}",
                        settings.key_label(action),
                        match panel {
                            PlayerPanel::Pause => "Pausa",
                            PlayerPanel::Notebook => "Servicio",
                            PlayerPanel::Formation => "Formación",
                            PlayerPanel::Map => "Mapa",
                            _ => "Ajustes",
                        }
                    ),
                    UiCommand::Open(panel),
                );
            }
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(12.0),
                bottom: Val::Px(58.0),
                width: Val::Px(280.0),
                height: Val::Px(300.0),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.05, 0.08, 0.92)),
            UiTargetCamera(camera),
            ZIndex(500),
            TrackMonitorRoot,
            Visibility::Hidden,
        ))
        .with_children(|p| {
            label(p, "MONITOR DE VÍA · próximos 5 km", 12.0, MUTED);
            p.spawn((
                Node {
                    height: Val::Px(264.0),
                    width: Val::Percent(100.0),
                    ..default()
                },
                TrackMonitorBody,
            ));
        });
}
fn open_panel(ui: &mut PlayerUiState, live: &mut Option<ResMut<LiveDrive>>, panel: PlayerPanel) {
    if ui.panel == PlayerPanel::Menu {
        ui.return_to_menu = true;
    }
    if ui.panel == panel {
        close_panel(ui, live);
        return;
    }
    if ui.panel == PlayerPanel::None {
        ui.pause_before = live.as_ref().is_some_and(|l| l.paused);
    }
    ui.panel = panel;
    ui.rebuild = true;
    ui.awaiting_key = None;
    ui.notice.clear();
    if let Some(l) = live {
        l.paused = true;
    }
}
fn close_panel(ui: &mut PlayerUiState, live: &mut Option<ResMut<LiveDrive>>) {
    let was_pause = ui.panel == PlayerPanel::Pause;
    ui.panel = if ui.return_to_menu {
        ui.return_to_menu = false;
        PlayerPanel::Menu
    } else {
        PlayerPanel::None
    };
    ui.awaiting_key = None;
    ui.rebuild = true;
    if let Some(l) = live {
        l.paused = if was_pause { false } else { ui.pause_before };
    }
}
fn player_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui: ResMut<PlayerUiState>,
    mut live: Option<ResMut<LiveDrive>>,
    mut settings: ResMut<PlayerSettings>,
    state: Res<State<ViewerAppState>>,
    teleport: Res<crate::teleport::TeleportDialog>,
) {
    if teleport.open {
        return;
    }
    if let Some(action) = ui.awaiting_key {
        if keys.just_pressed(KeyCode::Escape) {
            ui.awaiting_key = None;
            ui.notice = "Cambio de tecla cancelado".into();
            ui.rebuild = true;
            return;
        }
        if let Some(key) = keys.get_just_pressed().copied().next() {
            match settings.rebind(action, key) {
                Ok(()) => {
                    ui.awaiting_key = None;
                    ui.notice =
                        "Control actualizado; Guardar ajustes lo conserva para la próxima partida"
                            .into();
                }
                Err(e) => ui.notice = e,
            }
            ui.rebuild = true;
        }
        return;
    }
    if *state.get() == ViewerAppState::Menu
        && keys.just_pressed(KeyCode::Escape)
        && ui.panel != PlayerPanel::Menu
    {
        close_panel(&mut ui, &mut live);
        return;
    }
    if *state.get() != ViewerAppState::Playing || live.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if ui.panel != PlayerPanel::None {
            close_panel(&mut ui, &mut live)
        } else {
            open_panel(&mut ui, &mut live, PlayerPanel::Pause)
        }
        return;
    }
    for (action, panel) in [
        (PlayerAction::Pause, PlayerPanel::Pause),
        (PlayerAction::Notebook, PlayerPanel::Notebook),
        (PlayerAction::AdvancedHud, PlayerPanel::Advanced),
        (PlayerAction::Formation, PlayerPanel::Formation),
        (PlayerAction::Map, PlayerPanel::Map),
        (PlayerAction::Settings, PlayerPanel::Settings),
        (PlayerAction::Help, PlayerPanel::Help),
    ] {
        if settings.just_pressed(&keys, action)
            || (action == PlayerAction::Pause && keys.just_pressed(KeyCode::Pause))
        {
            open_panel(&mut ui, &mut live, panel);
            return;
        }
    }
    if settings.just_pressed(&keys, PlayerAction::TrackMonitor) {
        ui.monitor = !ui.monitor;
    }
}

#[derive(SystemParam)]
struct CameraForSave<'w, 's> {
    follow: Res<'w, CameraFollowMode>,
    look: Res<'w, DriverLookOffset>,
    origin: Res<'w, crate::floating_origin::FloatingOrigin>,
    mode: Res<'w, crate::camera::CameraMode>,
    driver: Res<'w, crate::camera::LiveDriverCab>,
    passenger: Res<'w, crate::camera::PassengerCamState>,
    cab2d: Res<'w, crate::cab_cvf_overlay::CabCvfOverlayState>,
    cameras: Query<
        'w,
        's,
        (
            &'static OrbitState,
            &'static crate::camera::FlyState,
            &'static Transform,
        ),
        With<Camera3d>,
    >,
}
fn handle_buttons(
    mut commands: Commands,
    buttons: Query<(&Interaction, &UiCommand), Changed<Interaction>>,
    mut ui: ResMut<PlayerUiState>,
    mut live: Option<ResMut<LiveDrive>>,
    mut menu: ResMut<PlayerLaunchMenu>,
    mut settings: ResMut<PlayerSettings>,
    mut launch: ResMut<PlayerLaunchQueue>,
    mut content: ResMut<ActivePlayerContent>,
    camera: CameraForSave,
    mut exit: MessageWriter<AppExit>,
    mouse: Res<ButtonInput<MouseButton>>,
    cab: Res<crate::cab_cvf::CabCvfState>,
) {
    for (interaction, command) in &buttons {
        if *interaction != Interaction::Pressed || !mouse.just_pressed(MouseButton::Left) {
            continue;
        }
        let result:Result<(),String>=match command {
            UiCommand::Open(panel)=>{open_panel(&mut ui,&mut live,*panel);Ok(())},
            UiCommand::Close=>{close_panel(&mut ui,&mut live);Ok(())},
            UiCommand::Exit => { exit.write(AppExit::Success); Ok(()) },
            UiCommand::ReturnMenu => {
                std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| {
                    let mut child = std::process::Command::new(exe);
                    child.arg("--menu").arg("--wait-parent").arg(std::process::id().to_string());
                    if let Some(root) = &content.route_root { child.arg("--route-root").arg(root); }
                    child.spawn().map_err(|e| e.to_string())?;
                    exit.write(AppExit::Success);
                    Ok(())
                })
            },
            UiCommand::Start=>menu.prepare().map(|request|{launch.0=Some(request);ui.panel=PlayerPanel::None;ui.notice="Cargando la partida…".into();}),
            UiCommand::Resume(slot)=>SavedGame::prepare_resume(&slot_path(*slot)).map(|request|{launch.0=Some(request);ui.panel=PlayerPanel::None;}),
            UiCommand::Save(slot)=>if let Some(l)=live.as_ref(){
                camera.cameras.single().map_err(|_|"Cámara no disponible".into()).and_then(|(orbit, fly, transform)| {
                    let mut saved = SavedCamera::capture(*camera.follow, orbit, &camera.look, &camera.origin);
                    saved.pose = Some(crate::saved_game::SavedCameraPose {
                        fly: *camera.mode == crate::camera::CameraMode::Fly,
                        position: (transform.translation + camera.origin.shift).to_array(),
                        rotation: transform.rotation.to_array(), fly_yaw: fly.yaw, fly_pitch: fly.pitch,
                        driver_eyepoint: camera.driver.eyepoint_index, cab2d_view: camera.cab2d.view_index,
                        passenger_car_slot: camera.passenger.car_slot, passenger_view: camera.passenger.view_index,
                        passenger_car: camera.passenger.consist_car, passenger_head: camera.passenger.head_msts.to_array(),
                        passenger_look: [camera.passenger.look_pitch, camera.passenger.look_yaw, camera.passenger.pitch_limit, camera.passenger.yaw_limit],
                    });
                    SavedGame::save(l, &content, saved, &slot_path(*slot))
                })
                    .map(|()|{ui.notice=format!("Partida guardada en la ranura {} · {:.0} m · {:.1} km/h",slot+1,l.session.state.odometer_m,l.session.velocity_mps()*3.6);})
            }else{Err("No hay una partida activa".into())},
            UiCommand::Load(slot)=>if let Some(l)=live.as_mut(){
                SavedGame::read(&slot_path(*slot)).and_then(|saved|{let weather=saved.weather;let camera=saved.restore(l)?;content.weather=weather;commands.insert_resource(PendingSavedCamera(camera));Ok(())})
                    .map(|()|{ui.notice="Partida restaurada y pausada; Continuar reanuda la conducción".into();})
            }else{Err("No hay una partida activa".into())},
            UiCommand::Cycle(field,delta)=>{match field {
                MenuField::Route=>menu.cycle_route(*delta),MenuField::Service=>menu.cycle_service(*delta),
                MenuField::Consist=>menu.consist=cycle(menu.consist,menu.consists.len(),*delta),
                MenuField::Path=>menu.path=cycle(menu.path,menu.paths.len()+1,*delta),
                MenuField::Time=>menu.start_time_s=(menu.start_time_s+f64::from(*delta)*900.0).rem_euclid(86400.0),
                MenuField::Season=>menu.season=cycle(menu.season,4,*delta),
                MenuField::Weather=>menu.weather=PlayerWeather::ALL[cycle(PlayerWeather::ALL.iter().position(|w|*w==menu.weather).unwrap_or(0),PlayerWeather::ALL.len(),*delta)],
            }Ok(())},
            UiCommand::NoteTab(tab)=>{ui.notebook_tab= *tab;Ok(())},UiCommand::Advanced(page)=>{ui.advanced_page= *page;Ok(())},
            UiCommand::SelectCar(car)=>{ui.selected_car= *car;Ok(())},
            UiCommand::Car(action)=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.operate_car(ui.selected_car,*action)).map(|()|{ui.notice="Operación aplicada a la formación".into();}),
            UiCommand::SecureTail=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|{
                for i in ui.selected_car+1..l.session.formation.coupled_count {
                    if !l.session.formation.cars[i].handbrake {l.session.operate_car(i,CarOperation::Handbrake)?;}
                }ui.notice="Frenos de mano aplicados a la sección posterior".into();Ok(())}),
            UiCommand::Uncouple=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.uncouple_after(ui.selected_car)).map(|()|{ui.notice="Sección posterior desacoplada y estacionada".into();}),
            UiCommand::Recouple=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.recouple()).map(|()|{ui.notice="Enganche acoplado; reconectá la manguera, abrí las llaves y soltá los frenos de mano".into();}),
            UiCommand::MapPick(selection)=>{ui.map_selection=Some(selection.clone());Ok(())},
            UiCommand::DispatchSignal(aspect)=>if let Some(MapSelection::Signal(id))=ui.map_selection.clone(){live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.dispatch_signal(&id,*aspect)).map(|()|{ui.notice="Orden de señal aplicada".into();})}else{Err("Seleccioná una señal en el mapa".into())},
            UiCommand::DispatchSwitch=>if let Some(MapSelection::Switch(id))=ui.map_selection.clone(){live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.dispatch_switch(&id)).map(|()|{ui.notice="Cambio e itinerario actualizados".into();})}else{Err("Seleccioná un cambio en el mapa".into())},
            UiCommand::Zoom(factor)=>{ui.map_scale=(ui.map_scale*factor).clamp(0.002,2.0);Ok(())},
            UiCommand::Pan(delta)=>{let scale=ui.map_scale;ui.map_center+= *delta/scale;Ok(())},
            UiCommand::FitMap=>{ui.map_initialized=false;Ok(())},
            UiCommand::Setting(field,step)=>{match field {
                SettingField::SeatHeight | SettingField::SeatBack | SettingField::WipeScale | SettingField::CabReset => {
                    if let Some(path) = &cab.cvf_path {
                        let profile = settings.cab_profiles.entry(path.to_string_lossy().into_owned()).or_default();
                        match field {
                            SettingField::SeatHeight => profile.seat_height_m=(profile.seat_height_m+step).clamp(-0.4,0.4),
                            SettingField::SeatBack => profile.seat_back_m=(profile.seat_back_m+step).clamp(-0.4,0.4),
                            SettingField::WipeScale => profile.wipe_scale=(profile.wipe_scale+step).clamp(0.6,1.4),
                            _ => *profile=default(),
                        }
                    }
                },
                SettingField::QuickStations=>settings.quick_station_practice= !settings.quick_station_practice,
                SettingField::Distance=>settings.view_distance_m=(settings.view_distance_m+step).clamp(500.0,4000.0),
                SettingField::Fov=>settings.cab_fov_deg=(settings.cab_fov_deg+step).clamp(35.0,90.0),
                SettingField::Scale=>settings.ui_scale=(settings.ui_scale+step).clamp(0.8,1.5),
                SettingField::Shadows=>settings.shadows= !settings.shadows,SettingField::Fog=>settings.fog= !settings.fog,
                SettingField::Units=>settings.mph= !settings.mph,
                SettingField::FogQuality=>settings.fog_quality=settings.fog_quality.next(),
                SettingField::WeatherExecution=>settings.weather_execution=settings.weather_execution.next(),
                SettingField::Renderer=>settings.renderer=settings.renderer.next(),
                SettingField::Audio=>settings.audio_enabled= !settings.audio_enabled,
                SettingField::Volume=>settings.audio_volume=(settings.audio_volume+step).clamp(0.0,1.0),
            }ui.notice="Vista previa aplicada; pulsá Guardar ajustes para conservarla".into();Ok(())},
            UiCommand::Rebind(action)=>{ui.awaiting_key=Some(*action);ui.notice=format!("Pulsá la nueva tecla para {} · Esc cancela",action.label());Ok(())},
            UiCommand::DefaultKeys=>{settings.keys=PlayerSettings::default().keys;ui.notice="Controles predeterminados restaurados".into();Ok(())},
            UiCommand::SaveSettings=>settings.save(&player_data_dir().join("settings.json")).map(|()|{ui.notice="Ajustes guardados".into();}),
        };
        if let Err(error) = result {
            ui.notice = error;
        }
        ui.rebuild = true;
    }
}
fn capture_ui_pointer(
    buttons: Query<&Interaction, With<Button>>,
    mut pointer: ResMut<UiPointerCapture>,
    mut colors: Query<(&Interaction, &UiCommand, &mut BackgroundColor)>,
) {
    pointer.0 = buttons.iter().any(|i| *i != Interaction::None);
    for (i, command, mut c) in &mut colors {
        if matches!(command, UiCommand::MapPick(_)) {
            continue;
        }
        c.0 = match i {
            Interaction::Pressed => ACCENT,
            Interaction::Hovered => Color::srgb(0.18, 0.30, 0.40),
            Interaction::None => FIELD,
        };
    }
}

fn build_panel(
    mut commands: Commands,
    mut ui: ResMut<PlayerUiState>,
    menu: Res<PlayerLaunchMenu>,
    settings: Res<PlayerSettings>,
    live: Option<Res<LiveDrive>>,
    content: Res<ActivePlayerContent>,
    cab: Res<crate::cab_cvf::CabCvfState>,
) {
    if !ui.rebuild {
        return;
    }
    ui.rebuild = false;
    if let Some(root) = ui.root.take() {
        commands.entity(root).despawn();
    }
    if ui.panel == PlayerPanel::None {
        return;
    }
    let Some(camera) = ui.camera else { return };
    if ui.panel == PlayerPanel::Map
        && !ui.map_initialized
        && let Some(l) = live.as_ref()
    {
        fit_map(&mut ui, &l.session);
    }
    let panel = ui.panel;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(7.0),
                right: Val::Percent(7.0),
                top: Val::Percent(8.0),
                bottom: Val::Percent(8.0),
                padding: UiRect::all(Val::Px(18.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(BG),
            UiTargetCamera(camera),
            ZIndex(1000),
            Name::new("player-menu"),
        ))
        .with_children(|p| {
            row(p, |p| {
                label(
                    p,
                    match panel {
                        PlayerPanel::Menu => "OPENRAILSRS · nueva partida",
                        PlayerPanel::Pause => "PARTIDA EN PAUSA",
                        PlayerPanel::Notebook => "LIBRETA DEL SERVICIO",
                        PlayerPanel::Formation => "OPERACIONES DE LA FORMACIÓN",
                        PlayerPanel::Map => "MAPA Y DESPACHADOR",
                        PlayerPanel::Advanced => "HUD AVANZADO",
                        PlayerPanel::Settings => "AJUSTES",
                        _ => "AYUDA Y CONTROLES",
                    },
                    22.0,
                    ACCENT,
                );
                if panel != PlayerPanel::Menu {
                    button(p, "Cerrar · Esc", UiCommand::Close);
                }
            });
            p.spawn((
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    overflow: Overflow::scroll_y(),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    ..default()
                },
                ScrollPosition::default(),
                PlayerScroll,
            ))
            .with_children(|p| match panel {
                PlayerPanel::Menu => build_menu(p, &menu),
                PlayerPanel::Pause => {
                    if let Some(l) = live.as_ref() {
                        label(
                            p,
                            format!(
                                "{}\n{} · {:.0} m · {:.1} km/h",
                                l.session.scenario_name,
                                clock(l.clock_time_s()),
                                l.session.state.odometer_m,
                                l.session.velocity_mps() * 3.6
                            ),
                            17.0,
                            TEXT,
                        );
                    }
                    button(p, "Continuar la partida", UiCommand::Close);
                    label(
                        p,
                        "GUARDADO · conserva física, servicio, puertas, formación y cámara",
                        13.0,
                        MUTED,
                    );
                    for slot in 0..3 {
                        row(p, |p| {
                            button(p, format!("Guardar {}", slot + 1), UiCommand::Save(slot));
                            button(p, format!("Cargar {}", slot + 1), UiCommand::Load(slot));
                            label(p, save_label(slot), 12.0, MUTED);
                        });
                    }
                    row(p, |p| {
                        button(p, "Ajustes", UiCommand::Open(PlayerPanel::Settings));
                        button(p, "Libreta", UiCommand::Open(PlayerPanel::Notebook));
                        button(p, "Menú de inicio", UiCommand::ReturnMenu);
                        button(p, "Salir", UiCommand::Exit);
                    });
                }
                PlayerPanel::Notebook => {
                    row(p, |p| {
                        for (tab, name) in ["Briefing", "Horarios", "Evaluación"].iter().enumerate()
                        {
                            button(p, *name, UiCommand::NoteTab(tab));
                        }
                    });
                    dynamic(p, DynamicText::Notebook, 14.0);
                }
                PlayerPanel::Formation => {
                    if let Some(l) = live.as_ref() {
                        build_formation(p, &ui, &l.session);
                    }
                }
                PlayerPanel::Map => {
                    if let Some(l) = live.as_ref() {
                        build_map(p, &ui, l);
                    }
                }
                PlayerPanel::Advanced => {
                    row(p, |p| {
                        for (page, name) in ADVANCED_PAGES.iter().enumerate() {
                            button(p, *name, UiCommand::Advanced(page));
                        }
                    });
                    dynamic(p, DynamicText::Advanced, 13.0);
                }
                PlayerPanel::Settings => build_settings(p, &settings, cab.cvf_path.as_deref()),
                PlayerPanel::Help => {
                    dynamic(p, DynamicText::Help, 14.0);
                }
                PlayerPanel::None => {}
            });
            let status = label(
                p,
                if ui.notice.is_empty() {
                    if panel == PlayerPanel::Menu {
                        &menu.status
                    } else {
                        "La partida queda pausada mientras esta ventana está abierta"
                    }
                } else {
                    &ui.notice
                },
                13.0,
                CAUTION,
            );
            p.commands().entity(status).insert(DynamicText::Status);
        })
        .id();
    ui.root = Some(root);
    let _ = &content;
}
fn selector(p: &mut ChildSpawnerCommands<'_>, name: &str, value: String, field: MenuField) {
    row(p, |p| {
        p.spawn(Node {
            width: Val::Px(145.0),
            ..default()
        })
        .with_children(|p| {
            label(p, name, 14.0, MUTED);
        });
        button(p, "‹", UiCommand::Cycle(field, -1));
        p.spawn(Node {
            flex_grow: 1.0,
            min_width: Val::Px(250.0),
            ..default()
        })
        .with_children(|p| {
            label(p, value, 15.0, TEXT);
        });
        button(p, "›", UiCommand::Cycle(field, 1));
    });
}
fn build_menu(p: &mut ChildSpawnerCommands<'_>, menu: &PlayerLaunchMenu) {
    selector(
        p,
        "Ruta",
        menu.routes.get(menu.route).cloned().unwrap_or_default(),
        MenuField::Route,
    );
    selector(
        p,
        "Actividad / servicio",
        menu.current().map(|c| c.name.clone()).unwrap_or_default(),
        MenuField::Service,
    );
    selector(p, "Formación", menu.consist_label(), MenuField::Consist);
    label(
        p,
        menu.consist_status(),
        12.0,
        Color::srgb(0.70, 0.80, 0.88),
    );
    selector(p, "Recorrido", menu.path_label(), MenuField::Path);
    selector(
        p,
        "Hora de salida",
        clock(menu.start_time_s),
        MenuField::Time,
    );
    selector(
        p,
        "Estación del año",
        ["Primavera", "Verano", "Otoño", "Invierno"][menu.season].into(),
        MenuField::Season,
    );
    selector(p, "Clima", menu.weather.label().into(), MenuField::Weather);
    if menu.path != 0 {
        label(
            p,
            "Elegir otro recorrido inicia una exploración hasta su destino. Para las tres paradas elegí Recorrido del servicio.",
            12.0,
            CAUTION,
        );
    }
    row(p, |p| {
        button(p, "Iniciar partida", UiCommand::Start);
        button(p, "Ajustes", UiCommand::Open(PlayerPanel::Settings));
        button(p, "Salir", UiCommand::Exit);
    });
    label(p, "CONTINUAR UNA PARTIDA GUARDADA", 13.0, MUTED);
    for slot in 0..3 {
        row(p, |p| {
            button(p, format!("Reanudar {}", slot + 1), UiCommand::Resume(slot));
            label(p, save_label(slot), 12.0, TEXT);
        });
    }
}
fn build_formation(p: &mut ChildSpawnerCommands<'_>, ui: &PlayerUiState, s: &LiveDriveSession) {
    label(
        p,
        format!(
            "{} coches acoplados · {:.1} t · {:.0} m · clic en un coche para seleccionarlo",
            s.formation.coupled_count,
            s.physics.mass_kg / 1000.0,
            s.formation.length_m()
        ),
        14.0,
        MUTED,
    );
    row(p, |p| {
        for (i, car) in s.formation.cars.iter().enumerate() {
            button(
                p,
                format!(
                    "{}{}\n{}",
                    if i == ui.selected_car { "▶ " } else { "" },
                    i + 1,
                    if i >= s.formation.coupled_count {
                        "Separado"
                    } else if car.handbrake {
                        "F. mano"
                    } else if car.powered {
                        "Motor"
                    } else {
                        "Coche"
                    }
                ),
                UiCommand::SelectCar(i),
            );
        }
    });
    dynamic(p, DynamicText::Car, 14.0);
    row(p, |p| {
        button(p, "Freno de mano", UiCommand::Car(CarOperation::Handbrake));
        button(p, "Manguera delantera", UiCommand::Car(CarOperation::Hose));
        button(
            p,
            "Llave delantera",
            UiCommand::Car(CarOperation::FrontCock),
        );
        button(p, "Llave trasera", UiCommand::Car(CarOperation::RearCock));
        button(
            p,
            "Conectar / cortar tracción",
            UiCommand::Car(CarOperation::Power),
        );
        button(p, "Batería", UiCommand::Car(CarOperation::Battery));
        button(
            p,
            "Mando múltiple",
            UiCommand::Car(CarOperation::MultipleUnit),
        );
    });
    row(p, |p| {
        button(p, "Asegurar sección posterior", UiCommand::SecureTail);
        button(p, "Desacoplar detrás del seleccionado", UiCommand::Uncouple);
        button(p, "Acoplar sección estacionada", UiCommand::Recouple);
    });
    label(
        p,
        "Detené el tren antes de operar. Cerrá las llaves antes de desconectar una manguera.\nPara desacoplar: asegurá la sección posterior. Para acoplar: enganches a menos de 1 m y tren detenido.\nDespués de acoplar reconectá la manguera, abrí las llaves y soltá los frenos de mano.",
        13.0,
        MUTED,
    );
}
fn settings_row(
    p: &mut ChildSpawnerCommands<'_>,
    label_value: String,
    field: SettingField,
    step: f32,
) {
    row(p, |p| {
        label(p, label_value, 14.0, TEXT);
        button(p, "−", UiCommand::Setting(field, -step));
        button(p, "+", UiCommand::Setting(field, step));
    });
}
fn build_settings(
    p: &mut ChildSpawnerCommands<'_>,
    s: &PlayerSettings,
    cab_path: Option<&std::path::Path>,
) {
    button(
        p,
        format!(
            "Práctica rápida en estaciones: {}",
            yes(s.quick_station_practice)
        ),
        UiCommand::Setting(SettingField::QuickStations, 0.0),
    );
    label(
        p,
        "Práctica: embarque hasta 5 s, sin espera de horario. Normal: respeta el servicio original.",
        12.0,
        MUTED,
    );
    settings_row(
        p,
        format!("Distancia del escenario: {:.0} m", s.view_distance_m),
        SettingField::Distance,
        500.0,
    );
    settings_row(
        p,
        format!("Campo visual de cabina: {:.0}°", s.cab_fov_deg),
        SettingField::Fov,
        5.0,
    );
    if let Some(path) = cab_path {
        let profile = s
            .cab_profiles
            .get(path.to_string_lossy().as_ref())
            .copied()
            .unwrap_or_default();
        label(
            p,
            format!(
                "CABINA · {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            13.0,
            ACCENT,
        );
        settings_row(
            p,
            format!(
                "Altura del asiento 3D: {:+.0} cm",
                profile.seat_height_m * 100.0
            ),
            SettingField::SeatHeight,
            0.05,
        );
        settings_row(
            p,
            format!(
                "Asiento 3D hacia atrás: {:+.0} cm",
                profile.seat_back_m * 100.0
            ),
            SettingField::SeatBack,
            0.05,
        );
        settings_row(
            p,
            format!("Alcance del barrido: {:.0}%", profile.wipe_scale * 100.0),
            SettingField::WipeScale,
            0.05,
        );
        button(
            p,
            "Restaurar puesto y barrido originales",
            UiCommand::Setting(SettingField::CabReset, 0.0),
        );
    }
    settings_row(
        p,
        format!("Tamaño de interfaz: {:.0}%", s.ui_scale * 100.0),
        SettingField::Scale,
        0.1,
    );
    row(p, |p| {
        button(
            p,
            format!("Sombras: {}", yes(s.shadows)),
            UiCommand::Setting(SettingField::Shadows, 0.0),
        );
        button(
            p,
            format!("Niebla: {}", yes(s.fog)),
            UiCommand::Setting(SettingField::Fog, 0.0),
        );
        button(
            p,
            format!("Velocidad: {}", if s.mph { "mph" } else { "km/h" }),
            UiCommand::Setting(SettingField::Units, 0.0),
        );
    });
    button(
        p,
        format!("Modelo de niebla: {}", s.fog_quality.label()),
        UiCommand::Setting(SettingField::FogQuality, 0.0),
    );
    button(
        p,
        format!("Cálculo del clima: {}", s.weather_execution.label()),
        UiCommand::Setting(SettingField::WeatherExecution, 0.0),
    );
    label(
        p,
        "Automático adapta el detalle. CPU calcula los copos; el dibujo usa el render elegido al iniciar.",
        12.0,
        MUTED,
    );
    button(
        p,
        format!("Renderizado al iniciar: {}", s.renderer.label()),
        UiCommand::Setting(SettingField::Renderer, 0.0),
    );
    label(
        p,
        "Para cambiar el render: guardá los ajustes y volvé a iniciar el visor. CPU requiere un controlador de software.",
        12.0,
        MUTED,
    );
    button(
        p,
        format!("Sonido original: {}", yes(s.audio_enabled)),
        UiCommand::Setting(SettingField::Audio, 0.0),
    );
    settings_row(
        p,
        format!("Volumen: {:.0}%", s.audio_volume * 100.0),
        SettingField::Volume,
        0.1,
    );
    label(
        p,
        "CONTROLES · clic en una asignación y pulsá la nueva tecla",
        14.0,
        ACCENT,
    );
    row(p, |p| {
        for a in PlayerAction::ALL {
            button(
                p,
                format!("{}: {}", a.label(), s.key_label(a)),
                UiCommand::Rebind(a),
            );
        }
    });
    row(p, |p| {
        button(p, "Restablecer controles", UiCommand::DefaultKeys);
        button(p, "Guardar ajustes", UiCommand::SaveSettings);
    });
}
fn save_label(slot: usize) -> String {
    match SavedGame::read(&slot_path(slot)) {
        Ok(s) => format!(
            "{} · {} · {:.0} m",
            s.session
                .gameplay
                .stop_targets
                .get(s.session.gameplay.next_stop_idx)
                .map(|s| s.name.as_str())
                .unwrap_or(&s.session.gameplay.destination_node),
            clock(s.start_clock_s + s.session.state.time_s()),
            s.session.state.odometer_m
        ),
        Err(_) => "Ranura vacía o partida no válida".into(),
    }
}
fn yes(v: bool) -> &'static str {
    if v { "Sí" } else { "No" }
}
pub fn clock(seconds: f64) -> String {
    let t = seconds.floor().rem_euclid(86400.0) as u32;
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

fn graph_point(s: &LiveDriveSession, edge: &str, pos: f64) -> Option<Vec2> {
    let e = s.graph.edge(edge)?;
    let a = s.graph.node(&e.from.0)?;
    let b = s.graph.node(&e.to.0)?;
    let t = (pos / e.length_m.max(0.01)).clamp(0.0, 1.0) as f32;
    Some(Vec2::new(a.x_m as f32, a.y_m as f32).lerp(Vec2::new(b.x_m as f32, b.y_m as f32), t))
}
fn fit_map(ui: &mut PlayerUiState, s: &LiveDriveSession) {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for eid in &s.state.path_edges {
        if let Some(e) = s.graph.edge(eid) {
            for id in [&e.from.0, &e.to.0] {
                if let Some(n) = s.graph.node(id) {
                    let p = Vec2::new(n.x_m as f32, n.y_m as f32);
                    min = min.min(p);
                    max = max.max(p);
                }
            }
        }
    }
    if min.is_finite() {
        ui.map_center = (min + max) * 0.5;
        let span = (max - min).max(Vec2::splat(100.0));
        ui.map_scale = (740.0 / span.x).min(300.0 / span.y).clamp(0.002, 2.0);
    }
    ui.map_initialized = true;
}
fn map_project(ui: &PlayerUiState, p: Vec2) -> Vec2 {
    let d = (p - ui.map_center) * ui.map_scale;
    Vec2::new(400.0 + d.x, 180.0 - d.y)
}
fn map_inside(p: Vec2) -> bool {
    p.x >= 5.0 && p.x <= 795.0 && p.y >= 5.0 && p.y <= 355.0
}
fn clip_map_line(a: Vec2, b: Vec2) -> Option<(Vec2, Vec2)> {
    let delta = b - a;
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for (start, step, min, max) in [(a.x, delta.x, 5.0, 795.0), (a.y, delta.y, 5.0, 355.0)] {
        if step.abs() < 1e-6 {
            if !(min..=max).contains(&start) {
                return None;
            }
        } else {
            let t1 = (min - start) / step;
            let t2 = (max - start) / step;
            low = low.max(t1.min(t2));
            high = high.min(t1.max(t2));
            if low > high {
                return None;
            }
        }
    }
    Some((a + delta * low, a + delta * high))
}
fn line(p: &mut ChildSpawnerCommands<'_>, a: Vec2, b: Vec2, width: f32, color: Color) {
    let d = b - a;
    let len = d.length();
    if len < 0.1 {
        return;
    }
    let center = (a + b) * 0.5;
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(center.x - len * 0.5),
            top: Val::Px(center.y - width * 0.5),
            width: Val::Px(len),
            height: Val::Px(width),
            ..default()
        },
        BackgroundColor(color),
        UiTransform::from_rotation(Rot2::radians(d.y.atan2(d.x))),
    ));
}
fn map_marker(
    p: &mut ChildSpawnerCommands<'_>,
    point: Vec2,
    value: String,
    color: Color,
    command: Option<UiCommand>,
) {
    if !map_inside(point) {
        return;
    }
    let mut entity = p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(point.x - 6.0),
            top: Val::Px(point.y - 8.0),
            min_width: Val::Px(15.0),
            min_height: Val::Px(18.0),
            padding: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(color),
        ZIndex(2),
    ));
    if let Some(command) = command {
        entity.insert((Button, command));
    }
    entity.with_children(|p| {
        label(p, value, 11.0, TEXT);
    });
}
fn build_map(p: &mut ChildSpawnerCommands<'_>, ui: &PlayerUiState, live: &LiveDrive) {
    let s = &live.session;
    row(p, |p| {
        button(p, "+ Zoom", UiCommand::Zoom(1.5));
        button(p, "− Zoom", UiCommand::Zoom(1.0 / 1.5));
        button(p, "Ajustar recorrido", UiCommand::FitMap);
        button(p, "←", UiCommand::Pan(Vec2::new(-100.0, 0.0)));
        button(p, "→", UiCommand::Pan(Vec2::new(100.0, 0.0)));
        button(p, "↑", UiCommand::Pan(Vec2::new(0.0, 100.0)));
        button(p, "↓", UiCommand::Pan(Vec2::new(0.0, -100.0)));
    });
    let occupied = s.occupied_edges();
    p.spawn((
        Node {
            width: Val::Px(800.0),
            height: Val::Px(360.0),
            min_height: Val::Px(360.0),
            flex_shrink: 0.0,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgb(0.015, 0.025, 0.04)),
    ))
    .with_children(|p| {
        for (id, e) in s
            .graph
            .edges_iter()
            .filter(|(id, _)| !id.ends_with("_r"))
            .take(20000)
        {
            let (Some(a), Some(b)) = (s.graph.node(&e.from.0), s.graph.node(&e.to.0)) else {
                continue;
            };
            let a = map_project(ui, Vec2::new(a.x_m as f32, a.y_m as f32));
            let b = map_project(ui, Vec2::new(b.x_m as f32, b.y_m as f32));
            let routed = s
                .state
                .path_edges
                .iter()
                .any(|p| p == id || p.strip_suffix("_r") == Some(id));
            let occupied = occupied.contains_key(id) || occupied.contains_key(&format!("{id}_r"));
            if let Some((a, b)) = clip_map_line(a, b) {
                line(
                    p,
                    a,
                    b,
                    if routed { 3.0 } else { 1.0 },
                    if occupied {
                        CAUTION
                    } else if routed {
                        ACCENT
                    } else {
                        Color::srgb(0.25, 0.33, 0.42)
                    },
                );
            }
        }
        for stop in &s.gameplay.stop_targets {
            if let Some((edge, pos)) = openrailsrs_sim::path_data::PathData::position_at_odometer(
                &s.state.path_edges,
                &s.path_data.edges,
                stop.cum_dist_m,
            ) && let Some(point) = graph_point(s, &edge, pos)
            {
                map_marker(
                    p,
                    map_project(ui, point),
                    format!("■ {}", stop.name),
                    Color::srgb(0.08, 0.3, 0.4),
                    None,
                );
            }
        }
        for (id, n) in s.graph.nodes_iter() {
            let point = map_project(ui, Vec2::new(n.x_m as f32, n.y_m as f32));
            match &n.kind {
                NodeKind::Switch { .. } => map_marker(
                    p,
                    point,
                    "◆".into(),
                    Color::srgb(0.35, 0.25, 0.6),
                    Some(UiCommand::MapPick(MapSelection::Switch(id.into()))),
                ),
                NodeKind::Station { name } => {
                    map_marker(p, point, name.clone(), Color::srgb(0.08, 0.3, 0.4), None)
                }
                NodeKind::Plain => {}
            }
        }
        for sig in s.graph.signals() {
            if let Some(point) = graph_point(s, &sig.edge_id, sig.position_m) {
                map_marker(
                    p,
                    map_project(ui, point),
                    "●".into(),
                    aspect_color(s.signal_aspect(&sig.id).unwrap_or(sig.aspect)),
                    Some(UiCommand::MapPick(MapSelection::Signal(sig.id.clone()))),
                );
            }
        }
        if let Some(edge) = s.current_edge_id()
            && let Some(pos) = graph_point(s, edge, s.pos_on_edge_m())
        {
            map_marker(
                p,
                map_project(ui, pos),
                "▶ Jugador".into(),
                Color::srgb(0.12, 0.43, 0.57),
                None,
            );
        }
        for service in live
            .traffic
            .services
            .iter()
            .filter(|service| service.departed)
        {
            if let Some(edge) = service.session.current_edge_id()
                && let Some(pos) =
                    graph_point(&service.session, edge, service.session.pos_on_edge_m())
            {
                map_marker(
                    p,
                    map_project(ui, pos),
                    format!("▶ {}", service.id),
                    Color::srgb(0.45, 0.24, 0.55),
                    None,
                );
            }
        }
        if let Some(head) = s.formation.parked_head_chainage_m {
            let offset = s.formation.offset_m(s.formation.coupled_count);
            if let Some((edge, pos)) = openrailsrs_sim::path_data::PathData::position_at_odometer(
                &s.state.path_edges,
                &s.path_data.edges,
                (head + offset).max(0.0),
            ) && let Some(point) = graph_point(s, &edge, pos)
            {
                map_marker(
                    p,
                    map_project(ui, point),
                    "■ Estacionado".into(),
                    Color::srgb(0.55, 0.32, 0.12),
                    None,
                );
            }
        }
    });
    label(
        p,
        "Cian: itinerario · Amarillo: ocupado · ● señal · ◆ cambio · clic para operar",
        12.0,
        MUTED,
    );
    dynamic(p, DynamicText::Map, 13.0);
    row(p, |p| {
        button(
            p,
            "Señal: Alto",
            UiCommand::DispatchSignal(Some(SignalAspect::Stop)),
        );
        button(
            p,
            "Precaución",
            UiCommand::DispatchSignal(Some(SignalAspect::Caution)),
        );
        button(
            p,
            "Vía libre",
            UiCommand::DispatchSignal(Some(SignalAspect::Clear)),
        );
        button(p, "Automática", UiCommand::DispatchSignal(None));
        button(p, "Cambiar desvío", UiCommand::DispatchSwitch);
    });
}
fn aspect_color(a: SignalAspect) -> Color {
    match a {
        SignalAspect::Clear => GOOD,
        SignalAspect::Caution => CAUTION,
        SignalAspect::Stop => BAD,
    }
}
fn aspect_name(a: SignalAspect) -> &'static str {
    match a {
        SignalAspect::Clear => "Vía libre",
        SignalAspect::Caution => "Precaución",
        SignalAspect::Stop => "Alto",
    }
}

fn update_panel_text(
    time: Res<Time>,
    mut elapsed: Local<f32>,
    ui: Res<PlayerUiState>,
    settings: Res<PlayerSettings>,
    live: Option<Res<LiveDrive>>,
    content: Res<ActivePlayerContent>,
    fps: Res<crate::hud::HudFps>,
    performance: Res<crate::performance::JourneyPerformance>,
    memory: Res<crate::gpu_memory::GraphicsMemory>,
    weather_particles: Res<crate::weather_particles::WeatherParticles>,
    audio: Res<crate::native_audio::NativeAudio>,
    tiles: Res<crate::world_tile_index::WorldTileEntityIndex>,
    mut texts: Query<(&DynamicText, &mut Text)>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.2 && !ui.is_changed() {
        return;
    }
    *elapsed = 0.0;
    for (kind, mut text) in &mut texts {
        let value = match kind {
            DynamicText::Status => ui.notice.clone(),
            DynamicText::Help => help_text(&settings),
            _ => {
                let Some(l) = live.as_ref() else { continue };
                let s = &l.session;
                match kind {
                    DynamicText::Notebook => notebook_text(l, &content, ui.notebook_tab),
                    DynamicText::Advanced => {
                        let mut text = advanced_text(l, &content, ui.advanced_page);
                        if ui.advanced_page == 9 {
                            if let Some(engine) = &audio.engine {
                                let report = engine.report();
                                text += &format!(
                                    "\nSonido SMS/WAV: {} programas · {} muestras · {:.1} MiB · {} voces\nSalida de audio: {}",
                                    report.programs,
                                    report.samples,
                                    report.decoded_mib,
                                    report.active_voices,
                                    if report.device {
                                        "activa"
                                    } else {
                                        "cargando o no disponible"
                                    }
                                );
                                if let Some(warning) = report.warnings.first() {
                                    text += &format!("\nAudio: {warning}");
                                }
                            }
                            text += &format!("\n{}", performance.hud_text());
                            text += &format!(
                                "\n{}\n{}",
                                memory.hud_text(),
                                weather_particles.hud_text()
                            );
                            text += &format!(
                                "\n\nFPS {:.1} · cuadro {:.1} ms\nEscenario: {} sectores activos · {} entidades en GPU\nDistancia de carga {:.0} m",
                                fps.smoothed,
                                fps.frame_ms,
                                tiles.tile_count(),
                                tiles.entity_count(),
                                settings.view_distance_m
                            );
                        }
                        text
                    }
                    DynamicText::Car => car_text(s, ui.selected_car),
                    DynamicText::Map => match &ui.map_selection {
                        Some(MapSelection::Signal(id)) => format!(
                            "Señal {id}: {} · {}",
                            aspect_name(s.signal_aspect(id).unwrap_or(SignalAspect::Stop)),
                            if s.signal_overrides.contains_key(id) {
                                "Orden del despachador"
                            } else {
                                "Automática"
                            }
                        ),
                        Some(MapSelection::Switch(id)) => format!(
                            "Cambio {id}: {:?} · las órdenes se rechazan si hay una formación sobre el cambio",
                            s.graph.switch_position(id).unwrap_or_default()
                        ),
                        None => format!(
                            "Jugador: {} · {:.1} km/h · {} tramos ocupados",
                            s.current_edge_id().unwrap_or("—"),
                            s.velocity_mps() * 3.6,
                            s.occupied_edges().len()
                        ),
                    },
                    _ => String::new(),
                }
            }
        };
        if !value.is_empty() && text.0 != value {
            text.0 = value;
        }
    }
}
fn notebook_text(l: &LiveDrive, content: &ActivePlayerContent, tab: usize) -> String {
    let s = &l.session;
    let g = &s.gameplay;
    match tab {
        0 => format!(
            "{}\n\n{}\n\nOBJETIVOS\nCompletar {}/{} paradas y llegar a {}.\nDetenerse a menos de 10 m del punto y por debajo de 0,36 km/h.\nAbrir puertas, completar el embarque y cerrarlas para salir.\nRespetar señales y límites. La tracción se corta con puertas abiertas.\n\nPROCEDIMIENTO\n{}\n\n{}",
            s.scenario_name,
            content.description,
            g.next_stop_idx,
            g.stop_targets.len(),
            g.destination,
            crate::driving_hud::service_instruction(s),
            "Usá Horarios para consultar todas las paradas y Evaluación para revisar el resultado."
        ),
        1 => {
            let mut out = format!(
                "HORARIOS · reloj {} · {} pasajeros\n\nESTACIÓN              LLEGADA PLAN.  SALIDA PLAN.   LLEGADA REAL  SALIDA REAL   DEMORA\n",
                clock(l.clock_time_s()),
                s.state.passengers
            );
            for (i, stop) in g.stop_targets.iter().enumerate() {
                let result = g.stop_results.iter().find(|r| r.node == stop.node_id);
                let (arrive, depart, delay) = result
                    .map(|r| {
                        (
                            clock(l.start_clock_s + r.actual_arrive_s),
                            clock(l.start_clock_s + r.actual_depart_s),
                            format!("{:+.0}s", r.delay_s),
                        )
                    })
                    .unwrap_or_else(|| {
                        if i == g.next_stop_idx
                            && let Some(arrival) = g.current_arrival_s()
                        {
                            (
                                clock(l.start_clock_s + arrival),
                                "—".into(),
                                format!("{:+.0}s", arrival - stop.arrive_s),
                            )
                        } else {
                            ("—".into(), "—".into(), "—".into())
                        }
                    });
                out += &format!(
                    "{} {:20} {}     {}     {:11} {:11} {}\n",
                    if i == g.next_stop_idx {
                        "▶"
                    } else if i < g.next_stop_idx {
                        "✓"
                    } else {
                        " "
                    },
                    stop.name,
                    clock(l.start_clock_s + stop.arrive_s),
                    clock(l.start_clock_s + stop.depart_s),
                    arrive,
                    depart,
                    delay
                );
            }
            out += &format!(
                "\n{}\nPasajeros: {:.0} s · espera de horario: {:.0} s\nModo: {}\n",
                crate::driving_hud::service_instruction(s),
                g.remaining_boarding_s(),
                g.remaining_schedule_s(s.time_s()),
                if g.quick_station_practice {
                    "Práctica rápida (horario libre)"
                } else {
                    "Servicio normal"
                }
            );
            out
        }
        _ => {
            let mut out = format!(
                "EVALUACIÓN · {}\n\nParadas cumplidas: {}/{}\nPenalización por demora: {:.1}\nPasajeros a bordo: {}\nDistancia recorrida: {:.0} m\nEnergía: {:.2} kWh · combustible: {:.2} kg\n",
                phase_name(g.phase),
                g.stop_results.len(),
                g.stop_targets.len(),
                g.accrued_penalty,
                s.state.passengers,
                s.state.odometer_m,
                s.state.cumulative_energy_j / 3_600_000.0,
                s.state.fuel_consumption_g / 1000.0
            );
            out += &format!(
                "Salidas anticipadas: {}\nModo: {}\n",
                g.stop_results
                    .iter()
                    .filter(|r| r.early_departure_s > 0.0)
                    .count(),
                if g.quick_station_practice {
                    "Práctica rápida"
                } else {
                    "Servicio normal"
                }
            );
            for r in &g.stop_results {
                out += &format!(
                    "\n{}: error {:.1} m · llegada {:.2} km/h · demora {:+.0} s · parada {:.0} s",
                    r.name,
                    r.position_error_m,
                    r.arrival_speed_mps * 3.6,
                    r.delay_s,
                    r.dwell_s
                );
            }
            if let Some(failure) = &g.failure {
                out += &format!("\n\nINCIDENCIA: {failure}");
            }
            out
        }
    }
}
fn phase_name(p: ServicePhase) -> &'static str {
    match p {
        ServicePhase::Approaching => "En recorrido",
        ServicePhase::Boarding => "Embarcando",
        ServicePhase::ReadyToDepart => "Salida autorizada",
        ServicePhase::Completed => "Servicio completado",
        ServicePhase::Failed => "Servicio fallido",
    }
}
fn car_text(s: &LiveDriveSession, index: usize) -> String {
    let Some(c) = s.formation.cars.get(index) else {
        return String::new();
    };
    let cylinder = if index < s.formation.coupled_count {
        s.state.brake_system.cylinders.get(index)
    } else {
        s.formation
            .parked_brakes
            .get(index - s.formation.coupled_count)
    };
    format!(
        "COCHE {} · {}\nMasa {:.1} t · longitud {:.1} m · {}\nFreno de mano: {} · manguera: {}\nLlaves delantera / trasera: {} / {}\nTracción: {} · batería: {} · mando múltiple: {}\nFuerza de frenado: {:.1} kN · tubería: {}",
        index + 1,
        c.name,
        c.mass_kg / 1000.0,
        c.length_m,
        if index < s.formation.coupled_count {
            "Acoplado"
        } else {
            "Estacionado"
        },
        yes(c.handbrake),
        if c.hose_connected {
            "Conectada"
        } else {
            "Desconectada"
        },
        if c.front_cock_open {
            "Abierta"
        } else {
            "Cerrada"
        },
        if c.rear_cock_open {
            "Abierta"
        } else {
            "Cerrada"
        },
        if !c.powered {
            "Sin motor"
        } else if c.power_on {
            "Conectada"
        } else {
            "Desconectada"
        },
        yes(c.battery_on),
        yes(c.mu_connected),
        cylinder
            .map(|b| b.effective_force_n(s.velocity_mps()) / 1000.0)
            .unwrap_or(0.0),
        cylinder
            .map(|b| if b.air_vented {
                "Venteada (emergencia)"
            } else if b.air_isolated {
                "Aislada"
            } else {
                "Conectada al mando"
            })
            .unwrap_or("—")
    )
}
fn advanced_text(l: &LiveDrive, content: &ActivePlayerContent, page: usize) -> String {
    let s = &l.session;
    let state = &s.state;
    let p = &s.physics;
    let mut out = format!("{} · {}\n\n", ADVANCED_PAGES[page], clock(l.clock_time_s()));
    match page {
        0 => {
            out += &format!(
                "Velocidad {:.2} km/h · límite {:.1} km/h\nRegulador {:.0}% · freno {:.0}% · inversor {:.2}\nDistancia {:.1} m · tiempo {:.1} s · escala ×{:.0}\nEstado: {}\n{}",
                s.velocity_mps() * 3.6,
                s.effective_speed_limit_mps() * 3.6,
                s.driver_throttle * 100.0,
                s.driver_brake * 100.0,
                s.driver_direction,
                state.odometer_m,
                s.time_s(),
                s.speed_mul,
                phase_name(s.gameplay.phase),
                crate::driving_hud::service_instruction(s)
            )
        }
        1 => {
            out += &format!(
                "Masa total {:.1} t + pasajeros {:.1} t\nLongitud {:.1} m · acoplados {} / {}\n\n",
                p.mass_kg / 1000.0,
                state.extra_mass_kg / 1000.0,
                s.formation.length_m(),
                s.formation.coupled_count,
                s.formation.cars.len()
            );
            for (i, c) in s.formation.cars.iter().enumerate() {
                out += &format!(
                    "{:2} {:28} {:6.1} t · {}\n",
                    i + 1,
                    c.name,
                    c.mass_kg / 1000.0,
                    if i < s.formation.coupled_count {
                        "Acoplado"
                    } else {
                        "Estacionado"
                    }
                );
            }
        }
        2 => {
            out += &format!(
                "Potencia máxima disponible {:.0} kW\nEsfuerzo máximo {:.1} kN\nCaldera {}\n\n",
                p.max_power_w / 1000.0,
                p.max_tractive_effort_n / 1000.0,
                state
                    .boiler_state
                    .as_ref()
                    .map(|b| format!(
                        "{:.2} bar · agua {:.0} kg · carbón {:.0} kg",
                        b.pressure_bar, b.water_kg, b.coal_kg
                    ))
                    .unwrap_or_else(|| "No instalada".into())
            );
            for (i, rpm) in state.diesel_rpm.iter().enumerate() {
                out += &format!(
                    "Motor {}: {:.0} rpm · demanda efectiva {:.1}% · temperatura normalizada {:.3}\n",
                    i + 1,
                    rpm,
                    state.diesel_apparent_throttle.get(i).unwrap_or(&0.0) * 100.0,
                    state.diesel_motor_heat.get(i).unwrap_or(&0.0)
                );
            }
        }
        3 | 4 => {
            for (i, c) in s
                .formation
                .cars
                .iter()
                .enumerate()
                .filter(|(_, c)| c.powered)
            {
                out += &format!(
                    "Unidad {} · {} · {} · {}\nBatería {} · mando múltiple {}\n",
                    i + 1,
                    c.name,
                    if c.power_on {
                        "Tracción conectada"
                    } else {
                        "Tracción cortada"
                    },
                    if i < s.formation.coupled_count {
                        "Mando de la formación"
                    } else {
                        "Sección estacionada"
                    },
                    yes(c.battery_on),
                    yes(c.mu_connected)
                );
            }
            out += &format!(
                "\nEnergía acumulada {:.3} kWh\nRecuperada {:.3} kWh · consumo diésel {:.3} kg",
                state.cumulative_energy_j / 3_600_000.0,
                state.regen_energy_j / 3_600_000.0,
                state.fuel_consumption_g / 1000.0
            );
        }
        5 => {
            out += &format!(
                "Mando de freno {:.0}% · propagación {:.0} m/s\n\nCOCHE   CILINDRO kN   FRENO MANO kN   TUBERÍA\n",
                s.driver_brake * 100.0,
                state.brake_system.pipe_speed_mps
            );
            for (i, b) in state.brake_system.cylinders.iter().enumerate() {
                out += &format!(
                    "{:3}     {:9.2}       {:9.2}     {}\n",
                    i + 1,
                    b.current_force_n / 1000.0,
                    b.handbrake_force_n / 1000.0,
                    if b.air_vented {
                        "Venteada"
                    } else if b.air_isolated {
                        "Aislada"
                    } else {
                        "Conectada"
                    }
                );
            }
        }
        6 => {
            let grade = s
                .path_data
                .get(state.edge_index)
                .map(|e| e.grade_at(state.pos_on_edge_m))
                .unwrap_or(0.0);
            out += &format!(
                "Tracción diésel calculada {:.2} kN\nFrenado efectivo {:.2} kN\nResistencia Davis {:.2} kN\nResistencia de pendiente {:.2} kN ({:+.2}%)\n\n",
                state.diesel_traction_force_n.iter().sum::<f64>() / 1000.0,
                state.brake_system.total_force_n(s.velocity_mps()) / 1000.0,
                p.davis.resistance_n(s.velocity_mps()) / 1000.0,
                (p.mass_kg + state.extra_mass_kg) * 9.81 * grade / 100.0 / 1000.0,
                grade
            );
            for (i, c) in state.couplers.iter().enumerate() {
                out += &format!(
                    "Enganche {}: extensión {:.4} m · {}\n",
                    i + 1,
                    c.extension_m,
                    if c.broken { "Roto" } else { "Conectado" }
                );
            }
        }
        7 => {
            out += &format!(
                "Tramo actual {} · posición {:.1} m\nTramos ocupados {} · señales con orden manual {}\n\n",
                s.current_edge_id().unwrap_or("—"),
                s.pos_on_edge_m(),
                s.occupied_edges().len(),
                s.signal_overrides.len()
            );
            for (id, aspect) in &s.signal_overrides {
                out += &format!("Señal {id}: {}\n", aspect_name(*aspect));
            }
            out += "\nTRÁFICO EN VIVO\n";
            for service in &l.traffic.services {
                out += &format!(
                    "{}: {} · {:.1} km/h · {} paradas · {}\n",
                    service.id,
                    if !service.departed {
                        "Salida pendiente"
                    } else if service.session.arrived {
                        "Recorrido terminado"
                    } else {
                        phase_name(service.session.gameplay.phase)
                    },
                    service.session.velocity_mps() * 3.6,
                    service.session.gameplay.stop_results.len(),
                    service.session.current_edge_id().unwrap_or("—")
                );
            }
        }
        8 => {
            out += &format!(
                "Clima {} · estación {}\nHora local {}\nSombras y campo visual se ajustan en F10\nEl sol se calcula con fecha, latitud, longitud y reloj de la ruta.",
                content.weather.label(),
                l.season,
                clock(l.clock_time_s())
            )
        }
        _ => {
            out += &format!(
                "Paso físico {:.3} s · odómetro {:.2} m\nTramo {} / {} · formación {} coches\nLa escenografía se carga alrededor del tren según la distancia elegida.\nF3 muestra métricas de FPS, streaming y render.",
                s.realtime_physics_dt(),
                state.odometer_m,
                state.edge_index + 1,
                state.path_edges.len(),
                s.formation.coupled_count
            )
        }
    }
    out
}
fn help_text(s: &PlayerSettings) -> String {
    let mut out = "CONTROLES ACTUALES\n\n".to_string();
    for action in PlayerAction::ALL {
        out += &format!("{:9} {}\n", s.key_label(action), action.label());
    }
    out + "\nEsc: pausa / cerrar ventana · flechas: mover cámara · RePág / AvPág: altura\nRueda: acercar / alejar · arrastrar en el fondo: mirar\nEn cabina 3D: arrastrar una palanca hacia arriba aumenta su valor;\nel clic en un interruptor lo conmuta. El control bajo el cursor aparece abajo.\nSonidos originales SMS/WAV: motor, rodadura, frenos y bocina. F10 permite ajustar volumen o silenciar."
}
fn scroll_panel(
    mut wheel: MessageReader<MouseWheel>,
    mut panels: Query<&mut ScrollPosition, With<PlayerScroll>>,
    ui: Res<PlayerUiState>,
) {
    if ui.panel == PlayerPanel::None {
        wheel.clear();
        return;
    }
    for ev in wheel.read() {
        let delta = ev.y
            * if ev.unit == MouseScrollUnit::Line {
                34.0
            } else {
                1.0
            };
        for mut scroll in &mut panels {
            scroll.0.y = (scroll.0.y - delta).max(0.0);
        }
    }
}

#[derive(Clone, Debug)]
pub struct MonitorEvent {
    pub distance_m: f64,
    pub text: String,
    pub color: Color,
}
pub fn monitor_events(s: &LiveDriveSession) -> Vec<MonitorEvent> {
    let head = s.head_chainage_m();
    let mut before = 0.0;
    let mut out = vec![];
    let mut previous_limit = None;
    for (index, eid) in s.state.path_edges.iter().enumerate() {
        let Some(edge) = s.graph.edge(eid) else {
            continue;
        };
        let physics = s.path_data.get(index);
        let initial_limit = physics.map_or(edge.speed_limit_mps, |p| p.speed_limit_at(0.0));
        if before - head >= 0.0
            && before - head <= 5000.0
            && previous_limit.is_some_and(|v: f64| (v - initial_limit).abs() > 0.01)
        {
            out.push(MonitorEvent {
                distance_m: before - head,
                text: format!("Límite {:.0} km/h", initial_limit * 3.6),
                color: CAUTION,
            });
        }
        if let Some(physics) = physics {
            for post in &physics.profile.speed_posts {
                let distance = before + post.position_m - head;
                if (0.0..=5000.0).contains(&distance) {
                    out.push(MonitorEvent {
                        distance_m: distance,
                        text: format!("Límite {:.0} km/h", post.speed_limit_kmh),
                        color: CAUTION,
                    });
                }
            }
        }
        for signal in s.graph.signals_on_edge(eid) {
            let d = before + signal.position_m - head;
            if (0.0..=5000.0).contains(&d) {
                let aspect = s.signal_aspect(&signal.id).unwrap_or(signal.aspect);
                out.push(MonitorEvent {
                    distance_m: d,
                    text: format!("● {}", aspect_name(aspect)),
                    color: aspect_color(aspect),
                });
            }
        }
        if let Some(node) = s.graph.node(&edge.to.0)
            && matches!(node.kind, NodeKind::Switch { .. })
        {
            let d = before + edge.length_m - head;
            if (0.0..=5000.0).contains(&d) {
                out.push(MonitorEvent {
                    distance_m: d,
                    text: format!("◆ Cambio {}", edge.to.0),
                    color: ACCENT,
                });
            }
        }
        previous_limit =
            Some(physics.map_or(edge.speed_limit_mps, |p| p.speed_limit_at(edge.length_m)));
        before += edge.length_m;
    }
    for stop in s
        .gameplay
        .stop_targets
        .iter()
        .skip(s.gameplay.next_stop_idx)
    {
        let d = stop.cum_dist_m - head;
        if (0.0..=5000.0).contains(&d) {
            out.push(MonitorEvent {
                distance_m: d,
                text: format!("■ {}", stop.name),
                color: ACCENT,
            });
        }
    }
    out.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    out
}
fn update_monitor(
    mut commands: Commands,
    time: Res<Time>,
    mut last: Local<f32>,
    ui: Res<PlayerUiState>,
    live: Option<Res<LiveDrive>>,
    mut roots: Query<&mut Visibility, With<TrackMonitorRoot>>,
    bodies: Query<(Entity, Option<&Children>), With<TrackMonitorBody>>,
) {
    for mut vis in &mut roots {
        *vis = if ui.monitor && ui.panel == PlayerPanel::None {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !ui.monitor {
        return;
    }
    *last += time.delta_secs();
    if *last < 0.4 {
        return;
    }
    *last = 0.0;
    let Some(l) = live else { return };
    let Ok((root, children)) = bodies.single() else {
        return;
    };
    if let Some(children) = children {
        for child in children {
            commands.entity(*child).despawn();
        }
    }
    let events = monitor_events(&l.session);
    commands.entity(root).with_children(|p| {
        line(p, Vec2::new(20.0, 14.0), Vec2::new(20.0, 245.0), 3.0, MUTED);
        label(
            p,
            format!(
                "Límite actual {:.0} km/h",
                l.session.effective_speed_limit_mps() * 3.6
            ),
            11.0,
            TEXT,
        );
        let mut last_y = 12.0_f32;
        for event in events.iter().take(7) {
            let y = (28.0 + (event.distance_m / 5000.0) as f32 * 195.0)
                .max(last_y + 26.0)
                .min(218.0);
            last_y = y;
            line(
                p,
                Vec2::new(15.0, y + 7.0),
                Vec2::new(27.0, y + 7.0),
                3.0,
                event.color,
            );
            p.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(32.0),
                top: Val::Px(y),
                ..default()
            })
            .with_children(|p| {
                label(
                    p,
                    format!("{} · {:.0} m", event.text, event.distance_m),
                    11.0,
                    event.color,
                );
            });
        }
        p.spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(4.0),
            bottom: Val::Px(0.0),
            ..default()
        })
        .with_children(|p| {
            label(p, "▲ Tren · distancia hacia adelante", 11.0, TEXT);
        });
    });
}
pub(crate) fn apply_settings(
    settings: Res<PlayerSettings>,
    opts: Option<ResMut<crate::launch::ViewerLaunchOpts>>,
    mut lights: Query<&mut DirectionalLight>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    mut fog: ResMut<crate::sky::FogState>,
    mut scale: ResMut<UiScale>,
) {
    crate::launch::set_viewing_distance_m(settings.view_distance_m);
    if let Some(mut opts) = opts {
        opts.cab_fov_deg = Some(settings.cab_fov_deg);
    }
    for mut light in &mut lights {
        light.shadow_maps_enabled =
            settings.shadows && sun.as_ref().is_none_or(|s| s.direction.y > 0.0025);
    }
    fog.enabled = settings.fog;
    scale.0 = settings.ui_scale;
}

fn apply_weather(
    mut commands: Commands,
    content: Res<ActivePlayerContent>,
    mut precipitation: ResMut<crate::precipitation::PrecipitationState>,
    rain: Query<Entity, With<crate::weather_particles::WeatherMesh>>,
) {
    precipitation.enabled = matches!(content.weather, PlayerWeather::Rain | PlayerWeather::Snow);
    precipitation.snow = content.weather == PlayerWeather::Snow;
    crate::shapes::set_scenery_snow(precipitation.snow);
    if !precipitation.enabled {
        for entity in &rain {
            commands.entity(entity).despawn();
        }
        commands.insert_resource(crate::weather_particles::WeatherParticles::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn live() -> LiveDrive {
        LiveDrive::from_scenario_path(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/chiltern_local/scenario.toml"),
        )
        .unwrap()
    }
    #[test]
    fn track_monitor_contains_real_station_signal_and_changing_distances() {
        let mut l = live();
        let events = monitor_events(&l.session);
        assert!(events.iter().any(|e| e.text.contains("Northolt Park")));
        assert!(events.iter().any(|e| e.text.starts_with('●')));
        let first = events
            .iter()
            .find(|e| e.text.starts_with('●'))
            .unwrap()
            .distance_m;
        l.session.state.pos_on_edge_m += 5.0;
        let second = monitor_events(&l.session)
            .into_iter()
            .find(|e| e.text.starts_with('●'))
            .unwrap()
            .distance_m;
        assert!((first - second - 5.0).abs() < 0.01);
    }
    #[test]
    fn playing_schedule_has_no_camera_weather_cycle() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(ViewerAppState::Playing)
            .add_plugins(PlayerUiPlugin)
            .add_systems(
                OnEnter(ViewerAppState::Playing),
                (
                    crate::scene::spawn_ground_and_lights,
                    crate::camera::spawn_camera,
                )
                    .chain()
                    .run_if(|| false),
            );
        let mut schedule = app
            .world_mut()
            .resource_mut::<bevy::ecs::schedule::Schedules>()
            .remove(OnEnter(ViewerAppState::Playing))
            .unwrap();
        schedule.initialize(app.world_mut()).unwrap();
    }

    #[test]
    fn menu_settings_preview_does_not_require_a_loaded_route() {
        let mut app = App::new();
        let settings = PlayerSettings {
            ui_scale: 1.2,
            ..default()
        };
        app.insert_resource(settings)
            .init_resource::<ActivePlayerContent>()
            .init_resource::<crate::sky::FogState>()
            .init_resource::<UiScale>()
            .add_systems(Update, apply_settings);
        app.update();
        assert_eq!(app.world().resource::<UiScale>().0, 1.2);
    }

    #[test]
    fn notebook_and_every_hud_page_report_current_service() {
        let l = live();
        let content = ActivePlayerContent::default();
        assert!(notebook_text(&l, &content, 1).contains("South Ruislip"));
        assert!(notebook_text(&l, &content, 1).contains("West Ruislip"));
        assert!(notebook_text(&l, &content, 2).contains("Paradas cumplidas: 0/3"));
        for (page, name) in ADVANCED_PAGES.iter().enumerate() {
            let text = advanced_text(&l, &content, page);
            assert!(text.starts_with(name));
            assert!(text.lines().count() > 2);
        }
    }
}
