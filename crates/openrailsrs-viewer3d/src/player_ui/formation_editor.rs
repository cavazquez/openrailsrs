use super::*;
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    editor: &crate::formation_builder::FormationBuilder,
    menu: &PlayerLaunchMenu,
    previews: Option<&previews::Previews>,
) {
    label(p, &editor.status, 13., ACCENT);
    label(
        p,
        "La copia propia conserva referencias a los ENG/WAG originales. Agregar, quitar, invertir y ordenar actualiza la vista completa; la revisión de recursos continúa en segundo plano.",
        13.,
        MUTED,
    );
    row(p, |p| {
        button(p, "Deshacer", UiCommand::FormationUndo);
        button(p, "Guardar formación propia", UiCommand::FormationSave);
    });
    if let Some(path) = menu.consists.get(menu.consist)
        && let Some(audit) = menu.consist_audits.get(path)
    {
        label(
            p,
            audit.label(),
            13.,
            if audit.player_ready() { GOOD } else { CAUTION },
        );
        label(p, audit.compatibility_summary(), 12., MUTED);
    } else {
        label(p, "Auditoría pendiente…", 13., MUTED);
    }
    if let Some(preview) = previews {
        previews::train_card(p, preview);
    }
    row(p, |p| {
        button(p, "Modelo anterior", UiCommand::FormationCatalogue(-1));
        let model = editor
            .catalogue
            .get(editor.selected)
            .map(|p| {
                p.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_else(|| "La auditoría aún no encontró vehículos".into());
        label(p, model, 14., TEXT);
        button(p, "Modelo siguiente", UiCommand::FormationCatalogue(1));
        button(p, "Agregar", UiCommand::FormationEdit(0, editor.selected));
    });
    for (index, slot) in editor.slots.iter().enumerate() {
        row(p, |p| {
            label(
                p,
                format!(
                    "{} · {}{}",
                    index + 1,
                    slot.file.file_stem().unwrap_or_default().to_string_lossy(),
                    if slot.flipped { " · invertido" } else { "" }
                ),
                14.,
                if slot.file.is_file() { TEXT } else { CAUTION },
            );
            button(p, "↑", UiCommand::FormationEdit(3, index));
            button(p, "↓", UiCommand::FormationEdit(4, index));
            button(p, "Invertir", UiCommand::FormationEdit(2, index));
            button(p, "Quitar", UiCommand::FormationEdit(1, index));
        });
    }
}
