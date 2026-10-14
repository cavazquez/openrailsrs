use super::*;
pub(super) fn path_label(p: &mut ChildSpawnerCommands<'_>, path: &std::path::Path) {
    p.spawn((
        Text::new(path.display().to_string()),
        TextFont {
            font_size: FontSize::Px(12.),
            ..default()
        },
        TextColor(MUTED),
        TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter),
        Node {
            width: Val::Percent(100.),
            max_width: Val::Percent(100.),
            min_width: Val::Px(0.),
            flex_shrink: 0.,
            ..default()
        },
    ));
}
pub(super) fn clipboard_text(clipboard: &mut bevy_clipboard::Clipboard) -> Result<String, String> {
    clipboard
        .fetch_text()
        .poll_result()
        .ok_or("La lectura del portapapeles está pendiente; volvé a pegar".to_string())?
        .map_err(|e| e.to_string())
}
pub(super) fn input_keys(
    mut events: MessageReader<KeyboardInput>,
    mut ui: ResMut<PlayerUiState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clipboard: Option<ResMut<bevy_clipboard::Clipboard>>,
) {
    for event in events.read() {
        let Some(field) = ui.library_focus else {
            continue;
        };
        if event.state != ButtonState::Pressed || ui.panel != PlayerPanel::Content {
            continue;
        }
        let value = if field == 0 {
            &mut ui.library_url
        } else {
            &mut ui.library_folder
        };
        if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
            if event.key_code == KeyCode::KeyV
                && let Some(clipboard) = &mut clipboard
                && let Ok(text) = clipboard_text(clipboard)
            {
                let room = 2048usize.saturating_sub(value.len());
                value.extend(text.chars().scan(0usize, |bytes, c| {
                    *bytes += c.len_utf8();
                    (*bytes <= room).then_some(c)
                }));
                ui.rebuild = true;
            }
            continue;
        }
        match &event.logical_key {
            Key::Backspace => {
                value.pop();
            }
            Key::Enter => {
                ui.library_focus = None;
            }
            Key::Character(text) if value.len() + text.len() <= 2048 => {
                value.push_str(text);
            }
            _ => continue,
        }
        ui.rebuild = true;
    }
}
pub(super) fn build_inputs(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    downloads: &crate::official_content::OfficialContent,
    tools: &crate::library_tools::LibraryTools,
) {
    if ui.library_tab == LibraryTab::Official {
        launcher::card(p, |p| {
            label(p, "Instalar desde URL", 18., TEXT);
            row(p, |p| {
                button(
                    p,
                    if ui.library_url.is_empty() {
                        "Pegá la URL del autor…"
                    } else {
                        &ui.library_url
                    },
                    UiCommand::LibraryFocus(0),
                );
                button(p, "Pegar", UiCommand::LibraryPaste(0));
                button(p, "Comprobar enlace", UiCommand::LibraryUrlInspect);
            });
            label(p, &downloads.url_status, 12., MUTED);
            if let Some(source) = &downloads.url_preview {
                label(
                    p,
                    format!(
                        "{} · autor / cuenta: {}\n{}\nDescarga en {}",
                        source.name,
                        source.author.name,
                        source.url,
                        openrailsrs_content::content_dir().display()
                    ),
                    13.,
                    TEXT,
                );
                label(
                    p,
                    if source.download_bytes > 0 {
                        format!(
                            "Descarga {:.0} MiB · instalado {:.1} GiB",
                            source.download_bytes as f64 / 1048576.,
                            source.install_bytes as f64 / 1073741824.
                        )
                    } else {
                        "Tamaño no informado: se controla el espacio durante descarga y extracción"
                            .into()
                    },
                    12.,
                    MUTED,
                );
                if let Some(revision) = &source.requested_ref {
                    label(p, format!("Revisión elegida: {revision}"), 12., MUTED);
                }
                if let Some(kind) = &source.package_type {
                    label(
                        p,
                        match kind.as_str() {
                            "repository" => {
                                "Repositorio GitHub; rutas y trenes se identifican al auditar"
                            }
                            _ => "Archivo ZIP directo del catálogo",
                        },
                        12.,
                        MUTED,
                    );
                }
                if let Some(commit) = &source.resolved_commit {
                    label(
                        p,
                        format!(
                            "Revisión disponible: {}",
                            commit.chars().take(12).collect::<String>()
                        ),
                        12.,
                        MUTED,
                    );
                }
                if let Some(date) = &source.source_date {
                    label(p, format!("Fecha del origen: {date}"), 12., MUTED);
                }
                button(
                    p,
                    if source.requires_confirmation {
                        "Confirmo que es el origen del autor · instalar"
                    } else {
                        "Instalar este contenido"
                    },
                    UiCommand::LibraryUrlInstall(source.requires_confirmation),
                );
            }
            label(
                p,
                "Repositorios GitHub y ZIP directos del catálogo. Los enlaces fuera del catálogo requieren comprobar el autor; nunca se ejecuta un instalador del contenido.",
                12.,
                MUTED,
            );
        });
    } else if ui.library_tab == LibraryTab::Tools {
        launcher::card(p, |p| {
            label(p, "Carpeta para descargar contenido", 18., TEXT);
            path_label(p, &openrailsrs_content::content_dir());
            row(p, |p| {
                button(
                    p,
                    if ui.library_folder.is_empty() {
                        "Escribí una carpeta o explorá…"
                    } else {
                        &ui.library_folder
                    },
                    UiCommand::LibraryFocus(1),
                );
                button(p, "Pegar", UiCommand::LibraryPaste(1));
                button(p, "Explorar carpeta", UiCommand::LibraryBrowse);
            });
            if let Some((path, directories)) = &tools.folder {
                path_label(p, path);
                row(p, |p| {
                    button(p, "Subir una carpeta", UiCommand::LibraryParent);
                    button(p, "Usar esta carpeta", UiCommand::LibraryStorage);
                });
                for (index, directory) in directories.iter().enumerate() {
                    button(
                        p,
                        format!(
                            "▸ {}",
                            directory.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        UiCommand::LibraryDirectory(index),
                    );
                }
            }
            label(
                p,
                "Se crea openrailsrs-content dentro de la carpeta elegida. Las partidas y las instalaciones anteriores se conservan. Snap sólo permite acceder a carpetas habilitadas por sus permisos.",
                12.,
                MUTED,
            );
        });
    }
}
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
