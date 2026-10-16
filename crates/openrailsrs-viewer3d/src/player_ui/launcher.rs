//! Start screens. Navigation and launch validation stay in the parent systems.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NewGameStep {
    Country,
    Route,
    Train,
    Environment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LibraryTab {
    Installed,
    Official,
    Tools,
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

pub(super) fn card(
    p: &mut ChildSpawnerCommands<'_>,
    f: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
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
    label(p, "Elegí tu próximo viaje", 24.0, TEXT);
    label(
        p,
        "Conducí desde la cabina o seguí el tren desde afuera.",
        13.0,
        MUTED,
    );
    for (title, description, panel) in [
        (
            "Nueva partida",
            "Elegí el país, la ruta, el tren y las condiciones del viaje.",
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
                padding: UiRect::axes(
                    Val::Px(20.0),
                    Val::Px(if panel == PlayerPanel::NewGame {
                        10.
                    } else {
                        7.
                    }),
                ),
                min_height: Val::Px(if panel == PlayerPanel::NewGame {
                    70.
                } else {
                    36.
                }),
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
            label(
                p,
                title,
                if panel == PlayerPanel::NewGame {
                    19.
                } else {
                    15.
                },
                TEXT,
            );
            if panel == PlayerPanel::NewGame {
                label(p, description, 13.0, MUTED);
            }
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

pub(super) fn build_steps(p: &mut ChildSpawnerCommands<'_>, ui: &PlayerUiState) {
    row(p, |p| {
        for (step, name) in [
            (NewGameStep::Country, "1 · País"),
            (NewGameStep::Route, "2 · Ruta y servicio"),
            (NewGameStep::Train, "3 · Tren"),
            (NewGameStep::Environment, "4 · Hora y clima"),
        ] {
            if ui.country_chosen || step == NewGameStep::Country {
                tab(
                    p,
                    name,
                    UiCommand::NewGameStep(step),
                    ui.new_game_step == step,
                );
            }
        }
    });
}

fn choice_columns(
    p: &mut ChildSpawnerCommands<'_>,
    choices: impl FnOnce(&mut ChildSpawnerCommands<'_>),
    preview: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        align_items: AlignItems::FlexStart,
        column_gap: Val::Px(18.0),
        row_gap: Val::Px(14.0),
        flex_shrink: 0.0,
        min_width: Val::Px(0.0),
        ..default()
    })
    .with_children(|p| {
        let column = || Node {
            flex_basis: Val::Px(350.0),
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            ..default()
        };
        p.spawn(column()).with_children(choices);
        p.spawn(column()).with_children(preview);
    });
}

pub(super) fn build_new_game(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
    settings: &PlayerSettings,
    previews: Option<&super::previews::Previews>,
) {
    if ui.new_game_advanced
        && ui.new_game_step == NewGameStep::Environment
        && let Some(preview) = previews
    {
        if let Some(inventory) = &preview.inventory {
            let estimate = crate::resource_estimate::Estimate::from_inventory(
                inventory,
                if menu.consists.get(menu.consist) == preview.train_estimate_path.as_ref() {
                    preview.train_bytes
                } else {
                    default()
                },
                settings,
            );
            card(p, |p| {
                if menu.consists.get(menu.consist) != preview.train_estimate_path.as_ref() {
                    label(
                        p,
                        "Tren: aún sin estimación del modelo; abrí su preview en Tren para incluirlo.",
                        12.,
                        CAUTION,
                    );
                }
                label(p, estimate.text(settings), 12., MUTED);
            });
        } else {
            label(p, "Estimando recursos de la ruta…", 13., MUTED);
        }
    }
    match ui.new_game_step {
        NewGameStep::Country => menu_country::build(p, ui, menu),
        NewGameStep::Route => {
            if ui.new_game_advanced {
                button(
                    p,
                    format!(
                        "País: {} · Cambiar",
                        menu_country::country_label(
                            ui.choice_search[ChoiceKind::Route.slot()].country
                        )
                    ),
                    UiCommand::NewGameStep(NewGameStep::Country),
                );
            }
            if !menu_country::route_available(ui, menu) {
                label(p, "No hay rutas instaladas de este país.", 21.0, TEXT);
                label(
                    p,
                    "Elegí otro país o abrí la biblioteca para instalar una ruta de su autor.",
                    14.0,
                    MUTED,
                );
                button(p, "Abrir biblioteca", UiCommand::Open(PlayerPanel::Content));
                return;
            }
            label(
                p,
                format!(
                    "{} · Ruta y servicio",
                    menu_country::country_label(ui.choice_search[ChoiceKind::Route.slot()].country)
                ),
                17.0,
                TEXT,
            );
            choice_columns(
                p,
                |p| {
                    if ui.new_game_advanced {
                        choices::selected(
                            p,
                            "Ruta",
                            menu.routes
                                .get(menu.route)
                                .map(String::as_str)
                                .unwrap_or("Sin rutas"),
                            ChoiceKind::Route,
                            ui.route_picker,
                        );
                        choices::selected(
                            p,
                            "Servicio",
                            menu.current()
                                .map(|s| s.name.as_str())
                                .unwrap_or("Sin servicios"),
                            ChoiceKind::Service,
                            ui.route_picker,
                        );
                    } else {
                        row(p, |p| {
                            tab(
                                p,
                                "Rutas",
                                UiCommand::ShowChoices(ChoiceKind::Route),
                                ui.route_picker == ChoiceKind::Route,
                            );
                            tab(
                                p,
                                "Servicios",
                                UiCommand::ShowChoices(ChoiceKind::Service),
                                ui.route_picker == ChoiceKind::Service,
                            );
                        });
                        label(
                            p,
                            format!(
                                "Servicio: {}",
                                short(
                                    menu.current().map_or("Sin servicios", |s| s.name.as_str()),
                                    42
                                )
                            ),
                            12.,
                            MUTED,
                        );
                    }
                    choices::build(p, ui, menu, ui.route_picker, previews, settings);
                },
                |p| {
                    label(p, "Vista previa de la ruta", 17.0, TEXT);
                    if let Some(preview) = previews {
                        super::previews::route_card(p, preview);
                        if let Some(railway) = &preview.railway {
                            label(
                                p,
                                if railway.gauge_m > 0.0 {
                                    format!(
                                        "Trocha {:.0} mm · {} · {:.0} V",
                                        railway.gauge_m * 1000.,
                                        railway.supply.kind.label(),
                                        railway.supply.voltage_v
                                    )
                                } else {
                                    "Trocha y alimentación: sin datos del escenario original".into()
                                },
                                12.0,
                                MUTED,
                            );
                            for missing in &railway.missing {
                                label(p, missing, 13.0, CAUTION);
                            }
                        }
                    }
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
                            if !menu.preview.description.is_empty() {
                                label(p, &menu.preview.description, 13.0, MUTED);
                            }
                            if !menu.preview.stops.is_empty() {
                                label(p, menu.preview.stops.join(" → "), 15.0, TEXT);
                            }
                            if let Some(minutes) = menu.preview.scheduled_minutes {
                                label(
                                    p,
                                    format!("Horario del servicio · {:.0} min", minutes),
                                    13.0,
                                    MUTED,
                                );
                            }
                        }
                        label(
                            p,
                            if menu.current().is_some_and(|s| s.native_activity) {
                                "Actividad del autor · conserva su recorrido y horario"
                            } else {
                                "Escenario de ejemplo · ver recursos para comprobar el contenido 3D"
                            },
                            12.0,
                            MUTED,
                        );
                    });
                    if ui.new_game_advanced {
                        selector(p, "Recorrido", menu.path_label(), MenuField::Path);
                    }
                },
            );
            if ui.new_game_advanced {
                row(p, |p| {
                    button(
                        p,
                        "Campañas y jornadas",
                        UiCommand::Open(PlayerPanel::Campaigns),
                    );
                    button(
                        p,
                        "Crear o editar un servicio propio",
                        UiCommand::ServiceEditorStart,
                    );
                });
                button(
                    p,
                    format!(
                        "Modo: {} · cambiar o preparar ejercicio",
                        settings.journey_setup.mode.label()
                    ),
                    UiCommand::Open(PlayerPanel::Practice),
                );
            }
        }
        NewGameStep::Train => {
            choices::search_controls(p, ui, menu, ChoiceKind::Consist, settings);
            super::train_selection::build(
                p,
                ui,
                menu,
                settings,
                previews.and_then(|p| p.railway.as_ref()),
                |p| {
                    label(p, menu.consist_label(), 17.0, TEXT);
                    if ui.new_game_advanced
                        && let Some(preview) = previews
                        && let Some(rail) = &preview.railway
                        && let Some(audit) = menu
                            .consists
                            .get(menu.consist)
                            .and_then(|path| menu.consist_audits.get(path))
                    {
                        let assessment = crate::clearance::compare(audit, rail.clearance.as_ref());
                        label(
                            p,
                            assessment.text(),
                            13.,
                            if matches!(assessment, crate::clearance::Assessment::Incompatible(_)) {
                                CAUTION
                            } else {
                                MUTED
                            },
                        );
                    }
                    if let Some(preview) = previews {
                        super::previews::train_card(p, preview);
                    }
                    card(p, |p| {
                        if let Some(audit) = menu
                            .consists
                            .get(menu.consist)
                            .and_then(|path| menu.consist_audits.get(path))
                        {
                            row(p, |p| {
                                for _ in 0..audit.vehicles.min(10) {
                                    p.spawn((
                                        Node {
                                            width: Val::Px(25.0),
                                            height: Val::Px(15.0),
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
                                14.0,
                                TEXT,
                            );
                            label(
                                p,
                                format!(
                                    "Cabina 2D: {} · Cabina 3D: {}",
                                    yes(audit.cab_2d),
                                    yes(audit.cab_3d)
                                ),
                                13.0,
                                MUTED,
                            );
                            if ui.new_game_advanced {
                                label(p, crate::content_validation::summary(audit), 12.0, MUTED);
                            }
                            if ui.new_game_advanced && !audit.warnings.is_empty() {
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
                        }
                        let availability = super::train_selection::assess(
                            menu,
                            menu.consist,
                            previews.and_then(|p| p.railway.as_ref()),
                        );
                        for reason in &availability.reasons {
                            label(p, reason, 13., CAUTION);
                        }
                        for warning in &availability.warnings {
                            label(p, warning, 12., MUTED);
                        }
                        let (message, color, _) =
                            readiness(menu, previews.and_then(|p| p.railway.as_ref()));
                        label(p, message, 13.0, color);
                        row(p, |p| {
                            button(
                                p,
                                "Ver detalles",
                                UiCommand::Open(PlayerPanel::MissingResources),
                            );
                            if ui.new_game_advanced || !availability.can_run() {
                                button(p, "Revisar archivos otra vez", UiCommand::MissingAudit);
                            }
                        });
                    });
                },
            );
            if ui.new_game_advanced {
                super::recommendations::build(p, menu, previews);
                row(p, |p| {
                    button(
                        p,
                        "Armar una formación propia",
                        UiCommand::FormationEditorStart,
                    );
                    button(
                        p,
                        "Ajustes propios por vehículo",
                        UiCommand::Open(PlayerPanel::VehicleProfile),
                    );
                });
            }
        }
        NewGameStep::Environment => {
            if ui.new_game_advanced {
                button(
                    p,
                    "Editar evolución del clima",
                    UiCommand::Open(PlayerPanel::WeatherEditor),
                );
                label(
                    p,
                    crate::forecast::text(
                        menu.environment,
                        settings.weather_profile,
                        menu.weather,
                        settings.weather_seed,
                        crate::weather_journey::JourneyOptions {
                            pace: settings.weather_pace,
                            winter: menu.season == 3,
                        },
                        0.,
                        None,
                    ),
                    12.,
                    MUTED,
                );
            }
            let mode = WeatherMode::selected(menu.environment, settings.weather_profile);
            label(p, "Prepará las condiciones del viaje", 21.0, TEXT);
            selector(
                p,
                "Hora de salida",
                clock(menu.start_time_s),
                MenuField::Time,
            );
            selector(
                p,
                "Modo del clima",
                mode.label().into(),
                MenuField::WeatherSource,
            );
            selector(
                p,
                match mode {
                    WeatherMode::Fixed => "Clima elegido",
                    WeatherMode::Random => "Clima inicial",
                    WeatherMode::LocalNow => "Clima de respaldo",
                },
                menu.weather.label().into(),
                MenuField::Weather,
            );
            if mode == WeatherMode::Fixed
                && matches!(menu.weather, PlayerWeather::Rain | PlayerWeather::Snow)
            {
                selector(
                    p,
                    "Intensidad",
                    settings
                        .weather_profile
                        .intensity_label(menu.weather)
                        .into(),
                    MenuField::WeatherIntensity,
                );
            } else if mode == WeatherMode::Random {
                selector(
                    p,
                    "Ritmo de cambios",
                    settings.weather_pace.label().into(),
                    MenuField::WeatherPace,
                );
                label(
                    p,
                    "Nubes, viento, niebla y precipitación cambian gradualmente. En invierno puede nevar; también si elegís Nieve como clima inicial. El clima sigue el tiempo de la partida.",
                    12.0,
                    MUTED,
                );
            }
            selector(
                p,
                "Estación del año",
                ["Primavera", "Verano", "Otoño", "Invierno"][menu.season % 4].into(),
                MenuField::Season,
            );
            label(
                p,
                format!("Hora visual: {}", menu.environment.time.label(),),
                13.0,
                MUTED,
            );
            if ui.new_game_advanced {
                selector(
                    p,
                    "Hora visual",
                    menu.environment.time.label().into(),
                    MenuField::TimeSource,
                );
                if mode == WeatherMode::Random {
                    selector(
                        p,
                        "Secuencia",
                        format!("{}", settings.weather_seed),
                        MenuField::WeatherSeed,
                    );
                    label(
                        p,
                        "Otra secuencia cambia el pronóstico del viaje. Reiniciar o continuar una partida conserva la secuencia elegida.",
                        12.0,
                        MUTED,
                    );
                }
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

fn readiness(
    menu: &PlayerLaunchMenu,
    railway: Option<&previews::RailwayInfo>,
) -> (String, Color, bool) {
    if menu.current().is_none() {
        return (
            "Instalá una ruta desde Biblioteca para jugar.".into(),
            CAUTION,
            false,
        );
    }
    let availability = super::train_selection::assess(menu, menu.consist, railway);
    if availability.can_run() {
        ("Listo para jugar".into(), GOOD, true)
    } else {
        (availability.reasons[0].clone(), CAUTION, false)
    }
}

pub(super) fn build_launch_footer(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
    settings: &PlayerSettings,
    railway: Option<&previews::RailwayInfo>,
) {
    let weather = match WeatherMode::selected(menu.environment, settings.weather_profile) {
        WeatherMode::Fixed if matches!(menu.weather, PlayerWeather::Rain | PlayerWeather::Snow) => {
            format!(
                "{} · intensidad {}",
                menu.weather.label(),
                settings.weather_profile.intensity_label(menu.weather)
            )
        }
        WeatherMode::Fixed => menu.weather.label().into(),
        WeatherMode::Random => format!("Clima aleatorio · {}", settings.weather_pace.label()),
        WeatherMode::LocalNow => format!("Clima del lugar (respaldo: {})", menu.weather.label()),
    };
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            min_width: Val::Px(0.0),
            padding: UiRect::top(Val::Px(8.0)),
            border: UiRect::top(Val::Px(1.0)),
            row_gap: Val::Px(6.0),
            ..default()
        },
        BorderColor::all(FIELD),
        LaunchFooter,
        Name::new("launch-footer"),
    ))
    .with_children(|p| {
        if ui.new_game_step == NewGameStep::Country {
            label(
                p,
                "Primero elegí un país. Después, la ruta y el servicio.",
                13.0,
                MUTED,
            );
            return;
        }
        if !menu_country::route_available(ui, menu) {
            button(
                p,
                "← Elegir país",
                UiCommand::NewGameStep(NewGameStep::Country),
            );
            return;
        }
        label(
            p,
            format!(
                "{} · {}\nSalida {} · {}",
                short(
                    menu.routes
                        .get(menu.route)
                        .map_or("Sin ruta", String::as_str),
                    40
                ),
                short(&menu.consist_label(), 40),
                clock(menu.start_time_s),
                weather
            ),
            12.0,
            TEXT,
        );
        let (message, color, ready) = readiness(menu, railway);
        row(p, |p| {
            match ui.new_game_step {
                NewGameStep::Country => unreachable!("country footer returns before route actions"),
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
                        "Hora y clima →",
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
                    label(p, "Jugar", 13.0, MUTED);
                });
            }
            if !ready && !menu.selected_audit_pending() {
                button(
                    p,
                    "Ver detalles",
                    UiCommand::Open(PlayerPanel::MissingResources),
                );
            }
            label(p, short(&message, 110), 12.0, color);
        });
    });
}

pub(super) fn short(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.into()
    } else {
        value.chars().take(limit - 1).chain(['…']).collect()
    }
}

pub(super) fn build_continue(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    history: &crate::save_history::SaveHistory,
) {
    label(p, "Partidas e historial", 21.0, TEXT);
    label(
        p,
        "Se conservan las versiones anteriores de cada guardado. Reanudar valida contenido y abre la partida pausada.",
        13.0,
        MUTED,
    );
    row(p, |p| {
        button(
            p,
            format!(
                "Buscar ruta, tren o fecha: {}{}",
                ui.history_query,
                if ui.history_focus { " ▏" } else { "" }
            ),
            UiCommand::HistorySearch,
        );
        button(p, "Actualizar", UiCommand::HistoryRefresh);
    });
    row(p, |p| {
        for filter in crate::save_history::Filter::ALL {
            button(
                p,
                format!(
                    "{}{}",
                    if ui.history_filter == filter {
                        "✓ "
                    } else {
                        ""
                    },
                    filter.label()
                ),
                UiCommand::HistoryFilter(filter),
            );
        }
    });
    if history.loading {
        label(p, "Revisando guardados en segundo plano…", 13.0, MUTED);
    }
    let mut count = 0;
    for (index, entry) in history
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| ui.history_filter.matches(e, &ui.history_query))
    {
        count += 1;
        card(p, |p| {
            label(p, entry.label(), 14.0, TEXT);
            match &entry.compatible {
                Ok(()) => {
                    button(p, "Reanudar", UiCommand::ResumeHistory(index));
                }
                Err(e) => {
                    label(p, format!("Incompatible: {e}"), 13.0, CAUTION);
                }
            }
        });
    }
    if count == 0 && !history.loading {
        label(
            p,
            "No hay partidas que coincidan con esta búsqueda.",
            13.0,
            MUTED,
        );
    }
}

pub(super) fn build_library_tabs(p: &mut ChildSpawnerCommands<'_>, selected: LibraryTab) {
    row(p, |p| {
        tab(
            p,
            "Copias y respaldos",
            UiCommand::LibraryTab(LibraryTab::Tools),
            selected == LibraryTab::Tools,
        );
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
    tools: &crate::library_tools::LibraryTools,
) {
    match selected {
        LibraryTab::Tools => super::library::build(p, downloads, tools),
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
                    }
                    if package.automatic() {
                        button(p, "Agregar descarga a la cola", UiCommand::ContentDownload);
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
                    openrailsrs_content::content_dir().display()
                ),
                12.0,
                MUTED,
            );
            label(
                p,
                "Se descarga del origen del catálogo o del repositorio original que confirmaste al pegar su URL. Los faltantes se consultan en el detalle de la selección.",
                12.0,
                MUTED,
            );
        }
    }
    if !downloads.queue.entries.is_empty() {
        label(p, "Cola de descargas del autor", 17., ACCENT);
        row(p, |p| {
            if downloads.queue.held {
                button(p, "Retomar cola", UiCommand::QueueResume);
            }
            button(p, "Limpiar terminadas", UiCommand::QueueClear);
        });
        for job in &downloads.queue.entries {
            if job.phase.pending() {
                button(
                    p,
                    format!("Cancelar {}", job.package),
                    UiCommand::QueueCancel(job.id),
                );
            }
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
        "home" => (PlayerPanel::Menu, NewGameStep::Country, false),
        "country" => (PlayerPanel::NewGame, NewGameStep::Country, false),
        "vehicle-profile" => (PlayerPanel::VehicleProfile, NewGameStep::Train, false),
        "bug-report" => (PlayerPanel::IssueReport, NewGameStep::Route, false),
        "library-tools" => {
            ui.library_tab = LibraryTab::Tools;
            (PlayerPanel::Content, NewGameStep::Route, false)
        }
        "campaigns" => (PlayerPanel::Campaigns, NewGameStep::Route, false),
        "formation-editor" => (PlayerPanel::FormationEditor, NewGameStep::Train, false),
        "service-editor" => (PlayerPanel::ServiceEditor, NewGameStep::Route, false),
        "practice" => (PlayerPanel::Practice, NewGameStep::Route, false),
        "weather-editor" => (PlayerPanel::WeatherEditor, NewGameStep::Environment, false),
        "driving-report" => (PlayerPanel::Report, NewGameStep::Route, false),
        "route" | "route-map" => (PlayerPanel::NewGame, NewGameStep::Route, false),
        "train" => (PlayerPanel::NewGame, NewGameStep::Train, false),
        "train-options" => (PlayerPanel::NewGame, NewGameStep::Train, true),
        "route-options" => (PlayerPanel::NewGame, NewGameStep::Route, true),
        "weather" => (PlayerPanel::NewGame, NewGameStep::Environment, false),
        "weather-options" => (PlayerPanel::NewGame, NewGameStep::Environment, true),
        "continue" => (PlayerPanel::Continue, NewGameStep::Route, false),
        "library" => (PlayerPanel::Content, NewGameStep::Route, false),
        "downloads" => {
            ui.library_tab = LibraryTab::Official;
            (PlayerPanel::Content, NewGameStep::Route, false)
        }
        "settings" => (PlayerPanel::Settings, NewGameStep::Route, false),
        "weather-settings" => {
            ui.settings_tab = SettingsTab::Environment;
            (PlayerPanel::Settings, NewGameStep::Route, false)
        }
        "controls" => {
            ui.settings_tab = SettingsTab::Controls;
            (PlayerPanel::Settings, NewGameStep::Route, false)
        }
        _ => return,
    };
    ui.panel = panel;
    ui.new_game_step = step;
    ui.country_chosen = step != NewGameStep::Country;
    ui.choice_search[ChoiceKind::Route.slot()].country = 0;
    ui.choice_search[ChoiceKind::Service.slot()].country = 0;
    ui.new_game_advanced = advanced;
    ui.navigation = vec![PlayerPanel::Menu];
}
