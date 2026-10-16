//! Direct, searchable choices. Incomplete content remains selectable for diagnosis.
use super::*;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
#[path = "choice_filters.rs"]
mod filters;
pub(super) use filters::{ChoiceFilter, cycle_filter, favorite_key, filtered};

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
    pub cursor_identity: Option<String>,
    pub country: usize,
    pub gauge: usize,
    pub traction: usize,
    pub resources: usize,
    pub favorites_only: bool,
    pub filters_expanded: bool,
    pub favorites_first: bool,
}

fn normalized(text: &str) -> String {
    text.nfd()
        .flat_map(char::to_lowercase)
        .scan(false, |latin, c| {
            if is_combining_mark(c) {
                Some((!(*latin)).then_some(c))
            } else {
                *latin = c.is_ascii_alphabetic() || ('\u{00c0}'..='\u{024f}').contains(&c);
                Some(Some(c))
            }
        })
        .flatten()
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
fn cursor_position(
    search: &ChoiceSearch,
    menu: &PlayerLaunchMenu,
    kind: ChoiceKind,
    matches: &[usize],
) -> Option<usize> {
    search
        .cursor_identity
        .as_ref()
        .map_or(search.cursor, |identity| {
            matches
                .iter()
                .position(|&i| favorite_key(menu, kind, i) == *identity)
        })
        .filter(|&p| p < matches.len())
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
    let id = wrapped_button(
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
    settings: &PlayerSettings,
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
    filters::build(p, search, menu, kind, settings);
    let matches = filtered(menu, kind, search, settings);
    let cursor_position = cursor_position(search, menu, kind, &matches);
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
                BorderColor::all(if selected || cursor_position == Some(position) {
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
                        "{}{}{}",
                        if selected { "✓ " } else { "" },
                        if settings
                            .library_favorites
                            .contains(&favorite_key(menu, kind, index))
                        {
                            "★ "
                        } else {
                            ""
                        },
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
                if let Some(warning) = audit.and_then(filters::gauge_warning) {
                    label(p, warning, 11.0, CAUTION);
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
        let all = matching(menu, kind, "");
        let country_routes = if kind == ChoiceKind::Route {
            all.iter()
                .any(|&i| menu_country::route_matches(menu, i, search.country))
        } else {
            !all.is_empty()
        };
        let cause = if all.is_empty() {
            "No hay contenido instalado para esta lista."
        } else if !country_routes {
            "No hay rutas instaladas para el país seleccionado."
        } else if !search.query.is_empty() && matching(menu, kind, &search.query).is_empty() {
            "Ningún nombre coincide con la búsqueda."
        } else {
            "Los filtros actuales no dejan coincidencias."
        };
        label(p, cause, 14.0, MUTED);
        row(p, |p| {
            if !search.query.is_empty() {
                button(p, "Limpiar búsqueda", UiCommand::ClearSearch(kind));
            }
            if country_routes && !all.is_empty() {
                button(
                    p,
                    "Quitar filtros",
                    UiCommand::ChoiceFilter(kind, ChoiceFilter::Reset),
                );
            }
            button(p, "Abrir Biblioteca", UiCommand::Open(PlayerPanel::Content));
        });
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
            "Escribí para buscar · ↑/↓ · Inicio/Fin · RePág/AvPág · Enter seleccionar · Esc salir"
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
    settings: Res<PlayerSettings>,
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
                search.cursor_identity = None;
            }
            Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::PageUp | Key::PageDown => {
                let matches = filtered(&menu, kind, search, &settings);
                let count = matches.len();
                if let Some(identity) = &search.cursor_identity {
                    search.cursor = matches
                        .iter()
                        .position(|&i| favorite_key(&menu, kind, i) == *identity);
                    if search.cursor.is_none() {
                        search.page = 0;
                    }
                }
                move_cursor(search, count, &input.logical_key);
                search.cursor_identity = search
                    .cursor
                    .and_then(|p| matches.get(p))
                    .map(|&i| favorite_key(&menu, kind, i));
            }
            Key::Enter => {
                let matches = filtered(&menu, kind, search, &settings);
                let position = if search.cursor_identity.is_none() && search.cursor.is_none() {
                    Some(search.page.saturating_mul(PAGE_SIZE))
                } else {
                    cursor_position(search, &menu, kind, &matches)
                };
                let index = position.and_then(|p| matches.get(p)).copied();
                if let Some(index) = index {
                    select(&mut menu, kind, index);
                    ui.choice_focus = None;
                } else if search.cursor_identity.is_some() {
                    ui.notice = "Los resultados cambiaron; volvé a elegir con las flechas.".into();
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
                    search.cursor_identity = None;
                }
            }
        }
        ui.rebuild = true;
    }
}

fn move_cursor(search: &mut ChoiceSearch, count: usize, key: &Key) {
    if count == 0 {
        search.cursor = None;
        search.page = 0;
        return;
    }
    let cursor = search
        .cursor
        .unwrap_or(search.page.saturating_mul(PAGE_SIZE))
        .min(count - 1);
    let next = match key {
        Key::Home => 0,
        Key::End => count - 1,
        Key::PageUp => cursor.saturating_sub(PAGE_SIZE),
        Key::PageDown => cursor.saturating_add(PAGE_SIZE).min(count - 1),
        Key::ArrowUp => cycle(cursor, count, -1),
        _ => cycle(cursor, count, 1),
    };
    search.cursor = Some(next);
    search.page = next / PAGE_SIZE;
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn asynchronous_results_cannot_confirm_another_identity_at_the_old_position() {
        let tmp = tempfile::tempdir().unwrap();
        let mut menu = PlayerLaunchMenu::discover(tmp.path(), None);
        menu.routes = vec!["Zulu".into(), "Beta".into()];
        let search = ChoiceSearch {
            cursor: Some(0),
            cursor_identity: Some("route:Beta".into()),
            ..Default::default()
        };
        assert_eq!(
            cursor_position(&search, &menu, ChoiceKind::Route, &[1, 0]),
            Some(0)
        );
        menu.routes.insert(0, "Alpha".into());
        assert_eq!(
            cursor_position(&search, &menu, ChoiceKind::Route, &[0, 2, 1]),
            Some(1)
        );
        assert_eq!(
            cursor_position(&search, &menu, ChoiceKind::Route, &[0, 1]),
            None
        );
    }
    #[test]
    fn unicode_equivalence_keeps_accents_out_of_search_identity_only() {
        assert_eq!(
            normalized("Éléctrico Ñandú"),
            normalized("E\u{301}le\u{301}ctrico N\u{303}andu\u{301}")
        );
        assert_eq!(normalized("CAFÉ 6000"), "cafe 6000");
        assert_eq!(normalized("東京"), "東京");
        assert_ne!(normalized("ガ"), normalized("カ"));
        assert_eq!(normalized("ガ"), normalized("カ\u{3099}"));
    }
    #[test]
    fn paging_clamps_to_filtered_results_and_recovers_after_shrink() {
        let mut s = ChoiceSearch::default();
        move_cursor(&mut s, 13, &Key::End);
        assert_eq!((s.cursor, s.page), (Some(12), 2));
        move_cursor(&mut s, 13, &Key::PageUp);
        assert_eq!((s.cursor, s.page), (Some(7), 1));
        move_cursor(&mut s, 13, &Key::PageDown);
        assert_eq!(s.cursor, Some(12));
        move_cursor(&mut s, 2, &Key::PageDown);
        assert_eq!((s.cursor, s.page), (Some(1), 0));
        move_cursor(&mut s, 0, &Key::Home);
        assert_eq!((s.cursor, s.page), (None, 0));
        move_cursor(&mut s, 8, &Key::Home);
        assert_eq!(s.cursor, Some(0));
    }
}
