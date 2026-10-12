use super::*;
pub(super) fn poll(
    mut tools: ResMut<crate::library_tools::LibraryTools>,
    mut ui: ResMut<PlayerUiState>,
    mut menu: ResMut<PlayerLaunchMenu>,
    mut downloads: ResMut<crate::official_content::OfficialContent>,
    mut history: ResMut<crate::save_history::SaveHistory>,

    mut reports: ResMut<crate::issue_report::Reports>,
) {
    if reports.poll() {
        ui.rebuild = true;
    }
    if ui.panel != PlayerPanel::Content || ui.library_tab != LibraryTab::Tools {
        tools.cancel_read();
    }
    if let Some(content_changed) = tools.poll() {
        if content_changed {
            downloads.refresh_installed();
            menu.refresh_installed_content();
        }
        history.request(player_data_dir());
        ui.rebuild = true;
    }
}
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    downloads: &crate::official_content::OfficialContent,
    tools: &crate::library_tools::LibraryTools,
) {
    label(p, &tools.status, 14., ACCENT);
    if tools.busy() {
        label(
            p,
            "Operación en curso. Los archivos del autor se conservan.",
            13.,
            MUTED,
        );
        return;
    }
    label(p, "Respaldos de ajustes y partidas", 18., TEXT);
    row(p, |p| {
        button(p, "Crear respaldo de lo guardado", UiCommand::BackupExport);
        button(p, "Exportar partidas portátiles", UiCommand::PortableExport);
        if tools.imported_settings.is_some() {
            button(p, "Usar ajustes importados", UiCommand::ApplyBackupSettings);
        }
    });
    label(
        p,
        format!(
            "Importar crea copias nuevas en Continuar; conserva las ranuras actuales. Para traer un respaldo de otro equipo, copialo en {}.",
            player_data_dir().join("backups").display()
        ),
        13.,
        MUTED,
    );
    for (index, path) in tools.backups.iter().enumerate() {
        button(
            p,
            format!(
                "Importar {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            UiCommand::BackupImport(index),
        );
    }
    label(p, "Versiones instaladas", 18., TEXT);
    if let Some(base) = &tools.compare_base {
        label(
            p,
            format!(
                "Referencia: {}",
                crate::official_content::installed_label(base)
            ),
            13.,
            ACCENT,
        );
    }
    for (index, path) in downloads.installed.iter().enumerate() {
        row(p, |p| {
            label(p, crate::official_content::installed_label(path), 13., TEXT);
            button(p, "Usar como referencia", UiCommand::VersionBase(index));
            if tools.compare_base.as_ref().is_some_and(|b| b != path) {
                button(
                    p,
                    "Comparar con referencia",
                    UiCommand::VersionCompare(index),
                );
            }
            button(p, "Revisar dependencias", UiCommand::InspectRemoval(index));
        });
    }
    if let Some(diff) = &tools.comparison {
        label(
            p,
            format!(
                "{} → {}\nNuevos {} · modificados {} · retirados {}",
                diff.before, diff.after, diff.added, diff.changed, diff.removed
            ),
            14.,
            ACCENT,
        );
        for detail in &diff.details {
            label(p, detail, 12., TEXT);
        }
        for save in &diff.saves {
            label(
                p,
                format!("Esta partida conserva la versión anterior: {save}"),
                12.,
                CAUTION,
            );
        }
    }
    if let Some(plan) = &tools.plan {
        super::launcher::card(p, |p| {
            label(p, &plan.label, 16., CAUTION);
            label(
                p,
                format!(
                    "Usada por {} partidas y {} paquetes. Se retira sólo esta copia; se conserva en recuperación y las partidas siguen disponibles.",
                    plan.saves.len(),
                    plan.packages.len()
                ),
                13.,
                TEXT,
            );
            for item in plan.saves.iter().chain(&plan.packages) {
                label(p, item, 12., CAUTION);
            }
            button(
                p,
                "Retirar esta copia · recuperable",
                UiCommand::RemoveSelected,
            );
        });
    }
    if !tools.trash.is_empty() {
        label(p, "Copias recuperables", 18., TEXT);
        for (index, entry) in tools.trash.iter().enumerate() {
            button(
                p,
                format!("Recuperar {}", entry.label),
                UiCommand::RestoreContent(index),
            );
        }
    }
}
