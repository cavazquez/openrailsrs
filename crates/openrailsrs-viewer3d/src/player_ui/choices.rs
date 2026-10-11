//! Direct, searchable choices. Incomplete content remains selectable for diagnosis.
use super::*;

pub(super) const PAGE_SIZE: usize = 5;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChoiceKind {
    Route,
    Service,
    Consist,
}
impl ChoiceKind {
    pub(super) fn slot(self) -> usize {
        match self {
            Self::Route => 0,
            Self::Service => 1,
            Self::Consist => 2,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Route => "rutas",
            Self::Service => "servicios",
            Self::Consist => "formaciones",
        }
    }
}
#[derive(Default)]
pub(super) struct ChoiceSearch {
    pub query: String,
    pub page: usize,
    pub cursor: Option<usize>,
}

fn normalized(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            _ => c,
        })
        .collect()
}
fn title(menu: &PlayerLaunchMenu, kind: ChoiceKind, index: usize) -> String {
    match kind {
        ChoiceKind::Route => menu.routes[index].clone(),
        ChoiceKind::Service => menu.services[index].name.clone(),
        ChoiceKind::Consist => menu.consists[index]
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    }
}
pub(super) fn matching(menu: &PlayerLaunchMenu, kind: ChoiceKind, query: &str) -> Vec<usize> {
    let candidates = match kind {
        ChoiceKind::Route => (0..menu.routes.len()).collect(),
        ChoiceKind::Service => menu.service_indices(),
        ChoiceKind::Consist => (0..menu.consists.len()).collect(),
    };
    let query = normalized(query);
    candidates
        .into_iter()
        .filter(|&index| {
            let mut text = title(menu, kind, index);
            if kind == ChoiceKind::Consist {
                text.push_str(&menu.consist_origin_label(index));
            }
            let text = normalized(&text);
            query.split_whitespace().all(|word| text.contains(word))
        })
        .collect()
}
pub(super) fn select(menu: &mut PlayerLaunchMenu, kind: ChoiceKind, index: usize) {
    match kind {
        ChoiceKind::Route if index < menu.routes.len() && index != menu.route => {
            menu.cycle_route(index as i32 - menu.route as i32)
        }
        ChoiceKind::Service => {
            let indices = menu.service_indices();
            if let Some(position) = indices.iter().position(|&i| i == index) {
                let current = indices.iter().position(|&i| i == menu.service).unwrap_or(0);
                if position != current {
                    menu.cycle_service(position as i32 - current as i32);
                }
            }
        }
        ChoiceKind::Consist if index < menu.consists.len() => {
            menu.cycle_consist(index as i32 - menu.consist as i32)
        }
        _ => (),
    }
}
pub(super) fn selected(
    p: &mut ChildSpawnerCommands<'_>,
    label_text: &str,
    value: &str,
    kind: ChoiceKind,
    active: ChoiceKind,
) {
    let id = button(
        p,
        format!("{label_text}: {value} · Elegir"),
        UiCommand::ShowChoices(kind),
    );
    if kind == active {
        p.commands().entity(id).insert(BorderColor::all(ACCENT));
    }
}
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
    kind: ChoiceKind,
    previews: Option<&super::previews::Previews>,
) {
    let search = &ui.choice_search[kind.slot()];
    let focused = ui.choice_focus == Some(kind);
    row(p, |p| {
        let content = if search.query.is_empty() {
            format!("Buscar {}…", kind.label())
        } else {
            search.query.clone()
        };
        let id = button(
            p,
            format!("{}{}", content, if focused { " ▏" } else { "" }),
            UiCommand::FocusSearch(kind),
        );
        p.commands().entity(id).entry::<Node>().and_modify(|mut n| {
            n.flex_grow = 1.0;
            n.min_width = Val::Px(0.0);
        });
        if focused {
            p.commands().entity(id).insert(BorderColor::all(ACCENT));
        }
        if !search.query.is_empty() {
            button(p, "Limpiar", UiCommand::ClearSearch(kind));
        }
    });
    let matches = matching(menu, kind, &search.query);
    let pages = matches.len().div_ceil(PAGE_SIZE).max(1);
    let page = search.page.min(pages - 1);
    label(
        p,
        format!(
            "{} {} · página {} de {}",
            matches.len(),
            kind.label(),
            page + 1,
            pages
        ),
        12.0,
        MUTED,
    );
    for (position, &index) in matches
        .iter()
        .enumerate()
        .skip(page * PAGE_SIZE)
        .take(PAGE_SIZE)
    {
        let selected = match kind {
            ChoiceKind::Route => menu.route == index,
            ChoiceKind::Service => menu.service == index,
            ChoiceKind::Consist => menu.consist == index,
        };
        let audit = (kind == ChoiceKind::Consist)
            .then(|| menu.consist_audits.get(&menu.consists[index]))
            .flatten();
        let incomplete = audit
            .is_some_and(|a| !a.content_valid() || !a.missing_resources.is_empty())
            || (kind == ChoiceKind::Route
                && menu.route_scenery_root(index).is_some_and(|root| {
                    previews.is_some_and(|p| p.route_missing.get(root) == Some(&true))
                }));
        let railway_warning = audit.and_then(|a| {
            previews
                .and_then(|p| p.railway.as_ref())
                .and_then(|r| super::previews::railway_warning(a, r))
        });
        let marker = if audit.is_some_and(|a| a.missing_resources.iter().any(|r| r.required)) {
            "Faltan recursos"
        } else if audit.is_some_and(|a| !a.content_valid()) {
            "Contenido no válido · ver diagnóstico"
        } else if audit.is_some_and(|a| !a.missing_resources.is_empty()) {
            "Faltan recursos opcionales"
        } else if audit.is_some_and(|a| a.powered_vehicles == 0) {
            "Sin tracción"
        } else if kind == ChoiceKind::Consist && audit.is_none() {
            "Revisando archivos…"
        } else {
            ""
        };
        let id = p
            .spawn((
                Button,
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    min_width: Val::Px(0.0),
                    flex_shrink: 0.0,
                    border: UiRect::left(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(FIELD),
                BorderColor::all(if selected || search.cursor == Some(position) {
                    ACCENT
                } else {
                    FIELD
                }),
                UiCommand::SelectChoice(kind, index),
                Name::new(format!("choice-{kind:?}-{index}")),
            ))
            .with_children(|p| {
                label(
                    p,
                    format!(
                        "{}{}",
                        if selected { "✓ " } else { "" },
                        title(menu, kind, index)
                    ),
                    14.0,
                    if incomplete { MUTED } else { TEXT },
                );
                if kind == ChoiceKind::Consist {
                    let origin = menu.consist_origin_label(index);
                    label(
                        p,
                        format!(
                            "{origin}{}",
                            if marker.is_empty() {
                                String::new()
                            } else {
                                format!(" · {marker}")
                            }
                        ),
                        11.0,
                        MUTED,
                    );
                }
                if let Some(warning) = &railway_warning {
                    label(p, format!("! {warning}"), 11.0, CAUTION);
                }
                if incomplete && kind == ChoiceKind::Route {
                    label(
                        p,
                        "Revisar recursos · seleccioná para ver los detalles",
                        11.0,
                        MUTED,
                    );
                }
            })
            .id();
        if incomplete {
            p.commands()
                .entity(id)
                .insert(BackgroundColor(Color::srgb(0.08, 0.09, 0.11)));
        }
    }
    if matches.is_empty() {
        label(
            p,
            "No hay coincidencias. Probá con otro nombre.",
            14.0,
            MUTED,
        );
    }
    if pages > 1 {
        row(p, |p| {
            button(p, "Página anterior", UiCommand::ChoicePage(kind, -1));
            button(p, "Página siguiente", UiCommand::ChoicePage(kind, 1));
        });
    }
    label(
        p,
        if focused {
            "Escribí para buscar · ↑/↓ elegir · Enter seleccionar · Esc salir del buscador"
        } else {
            "Podés seleccionar también los elementos en gris para ver sus detalles."
        },
        11.0,
        MUTED,
    );
}

pub(super) fn choice_keys(
    mut events: MessageReader<KeyboardInput>,
    mut ui: ResMut<PlayerUiState>,
    mut menu: ResMut<PlayerLaunchMenu>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    for input in events.read() {
        let Some(kind) = ui.choice_focus else {
            continue;
        };
        if input.state != ButtonState::Pressed {
            continue;
        }
        let search = &mut ui.choice_search[kind.slot()];
        match input.logical_key {
            Key::Backspace => {
                search.query.pop();
                search.page = 0;
                search.cursor = None;
            }
            Key::ArrowUp | Key::ArrowDown => {
                let count = matching(&menu, kind, &search.query).len();
                if count > 0 {
                    let cursor = search.cursor.unwrap_or(search.page * PAGE_SIZE);
                    search.cursor = Some(cycle(
                        cursor,
                        count,
                        if input.logical_key == Key::ArrowUp {
                            -1
                        } else {
                            1
                        },
                    ));
                    search.page = search.cursor.unwrap() / PAGE_SIZE;
                }
            }
            Key::Enter => {
                let index = matching(&menu, kind, &search.query)
                    .get(search.cursor.unwrap_or(search.page * PAGE_SIZE))
                    .copied();
                if let Some(index) = index {
                    select(&mut menu, kind, index);
                    ui.choice_focus = None;
                }
            }
            _ => {
                if !keys.pressed(KeyCode::ControlLeft)
                    && !keys.pressed(KeyCode::ControlRight)
                    && !keys.pressed(KeyCode::SuperLeft)
                    && !keys.pressed(KeyCode::SuperRight)
                    && let Some(text) = &input.text
                {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        if search.query.chars().count() < 100 {
                            search.query.push(c);
                        }
                    }
                    search.page = 0;
                    search.cursor = None;
                }
            }
        }
        ui.rebuild = true;
    }
}
