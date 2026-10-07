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

mod launcher;
use launcher::{LibraryTab, NewGameStep, SettingsTab};

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

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EnvironmentControls;

pub struct PlayerUiPlugin;
impl Plugin for PlayerUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerUiState>()
            .init_resource::<PlayerSettings>()
            .init_resource::<PlayerLaunchMenu>()
            .init_resource::<crate::official_content::OfficialContent>()
            .init_resource::<bevy_clipboard::Clipboard>()
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
                    poll_content_download,
                    poll_menu_audits,
                    handle_buttons.in_set(EnvironmentControls),
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
    NewGame,
    Continue,
    Content,
    MissingResources,
    Pause,
    Notebook,
    Formation,
    Map,
    Advanced,
    Traction,
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
    in_start_menu: bool,
    navigation: Vec<PlayerPanel>,
    new_game_step: NewGameStep,
    new_game_advanced: bool,
    library_tab: LibraryTab,
    settings_tab: SettingsTab,
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
            in_start_menu: false,
            navigation: Vec::new(),
            new_game_step: NewGameStep::Route,
            new_game_advanced: false,
            library_tab: LibraryTab::Installed,
            settings_tab: SettingsTab::General,
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
    Content,
    Notebook,
    Advanced,
    Traction,
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
    ContentFolder(Option<usize>),
    CopyContentDiagnostics,
    Open(PlayerPanel),
    Close,
    Start,
    Exit,
    ReturnMenu,
    Save(usize),
    Load(usize),
    Resume(usize),
    SelectRoute(usize),
    NewGameStep(NewGameStep),
    NewGameAdvanced,
    LibraryTab(LibraryTab),
    SettingsTab(SettingsTab),
    Cycle(MenuField, i32),
    NoteTab(usize),
    Advanced(usize),
    SelectCar(usize),
    Car(CarOperation),
    DieselEngine(usize),
    Sander,
    Steam(openrailsrs_sim::steam::SteamCommand),
    BeginRefill,
    CancelRefill,
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
    ContentCycle(i32),
    ContentDownload,
    ContentAudit(usize),
    ContentCancel,
    ContentCatalogue,
    MissingOrigin,
    MissingUpdate,
    MissingAudit,
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
    TimeSource,
    WeatherSource,
}
#[derive(Clone, Copy, Debug)]
enum SettingField {
    QuickStations,
    StrictService,
    TrafficBrakeAssistance,
    SeatHeight,
    SeatBack,
    WipeScale,
    CabReset,
    Distance,
    Fov,
    Scale,
    Shadows,
    AutomaticCant,
    Fog,
    FogQuality,
    WeatherExecution,
    TrainEffectExecution,
    TrainMotion,
    TimeSource,
    WeatherSource,
    ManualWeather,
    Lightning,
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
        Node {
            flex_shrink: 0.0,
            ..default()
        },
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
            min_width: Val::Px(0.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
            min_height: Val::Px(32.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(FIELD),
        Name::new(format!("ui-{command:?}")),
        command,
    ))
    .with_children(|p| {
        label(p, value, 13.0, TEXT);
    })
    .id()
}
fn row(p: &mut ChildSpawnerCommands<'_>, f: impl FnOnce(&mut ChildSpawnerCommands<'_>)) {
    p.spawn(Node {
        flex_shrink: 0.0,
        min_width: Val::Px(0.0),
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
    ui.in_start_menu = true;
    ui.navigation.clear();
    // Deterministic screenshots use the same screen builders as normal play.
    // This override is ignored outside an explicitly armed menu capture.
    if crate::capture::capture_enabled()
        && std::env::var("OPENRAILSRS_SCREENSHOT_MENU").is_ok_and(|v| v == "1")
        && let Ok(page) = std::env::var("OPENRAILSRS_SCREENSHOT_MENU_PAGE")
    {
        launcher::capture_page(&mut ui, &page);
    }
    ui.rebuild = true;
}
fn spawn_toolbar(
    mut commands: Commands,
    mut ui: ResMut<PlayerUiState>,
    live: Option<Res<LiveDrive>>,
    settings: Res<PlayerSettings>,
) {
    ui.in_start_menu = false;
    ui.navigation.clear();
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
                row_gap: Val::Px(6.0),
                max_width: Val::Percent(90.0),
                flex_wrap: FlexWrap::Wrap,
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
                (PlayerPanel::Traction, PlayerAction::TractionControls),
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
                            PlayerPanel::Traction => "Tracción",
                            PlayerPanel::Map => "Mapa",
                            _ => "Ajustes",
                        }
                    ),
                    UiCommand::Open(panel),
                );
            }
            button(
                p,
                "km/h ↔ mph",
                UiCommand::Setting(SettingField::Units, 0.0),
            );
        });
    commands.spawn((
        crate::environment::EnvironmentBadge,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(TEXT),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(12.0),
            bottom: Val::Px(12.0),
            max_width: Val::Px(430.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.05, 0.08, 0.85)),
        UiTargetCamera(camera),
        ZIndex(800),
        Visibility::Hidden,
    ));
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
    if ui.panel == panel {
        if panel != PlayerPanel::Menu {
            close_panel(ui, live);
        }
        return;
    }
    if ui.panel == PlayerPanel::None
        || (panel == PlayerPanel::Traction && ui.panel == PlayerPanel::Pause)
    {
        ui.pause_before = live.as_ref().is_some_and(|l| l.paused);
    }
    if panel == PlayerPanel::Menu {
        ui.navigation.clear();
    } else if let Some(index) = ui.navigation.iter().position(|p| *p == panel) {
        ui.navigation.truncate(index);
    } else if ui.panel != PlayerPanel::None {
        ui.navigation.push(ui.panel);
    }
    ui.panel = panel;
    ui.rebuild = true;
    ui.awaiting_key = None;
    ui.notice.clear();
    if let Some(l) = live {
        l.paused = if panel == PlayerPanel::Traction {
            ui.pause_before
        } else {
            true
        };
    }
}
fn close_panel(ui: &mut PlayerUiState, live: &mut Option<ResMut<LiveDrive>>) {
    let was_pause = ui.panel == PlayerPanel::Pause;
    ui.panel = if let Some(parent) = ui.navigation.pop() {
        parent
    } else if ui.in_start_menu {
        PlayerPanel::Menu
    } else {
        PlayerPanel::None
    };
    ui.awaiting_key = None;
    ui.notice.clear();
    ui.rebuild = true;
    if let Some(l) = live {
        l.paused = if ui.panel == PlayerPanel::None {
            if was_pause { false } else { ui.pause_before }
        } else if ui.panel == PlayerPanel::Traction {
            ui.pause_before
        } else {
            true
        };
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
        (PlayerAction::TractionControls, PlayerPanel::Traction),
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
    if settings.just_pressed(&keys, PlayerAction::SpeedUnits) {
        settings.toggle_speed_units();
        ui.rebuild = true;
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
    mut downloads: ResMut<crate::official_content::OfficialContent>,
    mut clipboard: Option<ResMut<bevy_clipboard::Clipboard>>,
    camera: CameraForSave,
    mut exit: MessageWriter<AppExit>,
    mouse: Res<ButtonInput<MouseButton>>,
    cab: Res<crate::cab_cvf::CabCvfState>,
    refills: Option<Res<crate::world_operations::RefillTargets>>,
) {
    for (interaction, command) in &buttons {
        if *interaction != Interaction::Pressed || !mouse.just_pressed(MouseButton::Left) {
            continue;
        }
        let result:Result<(),String>=match command {
            UiCommand::ContentCycle(delta)=>{downloads.cycle(*delta);Ok(())},
            UiCommand::ContentFolder(index)=>{
                let path=match index {Some(i)=>downloads.installed.get(*i).cloned(),None=>menu.current().map(|s|s.scenery_root.clone().unwrap_or_else(||s.route_dir.clone()))};
                path.ok_or("No hay una carpeta seleccionada".into()).and_then(|p|crate::official_content::open_folder(&p))
            },
            UiCommand::CopyContentDiagnostics=>clipboard.as_mut().ok_or("Portapapeles no disponible".into()).and_then(|c|c.set_text(crate::official_content::diagnostic_text(&menu)).map_err(|e|format!("No se pudo copiar: {e}"))).map(|()|{ui.notice="Diagnóstico copiado: incluye los faltantes y sus ubicaciones".into();}),
            UiCommand::ContentDownload=>downloads.start(),
            UiCommand::ContentAudit(index)=>downloads.reaudit(*index),
            UiCommand::ContentCancel=>downloads.cancel(),
            UiCommand::ContentCatalogue=>crate::official_content::open_catalogue(),
            UiCommand::MissingOrigin=>crate::official_content::missing_source(&menu).ok_or("No hay un origen verificado".to_string()).and_then(|s|crate::official_content::open_source_url(&s.page)),
            UiCommand::MissingAudit=>menu.reaudit_selected().map(|message|{ui.notice=message;}),
            UiCommand::MissingUpdate=>crate::official_content::missing_source(&menu).map(|s|s.package_id).and_then(|id|downloads.packages.iter().position(|p|p.id()==id)).ok_or("No hay una descarga automática verificada para este origen".to_string()).and_then(|index|{downloads.selected=index;downloads.start().map(|()|open_panel(&mut ui,&mut live,PlayerPanel::Content))}),
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
            UiCommand::SelectRoute(index) => {
                let delta = *index as i32 - menu.route as i32;
                menu.cycle_route(delta);
                ui.new_game_step = NewGameStep::Route;
                ui.new_game_advanced = false;
                open_panel(&mut ui, &mut live, PlayerPanel::NewGame);
                Ok(())
            },
            UiCommand::NewGameStep(step) => {
                ui.new_game_step = *step;
                ui.new_game_advanced = false;
                ui.notice.clear();
                Ok(())
            },
            UiCommand::NewGameAdvanced => {
                ui.new_game_advanced = !ui.new_game_advanced;
                Ok(())
            },
            UiCommand::LibraryTab(tab) => {
                ui.library_tab = *tab;
                Ok(())
            },
            UiCommand::SettingsTab(tab) => {
                ui.settings_tab = *tab;
                ui.awaiting_key = None;
                Ok(())
            },
            UiCommand::Save(slot)=>if let Some(l)=live.as_mut(){
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
                    .map(|()|{ui.notice=format!("Partida guardada en la ranura {} · {:.0} m · {:.1} {}",slot+1,l.session.state.odometer_m,settings.display_speed_mps(l.session.velocity_mps()),settings.speed_unit_label());})
            }else{Err("No hay una partida activa".into())},
            UiCommand::Load(slot)=>if let Some(l)=live.as_mut(){
                SavedGame::read(&slot_path(*slot)).and_then(|saved|{let weather=saved.weather;let environment=saved.environment.unwrap_or(crate::environment::EnvironmentSelection { manual_weather: weather, ..default() });let camera=saved.restore(l)?;content.weather=weather;content.environment=environment;commands.insert_resource(PendingSavedCamera(camera));Ok(())})
                    .map(|()|{ui.notice="Partida restaurada y pausada; Continuar reanuda la conducción".into();})
            }else{Err("No hay una partida activa".into())},
            UiCommand::Cycle(field,delta)=>{match field {
                MenuField::Route=>menu.cycle_route(*delta),MenuField::Service=>menu.cycle_service(*delta),
                MenuField::Consist=>menu.cycle_consist(*delta),
                MenuField::Path=>menu.path=cycle(menu.path,menu.paths.len()+1,*delta),
                MenuField::Time=>menu.start_time_s=(menu.start_time_s+f64::from(*delta)*900.0).rem_euclid(86400.0),
                MenuField::Season=>menu.season=cycle(menu.season,4,*delta),
                MenuField::Weather=>{menu.weather=PlayerWeather::ALL[cycle(PlayerWeather::ALL.iter().position(|w|*w==menu.weather).unwrap_or(0),PlayerWeather::ALL.len(),*delta)];menu.environment.manual_weather=menu.weather;},
                MenuField::TimeSource=>menu.environment.time=menu.environment.time.next(),
                MenuField::WeatherSource=>menu.environment.weather=menu.environment.weather.next(),
            }settings.environment=menu.environment;ui.notice.clear();Ok(())},
            UiCommand::NoteTab(tab)=>{ui.notebook_tab= *tab;Ok(())},UiCommand::Advanced(page)=>{ui.advanced_page= *page;Ok(())},
            UiCommand::SelectCar(car)=>{ui.selected_car= *car;Ok(())},
            UiCommand::Car(action)=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.operate_car(ui.selected_car,*action)).map(|()|{ui.notice="Operación aplicada a la formación".into();}),
            UiCommand::DieselEngine(vehicle)=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.toggle_diesel_engine(*vehicle)),
            UiCommand::Sander=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.toggle_sander()),
            UiCommand::Steam(command)=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.steam_command(*command)),
            UiCommand::BeginRefill=>live.as_mut().ok_or("No hay partida".into()).and_then(|l| {
                let target = refills.as_ref().and_then(|r|r.0.first()).ok_or("Alineá la toma de agua, carbón o diésel con un abastecedor compatible; no hay uno al alcance".to_string())?;
                l.session.begin_refill(target.station.clone(), target.vehicle, target.distance_m, target.width_m)
            }).map(|()| {ui.notice="Abastecimiento iniciado; Cancelar desconecta la toma".into();}),
            UiCommand::CancelRefill=>live.as_mut().ok_or("No hay partida".into()).map(|l|l.session.cancel_refill()),
            UiCommand::SecureTail=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|{
                for i in ui.selected_car+1..l.session.formation.coupled_count {
                    if !l.session.formation.cars[i].handbrake {l.session.operate_car(i,CarOperation::Handbrake)?;}
                }ui.notice="Frenos de mano aplicados a la sección posterior".into();Ok(())}),
            UiCommand::Uncouple=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.uncouple_after(ui.selected_car)).map(|()|{ui.notice="Sección posterior desacoplada y estacionada".into();}),
            UiCommand::Recouple=>live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.recouple()).map(|()|{ui.notice="Enganche acoplado; reconectá la manguera, abrí las llaves y soltá los frenos de mano".into();}),
            UiCommand::MapPick(selection)=>{ui.map_selection=Some(selection.clone());Ok(())},
            UiCommand::DispatchSignal(aspect)=>if let Some(MapSelection::Signal(id))=ui.map_selection.clone(){live.as_mut().ok_or("No hay partida".into()).and_then(|l|l.session.dispatch_signal(&id,*aspect)).map(|()|{ui.notice="Orden de señal aplicada".into();})}else{Err("Seleccioná una señal en el mapa".into())},
            UiCommand::DispatchSwitch=>if let Some(MapSelection::Switch(id))=ui.map_selection.clone(){live.as_mut().ok_or("No hay partida".into()).and_then(|l|{let l=&mut **l;l.traffic.dispatch_switch(&mut l.session,&id)}).map(|()|{ui.notice="Cambio e itinerario actualizados".into();})}else{Err("Seleccioná un cambio en el mapa".into())},
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
                SettingField::StrictService=>settings.strict_service= !settings.strict_service,
                SettingField::TrafficBrakeAssistance=>settings.traffic_brake_assistance= !settings.traffic_brake_assistance,
                SettingField::Distance=>settings.view_distance_m=(settings.view_distance_m+step).clamp(500.0,4000.0),
                SettingField::Fov=>settings.cab_fov_deg=(settings.cab_fov_deg+step).clamp(35.0,90.0),
                SettingField::Scale=>settings.ui_scale=(settings.ui_scale+step).clamp(0.8,1.5),
                SettingField::AutomaticCant=>settings.automatic_cant= !settings.automatic_cant,
                SettingField::Shadows=>settings.shadows= !settings.shadows,SettingField::Fog=>settings.fog= !settings.fog,
                SettingField::Units=>settings.toggle_speed_units(),
                SettingField::FogQuality=>settings.fog_quality=settings.fog_quality.next(),
                SettingField::WeatherExecution=>settings.weather_execution=settings.weather_execution.next(),
                SettingField::TrainEffectExecution=>settings.train_effect_execution=settings.train_effect_execution.next(),
                SettingField::TrainMotion=>settings.train_motion=settings.train_motion.next(),
                SettingField::TimeSource=>{content.environment.time=if live.is_some(){content.environment.time.next()}else{menu.environment.time.next()};menu.environment.time=content.environment.time;settings.environment.time=content.environment.time;},
                SettingField::WeatherSource=>{content.environment.weather=if live.is_some(){content.environment.weather.next()}else{menu.environment.weather.next()};menu.environment.weather=content.environment.weather;settings.environment.weather=content.environment.weather;},
                SettingField::ManualWeather=>{if live.is_none(){content.environment=menu.environment;}let selected=PlayerWeather::ALL[cycle(PlayerWeather::ALL.iter().position(|w|*w==content.environment.manual_weather).unwrap_or(0),PlayerWeather::ALL.len(),1)];content.environment.manual_weather=selected;content.environment.weather=crate::environment::EnvironmentSource::Manual;menu.weather=selected;menu.environment=content.environment;settings.environment=content.environment;},
                SettingField::Lightning=>settings.lightning= !settings.lightning,
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
    downloads: Res<crate::official_content::OfficialContent>,
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
                left: Val::Percent(6.0),
                right: Val::Percent(6.0),
                top: Val::Percent(6.0),
                bottom: Val::Percent(6.0),
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
                        PlayerPanel::Menu => "openrailsrs",
                        PlayerPanel::NewGame => "Nueva partida",
                        PlayerPanel::Continue => "Continuar",
                        PlayerPanel::Content => "Biblioteca",
                        PlayerPanel::MissingResources => "Detalle de recursos",
                        PlayerPanel::Pause => "PARTIDA EN PAUSA",
                        PlayerPanel::Notebook => "LIBRETA DEL SERVICIO",
                        PlayerPanel::Formation => "OPERACIONES DE LA FORMACIÓN",
                        PlayerPanel::Map => "MAPA Y DESPACHADOR",
                        PlayerPanel::Advanced => "HUD AVANZADO",
                        PlayerPanel::Traction => "VAPOR Y DIÉSEL · CONTROLES",
                        PlayerPanel::Settings => "AJUSTES",
                        _ => "AYUDA Y CONTROLES",
                    },
                    22.0,
                    ACCENT,
                );
                if panel != PlayerPanel::Menu {
                    button(
                        p,
                        if ui.in_start_menu { "Volver · Esc" } else { "Cerrar · Esc" },
                        UiCommand::Close,
                    );
                }
            });
            match panel {
                PlayerPanel::NewGame => launcher::build_steps(p, ui.new_game_step),
                PlayerPanel::Content => launcher::build_library_tabs(p, ui.library_tab),
                PlayerPanel::Settings => launcher::build_settings_tabs(p, ui.settings_tab),
                _ => {}
            }
            p.spawn((
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    overflow: Overflow::scroll_y(),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    ..default()
                },
                ScrollPosition::default(),
                PlayerScroll,
            ))
            .with_children(|p| match panel {
                PlayerPanel::Menu => launcher::build_home(p),
                PlayerPanel::NewGame => launcher::build_new_game(p, &ui, &menu),
                PlayerPanel::Continue => launcher::build_continue(p),
                PlayerPanel::Content => launcher::build_library(p, ui.library_tab, &downloads, &menu),
                PlayerPanel::MissingResources => {
                    row(p, |p| {
                        button(p, "Reauditar esta formación", UiCommand::MissingAudit);
                    });
                    label(p, menu.consist_status(), 13.0, TEXT);
                    content_diagnostics(p, &menu, true);
                }
                PlayerPanel::Pause => {
                    if let Some(l) = live.as_ref() {
                        label(
                            p,
                            format!(
                                "{}\n{} · {:.0} m · {:.1} {}",
                                l.session.scenario_name,
                                clock(l.clock_time_s()),
                                l.session.state.odometer_m,
                                settings.display_speed_mps(l.session.velocity_mps()),
                                settings.speed_unit_label()
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
                PlayerPanel::Settings => build_settings(
                    p,
                    ui.settings_tab,
                    &settings,
                    if live.is_some() {
                        content.environment
                    } else {
                        menu.environment
                    },
                    cab.cvf_path.as_deref(),
                ),
                PlayerPanel::Traction => {
                    if let Some(l) = live.as_ref() { build_traction(p, &l.session); }
                }
                PlayerPanel::Help => {
                    dynamic(p, DynamicText::Help, 14.0);
                }
                PlayerPanel::None => {}
            });
            match panel {
                PlayerPanel::NewGame => launcher::build_launch_footer(p, &ui, &menu),
                PlayerPanel::Menu => { button(p, "Salir", UiCommand::Exit); }
                PlayerPanel::Settings => {
                    row(p, |p| {
                        button(p, "Guardar ajustes", UiCommand::SaveSettings);
                        if ui.settings_tab == SettingsTab::Controls {
                            button(p, "Restablecer controles", UiCommand::DefaultKeys);
                        }
                    });
                }
                _ => {}
            }
            let status = label(
                p,
                if ui.notice.is_empty() {
                    if panel == PlayerPanel::MissingResources {
                        "Después de copiar archivos, pulsá Reauditar esta formación para actualizar el diagnóstico."
                    } else if ui.in_start_menu {
                        ""
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
            width: Val::Px(150.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|p| {
            label(p, name, 14.0, MUTED);
        });
        button(p, "‹", UiCommand::Cycle(field, -1));
        p.spawn(Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            flex_basis: Val::Px(0.0),
            ..default()
        })
        .with_children(|p| {
            label(p, value, 15.0, TEXT);
        });
        button(p, "›", UiCommand::Cycle(field, 1));
    });
}
fn content_diagnostics(p: &mut ChildSpawnerCommands<'_>, menu: &PlayerLaunchMenu, full: bool) {
    row(p, |p| {
        button(
            p,
            "Abrir carpeta del escenario",
            UiCommand::ContentFolder(None),
        );
        button(p, "Copiar diagnóstico", UiCommand::CopyContentDiagnostics);
        if !full && !menu.consists.is_empty() {
            button(p, "Revisar archivos otra vez", UiCommand::MissingAudit);
        }
    });
    if let Some(service) = menu.current() {
        label(
            p,
            format!(
                "Escenario: {}\nActividad / servicio: {}",
                service
                    .scenery_root
                    .as_deref()
                    .unwrap_or(&service.route_dir)
                    .display(),
                service.source.display()
            ),
            12.0,
            MUTED,
        );
    }
    let Some(consist) = menu.consists.get(menu.consist) else {
        return;
    };
    label(p, format!("Formación: {}", consist.display()), 12.0, MUTED);
    let Some(audit) = menu.consist_audits.get(consist) else {
        return;
    };
    let required = audit
        .missing_resources
        .iter()
        .filter(|r| r.required)
        .map(|r| &r.destinations)
        .collect::<std::collections::HashSet<_>>()
        .len();
    if required > 0 {
        label(
            p,
            format!("ARCHIVOS FALTANTES ({required}) · necesarios para iniciar"),
            13.0,
            CAUTION,
        );
    }
    for missing in audit
        .missing_resources
        .iter()
        .filter(|r| full || r.required)
        .take(if full { usize::MAX } else { 2 })
    {
        label(
            p,
            missing.guidance(),
            12.0,
            if missing.required { CAUTION } else { MUTED },
        );
    }
    if full && !audit.errors.is_empty() {
        label(
            p,
            format!("DIAGNÓSTICO DE LA FORMACIÓN\n{}", audit.errors.join("\n")),
            12.0,
            CAUTION,
        );
    }
    if full && !audit.warnings.is_empty() {
        label(
            p,
            format!("AVISOS DE COMPATIBILIDAD\n{}", audit.warnings.join("\n")),
            12.0,
            MUTED,
        );
    }
    if !full && !audit.missing_resources.is_empty() {
        button(
            p,
            "Ver todos los faltantes y sus ubicaciones",
            UiCommand::Open(PlayerPanel::MissingResources),
        );
    }
    if required == 0 {
        return;
    }
    if let Some(source) = crate::official_content::missing_source(menu) {
        label(p, source.title, 13.0, CAUTION);
        label(p, source.note, 12.0, MUTED);
        row(p, |p| {
            button(
                p,
                "Buscar en el repositorio original",
                UiCommand::MissingOrigin,
            );
            button(
                p,
                "Actualizar desde el repositorio original",
                UiCommand::MissingUpdate,
            );
        });
    } else {
        label(
            p,
            "No se identificó un repositorio original para estos recursos. Colocá los archivos originales en las ubicaciones indicadas, conservando sus nombres y carpetas, y pulsá Reauditar esta formación en el detalle de faltantes.",
            12.0,
            MUTED,
        );
    }
}

fn poll_content_download(
    mut downloads: ResMut<crate::official_content::OfficialContent>,
    mut menu: ResMut<PlayerLaunchMenu>,
    mut ui: ResMut<PlayerUiState>,
) {
    let was_busy = downloads.busy();
    let ready = downloads.poll();
    if ready {
        menu.refresh_installed_content();
        ui.rebuild = true;
    }
    if ui.panel == PlayerPanel::Content && (ready || was_busy != downloads.busy()) {
        ui.rebuild = true;
    }
}

fn poll_menu_audits(mut menu: ResMut<PlayerLaunchMenu>, mut ui: ResMut<PlayerUiState>) {
    let reauditing = menu.reaudit_pending();
    let pending = menu.selected_audit_pending();
    let status = menu.status.clone();
    if menu.poll_consist_audits() {
        if reauditing && !menu.reaudit_pending() {
            ui.notice = menu.status.clone();
        }
        if (pending != menu.selected_audit_pending() || status != menu.status || reauditing)
            && matches!(
                ui.panel,
                PlayerPanel::NewGame | PlayerPanel::MissingResources | PlayerPanel::Content
            )
        {
            ui.rebuild = true;
        }
    }
}

fn build_traction(p: &mut ChildSpawnerCommands<'_>, s: &LiveDriveSession) {
    button(p, "Arenado: activar / desactivar", UiCommand::Sander);
    label(
        p,
        "Podés usar estos controles en marcha o con la partida pausada. B cierra el panel; F8 muestra el detalle de tracción.",
        14.0,
        MUTED,
    );
    dynamic(p, DynamicText::Traction, 14.0);
    if s.state.boiler_state.is_some() || !s.physics.diesel.cars.is_empty() {
        row(p, |p| {
            button(
                p,
                "Abastecer en la toma más cercana",
                UiCommand::BeginRefill,
            );
            button(p, "Cancelar abastecimiento", UiCommand::CancelRefill);
        });
        label(
            p,
            "Detené el tren con la toma alineada y el regulador cerrado. El depósito se llena mientras la partida avanza; la pausa detiene también el abastecimiento.",
            14.0,
            MUTED,
        );
    }
    if s.state.boiler_state.is_some() {
        use openrailsrs_sim::steam::SteamCommand as S;
        row(p, |p| {
            button(
                p,
                "Fogonero automático / manual",
                UiCommand::Steam(S::AutomaticFireman),
            );
            button(p, "Corte −", UiCommand::Steam(S::Cutoff(-0.05)));
            button(p, "Corte +", UiCommand::Steam(S::Cutoff(0.05)));
            button(p, "Cilindros: purgas", UiCommand::Steam(S::CylinderCocks));
        });
        row(p, |p| {
            button(p, "Pala −", UiCommand::Steam(S::Firing(-0.1)));
            button(p, "Pala +", UiCommand::Steam(S::Firing(0.1)));
            button(p, "Tiro −", UiCommand::Steam(S::Damper(-0.1)));
            button(p, "Tiro +", UiCommand::Steam(S::Damper(0.1)));
            button(p, "Inyector 1", UiCommand::Steam(S::Injector1));
            button(p, "Inyector 2", UiCommand::Steam(S::Injector2));
            button(p, "Soplador", UiCommand::Steam(S::Blower));
        });
    }
    row(p, |p| {
        for car in &s.physics.diesel.cars {
            button(
                p,
                format!("Motor {}: arrancar / parar", car.vehicle + 1),
                UiCommand::DieselEngine(car.vehicle),
            );
        }
    });
    if s.state.boiler_state.is_none() && s.physics.diesel.cars.is_empty() {
        label(
            p,
            "Esta formación no tiene controles de vapor ni motor diésel declarado.",
            14.0,
            MUTED,
        );
    }
}

fn traction_text(s: &LiveDriveSession, settings: &PlayerSettings) -> String {
    let mut out = String::new();
    if let Some(rail) = &s.state.rail_adhesion {
        out += &format!(
            "Vía: {} · agarre del clima {:.0}%\nArenado: {}\n\n",
            rail.weather.label(),
            rail.weather_factor * 100.,
            rail.sander_status()
        );
        if let Some(config) = &s.physics.rail_adhesion {
            for (i, (car, v)) in rail
                .cars
                .iter()
                .zip(&config.vehicles)
                .enumerate()
                .filter(|(_, (_, v))| v.powered)
            {
                out += &format!(
                    "Vehículo {} · arena {:.2} / {:.2} L\nRuedas {:.1} {} · {} · factor {:.2}\nEsfuerzo pedido {:.1} kN · transmitido {:.1} kN\n\n",
                    i + 1,
                    car.sand_m3 * 1000.,
                    v.profile.sander.capacity_m3 * 1000.,
                    settings.display_speed_mps(car.wheel_speed_mps.abs()),
                    settings.speed_unit_label(),
                    if car.slipping { "PATINA" } else { "Con agarre" },
                    car.factor,
                    car.requested_force_n / 1000.,
                    car.rail_force_n / 1000.
                );
            }
        }
    }
    if let Some(op) = &s.refilling {
        out += &format!(
            "Abastecedor {} · vehículo {} · {} · {:.1} kg transferidos\n\n",
            op.station.id,
            op.vehicle + 1,
            if op.returning {
                "desconectando"
            } else if op.opening < 1. {
                "conectando"
            } else {
                "llenando"
            },
            op.delivered_kg
        );
    }
    if let Some(b) = &s.state.boiler_state {
        out += &format!(
            "{} · {:.2} bar · agua de caldera {:.1}%\nTénder: {:.1} L de agua · {:.1} kg de carbón\nFuego {:.1} kg · producción {:.2} kg/s · consumo {:.2} kg/s\nFogonero {} · corte {:.0}% · pala {:.0}% · tiro {:.0}%\nInyector 1 {} · inyector 2 {} · soplador {} · purgas {}\nInyección {:.2} kg/s · carbón quemado {:.3} kg/s\n\n",
            b.status(),
            b.pressure_bar,
            b.water_kg / b.initial_water_kg * 100.,
            b.tender_water_kg,
            b.coal_kg,
            b.fire_mass_kg,
            b.evaporation_kg_s,
            b.steam_usage_kg_s,
            if b.controls.automatic_fireman {
                "automático"
            } else {
                "manual"
            },
            b.controls.cutoff * 100.,
            b.controls.firing * 100.,
            b.controls.damper * 100.,
            yes(b.controls.injector1),
            yes(b.controls.injector2),
            yes(b.controls.blower),
            yes(b.controls.cylinder_cocks),
            b.injection_kg_s,
            b.coal_burn_kg_s
        );
    }
    for (car, state) in s.physics.diesel.cars.iter().zip(&s.state.diesel.cars) {
        out += &format!(
            "Vehículo {} · motor {} · {:.0} rpm\nCombustible {:.2} / {:.1} L · consumo {:.2} L/h\nBatería {} · tracción {}\n\n",
            car.vehicle + 1,
            state.phase.label(),
            state.rpm,
            state.fuel_l,
            car.params.capacity_l,
            state.flow_lps * 3600.,
            yes(car.battery),
            yes(car.connected && s.state.diesel.power_available(car.vehicle))
        );
    }
    out
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
    if s.physics
        .diesel
        .cars
        .iter()
        .any(|c| c.vehicle == ui.selected_car)
    {
        button(
            p,
            "Arrancar / detener motor diésel",
            UiCommand::DieselEngine(ui.selected_car),
        );
    }
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
    tab: SettingsTab,
    s: &PlayerSettings,
    environment: crate::environment::EnvironmentSelection,
    cab_path: Option<&std::path::Path>,
) {
    match tab {
        SettingsTab::General => {
            button(
                p,
                format!("Velocidad: {}", s.speed_unit_label()),
                UiCommand::Setting(SettingField::Units, 0.0),
            );
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
            button(
                p,
                format!(
                    "Terminar servicio al omitir una parada: {}",
                    yes(s.strict_service)
                ),
                UiCommand::Setting(SettingField::StrictService, 0.0),
            );
            label(
                p,
                "Desactivado: seguís conduciendo, con 1000 puntos de penalización por parada omitida.",
                12.0,
                MUTED,
            );
            button(
                p,
                format!(
                    "Frenado asistido ante tráfico: {}",
                    yes(s.traffic_brake_assistance)
                ),
                UiCommand::Setting(SettingField::TrafficBrakeAssistance, 0.0),
            );
            label(
                p,
                "Desactivado: regulador y frenos manuales. Respetá las señales; el TCS original, si está activo, sigue interviniendo.",
                12.0,
                MUTED,
            );
        }
        SettingsTab::Graphics => {
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
            button(
                p,
                format!(
                    "Peralte automático: {} · próxima partida",
                    yes(s.automatic_cant)
                ),
                UiCommand::Setting(SettingField::AutomaticCant, 0.),
            );
            label(
                p,
                "Guardá los ajustes y volvé a iniciar la partida para cambiar el peralte. La vía, el tren y la cabina comparten el perfil; los cambios y el peralte ya escrito por el autor se conservan.",
                12.,
                MUTED,
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
            });
            button(
                p,
                format!("Movimiento del tren: {}", s.train_motion.label()),
                UiCommand::Setting(SettingField::TrainMotion, 0.0),
            );
            label(
                p,
                "Suspensión de carrocería y cabina. Se puede apagar; las ruedas y bogies conservan su posición sobre la vía.",
                12.0,
                MUTED,
            );
            button(
                p,
                format!("Modelo de niebla: {}", s.fog_quality.label()),
                UiCommand::Setting(SettingField::FogQuality, 0.0),
            );
            label(
                p,
                "Automática muestra haces de faros con niebla en GPU. Atmosférica conserva la visibilidad reducida con menor costo. La niebla volumétrica usa absorción, dispersión y sombras.",
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
                format!("Humo y vapor: {}", s.train_effect_execution.label()),
                UiCommand::Setting(SettingField::TrainEffectExecution, 0.0),
            );
            label(
                p,
                "Se aplica durante la partida. Automático usa GPU y reduce el detalle bajo carga. CPU y Mixto conservan las mismas salidas de humo y vapor.",
                12.0,
                MUTED,
            );
        }
        SettingsTab::Environment => {
            label(p, "HORA Y CLIMA DEL LUGAR", 13.0, MUTED);
            button(
                p,
                format!("Hora visual: {}", environment.time.label()),
                UiCommand::Setting(SettingField::TimeSource, 0.0),
            );
            button(
                p,
                format!("Origen del clima: {}", environment.weather.label()),
                UiCommand::Setting(SettingField::WeatherSource, 0.0),
            );
            button(
                p,
                format!("Elegir clima: {}", environment.manual_weather.label()),
                UiCommand::Setting(SettingField::ManualWeather, 0.0),
            );
            button(
                p,
                format!("Rayos y destellos: {}", yes(s.lightning)),
                UiCommand::Setting(SettingField::Lightning, 0.0),
            );
            label(
                p,
                "Elegir un clima vuelve al modo manual. La hora real usa la fecha y zona de la ruta; el horario del servicio sigue separado. Open-Meteo requiere conexión (datos estimados).",
                12.0,
                MUTED,
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
        }
        SettingsTab::Audio => {
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
        }
        SettingsTab::Controls => {
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
        }
    }
}
fn save_label(slot: usize) -> String {
    match SavedGame::read(&slot_path(slot)) {
        Ok(s) => saved_game_label(&s),
        Err(_) => "Ranura vacía o partida no válida".into(),
    }
}
fn saved_game_label(s: &SavedGame) -> String {
    format!(
        "{} · {} · {:.0} m",
        s.session
            .gameplay
            .stop_targets
            .get(s.session.gameplay.next_stop_idx)
            .map(|s| s.name.as_str())
            .unwrap_or(&s.session.gameplay.destination_node),
        clock(s.start_clock_s + s.session.state.time_s()),
        s.session.state.odometer_m
    )
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
    particle_effects: (
        Res<crate::weather_particles::WeatherParticles>,
        Option<Res<crate::train_effects::TrainEffects>>,
    ),
    audio: Res<crate::native_audio::NativeAudio>,
    environment: Res<crate::environment::LiveEnvironment>,
    tiles: Res<crate::world_tile_index::WorldTileEntityIndex>,
    sources: (
        Option<Res<crate::shapes::RouteAssets>>,
        Res<crate::official_content::OfficialContent>,
    ),
    track_cache: Option<Res<crate::track_position::TrackPositionResolverCache>>,
    mut texts: Query<(&DynamicText, &mut Text)>,
) {
    let (assets, downloads) = sources;
    let (weather_particles, train_effects) = particle_effects;
    *elapsed += time.delta_secs();
    if *elapsed < 0.2 && !ui.is_changed() {
        return;
    }
    *elapsed = 0.0;
    for (kind, mut text) in &mut texts {
        let value = match kind {
            DynamicText::Content => downloads.status.clone(),
            DynamicText::Status => ui.notice.clone(),
            DynamicText::Help => help_text(&settings),
            _ => {
                let Some(l) = live.as_ref() else { continue };
                let s = &l.session;
                match kind {
                    DynamicText::Notebook => notebook_text(l, &content, ui.notebook_tab, &settings),
                    DynamicText::Traction => traction_text(s, &settings),
                    DynamicText::Advanced => {
                        let mut text = advanced_text(l, &content, ui.advanced_page, &settings);
                        if ui.advanced_page == 2 {
                            if let (Some(assets), Some(track_cache)) =
                                (assets.as_ref(), track_cache.as_ref())
                                && let Some(tdb) = assets.track_db()
                                && let Some(edge) = s.current_edge_id()
                                && let Some((radius, roll, gauge)) = track_cache
                                    .resolver(tdb, Some(assets.tsection()))
                                    .curve_on_graph_edge(&s.graph, edge, s.pos_on_edge_m())
                            {
                                let gauge =
                                    gauge.filter(|g| g.is_finite() && *g > 0.).unwrap_or(1.435);
                                let cant = gauge * roll.sin().abs();
                                let comfort = s
                                    .curve_parameters
                                    .iter()
                                    .take(s.formation.coupled_count)
                                    .filter_map(|p| {
                                        p.evaluate(radius, cant, s.velocity_mps(), gauge)
                                    })
                                    .min_by(|a, b| {
                                        a.comfortable_speed_mps.total_cmp(&b.comfortable_speed_mps)
                                    });
                                if let Some(c) = comfort {
                                    text += &format!(
                                        "\n\nCurva nativa: radio {:.0} m · peralte {:.0} mm\nTrocha de vía {:.3} m · confort de la formación {:.1} {}\nAceleración lateral {:.3} m/s² · {}",
                                        radius,
                                        cant * 1000.,
                                        gauge,
                                        settings.display_speed_mps(c.comfortable_speed_mps),
                                        settings.speed_unit_label(),
                                        c.lateral_acceleration_mps2,
                                        if s.velocity_mps().abs() > c.comfortable_speed_mps {
                                            "Exceso de confort en curva"
                                        } else {
                                            "Dentro del confort en curva"
                                        }
                                    );
                                }
                            } else {
                                text += "\n\nSin curvatura nativa disponible en este tramo";
                            }
                        }
                        if ui.advanced_page == 8 {
                            text = environment.hud_text(&content, l.clock_time_s());
                            text += &format!(
                                "\nReloj del servicio: {} · estación del año {}\nF10: hora, clima manual/actual y rayos. La simulación puede pausarse sin detener el reloj real.",
                                clock(l.clock_time_s()),
                                l.season
                            );
                        }
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
                            if let Some(effects) = train_effects.as_ref() {
                                text += &format!("\n{}", effects.hud_text());
                            }
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
                            "Jugador: {} · {:.1} {} · {} tramos ocupados",
                            s.current_edge_id().unwrap_or("—"),
                            settings.display_speed_mps(s.velocity_mps()),
                            settings.speed_unit_label(),
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
fn notebook_text(
    l: &LiveDrive,
    content: &ActivePlayerContent,
    tab: usize,
    settings: &PlayerSettings,
) -> String {
    let s = &l.session;
    let g = &s.gameplay;
    match tab {
        0 => format!(
            "{}\n\n{}\n\nOBJETIVOS\nCompletar {}/{} paradas y llegar a {}.\nDetenerse a menos de 10 m del punto y por debajo de {:.2} {}.\nAbrir puertas, completar el embarque y cerrarlas para salir.\nRespetar señales y límites. La tracción se corta con puertas abiertas.\n\nPROCEDIMIENTO\n{}\n\n{}",
            s.scenario_name,
            content.description,
            g.passed_stops.len(),
            g.stop_targets.len(),
            g.destination,
            settings.display_speed_mps(0.1),
            settings.speed_unit_label(),
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
                let missed = g.missed_stops.iter().any(|r| r.node == stop.node_id);
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
                            (
                                "—".into(),
                                "—".into(),
                                if missed {
                                    "OMITIDA".into()
                                } else {
                                    "—".into()
                                },
                            )
                        }
                    });
                out += &format!(
                    "{} {:20} {}     {}     {:11} {:11} {}\n",
                    if i == g.next_stop_idx {
                        "▶"
                    } else if missed {
                        "✗"
                    } else if result.is_some() {
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
                "EVALUACIÓN · {}\n\nParadas cumplidas: {}/{}\nPenalización total: {:.1}\nPasajeros a bordo: {}\nDistancia recorrida: {:.0} m\nEnergía: {:.2} kWh · combustible: {:.2} kg\n",
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
                    "\n{}: error {:.1} m · llegada {:.2} {} · demora {:+.0} s · parada {:.0} s",
                    r.name,
                    r.position_error_m,
                    settings.display_speed_mps(r.arrival_speed_mps),
                    settings.speed_unit_label(),
                    r.delay_s,
                    r.dwell_s
                );
            }
            for r in &g.missed_stops {
                out += &format!(
                    "\n{}: OMITIDA · +{:.0} puntos · no se transfirieron pasajeros",
                    r.name, r.penalty
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
fn advanced_text(
    l: &LiveDrive,
    content: &ActivePlayerContent,
    page: usize,
    settings: &PlayerSettings,
) -> String {
    let s = &l.session;
    let state = &s.state;
    let p = &s.physics;
    let unit = settings.speed_unit_label();
    let mut out = format!("{} · {}\n\n", ADVANCED_PAGES[page], clock(l.clock_time_s()));
    match page {
        0 => {
            out += &format!(
                "Velocidad {:.2} {unit} · límite {:.1} {unit}\nRegulador {:.0}% · freno {:.0}% · inversor {:.2}\nDistancia {:.1} m · tiempo {:.1} s · escala ×{:.0}\nEstado: {}\n{}",
                settings.display_speed_mps(s.velocity_mps()),
                settings.display_speed_mps(s.effective_speed_limit_mps()),
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
            out += &traction_text(s, settings);
            for (car, electric) in p.electric.cars.iter().zip(&state.electric.cars) {
                out += &format!(
                    "Vehículo {} · {} · vía {:.0} V · contacto {:.0} V\nPantógrafo {:.0}% · disyuntor {}\n{}\n\n",
                    car.vehicle + 1,
                    car.params.pickup.label(),
                    electric.line_voltage_v,
                    electric.contact_voltage_v,
                    electric.pantograph_fraction * 100.,
                    electric.breaker.label(),
                    electric.loss.label()
                );
            }
            out += &format!(
                "Potencia nominal instalada {:.0} kW\nEsfuerzo máximo {:.1} kN\nCaldera {}\n\n",
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
                s.wheel_rail_brake_forces_n().iter().sum::<f64>() / 1000.0,
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
            out += &format!(
                "Reservas del próximo bloque: {} intervalos propios · {} de otros servicios\n",
                s.own_track_reservations.len(),
                s.external_track_reservations.len()
            );
            out += &format!(
                "Cambios enclavados: {} propios · {} de otros · desvíos alternativos {}\n",
                s.dispatcher.own_locks.len(),
                s.dispatcher.other_locks.len(),
                s.dispatcher.reroutes
            );
            if !s.dispatcher.waiting_for.is_empty() {
                out += &format!(
                    "Esperando a: {}{}\n",
                    s.dispatcher.waiting_for.join(", "),
                    if s.dispatcher.deadlock {
                        " · conflicto circular; buscando alternativa"
                    } else {
                        ""
                    }
                );
            }
            for lock in s
                .dispatcher
                .own_locks
                .iter()
                .chain(&s.dispatcher.other_locks)
            {
                out += &format!(
                    "Cambio {}: {:?} · {}\n",
                    lock.node, lock.position, lock.owner
                );
            }
            for service in &l.traffic.services {
                out += &format!(
                    "{}: {} · {:.1} {unit} · {} paradas · {}\n",
                    service.id,
                    if !service.departed {
                        "Salida pendiente"
                    } else if service.session.arrived {
                        "Recorrido terminado"
                    } else {
                        phase_name(service.session.gameplay.phase)
                    },
                    settings.display_speed_mps(service.session.velocity_mps()),
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
pub fn monitor_events(s: &LiveDriveSession, settings: &PlayerSettings) -> Vec<MonitorEvent> {
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
                text: format!(
                    "Límite {:.0} {}",
                    settings.display_speed_mps(initial_limit),
                    settings.speed_unit_label()
                ),
                color: CAUTION,
            });
        }
        if let Some(physics) = physics {
            for post in &physics.profile.speed_posts {
                let distance = before + post.position_m - head;
                if (0.0..=5000.0).contains(&distance) {
                    out.push(MonitorEvent {
                        distance_m: distance,
                        text: format!(
                            "Límite {:.0} {}",
                            settings.display_speed_kmh(post.speed_limit_kmh),
                            settings.speed_unit_label()
                        ),
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
    settings: Res<PlayerSettings>,
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
    if *last < 0.4 && !settings.is_changed() && !ui.is_changed() {
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
    let events = monitor_events(&l.session, &settings);
    commands.entity(root).with_children(|p| {
        line(p, Vec2::new(20.0, 14.0), Vec2::new(20.0, 245.0), 3.0, MUTED);
        label(
            p,
            format!(
                "Límite actual {:.0} {}",
                settings.display_speed_mps(l.session.effective_speed_limit_mps()),
                settings.speed_unit_label()
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

pub(crate) fn apply_weather(
    mut commands: Commands,
    content: Res<ActivePlayerContent>,
    mut precipitation: ResMut<crate::precipitation::PrecipitationState>,
    rain: Query<Entity, With<crate::weather_particles::WeatherMesh>>,
) {
    precipitation.enabled = matches!(
        content.weather,
        PlayerWeather::Rain | PlayerWeather::Snow | PlayerWeather::Storm
    );
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
    fn menu_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(ViewerAppState::Menu)
            .init_resource::<PlayerUiState>()
            .insert_resource(PlayerLaunchMenu::discover(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
                None,
            ))
            .init_resource::<crate::official_content::OfficialContent>()
            .init_resource::<PlayerSettings>()
            .init_resource::<PlayerLaunchQueue>()
            .init_resource::<ActivePlayerContent>()
            .init_resource::<CameraFollowMode>()
            .init_resource::<DriverLookOffset>()
            .init_resource::<crate::floating_origin::FloatingOrigin>()
            .init_resource::<crate::camera::CameraMode>()
            .init_resource::<crate::camera::LiveDriverCab>()
            .init_resource::<crate::camera::PassengerCamState>()
            .init_resource::<crate::cab_cvf_overlay::CabCvfOverlayState>()
            .init_resource::<crate::cab_cvf::CabCvfState>()
            .init_resource::<crate::teleport::TeleportDialog>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<AppExit>()
            .add_systems(Update, (player_keys, handle_buttons, build_panel).chain());
        let camera = app.world_mut().spawn_empty().id();
        let mut ui = app.world_mut().resource_mut::<PlayerUiState>();
        ui.panel = PlayerPanel::Menu;
        ui.in_start_menu = true;
        ui.camera = Some(camera);
        app
    }
    fn click(app: &mut App, predicate: impl Fn(&UiCommand) -> bool) {
        let mut query = app.world_mut().query::<(Entity, &UiCommand)>();
        let entity = query
            .iter(app.world())
            .find(|(_, cmd)| predicate(cmd))
            .map(|(entity, _)| entity)
            .expect("expected a visible command");
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .entity_mut(entity)
            .insert(Interaction::Pressed);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
    }
    fn escape(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }
    #[test]
    fn home_actions_and_nested_details_return_without_losing_the_trip() {
        let mut app = menu_app();
        app.update();
        for destination in [
            PlayerPanel::NewGame,
            PlayerPanel::Continue,
            PlayerPanel::Content,
            PlayerPanel::Settings,
        ] {
            click(
                &mut app,
                |cmd| matches!(cmd, UiCommand::Open(panel) if *panel == destination),
            );
            assert_eq!(app.world().resource::<PlayerUiState>().panel, destination);
            escape(&mut app);
            assert_eq!(
                app.world().resource::<PlayerUiState>().panel,
                PlayerPanel::Menu
            );
        }
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::NewGame))
        });
        {
            let mut menu = app.world_mut().resource_mut::<PlayerLaunchMenu>();
            menu.start_time_s = 12345.;
            menu.weather = PlayerWeather::Snow;
        }
        let before = {
            let menu = app.world().resource::<PlayerLaunchMenu>();
            (menu.route, menu.service, menu.consist, menu.path)
        };
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::NewGameStep(NewGameStep::Train))
        });
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::MissingResources))
        });
        escape(&mut app);
        let ui = app.world().resource::<PlayerUiState>();
        assert_eq!(ui.panel, PlayerPanel::NewGame);
        assert_eq!(ui.new_game_step, NewGameStep::Train);
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::NewGameStep(NewGameStep::Environment))
        });
        click(&mut app, |cmd| matches!(cmd, UiCommand::NewGameAdvanced));
        assert!(app.world().resource::<PlayerUiState>().new_game_advanced);
        let menu = app.world().resource::<PlayerLaunchMenu>();
        assert_eq!((menu.route, menu.service, menu.consist, menu.path), before);
        assert_eq!(menu.start_time_s, 12345.);
        assert_eq!(menu.weather, PlayerWeather::Snow);
    }
    #[test]
    fn launch_and_save_actions_stay_outside_scrolling_content() {
        let mut app = menu_app();
        {
            let mut menu = app.world_mut().resource_mut::<PlayerLaunchMenu>();
            let path = menu.consists[menu.consist].clone();
            menu.consist_audits.insert(
                path,
                openrailsrs_train::ConsistAudit {
                    vehicles: 2,
                    powered_vehicles: 1,
                    ..default()
                },
            );
        }
        app.update();
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::NewGame))
        });
        for step in [
            NewGameStep::Route,
            NewGameStep::Train,
            NewGameStep::Environment,
        ] {
            click(
                &mut app,
                |cmd| matches!(cmd, UiCommand::NewGameStep(s) if *s == step),
            );
            let ui = app.world().resource::<PlayerUiState>();
            let root = ui.root.unwrap();
            let mut footers = app
                .world_mut()
                .query_filtered::<Entity, With<launcher::LaunchFooter>>();
            let footer = footers.single(app.world()).unwrap();
            assert_eq!(app.world().get::<ChildOf>(footer).unwrap().parent(), root);
            let mut buttons = app.world_mut().query::<(Entity, &UiCommand)>();
            let mut start = buttons
                .iter(app.world())
                .find(|(_, cmd)| matches!(cmd, UiCommand::Start))
                .unwrap()
                .0;
            while start != footer {
                assert!(
                    app.world().get::<PlayerScroll>(start).is_none(),
                    "Jugar must remain visible when the body scrolls"
                );
                start = app.world().get::<ChildOf>(start).unwrap().parent();
            }
        }
        escape(&mut app);
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::Settings))
        });
        let root = app.world().resource::<PlayerUiState>().root.unwrap();
        let mut buttons = app.world_mut().query::<(Entity, &UiCommand)>();
        let save = buttons
            .iter(app.world())
            .find(|(_, cmd)| matches!(cmd, UiCommand::SaveSettings))
            .unwrap()
            .0;
        let row = app.world().get::<ChildOf>(save).unwrap().parent();
        assert_eq!(app.world().get::<ChildOf>(row).unwrap().parent(), root);
    }
    #[test]
    fn incomplete_or_pending_train_cannot_start_from_any_step() {
        let mut app = menu_app();
        app.update();
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::NewGame))
        });
        for incomplete in [false, true] {
            {
                let mut menu = app.world_mut().resource_mut::<PlayerLaunchMenu>();
                menu.consist_audits.clear();
                if incomplete {
                    let path = menu.consists[menu.consist].clone();
                    menu.consist_audits.insert(
                        path,
                        openrailsrs_train::ConsistAudit {
                            vehicles: 1,
                            powered_vehicles: 1,
                            errors: vec!["Missing vehicle".into()],
                            ..default()
                        },
                    );
                }
            }
            for step in [
                NewGameStep::Route,
                NewGameStep::Train,
                NewGameStep::Environment,
            ] {
                click(
                    &mut app,
                    |cmd| matches!(cmd, UiCommand::NewGameStep(s) if *s == step),
                );
                let mut commands = app.world_mut().query::<&UiCommand>();
                assert!(
                    !commands
                        .iter(app.world())
                        .any(|cmd| matches!(cmd, UiCommand::Start))
                );
                assert!(app.world().resource::<PlayerLaunchQueue>().0.is_none());
            }
        }
    }
    #[test]
    fn library_selection_uses_the_chosen_route_and_returns_to_the_library() {
        let mut app = menu_app();
        app.update();
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::Content))
        });
        let chosen = app.world().resource::<PlayerLaunchMenu>().routes.len() - 1;
        click(
            &mut app,
            |cmd| matches!(cmd, UiCommand::SelectRoute(i) if *i == chosen),
        );
        assert_eq!(app.world().resource::<PlayerLaunchMenu>().route, chosen);
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::NewGame
        );
        escape(&mut app);
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::Content
        );
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::LibraryTab(LibraryTab::Official))
        });
        assert_eq!(
            app.world().resource::<PlayerUiState>().library_tab,
            LibraryTab::Official
        );
        escape(&mut app);
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::Menu
        );
    }
    #[test]
    fn settings_tabs_preserve_changes_and_return_to_the_paused_game() {
        let mut app = menu_app();
        app.insert_resource(live());
        app.world_mut().resource_mut::<LiveDrive>().paused = false;
        app.world_mut()
            .resource_mut::<PlayerUiState>()
            .in_start_menu = false;
        app.world_mut().resource_mut::<PlayerUiState>().panel = PlayerPanel::None;
        app.world_mut()
            .resource_mut::<NextState<ViewerAppState>>()
            .set(ViewerAppState::Playing);
        app.update();
        escape(&mut app);
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Open(PlayerPanel::Settings))
        });
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::SettingsTab(SettingsTab::Audio))
        });
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::Setting(SettingField::Audio, _))
        });
        let audio = app.world().resource::<PlayerSettings>().audio_enabled;
        click(&mut app, |cmd| {
            matches!(cmd, UiCommand::SettingsTab(SettingsTab::Controls))
        });
        assert_eq!(
            app.world().resource::<PlayerSettings>().audio_enabled,
            audio
        );
        escape(&mut app);
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::Pause
        );
        assert!(app.world().resource::<LiveDrive>().paused);
        escape(&mut app);
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::None
        );
        assert!(!app.world().resource::<LiveDrive>().paused);
    }
    fn live() -> LiveDrive {
        LiveDrive::from_scenario_path(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/chiltern_local/scenario.toml"),
        )
        .unwrap()
    }
    #[test]
    fn traction_panel_keeps_pause_and_buttons_control_the_selected_engine() {
        for paused in [false, true] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/traction_operation/scenario_two_diesel.toml");
            let mut l = LiveDrive::from_scenario_path(&path).unwrap();
            l.paused = paused;
            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin)
                .insert_state(ViewerAppState::Playing)
                .insert_resource(l)
                .init_resource::<PlayerUiState>()
                .init_resource::<PlayerLaunchMenu>()
                .init_resource::<crate::official_content::OfficialContent>()
                .init_resource::<PlayerSettings>()
                .init_resource::<PlayerLaunchQueue>()
                .init_resource::<ActivePlayerContent>()
                .init_resource::<CameraFollowMode>()
                .init_resource::<DriverLookOffset>()
                .init_resource::<crate::floating_origin::FloatingOrigin>()
                .init_resource::<crate::camera::CameraMode>()
                .init_resource::<crate::camera::LiveDriverCab>()
                .init_resource::<crate::camera::PassengerCamState>()
                .init_resource::<crate::cab_cvf_overlay::CabCvfOverlayState>()
                .init_resource::<crate::cab_cvf::CabCvfState>()
                .init_resource::<crate::teleport::TeleportDialog>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .add_message::<AppExit>()
                .add_systems(
                    Update,
                    (player_keys, handle_buttons, crate::live::live_driver_input).chain(),
                );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyB);
            app.update();
            assert_eq!(
                app.world().resource::<PlayerUiState>().panel,
                PlayerPanel::Traction
            );
            assert_eq!(app.world().resource::<LiveDrive>().paused, paused);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyD);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            app.world_mut()
                .spawn((Interaction::Pressed, UiCommand::DieselEngine(1)));
            app.update();
            let l = app.world().resource::<LiveDrive>();
            assert!(!l.session.state.diesel.cars[1].command_running);
            assert!(l.session.state.diesel.cars[0].command_running);
            assert_eq!(
                l.session.driver_throttle, 0.,
                "panel input must not drive the train"
            );
            assert_eq!(l.paused, paused);
            let sand_before = l.session.physics.rail_adhesion.as_ref().unwrap().vehicles[0]
                .profile
                .sander
                .capacity_m3;
            app.world_mut()
                .spawn((Interaction::Pressed, UiCommand::Sander));
            app.update();
            let l = app.world().resource::<LiveDrive>();
            let rail = l.session.state.rail_adhesion.as_ref().unwrap();
            assert!(rail.sander_command);
            assert_eq!(rail.cars[0].sand_m3, sand_before);
            assert_eq!(rail.cars[0].consumed_sand_m3, 0.);
            assert_eq!(l.paused, paused);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyB);
            app.update();
            assert_eq!(
                app.world().resource::<PlayerUiState>().panel,
                PlayerPanel::None
            );
            assert_eq!(app.world().resource::<LiveDrive>().paused, paused);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyK);
            app.update();
            assert!(
                !app.world()
                    .resource::<LiveDrive>()
                    .session
                    .state
                    .diesel
                    .cars[0]
                    .command_running
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.update();
            assert!(app.world().resource::<LiveDrive>().paused);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyB);
            app.update();
            assert_eq!(
                app.world().resource::<PlayerUiState>().panel,
                PlayerPanel::Traction
            );
            assert!(
                app.world().resource::<LiveDrive>().paused,
                "opening the fireman from the pause menu must keep the game paused"
            );
        }
    }
    #[test]
    fn track_monitor_contains_real_station_signal_and_changing_distances() {
        let mut l = live();
        let edge_index = l.session.state.edge_index;
        let post_position = l.session.pos_on_edge_m() + 50.0;
        l.session.path_data.edges[edge_index]
            .profile
            .speed_posts
            .push(openrailsrs_track::PositionSpeedLimit {
                position_m: post_position,
                speed_limit_kmh: 96.56064,
            });
        let settings = PlayerSettings::default();
        let events = monitor_events(&l.session, &settings);
        assert!(events.iter().any(|e| e.text.contains("Northolt Park")));
        assert!(events.iter().any(|e| e.text.starts_with('●')));
        assert!(events.iter().any(|e| e.text == "Límite 97 km/h"));
        let imperial = monitor_events(
            &l.session,
            &PlayerSettings {
                mph: true,
                ..default()
            },
        );
        assert!(imperial.iter().any(|e| e.text == "Límite 60 mph"));
        assert!(imperial.iter().all(|e| !e.text.contains("km/h")));
        assert_eq!(
            events.iter().map(|e| e.distance_m).collect::<Vec<_>>(),
            imperial.iter().map(|e| e.distance_m).collect::<Vec<_>>()
        );
        let first = events
            .iter()
            .find(|e| e.text.starts_with('●'))
            .unwrap()
            .distance_m;
        l.session.state.pos_on_edge_m += 5.0;
        let second = monitor_events(&l.session, &settings)
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
    fn choosing_manual_weather_preserves_real_time_in_menu_and_game() {
        use crate::environment::{EnvironmentSelection, EnvironmentSource};
        for playing in [false, true] {
            let mut app = App::new();
            app.init_resource::<PlayerUiState>()
                .init_resource::<PlayerLaunchMenu>()
                .init_resource::<crate::official_content::OfficialContent>()
                .init_resource::<PlayerSettings>()
                .init_resource::<PlayerLaunchQueue>()
                .init_resource::<ActivePlayerContent>()
                .init_resource::<CameraFollowMode>()
                .init_resource::<DriverLookOffset>()
                .init_resource::<crate::floating_origin::FloatingOrigin>()
                .init_resource::<crate::camera::CameraMode>()
                .init_resource::<crate::camera::LiveDriverCab>()
                .init_resource::<crate::camera::PassengerCamState>()
                .init_resource::<crate::cab_cvf_overlay::CabCvfOverlayState>()
                .init_resource::<crate::cab_cvf::CabCvfState>()
                .init_resource::<ButtonInput<MouseButton>>()
                .add_message::<AppExit>()
                .add_systems(Update, handle_buttons);
            let selection = EnvironmentSelection {
                time: EnvironmentSource::LocalNow,
                weather: EnvironmentSource::LocalNow,
                manual_weather: PlayerWeather::Snow,
            };
            {
                let mut menu = app.world_mut().resource_mut::<PlayerLaunchMenu>();
                menu.environment = selection;
                menu.weather = PlayerWeather::Snow;
            }
            if playing {
                app.insert_resource(live());
                app.world_mut()
                    .resource_mut::<ActivePlayerContent>()
                    .environment = selection;
            }
            // Before launch, active content still has its default time source.
            // The actual settings button must use the selection in the menu.
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            app.world_mut().spawn((
                Interaction::Pressed,
                UiCommand::Setting(SettingField::ManualWeather, 0.0),
            ));
            app.update();
            let chosen = app.world().resource::<ActivePlayerContent>().environment;
            assert_eq!(chosen.time, EnvironmentSource::LocalNow);
            assert_eq!(chosen.weather, EnvironmentSource::Manual);
            assert_eq!(chosen.manual_weather, PlayerWeather::Overcast);
            assert_eq!(
                app.world().resource::<PlayerLaunchMenu>().environment,
                chosen
            );
            assert_eq!(app.world().resource::<PlayerSettings>().environment, chosen);
        }
    }

    #[test]
    fn toolbar_and_u_change_display_units_without_pausing_or_changing_physics() {
        let mut l = live();
        l.paused = false;
        l.session.state.velocity_mps = 10.0;
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(ViewerAppState::Playing)
            .insert_resource(l)
            .init_resource::<PlayerUiState>()
            .init_resource::<PlayerLaunchMenu>()
            .init_resource::<crate::official_content::OfficialContent>()
            .init_resource::<PlayerSettings>()
            .init_resource::<PlayerLaunchQueue>()
            .init_resource::<ActivePlayerContent>()
            .init_resource::<CameraFollowMode>()
            .init_resource::<DriverLookOffset>()
            .init_resource::<crate::floating_origin::FloatingOrigin>()
            .init_resource::<crate::camera::CameraMode>()
            .init_resource::<crate::camera::LiveDriverCab>()
            .init_resource::<crate::camera::PassengerCamState>()
            .init_resource::<crate::cab_cvf_overlay::CabCvfOverlayState>()
            .init_resource::<crate::cab_cvf::CabCvfState>()
            .init_resource::<crate::teleport::TeleportDialog>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<AppExit>()
            .add_systems(Startup, spawn_toolbar)
            .add_systems(Update, (player_keys, handle_buttons).chain());
        let camera = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<PlayerUiState>().camera = Some(camera);
        app.update();
        let mut buttons = app.world_mut().query::<(Entity, &UiCommand)>();
        let button = buttons
            .iter(app.world())
            .find(|(_, command)| matches!(command, UiCommand::Setting(SettingField::Units, _)))
            .map(|(entity, _)| entity)
            .expect("the toolbar must offer the speed-units button");

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyU);
        app.update();
        let settings = app.world().resource::<PlayerSettings>();
        assert!(settings.mph);
        let l = app.world().resource::<LiveDrive>();
        assert!(!l.paused);
        assert_eq!(l.session.velocity_mps(), 10.0);
        let text = advanced_text(l, &ActivePlayerContent::default(), 0, settings);
        assert!(text.contains("Velocidad 22.37 mph"), "{text}");
        assert!(!text.contains("km/h"));
        let brief = notebook_text(l, &ActivePlayerContent::default(), 0, settings);
        assert!(brief.contains("0.22 mph"), "{brief}");
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::None
        );

        // Holding U must not repeatedly toggle. A mouse click uses the real
        // toolbar command, independently of the keyboard shortcut.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert!(app.world().resource::<PlayerSettings>().mph);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        app.update();
        let settings = app.world().resource::<PlayerSettings>();
        assert!(!settings.mph);
        let l = app.world().resource::<LiveDrive>();
        assert!(!l.paused);
        assert_eq!(l.session.velocity_mps(), 10.0);
        let text = advanced_text(l, &ActivePlayerContent::default(), 0, settings);
        assert!(text.contains("Velocidad 36.00 km/h"), "{text}");
        assert_eq!(
            app.world().resource::<PlayerUiState>().panel,
            PlayerPanel::None
        );
    }

    #[test]
    fn notebook_and_every_hud_page_report_current_service() {
        let l = live();
        let content = ActivePlayerContent::default();
        assert!(
            notebook_text(&l, &content, 1, &PlayerSettings::default()).contains("South Ruislip")
        );
        assert!(
            notebook_text(&l, &content, 1, &PlayerSettings::default()).contains("West Ruislip")
        );
        assert!(
            notebook_text(&l, &content, 2, &PlayerSettings::default())
                .contains("Paradas cumplidas: 0/3")
        );
        for (page, name) in ADVANCED_PAGES.iter().enumerate() {
            let text = advanced_text(&l, &content, page, &PlayerSettings::default());
            assert!(text.starts_with(name));
            assert!(text.lines().count() > 2);
        }
    }
}
