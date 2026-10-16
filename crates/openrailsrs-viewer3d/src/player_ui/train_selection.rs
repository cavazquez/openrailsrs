//! One cached assessment drives the two train lists and the launch button.
//! Missing infrastructure metadata is a warning, never a compatibility claim.
use super::*;
use openrailsrs_core::electrification::ElectricPickup;
use openrailsrs_train::ConsistAudit;

pub(super) const PAGE_SIZE: usize = 3;

#[derive(Default, Debug)]
pub(super) struct Availability {
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}
impl Availability {
    pub fn can_run(&self) -> bool {
        self.reasons.is_empty()
    }
    pub fn group(&self) -> usize {
        usize::from(!self.can_run())
    }
}

pub(super) fn route_status(audit: &ConsistAudit, rail: &previews::RailwayInfo) -> Availability {
    let mut status = Availability::default();
    if let crate::clearance::Assessment::Incompatible(message) =
        crate::clearance::compare(audit, rail.clearance.as_ref())
    {
        status.reasons.push(message);
    }
    for vehicle in &audit.compatibility {
        if let Some(gauge) = vehicle.declared.curve.track_gauge_m
            && gauge.is_finite()
            && gauge > 0.
            && rail.gauge_m.is_finite()
            && rail.gauge_m > 0.
            && (gauge - rail.gauge_m).abs() > 0.025
        {
            status.reasons.push(format!(
                "Trocha incompatible: coche {} de {:.0} mm · vía de {:.0} mm",
                vehicle.vehicle + 1,
                gauge * 1000.,
                rail.gauge_m * 1000.
            ));
            break;
        }
    }
    if rail.gauge_m <= 0.
        || !rail.gauge_m.is_finite()
        || audit.compatibility.len() != audit.vehicles
        || audit.compatibility.iter().any(|v| {
            v.declared
                .curve
                .track_gauge_m
                .is_none_or(|g| !g.is_finite() || g <= 0.)
        })
    {
        status
            .warnings
            .push("Trocha: faltan datos para confirmar la compatibilidad".into());
    }
    let engines: Vec<_> = audit
        .compatibility
        .iter()
        .filter(|v| v.declared.engine_type.is_some())
        .collect();
    // A mixed diesel/electric formation can move using its independent engines.
    // Never classify absent engine metadata as an electric-only formation.
    if !engines.is_empty()
        && engines.len() == audit.powered_vehicles
        && engines.iter().all(|v| v.declared.electric.is_some())
    {
        let supplies: Vec<_> = std::iter::once(&rail.supply)
            .chain(&rail.section_supplies)
            .collect();
        let matches = engines.iter().any(|v| {
            let engine = v.declared.electric.as_ref().unwrap();
            supplies.iter().any(|s| {
                s.kind == engine.pickup
                    && s.voltage_v.is_finite()
                    && s.voltage_v >= engine.minimum_voltage_v
                    && engine
                        .maximum_voltage_v
                        .is_none_or(|max| s.voltage_v <= max)
            })
        });
        let unknown_voltage = engines.iter().any(|v| {
            let engine = v.declared.electric.as_ref().unwrap();
            supplies
                .iter()
                .any(|s| s.kind == engine.pickup && (!s.voltage_v.is_finite() || s.voltage_v <= 0.))
        });
        if !matches && unknown_voltage {
            status.warnings.insert(
                0,
                "Alimentación: la ruta no declara una tensión comprobable".into(),
            );
        } else if !matches {
            status.reasons.push(format!(
                "Tren eléctrico sin alimentación compatible · ruta: {}",
                rail.supply.kind.label()
            ));
        } else if rail.supply.kind == ElectricPickup::None {
            status.warnings.insert(0, "Alimentación eléctrica sólo en algunos tramos; hay que gestionar los tramos sin corriente".into());
        }
    }
    status
}

pub(super) fn assess(
    menu: &PlayerLaunchMenu,
    index: usize,
    railway: Option<&previews::RailwayInfo>,
) -> Availability {
    let Some(path) = menu.consists.get(index) else {
        return Availability {
            reasons: vec!["No hay una formación seleccionada".into()],
            ..default()
        };
    };
    let Some(audit) = menu.consist_audits.get(path) else {
        return Availability {
            reasons: vec![menu.audit_error().map_or_else(
                || "Revisando archivos… todavía no se puede iniciar".into(),
                |e| format!("No se pudo revisar la formación: {e}"),
            )],
            ..default()
        };
    };
    let mut status = railway.map_or_else(
        || Availability {
            warnings: vec![
                "Datos de vía pendientes; compatibilidad de la ruta sin comprobar".into(),
            ],
            ..default()
        },
        |rail| route_status(audit, rail),
    );
    let missing: Vec<_> = audit
        .missing_resources
        .iter()
        .filter(|r| r.required)
        .collect();
    if !missing.is_empty() {
        status.reasons.insert(
            0,
            format!(
                "Faltan {} archivos necesarios: {}",
                missing.len(),
                missing[0].name
            ),
        );
    } else if let Some(error) = audit.errors.first() {
        status
            .reasons
            .insert(0, format!("Archivo inválido o faltante: {error}"));
    }
    if audit.vehicles == 0 {
        status
            .reasons
            .push("La formación no contiene vehículos válidos".into());
    } else if audit.powered_vehicles == 0 {
        status
            .reasons
            .push("Sin locomotora o vehículo con tracción".into());
    }
    if audit.missing_resources.iter().any(|r| !r.required) {
        status
            .warnings
            .push("Faltan recursos opcionales; se puede jugar".into());
    }
    status
}

pub(super) fn matches(
    menu: &PlayerLaunchMenu,
    search: &ChoiceSearch,
    settings: &PlayerSettings,
    railway: Option<&previews::RailwayInfo>,
) -> Vec<usize> {
    let mut result = choices::filtered(menu, ChoiceKind::Consist, search, settings);
    result.sort_by_key(|&index| assess(menu, index, railway).group());
    result
}

/// Keep an explicitly chosen train visible even when an audit moves it between
/// the available and unavailable lists. Paging the other list stays independent.
pub(super) fn reveal(
    search: &mut ChoiceSearch,
    menu: &PlayerLaunchMenu,
    index: usize,
    settings: &PlayerSettings,
    railway: Option<&previews::RailwayInfo>,
) {
    let matches = matches(menu, search, settings, railway);
    let Some(position) = matches.iter().position(|&i| i == index) else {
        return;
    };
    let group = assess(menu, index, railway).group();
    let rank = matches
        .iter()
        .take(position)
        .filter(|&&i| assess(menu, i, railway).group() == group)
        .count();
    search.train_pages[group] = rank / PAGE_SIZE;
    search.cursor = Some(position);
    search.cursor_identity = Some(choices::favorite_key(menu, ChoiceKind::Consist, index));
}

pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
    settings: &PlayerSettings,
    railway: Option<&previews::RailwayInfo>,
    preview: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    let search = &ui.choice_search[ChoiceKind::Consist.slot()];
    let matches = matches(menu, search, settings, railway);
    let cursor = choices::cursor_position(search, menu, ChoiceKind::Consist, &matches);
    p.spawn(Node {
        flex_wrap: FlexWrap::Wrap, align_items: AlignItems::FlexStart,
        column_gap: Val::Px(16.), row_gap: Val::Px(12.), flex_shrink: 0.,
        ..default()
    }).with_children(|p| {
        let column = || Node {
            flex_basis: Val::Px(250.), flex_grow: 1., min_width: Val::Px(0.),
            flex_direction: FlexDirection::Column, row_gap: Val::Px(7.), ..default()
        };
        for group in 0..2 {
            let entries: Vec<_> = matches.iter().copied()
                .filter(|&i| assess(menu, i, railway).group() == group).collect();
            p.spawn((column(), Name::new(if group == 0 { "train-ready-list" } else { "train-unavailable-list" })))
                .with_children(|p| {
                    let heading = label(p, format!("{} ({})", if group == 0 { "Pueden circular" } else { "No disponibles" }, entries.len()), 16., if group == 0 { GOOD } else { MUTED });
                    p.commands().entity(heading).insert(Name::new(if group == 0 { "train-ready-heading" } else { "train-unavailable-heading" }));
                    let pages = entries.len().div_ceil(PAGE_SIZE).max(1);
                    let cursor_index = cursor.and_then(|i| matches.get(i));
                    let page = cursor_index.and_then(|index| entries.iter().position(|i| i == index))
                        .filter(|_| ui.choice_focus == Some(ChoiceKind::Consist))
                        .map_or(search.train_pages[group], |i| i / PAGE_SIZE).min(pages - 1);
                    for &index in entries.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
                        let status = assess(menu, index, railway);
                        let selected = menu.consist == index;
                        p.spawn((Button, Node {
                            flex_direction: FlexDirection::Column, row_gap: Val::Px(3.),
                            padding: UiRect::all(Val::Px(8.)), flex_shrink: 0.,
                            min_width: Val::Px(0.), border: UiRect::left(Val::Px(3.)), ..default()
                        }, BackgroundColor(if group == 0 { FIELD } else { Color::srgb(0.08, 0.09, 0.11) }),
                        BorderColor::all(if selected || cursor_index == Some(&index) { ACCENT } else { FIELD }),
                        UiCommand::SelectChoice(ChoiceKind::Consist, index),
                        Name::new(format!("choice-Consist-{index}"))))
                        .with_children(|p| {
                            label(p, format!("{}{}", if selected { "✓ " } else { "" }, choices::title(menu, ChoiceKind::Consist, index)), 14., if group == 0 { TEXT } else { MUTED });
                            label(p, menu.consist_origin_label(index), 11., MUTED);
                            if let Some(reason) = status.reasons.first() {
                                label(p, launcher::short(reason, 120), 11., CAUTION);
                            } else if let Some(warning) = status.warnings.first() {
                                label(p, warning, 11., MUTED);
                            }
                        });
                    }
                    if entries.is_empty() {
                        label(p, if group == 0 { "No hay trenes disponibles con esta búsqueda. Revisá la otra lista o abrí Biblioteca." } else { "No hay impedimentos detectados en las formaciones de esta búsqueda." }, 12., MUTED);
                    }
                    if pages > 1 {
                        row(p, |p| {
                            button(p, "←", UiCommand::TrainChoicePage(group, -1));
                            label(p, format!("{} / {pages}", page + 1), 12., MUTED);
                            button(p, "→", UiCommand::TrainChoicePage(group, 1));
                        });
                    }
                });
        }
        p.spawn(column()).with_children(preview);
    });
    if matches.is_empty() {
        button(p, "Abrir Biblioteca", UiCommand::Open(PlayerPanel::Content));
    }
    label(
        p,
        "La disponibilidad usa los archivos y datos declarados de esta ruta. Los trenes en gris se pueden seleccionar para consultar el motivo.",
        11.,
        MUTED,
    );
    if ui.choice_focus == Some(ChoiceKind::Consist) {
        label(
            p,
            "↑/↓ · Inicio/Fin · RePág/AvPág · Enter seleccionar · Esc salir",
            11.,
            MUTED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::electrification::{ElectricSupply, ElectricVehicleParams};
    fn audit(kind: &str, gauge: f64) -> ConsistAudit {
        let ast =
            openrailsrs_formats::parse_named_stf(&format!("Engine ( Type ( {kind} ) )")).unwrap();
        let mut declared = openrailsrs_formats::parse_vehicle_content_metadata(&ast, true);
        declared.curve.track_gauge_m = Some(gauge);
        declared.size_m = Some([3., 4., 20.]);
        if kind == "Electric" {
            declared.electric = Some(ElectricVehicleParams {
                minimum_voltage_v: 20000.,
                maximum_voltage_v: Some(30000.),
                ..default()
            });
        }
        ConsistAudit {
            vehicles: 1,
            powered_vehicles: 1,
            compatibility: vec![openrailsrs_train::consist_audit::VehicleCompatibility {
                vehicle: 0,
                stock_path: "own.eng".into(),
                source_sha256: String::new(),
                declared,
                limitations: vec![],
            }],
            ..default()
        }
    }
    fn rail() -> previews::RailwayInfo {
        previews::RailwayInfo {
            gauge_m: 1.435,
            supply: ElectricSupply {
                kind: ElectricPickup::None,
                voltage_v: 0.,
            },
            section_supplies: vec![],
            missing: vec![],
            clearance: None,
        }
    }
    #[test]
    fn known_track_and_power_impediments_are_distinct_from_partial_information() {
        let diesel = audit("Diesel", 1.435);
        let electric = audit("Electric", 1.435);
        let mut route = rail();
        assert!(route_status(&diesel, &route).can_run());
        assert!(route_status(&electric, &route).reasons[0].contains("alimentación"));
        assert!(
            route_status(&audit("Diesel", 1.6), &route).reasons[0].contains("Trocha incompatible")
        );
        route.section_supplies.push(ElectricSupply {
            kind: ElectricPickup::Overhead,
            voltage_v: 25000.,
        });
        let partial = route_status(&electric, &route);
        assert!(partial.can_run());
        assert!(
            partial
                .warnings
                .iter()
                .any(|w| w.contains("algunos tramos"))
        );
        route.supply = route.section_supplies.pop().unwrap();
        assert!(route_status(&electric, &route).can_run());
        route.supply.voltage_v = 1500.;
        assert!(!route_status(&electric, &route).can_run());
        route.supply.voltage_v = 0.;
        let unknown = route_status(&electric, &route);
        assert!(unknown.can_run());
        assert!(unknown.warnings.iter().any(|w| w.contains("tensión")));
        let mut mixed = electric;
        mixed.vehicles += 1;
        mixed.powered_vehicles += 1;
        mixed.compatibility.extend(diesel.compatibility);
        assert!(route_status(&mixed, &rail()).can_run());
    }
    #[test]
    fn required_assets_static_stock_and_pending_audits_do_not_hide_optional_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let mut menu = PlayerLaunchMenu::discover(tmp.path(), None);
        menu.consists = vec![tmp.path().join("own.con")];
        menu.consist = 0;
        menu.consist_audits.clear();
        assert!(!assess(&menu, 0, None).can_run());
        let mut report = audit("Diesel", 1.435);
        report
            .missing_resources
            .push(openrailsrs_train::consist_audit::MissingResource {
                name: "optional.wav".into(),
                referenced_by: "own.sms".into(),
                destinations: vec![],
                required: false,
            });
        menu.consist_audits.insert(menu.consists[0].clone(), report);
        assert!(assess(&menu, 0, None).can_run());
        assert!(
            assess(&menu, 0, None)
                .warnings
                .iter()
                .any(|w| w.contains("sin comprobar"))
        );
        menu.consist_audits
            .get_mut(&menu.consists[0])
            .unwrap()
            .missing_resources[0]
            .required = true;
        let blocked = assess(&menu, 0, None);
        assert!(!blocked.can_run());
        assert!(blocked.reasons[0].contains("optional.wav"));
        let report = menu.consist_audits.get_mut(&menu.consists[0]).unwrap();
        report.missing_resources.clear();
        report.powered_vehicles = 0;
        assert!(assess(&menu, 0, None).reasons[0].contains("tracción"));
    }
    #[test]
    fn declared_clearance_impediments_block_but_missing_gauge_data_is_only_a_warning() {
        let mut route = rail();
        let mut train = audit("Diesel", 1.435);
        train.compatibility[0].declared.curve.track_gauge_m = None;
        assert!(route_status(&train, &route).can_run());
        assert!(!route_status(&train, &route).warnings.is_empty());
        route.clearance = Some(crate::clearance::InfrastructureEnvelope {
            source: "Own test dimensions".into(),
            max_width_m: 2.5,
            max_height_m: 4.5,
            radius_m: Some(0.),
            raised_collectors_verified: false,
        });
        assert!(route_status(&train, &route).reasons[0].contains("Gálibo incompatible"));
    }
}
