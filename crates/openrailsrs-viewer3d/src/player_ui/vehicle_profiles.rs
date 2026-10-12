use super::*;
use openrailsrs_core::vehicle_profile::VehicleProfile;
use openrailsrs_train::consist_audit::VehicleCompatibility;

pub(super) fn stocks(menu: &PlayerLaunchMenu) -> Vec<&VehicleCompatibility> {
    let Some(audit) = menu
        .consists
        .get(menu.consist)
        .and_then(|p| menu.consist_audits.get(p))
    else {
        return vec![];
    };
    let mut seen = std::collections::HashSet::new();
    audit
        .compatibility
        .iter()
        .filter(|s| s.source_sha256.len() == 64 && seen.insert(s.source_sha256.clone()))
        .collect()
}
pub(super) fn adjust(
    menu: &PlayerLaunchMenu,
    settings: &mut PlayerSettings,
    index: usize,
    change: Option<(u8, f64)>,
) -> Result<(), String> {
    let list = stocks(menu);
    let stock = list
        .get(index.min(list.len().saturating_sub(1)))
        .ok_or("Esperá la auditoría de la formación")?;
    let key = &stock.source_sha256;
    if let Some((field, delta)) = change {
        if settings.vehicle_profiles.len() >= 256 && !settings.vehicle_profiles.contains_key(key) {
            return Err("Hay 256 perfiles propios. Restaurá uno antes de agregar otro.".into());
        }
        let p = settings
            .vehicle_profiles
            .entry(key.clone())
            .or_insert_with(|| VehicleProfile {
                source_sha256: key.clone(),
                ..default()
            });
        let value = match field {
            0 => &mut p.mass_factor,
            1 => &mut p.brake_factor,
            _ => &mut p.resistance_factor,
        };
        *value = (*value + delta).clamp(0.25, 4.);
    } else {
        settings.vehicle_profiles.remove(key);
    }
    Ok(())
}
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    menu: &PlayerLaunchMenu,
    s: &PlayerSettings,
) {
    label(
        p,
        "Los cambios se aplican al cargar una nueva partida. ENG, WAG e Includes originales se conservan. Una física personalizada requiere su propia validación; no hereda la paridad del original.",
        14.,
        MUTED,
    );
    let list = stocks(menu);
    let Some(stock) = list.get(ui.profile_car.min(list.len().saturating_sub(1))) else {
        label(p, "Esperando los datos del tren seleccionado", 15., MUTED);
        return;
    };
    row(p, |p| {
        button(p, "← Vehículo", UiCommand::ProfileCar(-1));
        label(
            p,
            stock
                .stock_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            16.,
            TEXT,
        );
        button(p, "Vehículo →", UiCommand::ProfileCar(1));
    });
    label(
        p,
        format!("Versión de origen: {}", &stock.source_sha256[..12]),
        13.,
        MUTED,
    );
    let profile = s
        .vehicle_profiles
        .get(&stock.source_sha256)
        .cloned()
        .unwrap_or_default();
    for (field, name, value) in [
        (0, "Masa", profile.mass_factor),
        (1, "Fuerza máxima de freno", profile.brake_factor),
        (2, "Resistencia Davis", profile.resistance_factor),
    ] {
        row(p, |p| {
            label(
                p,
                format!("{name}: autor 100% → propio {:.0}%", value * 100.),
                15.,
                TEXT,
            );
            button(p, "− 5%", UiCommand::ProfileAdjust(field, -0.05));
            button(p, "+ 5%", UiCommand::ProfileAdjust(field, 0.05));
        });
    }
    label(
        p,
        "El perfil alcanza a las copias de este mismo vehículo en la formación. Una versión distinta del autor empieza con sus valores originales. La partida guardada conserva los factores que usó.",
        13.,
        MUTED,
    );
    button(p, "Restaurar valores del autor", UiCommand::ProfileReset);
    button(p, "Guardar ajustes", UiCommand::SaveSettings);
}
