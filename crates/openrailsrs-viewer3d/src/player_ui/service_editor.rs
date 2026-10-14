use super::*;
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    editor: &crate::service_builder::ServiceBuilder,
) {
    label(p, &editor.status, 14., ACCENT);
    if let Some(graph) = &editor.graph {
        label(
            p,
            "Itinerario sobre el mapa · seleccioná los puntos en orden",
            16.,
            ACCENT,
        );
        let mut nodes = graph
            .nodes_iter()
            .map(|(id, n)| (id.to_string(), n.x_m, n.y_m))
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.0.cmp(&b.0));
        let center = editor
            .scenario
            .as_ref()
            .and_then(|s| {
                graph.node(
                    s.route
                        .legs
                        .last()
                        .map_or(&s.route.start, |l| &l.destination),
                )
            })
            .map_or((0., 0.), |n| (n.x_m, n.y_m));
        nodes.sort_by(|a, b| {
            ((a.1 - center.0).hypot(a.2 - center.1))
                .total_cmp(&(b.1 - center.0).hypot(b.2 - center.1))
        });
        nodes.truncate(128);
        let bounds = nodes
            .iter()
            .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, n| {
                (b.0.min(n.1), b.1.min(n.2), b.2.max(n.1), b.3.max(n.2))
            });
        p.spawn((
            Name::new("itinerary-map"),
            Node {
                width: px(600),
                max_width: percent(100),
                height: px(220),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                ..default()
            },
            BackgroundColor(FIELD),
        ))
        .with_children(|p| {
            let point = |id: &str| {
                nodes.iter().find(|n| n.0 == id).map(|n| {
                    Vec2::new(
                        ((n.1 - bounds.0) / (bounds.2 - bounds.0).max(1.) * 560. + 12.) as f32,
                        ((n.2 - bounds.1) / (bounds.3 - bounds.1).max(1.) * 190. + 10.) as f32,
                    )
                })
            };
            for (id, edge) in graph.edges_iter() {
                let Some((a, b)) = point(&edge.from.0).zip(point(&edge.to.0)) else {
                    continue;
                };
                let selected = editor
                    .scenario
                    .as_ref()
                    .is_some_and(|s| s.route.legs.iter().any(|l| l.edges.iter().any(|e| e == id)));
                line(
                    p,
                    a,
                    b,
                    if selected { 3. } else { 1. },
                    if selected { ACCENT } else { MUTED },
                );
            }
            for (id, x, y) in &nodes {
                let xx = ((*x - bounds.0) / (bounds.2 - bounds.0).max(1.) * 94. + 2.) as f32;
                let yy = ((*y - bounds.1) / (bounds.3 - bounds.1).max(1.) * 90. + 2.) as f32;
                p.spawn((
                    Button,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(xx),
                        top: percent(yy),
                        width: px(16),
                        height: px(16),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                    UiCommand::ItineraryPoint(id.clone()),
                    Name::new(format!("itinerary-node:{id}")),
                ))
                .with_children(|p| {
                    label(p, id, 9., TEXT);
                });
            }
        });
        row(p, |p| {
            button(p, "Invertir y regresar", UiCommand::ItineraryReverse);
            button(p, "Espera −30 s", UiCommand::ItineraryWait(-30.));
            button(p, "Espera +30 s", UiCommand::ItineraryWait(30.));
        });
        if let Some(s) = &editor.scenario {
            for (i, l) in s.route.legs.iter().enumerate() {
                label(
                    p,
                    format!(
                        "{} · {} → {} · {} · espera {:.0} s",
                        i + 1,
                        l.start,
                        l.destination,
                        if l.reversed { "invertido" } else { "adelante" },
                        l.wait_s
                    ),
                    13.,
                    TEXT,
                );
            }
        }
    }
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
