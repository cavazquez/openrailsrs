use super::*;
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    editor: &crate::service_builder::ServiceBuilder,
) {
    label(p, &editor.status, 14., ACCENT);
    label(
        p,
        "Editá una copia del recorrido elegido. Generar conserva su sentido y sus vías; comprobá origen, destino, longitud y alimentación antes de guardar.",
        13.,
        MUTED,
    );
    for (origin, index, title) in [
        (true, editor.origin, "Origen"),
        (false, editor.destination, "Destino"),
    ] {
        row(p, |p| {
            button(p, "◀", UiCommand::ServiceChoice(origin, -1));
            label(
                p,
                format!(
                    "{title}: {}",
                    editor
                        .candidates
                        .get(index)
                        .map_or("Sin elegir", |c| c.name.as_str())
                ),
                14.,
                TEXT,
            );
            button(p, "▶", UiCommand::ServiceChoice(origin, 1));
        });
    }
    for (index, candidate) in editor.candidates.iter().enumerate() {
        row(p, |p| {
            label(p, &candidate.name, 13., TEXT);
            button(p, "Origen", UiCommand::ServiceStation(true, index));
            button(p, "Destino", UiCommand::ServiceStation(false, index));
        });
    }
    row(p, |p| {
        button(
            p,
            "Generar recorrido comprobado",
            UiCommand::ServiceGenerate,
        );
        button(p, "Deshacer", UiCommand::ServiceUndo);
        button(p, "Guardar servicio propio", UiCommand::ServiceSave);
    });
    let Some(s) = &editor.scenario else {
        return;
    };
    label(p, &s.scenario.name, 16., ACCENT);
    for (i, c) in editor
        .candidates
        .iter()
        .enumerate()
        .filter(|(i, _)| *i > editor.origin && *i < editor.destination)
    {
        button(
            p,
            format!(
                "{} {}",
                if s.route.stops.iter().any(|s| s.node == c.node) {
                    "●"
                } else {
                    "○"
                },
                c.name
            ),
            UiCommand::ServiceToggleStop(i),
        );
    }
    label(p, "Horarios y tiempo de parada", 18., TEXT);
    for (index, stop) in s.route.stops.iter().enumerate() {
        label(p, stop.name.as_deref().unwrap_or(&stop.node), 15., TEXT);
        for (field, name, value, step) in [
            (0, "Llegada", stop.arrive_s, 15.),
            (1, "Salida", stop.depart_s, 15.),
            (2, "Parada mínima", stop.dwell_s, 5.),
        ] {
            row(p, |p| {
                label(p, format!("{name}: {:.0} s", value), 13., TEXT);
                button(p, "−", UiCommand::ServiceStopEdit(index, field, -step));
                button(p, "+", UiCommand::ServiceStopEdit(index, field, step));
            });
        }
    }
    label(
        p,
        "La lista previsualiza el horario de toda la partida. Los errores de conexión, paradas, recursos o alimentación se informan antes de guardar; los archivos originales permanecen intactos.",
        12.,
        MUTED,
    );
}
