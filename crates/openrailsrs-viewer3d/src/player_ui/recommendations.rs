use super::launcher::card;
use super::*;
struct Suggestion {
    index: usize,
    score: i32,
    reason: String,
}
fn suggestions(menu: &PlayerLaunchMenu, previews: Option<&previews::Previews>) -> Vec<Suggestion> {
    let mut result = vec![];
    for (index, path) in menu.consists.iter().enumerate() {
        let Some(audit) = menu.consist_audits.get(path) else {
            continue;
        };
        let mut reason = vec![];
        let mut score = 0;
        if audit.player_ready() {
            score += 40;
            reason.push("recursos y tracción disponibles".into());
        } else {
            score -= 100;
            reason.push(if audit.powered_vehicles == 0 {
                "material sin tracción".into()
            } else {
                "recursos incompletos".into()
            });
        }
        if menu.preview.author_consist.as_ref() == Some(path) {
            score += 30;
            reason.push("formación del servicio original".into());
        }
        let origin = menu.consist_origin_label(index);
        reason.push(origin);
        if let Some(rail) = previews.and_then(|p| p.railway.as_ref()) {
            if let Some(warning) = previews::railway_warning(audit, rail) {
                score -= 60;
                reason.push(warning);
            } else {
                let explicit = audit
                    .compatibility
                    .iter()
                    .all(|v| v.declared.curve.track_gauge_m.is_some());
                if explicit && rail.gauge_m > 0. {
                    score += 15;
                    reason.push("trocha declarada compatible".into());
                } else {
                    reason.push("sin incompatibilidad declarada; trocha parcial".into());
                }
                if audit
                    .compatibility
                    .iter()
                    .any(|v| v.declared.electric.is_some())
                {
                    score += 5;
                    reason.push("hay alimentación compatible; revisar cobertura por tramos".into());
                }
            }
        } else {
            reason.push("alimentación y trocha de ruta aún sin comprobar".into());
        }
        if audit.cab_3d || audit.cab_2d {
            score += 5;
            reason.push("cabina disponible".into());
        }
        result.push(Suggestion {
            index,
            score,
            reason: reason.join(" · "),
        });
    }
    result.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    result.truncate(3);
    result
}
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    menu: &PlayerLaunchMenu,
    previews: Option<&previews::Previews>,
) {
    card(p, |p| {
        label(p, "Formaciones sugeridas", 16., TEXT);
        let choices = suggestions(menu, previews);
        if choices.is_empty() {
            label(
                p,
                "Las sugerencias aparecen al terminar la auditoría. Todas las formaciones siguen en la lista.",
                13.,
                MUTED,
            );
        }
        for c in choices {
            let name = menu.consists[c.index]
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            button(
                p,
                name,
                UiCommand::SelectChoice(ChoiceKind::Consist, c.index),
            );
            label(p, c.reason, 12., if c.score >= 0 { MUTED } else { CAUTION });
        }
        label(
            p,
            "La recomendación orienta la elección; no certifica todos los sistemas ni oculta opciones con avisos.",
            12.,
            MUTED,
        );
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn author_stock_is_explained_but_missing_stock_stays_selectable() {
        let mut menu = PlayerLaunchMenu::default();
        menu.consists = vec!["/tmp/a.con".into(), "/tmp/b.con".into()];
        menu.consist_audits.clear();
        for p in &menu.consists {
            menu.consist_audits.insert(
                p.clone(),
                openrailsrs_train::ConsistAudit {
                    vehicles: 1,
                    powered_vehicles: 1,
                    ..default()
                },
            );
        }
        menu.preview.author_consist = Some(menu.consists[1].clone());
        let list = suggestions(&menu, None);
        assert_eq!(list[0].index, 1);
        assert!(list[0].reason.contains("servicio original"));
        menu.consist_audits
            .get_mut(&menu.consists[1])
            .unwrap()
            .errors
            .push("missing".into());
        assert_eq!(suggestions(&menu, None)[0].index, 0);
        assert_eq!(menu.consists.len(), 2);
    }
}
