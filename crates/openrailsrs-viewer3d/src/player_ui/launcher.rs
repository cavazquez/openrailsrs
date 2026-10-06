//! Start screens. Navigation and launch validation stay in the parent systems.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NewGameStep {
    Route,
    Train,
    Environment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LibraryTab {
    Installed,
    Official,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsTab {
    General,
    Graphics,
    Environment,
    Audio,
    Controls,
}

#[derive(Component)]
pub(super) struct LaunchFooter;

fn card(p: &mut ChildSpawnerCommands<'_>, f: impl FnOnce(&mut ChildSpawnerCommands<'_>)) {
    p.spawn((
        Node {
            padding: UiRect::all(Val::Px(14.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(9.0),
            flex_shrink: 0.0,
            min_width: Val::Px(0.0),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.065, 0.09, 0.13)),
    ))
    .with_children(f);
}

pub(super) fn build_home(p: &mut ChildSpawnerCommands<'_>) {
    label(p, "Elegí tu próximo viaje", 28.0, TEXT);
    label(
        p,
        "Conducí desde la cabina o seguí el tren desde afuera.",
        15.0,
        MUTED,
    );
    for (title, description, panel) in [
        (
            "Nueva partida",
            "Elegí una ruta, un tren y las condiciones del viaje.",
            PlayerPanel::NewGame,
        ),
        (
            "Continuar",
            "Retomá una de tus partidas guardadas.",
            PlayerPanel::Continue,
        ),
        (
            "Biblioteca",
            "Rutas instaladas, recursos y descargas de sus autores.",
            PlayerPanel::Content,
        ),
        (
            "Ajustes",
            "Imagen, sonido, controles y preferencias de juego.",
            PlayerPanel::Settings,
        ),
    ] {
        p.spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(20.0), Val::Px(12.0)),
                min_height: Val::Px(70.0),
                flex_shrink: 0.0,
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(FIELD),
            UiCommand::Open(panel),
            Name::new(format!("home-{title}")),
        ))
        .with_children(|p| {
            label(p, title, 19.0, TEXT);
            label(p, description, 13.0, MUTED);
        });
    }
}

fn tab(p: &mut ChildSpawnerCommands<'_>, name: &str, command: UiCommand, selected: bool) {
    let entity = button(p, name, command);
    if selected {
        p.commands().entity(entity).insert(BorderColor::all(ACCENT));
        p.commands()
            .entity(entity)
            .entry::<Node>()
            .and_modify(|mut node| {
                node.border = UiRect::bottom(Val::Px(2.0));
            });
    }
}

pub(super) fn build_steps(p: &mut ChildSpawnerCommands<'_>, selected: NewGameStep) {
    row(p, |p| {
        for (step, name) in [
            (NewGameStep::Route, "1 · Ruta y servicio"),
            (NewGameStep::Train, "2 · Tren"),
            (NewGameStep::Environment, "3 · Hora y clima"),
        ] {
            tab(p, name, UiCommand::NewGameStep(step), selected == step);
        }
    });
}

pub(super) fn build_new_game(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
) {
    match ui.new_game_step {
        NewGameStep::Route => {
            label(p, "¿Dónde querés conducir?", 21.0, TEXT);
            selector(
                p,
                "Ruta",
                menu.routes
                    .get(menu.route)
                    .cloned()
                    .unwrap_or_else(|| "Sin rutas".into()),
                MenuField::Route,
            );
            selector(
                p,
                "Servicio",
                menu.current()
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "Sin servicios".into()),
                MenuField::Service,
            );
            card(p, |p| {
                if menu.path != 0 {
                    label(
                        p,
                        format!("Exploración · {}", menu.path_label()),
                        16.0,
                        CAUTION,
                    );
                    label(
                        p,
                        "Este recorrido termina en su destino y no usa las paradas del servicio.",
                        13.0,
                        MUTED,
                    );
                } else {
                    label(p, &menu.preview.description, 14.0, TEXT);
                    if !menu.preview.stops.is_empty() {
                        label(p, menu.preview.stops.join(" → "), 16.0, ACCENT);
                        label(
                            p,
                            format!(
                                "{} paradas{}",
                                menu.preview.stops.len(),
                                menu.preview
                                    .scheduled_minutes
                                    .filter(|m| m.is_finite() && *m > 0.)
                                    .map(|m| format!(" · horario previsto: {m:.0} min"))
                                    .unwrap_or_default()
                            ),
                            13.0,
                            MUTED,
                        );
                    }
                }
                label(
                    p,
                    if menu.current().is_some_and(|s| s.scenery_root.is_some()) {
                        "Escenografía original disponible"
                    } else {
                        "Escenario de ejemplo · ver recursos para comprobar el contenido 3D"
                    },
                    12.0,
                    MUTED,
                );
            });
            button(
                p,
                if ui.new_game_advanced {
                    "Ocultar recorrido alternativo"
                } else {
                    "Cambiar recorrido (exploración)"
                },
                UiCommand::NewGameAdvanced,
            );
            if ui.new_game_advanced {
                selector(p, "Recorrido", menu.path_label(), MenuField::Path);
                label(
                    p,
                    "Recorrido del servicio conserva sus paradas y horario.",
                    12.0,
                    MUTED,
                );
            }
        }
        NewGameStep::Train => {
            label(p, "¿Qué tren vas a llevar?", 21.0, TEXT);
            selector(p, "Formación", menu.consist_label(), MenuField::Consist);
            card(p, |p| {
                if let Some(audit) = menu
                    .consists
                    .get(menu.consist)
                    .and_then(|path| menu.consist_audits.get(path))
                {
                    // A lightweight diagram follows the audited consist, without
                    // loading vehicle meshes or textures just to browse a menu.
                    row(p, |p| {
                        for _ in 0..audit.vehicles.min(10) {
                            p.spawn((
                                Node {
                                    width: Val::Px(42.0),
                                    height: Val::Px(22.0),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    ..default()
                                },
                                BackgroundColor(MUTED),
                            ));
                        }
                        if audit.vehicles > 10 {
                            label(p, format!("+{}", audit.vehicles - 10), 13.0, MUTED);
                        }
                    });
                    label(
                        p,
                        format!(
                            "{} vehículos ({} con tracción) · {:.0} m · {:.0} t",
                            audit.vehicles,
                            audit.powered_vehicles,
                            audit.length_m,
                            audit.mass_kg / 1000.0
                        ),
                        16.0,
                        TEXT,
                    );
                    label(
                        p,
                        format!(
                            "Cabina 2D: {} · Cabina 3D: {}",
                            yes(audit.cab_2d),
                            yes(audit.cab_3d)
                        ),
                        14.0,
                        MUTED,
                    );
                    let (message, color, _) = readiness(menu);
                    label(p, message, 14.0, color);
                    if !audit.warnings.is_empty() {
                        label(
                            p,
                            format!(
                                "{} avisos de compatibilidad · Ver detalles",
                                audit.warnings.len()
                            ),
                            12.0,
                            CAUTION,
                        );
                    }
                } else {
                    let (message, color, _) = readiness(menu);
                    label(p, message, 14.0, color);
                }
                row(p, |p| {
                    button(
                        p,
                        "Ver detalles",
                        UiCommand::Open(PlayerPanel::MissingResources),
                    );
                    button(p, "Revisar archivos otra vez", UiCommand::MissingAudit);
                });
                label(
                    p,
                    "El esquema indica el tamaño de la formación. La revisión comprueba recursos; el detalle informa los límites de sus sistemas.",
                    12.0,
                    MUTED,
                );
            });
        }
        NewGameStep::Environment => {
            label(p, "Prepará las condiciones del viaje", 21.0, TEXT);
            selector(
                p,
                "Hora de salida",
                clock(menu.start_time_s),
                MenuField::Time,
            );
            selector(
                p,
                if menu.environment.weather == crate::environment::EnvironmentSource::Manual {
                    "Clima elegido"
                } else {
                    "Clima de respaldo"
                },
                menu.weather.label().into(),
                MenuField::Weather,
            );
            selector(
                p,
                "Estación del año",
                ["Primavera", "Verano", "Otoño", "Invierno"][menu.season % 4].into(),
                MenuField::Season,
            );
            label(
                p,
                format!(
                    "Hora visual: {} · Clima: {}",
                    menu.environment.time.label(),
                    menu.environment.weather.label()
                ),
                13.0,
                MUTED,
            );
            button(
                p,
                if ui.new_game_advanced {
                    "Ocultar opciones de hora y clima"
                } else {
                    "Hora y clima del lugar · opciones"
                },
                UiCommand::NewGameAdvanced,
            );
            if ui.new_game_advanced {
                selector(
                    p,
                    "Hora visual",
                    menu.environment.time.label().into(),
                    MenuField::TimeSource,
                );
                selector(
                    p,
                    "Origen del clima",
                    menu.environment.weather.label().into(),
                    MenuField::WeatherSource,
                );
                label(
                    p,
                    "Actual del lugar usa la zona horaria de la ruta y consulta Open-Meteo con conexión. El clima elegido sirve de respaldo. La hora de salida conserva el horario del servicio.",
                    12.0,
                    MUTED,
                );
            }
        }
    }
}

fn readiness(menu: &PlayerLaunchMenu) -> (String, Color, bool) {
    if menu.current().is_none() {
        return (
            "Instalá una ruta desde Biblioteca para jugar.".into(),
            CAUTION,
            false,
        );
    }
    if let Some(error) = menu.audit_error() {
        return (
            format!("No se pudo revisar la formación: {error}"),
            CAUTION,
            false,
        );
    }
    if menu.selected_audit_pending() {
        return (
            "Revisando formación… Podés seguir eligiendo.".into(),
            MUTED,
            false,
        );
    }
    let Some(audit) = menu
        .consists
        .get(menu.consist)
        .and_then(|p| menu.consist_audits.get(p))
    else {
        return (
            "La formación se comprobará al importar la actividad.".into(),
            MUTED,
            true,
        );
    };
    if audit.player_ready() {
        ("Recursos necesarios completos".into(), GOOD, true)
    } else if audit.powered_vehicles == 0 && audit.errors.is_empty() {
        (
            "Esta formación no tiene tracción. Elegí otro tren.".into(),
            CAUTION,
            false,
        )
    } else {
        (
            "Faltan recursos o hay archivos inválidos. Ver detalles.".into(),
            CAUTION,
            false,
        )
    }
}

pub(super) fn build_launch_footer(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
) {
    let service = if menu.path == 0 {
        menu.current()
            .map_or("Sin servicio".into(), |s| s.name.clone())
    } else {
        format!("Exploración · {}", menu.path_label())
    };
    let weather = if menu.environment.weather == crate::environment::EnvironmentSource::Manual {
        menu.weather.label().into()
    } else {
        format!("Clima del lugar (respaldo: {})", menu.weather.label())
    };
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            min_width: Val::Px(0.0),
            padding: UiRect::top(Val::Px(10.0)),
            border: UiRect::top(Val::Px(1.0)),
            row_gap: Val::Px(8.0),
            ..default()
        },
        BorderColor::all(FIELD),
        LaunchFooter,
        Name::new("launch-footer"),
    ))
    .with_children(|p| {
        label(
            p,
            format!(
                "{} · {}\n{} · Salida {} · {}",
                short(
                    menu.routes
                        .get(menu.route)
                        .map_or("Sin ruta", String::as_str),
                    70
                ),
                short(&service, 95),
                short(&menu.consist_label(), 90),
                clock(menu.start_time_s),
                weather
            ),
            13.0,
            TEXT,
        );
        let (message, color, ready) = readiness(menu);
        row(p, |p| {
            match ui.new_game_step {
                NewGameStep::Route => {
                    button(
                        p,
                        "Siguiente: tren →",
                        UiCommand::NewGameStep(NewGameStep::Train),
                    );
                }
                NewGameStep::Train => {
                    button(p, "← Ruta", UiCommand::NewGameStep(NewGameStep::Route));
                    button(
                        p,
                        "Siguiente: hora y clima →",
                        UiCommand::NewGameStep(NewGameStep::Environment),
                    );
                }
                NewGameStep::Environment => {
                    button(p, "← Tren", UiCommand::NewGameStep(NewGameStep::Train));
                }
            }
            if ready {
                button(p, "Jugar", UiCommand::Start);
            } else {
                p.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                        min_height: Val::Px(32.0),
                        flex_shrink: 0.0,
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(FIELD),
                    Name::new("launch-unavailable"),
                ))
                .with_children(|p| {
                    label(p, "Jugar · no disponible", 13.0, MUTED);
                });
            }
            if !ready && !menu.selected_audit_pending() {
                button(
                    p,
                    "Ver detalles",
                    UiCommand::Open(PlayerPanel::MissingResources),
                );
            }
            label(p, message, 12.0, color);
        });
    });
}

fn short(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.into()
    } else {
        value.chars().take(limit - 1).chain(['…']).collect()
    }
}

pub(super) fn build_continue(p: &mut ChildSpawnerCommands<'_>) {
    label(p, "Tus partidas guardadas", 21.0, TEXT);
    label(
        p,
        "Al reanudar se recuperan el tren, el servicio y la cámara. La partida empieza pausada.",
        14.0,
        MUTED,
    );
    for slot in 0..3 {
        card(p, |p| {
            label(p, format!("Partida {}", slot + 1), 16.0, ACCENT);
            match SavedGame::read(&slot_path(slot)) {
                Ok(saved) => {
                    label(p, saved_game_label(&saved), 14.0, TEXT);
                    button(p, "Reanudar", UiCommand::Resume(slot));
                }
                Err(_) => {
                    label(p, "Sin una partida válida en esta ranura.", 13.0, MUTED);
                }
            }
        });
    }
    button(
        p,
        "Empezar una nueva partida",
        UiCommand::Open(PlayerPanel::NewGame),
    );
}

pub(super) fn build_library_tabs(p: &mut ChildSpawnerCommands<'_>, selected: LibraryTab) {
    row(p, |p| {
        tab(
            p,
            "Instalado",
            UiCommand::LibraryTab(LibraryTab::Installed),
            selected == LibraryTab::Installed,
        );
        tab(
            p,
            "Descargas del autor",
            UiCommand::LibraryTab(LibraryTab::Official),
            selected == LibraryTab::Official,
        );
    });
}

pub(super) fn build_library(
    p: &mut ChildSpawnerCommands<'_>,
    selected: LibraryTab,
    downloads: &crate::official_content::OfficialContent,
    menu: &PlayerLaunchMenu,
) {
    match selected {
        LibraryTab::Installed => {
            label(p, "Rutas disponibles", 21.0, TEXT);
            for (index, name) in menu.routes.iter().enumerate() {
                card(p, |p| {
                    label(p, name, 17.0, TEXT);
                    button(p, "Elegir esta ruta", UiCommand::SelectRoute(index));
                });
            }
            if menu.routes.is_empty() {
                label(
                    p,
                    "No hay rutas disponibles. Buscá contenido en Descargas del autor.",
                    14.0,
                    MUTED,
                );
            }
            label(p, "Copias descargadas", 17.0, ACCENT);
            if downloads.installed.is_empty() {
                label(
                    p,
                    "Todavía no instalaste paquetes desde esta biblioteca.",
                    13.0,
                    MUTED,
                );
            }
            for (index, path) in downloads.installed.iter().enumerate() {
                card(p, |p| {
                    label(
                        p,
                        crate::official_content::installed_label(path),
                        14.0,
                        TEXT,
                    );
                    row(p, |p| {
                        if !downloads.busy() {
                            button(p, "Revisar esta copia", UiCommand::ContentAudit(index));
                        }
                        button(p, "Abrir carpeta", UiCommand::ContentFolder(Some(index)));
                    });
                });
            }
            row(p, |p| {
                button(
                    p,
                    "Recursos de la selección · detalles",
                    UiCommand::Open(PlayerPanel::MissingResources),
                );
                button(p, "Copiar diagnóstico", UiCommand::CopyContentDiagnostics);
            });
        }
        LibraryTab::Official => {
            let package = downloads.selected();
            label(p, "Contenido publicado por sus autores", 21.0, TEXT);
            card(p, |p| {
                row(p, |p| {
                    button(p, "‹", UiCommand::ContentCycle(-1));
                    label(p, &package.name, 18.0, TEXT);
                    button(p, "›", UiCommand::ContentCycle(1));
                });
                label(
                    p,
                    format!(
                        "{} · {}\nDescarga: {} · espacio instalado: {}",
                        package.author.name,
                        if package.compensation == "free" {
                            "Gratuito"
                        } else {
                            "Consultar condiciones del autor"
                        },
                        if package.download_bytes > 0 {
                            format!("{:.0} MiB", package.download_bytes as f64 / 1048576.)
                        } else {
                            "tamaño sin informar".into()
                        },
                        if package.install_bytes > 0 {
                            format!("{:.1} GiB", package.install_bytes as f64 / 1073741824.)
                        } else {
                            "tamaño sin informar".into()
                        }
                    ),
                    14.0,
                    MUTED,
                );
                label(p, &package.url, 12.0, MUTED);
                row(p, |p| {
                    if downloads.busy() {
                        button(p, "Cancelar operación", UiCommand::ContentCancel);
                    } else if package.automatic() {
                        button(
                            p,
                            "Buscar actualización e instalar",
                            UiCommand::ContentDownload,
                        );
                    }
                    button(p, "Ver catálogo oficial", UiCommand::ContentCatalogue);
                });
            });
            label(
                p,
                "Cada actualización se conserva junto a las copias anteriores, con fecha o identificador del autor. Las descargas van a tus datos de usuario.",
                13.0,
                MUTED,
            );
            label(
                p,
                format!(
                    "Carpeta de descargas: {}",
                    player_data_dir().join("official-content").display()
                ),
                12.0,
                MUTED,
            );
            label(
                p,
                "Solo se descarga del origen identificado en el catálogo. Los recursos faltantes y sus destinos se consultan en el detalle de la selección.",
                12.0,
                MUTED,
            );
        }
    }
    dynamic(p, DynamicText::Content, 13.0);
    if downloads.busy() && selected == LibraryTab::Installed {
        button(p, "Cancelar operación", UiCommand::ContentCancel);
    }
}

pub(super) fn build_settings_tabs(p: &mut ChildSpawnerCommands<'_>, selected: SettingsTab) {
    row(p, |p| {
        for (tab_id, name) in [
            (SettingsTab::General, "Juego"),
            (SettingsTab::Graphics, "Imagen y cabina"),
            (SettingsTab::Environment, "Hora y clima"),
            (SettingsTab::Audio, "Sonido"),
            (SettingsTab::Controls, "Controles"),
        ] {
            tab(p, name, UiCommand::SettingsTab(tab_id), selected == tab_id);
        }
    });
}

pub(super) fn capture_page(ui: &mut PlayerUiState, page: &str) {
    let (panel, step, advanced) = match page {
        "route" => (PlayerPanel::NewGame, NewGameStep::Route, false),
        "train" => (PlayerPanel::NewGame, NewGameStep::Train, false),
        "weather" => (PlayerPanel::NewGame, NewGameStep::Environment, false),
        "weather-options" => (PlayerPanel::NewGame, NewGameStep::Environment, true),
        "continue" => (PlayerPanel::Continue, NewGameStep::Route, false),
        "library" => (PlayerPanel::Content, NewGameStep::Route, false),
        "downloads" => {
            ui.library_tab = LibraryTab::Official;
            (PlayerPanel::Content, NewGameStep::Route, false)
        }
        "settings" => (PlayerPanel::Settings, NewGameStep::Route, false),
        "controls" => {
            ui.settings_tab = SettingsTab::Controls;
            (PlayerPanel::Settings, NewGameStep::Route, false)
        }
        _ => return,
    };
    ui.panel = panel;
    ui.new_game_step = step;
    ui.new_game_advanced = advanced;
    ui.navigation = vec![PlayerPanel::Menu];
}
