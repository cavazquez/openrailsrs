//! Filters use declared stock metadata and cached resource audits. Unknown data
//! stays explicitly unknown; filters never disable selecting incomplete content.
use super::*;
#[derive(Clone, Copy, Debug)]
pub(in crate::player_ui) enum ChoiceFilter {
    Toggle,
    Country,
    Gauge,
    Traction,
    Resources,
    Favorites,
    Reset,
}
use crate::player_ui::menu_country::{COUNTRIES, country};
const GAUGES: [Option<f64>; 5] = [None, Some(1.0), Some(1.435), Some(1.6), Some(1.668)];
const TRACTIONS: [&str; 5] = [
    "Toda tracción",
    "Diesel",
    "Electric",
    "Steam",
    "Sin tracción",
];
const RESOURCES: [&str; 4] = [
    "Todos los recursos",
    "Completos",
    "Faltantes",
    "Sin revisar",
];
pub(in crate::player_ui) fn favorite_key(
    menu: &PlayerLaunchMenu,
    kind: ChoiceKind,
    index: usize,
) -> String {
    match kind {
        ChoiceKind::Route => format!("route:{}", menu.routes[index]),
        ChoiceKind::Service => format!("service:{}", menu.services[index].source.display()),
        ChoiceKind::Consist => format!("consist:{}", menu.consists[index].display()),
    }
}
fn origin(menu: &PlayerLaunchMenu, kind: ChoiceKind, index: usize) -> String {
    match kind {
        ChoiceKind::Route => menu.routes[index].clone(),
        ChoiceKind::Service => menu.routes.get(menu.route).cloned().unwrap_or_default(),
        ChoiceKind::Consist => menu.consist_origin_label(index),
    }
}
pub(in crate::player_ui) fn filtered(
    menu: &PlayerLaunchMenu,
    kind: ChoiceKind,
    search: &ChoiceSearch,
    settings: &PlayerSettings,
) -> Vec<usize> {
    let mut result: Vec<usize> = matching(menu, kind, &search.query)
        .into_iter()
        .filter(|&index| {
            if search.favorites_only
                && !settings
                    .library_favorites
                    .contains(&favorite_key(menu, kind, index))
            {
                return false;
            }
            if search.country != 0 && country(&origin(menu, kind, index)) != search.country {
                return false;
            }
            if kind != ChoiceKind::Consist {
                return true;
            }
            let audit = menu.consist_audits.get(&menu.consists[index]);
            if let Some(gauge) = GAUGES[search.gauge]
                && !audit.is_some_and(|a| {
                    gauge_matches(
                        a.compatibility
                            .iter()
                            .map(|v| v.declared.curve.track_gauge_m),
                        a.vehicles,
                        gauge,
                    )
                })
            {
                return false;
            }
            if search.traction != 0
                && !audit.is_some_and(|a| {
                    if search.traction == 4 {
                        a.powered_vehicles == 0
                    } else {
                        a.compatibility.iter().any(|v| {
                            v.declared
                                .engine_type
                                .as_deref()
                                .is_some_and(|t| t.eq_ignore_ascii_case(TRACTIONS[search.traction]))
                        })
                    }
                })
            {
                return false;
            }
            match search.resources {
                1 => audit.is_some_and(|a| a.content_valid() && a.missing_resources.is_empty()),
                2 => audit.is_some_and(|a| !a.content_valid() || !a.missing_resources.is_empty()),
                3 => audit.is_none(),
                _ => true,
            }
        })
        .collect();
    result.sort_by_cached_key(|&index| {
        (
            search.favorites_first
                && !settings
                    .library_favorites
                    .contains(&favorite_key(menu, kind, index)),
            super::normalized(&super::title(menu, kind, index)),
            origin(menu, kind, index),
            favorite_key(menu, kind, index),
        )
    });
    result
}

fn gauge_matches(gauges: impl Iterator<Item = Option<f64>>, vehicles: usize, gauge: f64) -> bool {
    let values = gauges.collect::<Vec<_>>();
    vehicles > 0
        && values.len() == vehicles
        && values
            .into_iter()
            .all(|m| m.is_some_and(|m| m.is_finite() && (m - gauge).abs() < 0.005))
}

pub(super) fn gauge_warning(audit: &openrailsrs_train::ConsistAudit) -> Option<&'static str> {
    let gauges = audit
        .compatibility
        .iter()
        .filter_map(|v| v.declared.curve.track_gauge_m)
        .filter(|m| m.is_finite() && *m > 0.)
        .collect::<Vec<_>>();
    if let Some(first) = gauges.first()
        && gauges.iter().any(|m| (m - first).abs() >= 0.005)
    {
        return Some("Trocha mixta: revisar compatibilidad de los coches");
    }
    if gauges.len() != audit.vehicles || gauges.is_empty() {
        return Some("Trocha: datos incompletos");
    }
    None
}
pub(in crate::player_ui) fn cycle_filter(search: &mut ChoiceSearch, filter: ChoiceFilter) {
    match filter {
        ChoiceFilter::Toggle => search.filters_expanded = !search.filters_expanded,
        ChoiceFilter::Country => search.country = (search.country + 1) % COUNTRIES.len(),
        ChoiceFilter::Gauge => search.gauge = (search.gauge + 1) % GAUGES.len(),
        ChoiceFilter::Traction => search.traction = (search.traction + 1) % TRACTIONS.len(),
        ChoiceFilter::Resources => search.resources = (search.resources + 1) % RESOURCES.len(),
        ChoiceFilter::Favorites => search.favorites_only = !search.favorites_only,
        ChoiceFilter::Reset => *search = default(),
    }
    search.page = 0;
    search.cursor = None;
    search.cursor_identity = None;
}
pub(in crate::player_ui) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    search: &ChoiceSearch,
    menu: &PlayerLaunchMenu,
    kind: ChoiceKind,
    settings: &PlayerSettings,
) {
    let index = match kind {
        ChoiceKind::Route => menu.route,
        ChoiceKind::Service => menu.service,
        ChoiceKind::Consist => menu.consist,
    };
    let count = match kind {
        ChoiceKind::Route => menu.routes.len(),
        ChoiceKind::Service => menu.services.len(),
        ChoiceKind::Consist => menu.consists.len(),
    };
    let active = usize::from(kind == ChoiceKind::Consist && search.country != 0)
        + usize::from(search.favorites_only)
        + usize::from(search.gauge != 0)
        + usize::from(search.traction != 0)
        + usize::from(search.resources != 0);
    row(p, |p| {
        button(
            p,
            format!(
                "{} · {active} activos",
                if search.filters_expanded {
                    "Ocultar filtros"
                } else {
                    "Filtros"
                }
            ),
            UiCommand::ChoiceFilter(kind, ChoiceFilter::Toggle),
        );
        if index < count {
            let favorite = settings
                .library_favorites
                .contains(&favorite_key(menu, kind, index));
            button(
                p,
                if favorite {
                    "★ Favorito"
                } else {
                    "☆ Favorito"
                },
                UiCommand::ToggleFavorite(kind, index),
            );
        }
        button(
            p,
            if search.favorites_first {
                "Orden: favoritos"
            } else {
                "Orden: nombre"
            },
            UiCommand::ChoiceSort(kind),
        );
    });
    if !search.filters_expanded {
        return;
    }
    row(p, |p| {
        if kind == ChoiceKind::Consist {
            button(
                p,
                crate::player_ui::menu_country::country_label(search.country),
                UiCommand::ChoiceFilter(kind, ChoiceFilter::Country),
            );
        }
        button(
            p,
            if search.favorites_only {
                "★ Favoritos"
            } else {
                "Todos / favoritos"
            },
            UiCommand::ChoiceFilter(kind, ChoiceFilter::Favorites),
        );
        button(
            p,
            "Quitar filtros",
            UiCommand::ChoiceFilter(kind, ChoiceFilter::Reset),
        );
    });
    if kind == ChoiceKind::Consist {
        row(p, |p| {
            button(
                p,
                GAUGES[search.gauge].map_or_else(
                    || "Todas las trochas".into(),
                    |m| format!("Trocha {:.0} mm", m * 1000.0),
                ),
                UiCommand::ChoiceFilter(kind, ChoiceFilter::Gauge),
            );
            button(
                p,
                TRACTIONS[search.traction],
                UiCommand::ChoiceFilter(kind, ChoiceFilter::Traction),
            );
            button(
                p,
                RESOURCES[search.resources],
                UiCommand::ChoiceFilter(kind, ChoiceFilter::Resources),
            );
        });
    }
    if kind == ChoiceKind::Consist
        && (search.gauge != 0 || search.traction != 0 || search.resources != 0)
    {
        label(
            p,
            "Los filtros se actualizan al terminar la auditoría. La trocha exige datos compatibles de todos los coches; mezcla o datos desconocidos no se consideran confirmación.",
            11.0,
            MUTED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gauge_requires_every_vehicle_and_distinguishes_missing_data() {
        assert!(gauge_matches(
            [Some(1.435), Some(1.435)].into_iter(),
            2,
            1.435
        ));
        for values in [
            [Some(1.435), Some(1.6)],
            [Some(1.435), None],
            [Some(1.435), Some(f64::NAN)],
        ] {
            assert!(!gauge_matches(values.into_iter(), 2, 1.435));
        }
        assert!(!gauge_matches([Some(1.435)].into_iter(), 2, 1.435));
        assert!(!gauge_matches([].into_iter(), 0, 1.435));
    }
    #[test]
    fn equal_names_and_origins_sort_by_identity_after_discovery_order_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut menu = PlayerLaunchMenu::discover(tmp.path(), None);
        menu.services.clear();
        menu.consists = vec![
            tmp.path().join("B/consists/Train.con"),
            tmp.path().join("A/consists/Train.con"),
        ];
        let expected = vec![menu.consists[1].clone(), menu.consists[0].clone()];
        let order = |menu: &PlayerLaunchMenu| {
            filtered(
                menu,
                ChoiceKind::Consist,
                &ChoiceSearch::default(),
                &PlayerSettings::default(),
            )
            .into_iter()
            .map(|i| menu.consists[i].clone())
            .collect::<Vec<_>>()
        };
        assert_eq!(order(&menu), expected);
        menu.consists.reverse();
        assert_eq!(order(&menu), expected);
    }
    #[test]
    fn ordering_keeps_identity_and_filtered_favorites_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut menu = PlayerLaunchMenu::discover(tmp.path(), None);
        menu.routes = vec!["Zulu".into(), "Álamo".into(), "Beta".into()];
        menu.route = 0;
        let mut settings = PlayerSettings::default();
        let mut search = ChoiceSearch::default();
        assert_eq!(
            filtered(&menu, ChoiceKind::Route, &search, &settings),
            vec![1, 2, 0]
        );
        settings
            .library_favorites
            .insert(favorite_key(&menu, ChoiceKind::Route, 0));
        search.favorites_first = true;
        assert_eq!(
            filtered(&menu, ChoiceKind::Route, &search, &settings),
            vec![0, 1, 2]
        );
        assert_eq!(menu.route, 0);
        search.query = "beta".into();
        assert_eq!(
            filtered(&menu, ChoiceKind::Route, &search, &settings),
            vec![2]
        );
    }
    #[test]
    fn incomplete_and_unknown_stock_stay_selectable_and_favorites_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut menu = PlayerLaunchMenu::discover(tmp.path(), None);
        menu.consists = vec![
            tmp.path().join("missing.con"),
            tmp.path().join("unknown.con"),
        ];
        menu.consist_audits.clear();
        let audit = openrailsrs_train::ConsistAuditor::new(vec![]).inspect(&menu.consists[0]);
        menu.consist_audits.insert(menu.consists[0].clone(), audit);
        let mut settings = PlayerSettings::default();
        let mut search = ChoiceSearch::default();
        assert_eq!(
            filtered(&menu, ChoiceKind::Consist, &search, &settings),
            vec![0, 1]
        );
        search.resources = 2;
        assert_eq!(
            filtered(&menu, ChoiceKind::Consist, &search, &settings),
            vec![0]
        );
        select(&mut menu, ChoiceKind::Consist, 0);
        assert_eq!(menu.consist, 0);
        search.resources = 0;
        search.favorites_only = true;
        settings
            .library_favorites
            .insert(favorite_key(&menu, ChoiceKind::Consist, 1));
        assert_eq!(
            filtered(&menu, ChoiceKind::Consist, &search, &settings),
            vec![1]
        );
        let restored: PlayerSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            filtered(&menu, ChoiceKind::Consist, &search, &restored),
            vec![1]
        );
        search.resources = 3;
        assert_eq!(
            filtered(&menu, ChoiceKind::Consist, &search, &restored),
            vec![1]
        );
    }
}
