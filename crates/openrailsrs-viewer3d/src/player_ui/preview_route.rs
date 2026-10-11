//! Menu schematic from the selected service's actual graph and authored path.
use super::*;
use openrailsrs_route::path::{edge_path_ignoring_switches, edge_path_via_waypoints};

pub(super) fn load(key: &RouteKey) -> Result<(Image, String), String> {
    let track = key.imported.join("track.toml");
    if std::fs::metadata(&track).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
        return Err("El mapa importado supera el presupuesto de la vista previa".into());
    }
    let loaded =
        openrailsrs_route::load_route_from_dir(&key.imported).map_err(|e| e.to_string())?;
    let mut scenario: openrailsrs_scenarios::ScenarioFile = if let Some(id) = &key.native_timetable
    {
        let consist = openrailsrs_msts::prepare_timetable_consist(
            &key.root,
            &key.source,
            id,
            &crate::player_settings::player_data_dir().join("cache/timetable"),
        )
        .map_err(|e| e.to_string())?;
        openrailsrs_msts::import_native_timetable_with_consist(
            &key.root,
            &key.source,
            id,
            &key.imported,
            &consist,
        )
        .map_err(|e| e.to_string())?
    } else if key.native_activity {
        bounded(&key.source)?;
        let (text, _, _) = openrailsrs_msts::import_activity_with_summary(
            &key.root,
            &key.source,
            Some(&key.imported),
        )
        .map_err(|e| e.to_string())?;
        toml::from_str(&text).map_err(|e| e.to_string())?
    } else {
        openrailsrs_scenarios::load_scenario(&key.source).map_err(|e| e.to_string())?
    };
    let mut graph = loaded.graph;
    for switch in &scenario.route.switches {
        graph
            .set_switch(
                &switch.node,
                match switch.position {
                    openrailsrs_scenarios::model::SwitchPositionDef::Straight => {
                        openrailsrs_track::SwitchPosition::Straight
                    }
                    openrailsrs_scenarios::model::SwitchPositionDef::Diverging => {
                        openrailsrs_track::SwitchPosition::Diverging
                    }
                },
            )
            .map_err(|e| e.to_string())?;
    }
    if let Some(path) = &key.path {
        bounded(path)?;
        let hints = openrailsrs_msts::placement_for_pat(&graph, &loaded.msts_aliases, path, 0.0)
            .map_err(|e| e.to_string())?;
        scenario.route.start = hints.start;
        scenario.route.destination = hints.destination;
        scenario.route.start_offset_m = Some(hints.start_offset_m);
        scenario.route.waypoints = openrailsrs_msts::pat_waypoints_with_offset(
            &graph,
            &loaded.msts_aliases,
            &openrailsrs_formats::PathFile::from_path(path).map_err(|e| e.to_string())?,
            &scenario.route.start,
            &scenario.route.destination,
            hints.start_offset_m,
        )
        .map_err(|e| e.to_string())?;
        scenario.route.stops.clear();
        scenario.route.legs.clear();
    }
    let path = if !scenario.route.legs.is_empty() {
        Ok(scenario
            .route
            .legs
            .iter()
            .flat_map(|leg| leg.edges.clone())
            .collect())
    } else if scenario.route.waypoints.len() >= 2 {
        edge_path_via_waypoints(&graph, &scenario.route.waypoints)
    } else {
        edge_path_ignoring_switches(&graph, &scenario.route.start, &scenario.route.destination)
    }
    .map_err(|e| e.to_string())?;
    let points = graph
        .nodes_iter()
        .map(|(_, n)| (n.x_m, n.y_m))
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return Err("No hay coordenadas para dibujar el recorrido".into());
    }
    let xmin = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let xmax = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let ymin = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let ymax = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let span = (xmax - xmin).max((ymax - ymin) * 2.0);
    if !span.is_finite() || span < 0.01 {
        return Err("El mapa no tiene extensión geográfica".into());
    }
    let scale = 704.0 / span;
    let project = |x: f64, y: f64| {
        (
            (384.0 + (x - (xmin + xmax) * 0.5) * scale)
                .round()
                .clamp(0.0, 767.0) as i32,
            (192.0 - (y - (ymin + ymax) * 0.5) * scale)
                .round()
                .clamp(0.0, 383.0) as i32,
        )
    };
    let node = |id: &str| {
        graph
            .node(id)
            .filter(|n| n.x_m.is_finite() && n.y_m.is_finite())
            .map(|n| project(n.x_m, n.y_m))
    };
    let mut canvas = image::RgbaImage::from_pixel(768, 384, image::Rgba([17, 25, 36, 255]));
    for (_, edge) in graph
        .edges_iter()
        .filter(|(_, e)| !e.id.0.ends_with("_r"))
        .take(50_000)
    {
        if let (Some(a), Some(b)) = (node(&edge.from.0), node(&edge.to.0)) {
            line(&mut canvas, a, b, [57, 72, 87, 255], 1);
        }
    }
    let default_supply = std::fs::read_dir(&key.root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .find_map(|entry| {
            (entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("trk")))
            .then(|| openrailsrs_formats::RouteFile::from_trk_path(&entry.path()).ok())
            .flatten()
        })
        .map(|route| openrailsrs_core::electrification::ElectricSupply {
            kind: if route.overhead_wire.electrified {
                openrailsrs_core::electrification::ElectricPickup::Overhead
            } else {
                Default::default()
            },
            voltage_v: route.max_line_voltage_v,
        })
        .unwrap_or_default();
    let supply = scenario
        .route
        .electric_supply
        .as_ref()
        .map_or(&default_supply, |s| &s.supply);
    let powered = |s: &openrailsrs_core::electrification::ElectricSupply| {
        s.kind != openrailsrs_core::electrification::ElectricPickup::None && s.voltage_v > 0.0
    };
    let edge_point = |id: &str, position: f64| {
        let edge = graph.edge(id)?;
        let a = graph.node(&edge.from.0)?;
        let b = graph.node(&edge.to.0)?;
        let fraction = (position / edge.length_m.max(0.01)).clamp(0.0, 1.0);
        Some(project(
            a.x_m + (b.x_m - a.x_m) * fraction,
            a.y_m + (b.y_m - a.y_m) * fraction,
        ))
    };
    let mut length = -scenario.route.start_offset_m.unwrap_or(0.0);
    let mut low = 0.0_f64;
    let mut high = 0.0_f64;
    for id in &path {
        let Some(edge) = graph.edge(id) else { continue };
        length += edge.length_m;
        low = low.min(edge.grade_percent);
        high = high.max(edge.grade_percent);
        if let Some(profile) = graph.physics_profile(id) {
            for grade in &profile.grades {
                low = low.min(grade.grade_percent);
                high = high.max(grade.grade_percent);
            }
        }
        if let (Some(a), Some(b)) = (node(&edge.from.0), node(&edge.to.0)) {
            line(
                &mut canvas,
                a,
                b,
                if powered(supply) {
                    [172, 144, 244, 255]
                } else {
                    [75, 203, 238, 255]
                },
                2,
            );
        }
        if let Some(config) = &scenario.route.electric_supply {
            for section in &config.sections {
                if section.edge.trim_end_matches("_r") == id.trim_end_matches("_r") {
                    let (start, end) = if id.ends_with("_r") {
                        (
                            edge.length_m - section.end_m,
                            edge.length_m - section.start_m,
                        )
                    } else {
                        (section.start_m, section.end_m)
                    };
                    if let (Some(a), Some(b)) = (edge_point(id, start), edge_point(id, end)) {
                        line(
                            &mut canvas,
                            a,
                            b,
                            if powered(&section.supply) {
                                [172, 144, 244, 255]
                            } else {
                                [75, 203, 238, 255]
                            },
                            2,
                        );
                    }
                }
            }
        }
    }
    let legs = &scenario.route.legs;
    let stops = if legs.is_empty() {
        scenario.route.stops.iter().collect::<Vec<_>>()
    } else {
        legs.iter().flat_map(|leg| &leg.stops).collect()
    };
    for stop in &stops {
        let edge = path
            .iter()
            .find_map(|id| graph.edge(id).filter(|edge| edge.to.0 == stop.node));
        if let Some(edge) = edge
            && let Some(p) = edge_point(&edge.id.0, edge.length_m + stop.offset_m)
        {
            dot(&mut canvas, p, [248, 201, 74, 255]);
        }
    }
    if !legs.is_empty() {
        length = legs
            .iter()
            .map(|leg| leg.end_chainage_m - leg.start_offset_m)
            .sum();
    }
    let first_path = legs
        .first()
        .map_or(path.as_slice(), |leg| leg.edges.as_slice());
    let first_offset = legs
        .first()
        .map_or(scenario.route.start_offset_m.unwrap_or(0.0), |leg| {
            leg.start_offset_m
        });
    let last_path = legs
        .last()
        .map_or(path.as_slice(), |leg| leg.edges.as_slice());
    let last_offset = legs.last().map_or(
        last_path
            .iter()
            .filter_map(|id| graph.edge(id))
            .map(|edge| edge.length_m)
            .sum(),
        |leg| leg.end_chainage_m,
    );
    let at_chainage = |edges: &[String], mut position: f64| {
        for id in edges {
            let edge = graph.edge(id)?;
            if position <= edge.length_m {
                return edge_point(id, position);
            }
            position -= edge.length_m;
        }
        None
    };
    if let Some(p) = at_chainage(first_path, first_offset) {
        dot(&mut canvas, p, [75, 234, 146, 255]);
    }
    if let Some(p) = at_chainage(last_path, last_offset) {
        dot(&mut canvas, p, [241, 105, 117, 255]);
    }
    let summary = format!(
        "{:.1} km · {} paradas · pendientes {:.1}% a {:.1}%\nAzul: recorrido · violeta: alimentación · amarillo: paradas · verde/rojo: inicio/final\nEsquema de la vía; las curvas se simplifican entre nodos.",
        length.max(0.0) / 1000.0,
        stops.len(),
        low,
        high
    );
    Ok((
        Image::from_dynamic(
            image::DynamicImage::ImageRgba8(canvas),
            true,
            bevy::asset::RenderAssetUsages::default(),
        ),
        summary,
    ))
}
fn line(canvas: &mut image::RgbaImage, a: (i32, i32), b: (i32, i32), color: [u8; 4], width: i32) {
    let (mut x, mut y) = a;
    let dx = (b.0 - x).abs();
    let dy = -(b.1 - y).abs();
    let sx = if x < b.0 { 1 } else { -1 };
    let sy = if y < b.1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        for i in -width..=width {
            for j in -width..=width {
                if (0..768).contains(&(x + i)) && (0..384).contains(&(y + j)) {
                    canvas.put_pixel((x + i) as u32, (y + j) as u32, image::Rgba(color));
                }
            }
        }
        if (x, y) == b {
            break;
        }
        let twice = error * 2;
        if twice >= dy {
            error += dy;
            x += sx
        }
        if twice <= dx {
            error += dx;
            y += sy
        }
    }
}
fn dot(canvas: &mut image::RgbaImage, p: (i32, i32), color: [u8; 4]) {
    for x in -4..=4 {
        for y in -4..=4 {
            if x * x + y * y <= 16 && (0..768).contains(&(p.0 + x)) && (0..384).contains(&(p.1 + y))
            {
                canvas.put_pixel((p.0 + x) as u32, (p.1 + y) as u32, image::Rgba(color));
            }
        }
    }
}
