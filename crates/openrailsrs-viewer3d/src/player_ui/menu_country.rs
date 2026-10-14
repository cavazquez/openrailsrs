//! The first launch choice groups known content; it never limits rolling stock.
use super::*;

pub(super) const COUNTRIES: [&str; 8] = [
    "Todos los países",
    "Argentina",
    "Australia",
    "Estados Unidos",
    "Portugal",
    "Reino Unido",
    "Sin dato de país",
    "Francia",
];

/// Known package identities, rather than arbitrary place names in a description.
/// Installed content without an identified origin stays in "Sin dato de país".
pub(super) fn country(text: &str) -> usize {
    let lower = text.to_lowercase();
    let name = lower
        .trim()
        .strip_prefix("ejemplo · ")
        .unwrap_or(lower.trim());
    for (index, aliases) in [
        (1, &["mitre", "belgrano cc"][..]),
        (
            2,
            &["zig zag", "zigzag", "newcastle", "burrinjuck", "craven"][..],
        ),
        (3, &["bnsf", "long island", "lirr"][..]),
        (4, &["portugal", "cpv"][..]),
        (5, &["chiltern", "mideast", "scottish", "demo model"][..]),
        // Verified author repository Jeanprisca/SNCF-TGV-Atlantique. A train-only
        // package is still not a French route; this identity labels stock only.
        (7, &["sncf-tgv-atlantique"][..]),
    ] {
        if aliases.iter().any(|alias| {
            name.strip_prefix(*alias).is_some_and(|suffix| {
                suffix.is_empty() || suffix.starts_with(|c: char| !c.is_alphabetic())
            })
        }) {
            return index;
        }
    }
    6
}

pub(super) fn country_label(index: usize) -> &'static str {
    COUNTRIES.get(index).copied().unwrap_or(COUNTRIES[6])
}

pub(super) fn route_matches(menu: &PlayerLaunchMenu, index: usize, selected: usize) -> bool {
    selected < COUNTRIES.len()
        && menu
            .routes
            .get(index)
            .is_some_and(|name| selected == 0 || country(name) == selected)
}

pub(super) fn route_available(ui: &PlayerUiState, menu: &PlayerLaunchMenu) -> bool {
    ui.country_chosen
        && route_matches(
            menu,
            menu.route,
            ui.choice_search[ChoiceKind::Route.slot()].country,
        )
}

pub(super) fn select(
    ui: &mut PlayerUiState,
    menu: &mut PlayerLaunchMenu,
    index: usize,
) -> Result<(), String> {
    if index >= COUNTRIES.len() {
        return Err("El país ya no está disponible; elegilo de la lista.".into());
    }
    for kind in [ChoiceKind::Route, ChoiceKind::Service] {
        ui.choice_search[kind.slot()] = ChoiceSearch {
            country: index,
            ..default()
        };
    }
    if !route_matches(menu, menu.route, index)
        && let Some(route) = (0..menu.routes.len()).find(|&route| route_matches(menu, route, index))
    {
        // Preserve the normal asynchronous audit, service and preview refresh.
        choices::select(menu, ChoiceKind::Route, route);
    }
    ui.country_chosen = true;
    ui.new_game_step = NewGameStep::Route;
    ui.new_game_advanced = false;
    ui.route_picker = ChoiceKind::Route;
    ui.choice_focus = None;
    ui.notice.clear();
    Ok(())
}

pub(super) fn build(p: &mut ChildSpawnerCommands<'_>, ui: &PlayerUiState, menu: &PlayerLaunchMenu) {
    label(p, "¿En qué país querés conducir?", 21.0, TEXT);
    label(
        p,
        "Elegí un país para ver sus rutas y servicios instalados.",
        14.0,
        MUTED,
    );
    for (pair, names) in COUNTRIES.chunks(2).enumerate() {
        row(p, |p| {
            for (column, name) in names.iter().enumerate() {
                let index = pair * 2 + column;
                let count = (0..menu.routes.len())
                    .filter(|&route| route_matches(menu, route, index))
                    .count();
                let entity = button(
                    p,
                    format!(
                        "{name} · {count} {}",
                        if count == 1 { "ruta" } else { "rutas" }
                    ),
                    UiCommand::SelectCountry(index),
                );
                p.commands()
                    .entity(entity)
                    .entry::<Node>()
                    .and_modify(|mut node| {
                        node.flex_basis = Val::Percent(45.0);
                        node.flex_grow = 1.0;
                        node.min_width = Val::Px(0.0);
                    });
                if ui.country_chosen && ui.choice_search[ChoiceKind::Route.slot()].country == index
                {
                    p.commands().entity(entity).insert(BorderColor::all(ACCENT));
                }
            }
        });
    }
    label(
        p,
        "Los paquetes conocidos se agrupan por su origen. Los demás aparecen en Sin dato de país.",
        12.0,
        MUTED,
    );
    label(
        p,
        "Después podés elegir trenes de cualquier país.",
        13.0,
        TEXT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_identified_package_origins_receive_a_country() {
        assert_eq!(country("Chiltern · v4 · 2026-10"), 5);
        assert_eq!(country("Mitre · versión del autor"), 1);
        assert_eq!(country("Ejemplo · Mitre"), 1);
        assert_eq!(country("SNCF-TGV-Atlantique · 1096783"), 7);
        assert_eq!(country("My railway near Newcastle"), 6);
        assert_eq!(country("Chilternish experimental"), 6);
        assert_eq!(country("Unknown route"), 6);
    }
}
