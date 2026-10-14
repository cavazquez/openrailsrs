//! Player-owned service drafts use the already resolved author itinerary.
use bevy::prelude::*;
use openrailsrs_scenarios::{ScenarioFile, StopDef};
use std::path::{Path, PathBuf};
#[derive(Clone)]
pub struct Candidate {
    pub name: String,
    pub node: String,
    pub offset_m: f64,
    pub chainage_m: f64,
}
#[derive(Resource, Default)]
pub struct ServiceBuilder {
    pub graph: Option<openrailsrs_track::TrackGraph>,
    pub formation_length_m: f64,
    pub scenario: Option<ScenarioFile>,
    pub candidates: Vec<Candidate>,
    pub origin: usize,
    pub destination: usize,
    pub selected_stop: usize,
    pub status: String,
    pub native_root: Option<PathBuf>,
    undo: Vec<ScenarioFile>,
    template: Option<ScenarioFile>,
}
impl ServiceBuilder {
    pub fn load(&mut self, menu: &crate::player_launch::PlayerLaunchMenu) -> Result<(), String> {
        let launch = menu.prepare()?;
        let scenario =
            openrailsrs_scenarios::load_scenario(&launch.path).map_err(|e| e.to_string())?;
        self.load_scenario(scenario, launch.route_root)
    }
    pub fn load_scenario(
        &mut self,
        mut scenario: ScenarioFile,
        native_root: Option<PathBuf>,
    ) -> Result<(), String> {
        let probe = openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
            .map_err(|e| e.to_string())?;
        self.graph = Some(probe.graph.clone());
        self.formation_length_m = probe.formation.cars.iter().map(|c| c.length_m).sum();
        self.candidates = vec![Candidate {
            name: format!("Origen · {}", scenario.route.start),
            node: scenario.route.start.clone(),
            offset_m: 0.,
            chainage_m: probe.start_chainage_m,
        }];
        for (stop, target) in scenario
            .route
            .stops
            .iter()
            .zip(&probe.gameplay.stop_targets)
        {
            if target.cum_dist_m > probe.start_chainage_m + 10. {
                self.candidates.push(Candidate {
                    name: target.name.clone(),
                    node: stop.node.clone(),
                    offset_m: stop.offset_m,
                    chainage_m: target.cum_dist_m,
                });
            }
        }
        let total = probe
            .path_data
            .edges
            .iter()
            .map(|e| e.length_m)
            .sum::<f64>();
        if self
            .candidates
            .last()
            .is_none_or(|c| c.node != scenario.route.destination)
        {
            self.candidates.push(Candidate {
                name: format!("Destino · {}", scenario.route.destination),
                node: scenario.route.destination.clone(),
                offset_m: 0.,
                chainage_m: total,
            });
        }
        self.candidates
            .sort_by(|a, b| a.chainage_m.total_cmp(&b.chainage_m));
        self.origin = 0;
        self.destination = self.candidates.len() - 1;
        self.selected_stop = 0;
        self.undo.clear();
        self.native_root = native_root;
        scenario.scenario.name = format!("Servicio propio · {}", scenario.scenario.name);
        scenario.validate = None;
        self.template = Some(scenario.clone());
        self.scenario = Some(scenario);
        self.status =
            "Copia editable sobre el recorrido original; guardado separado del autor.".into();
        Ok(())
    }
    pub fn edit_stop(&mut self, index: usize, field: u8, delta: f64) -> Result<(), String> {
        let old = self.scenario.as_ref().ok_or("Abrí un servicio primero")?;
        let mut candidate = old.clone();
        let stop = candidate
            .route
            .stops
            .get_mut(index)
            .ok_or("Parada no disponible")?;
        match field {
            0 => {
                stop.arrive_s = (stop.arrive_s + delta).max(0.);
                stop.depart_s = stop.depart_s.max(stop.arrive_s);
            }
            1 => stop.depart_s = (stop.depart_s + delta).max(stop.arrive_s),
            2 => stop.dwell_s = (stop.dwell_s + delta).max(0.),
            _ => return Err("Campo desconocido".into()),
        }
        validate_times(&candidate)?;
        if candidate
            .route
            .stops
            .windows(2)
            .any(|s| s[0].depart_s > s[1].arrive_s)
        {
            return Err("La siguiente llegada debe ocurrir después de la salida anterior".into());
        }
        self.undo.push(old.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.scenario = Some(candidate);
        Ok(())
    }
    pub fn itinerary_point(&mut self, node: &str) -> Result<(), String> {
        let graph = self
            .graph
            .as_ref()
            .ok_or("Abrí una ruta para editar su itinerario")?;
        let old = self.scenario.as_ref().ok_or("Abrí un servicio")?;
        let mut candidate = old.clone();
        let (mut start, reversed, mut start_offset) = candidate.route.legs.last().map_or(
            (
                candidate.route.start.clone(),
                false,
                candidate.route.start_offset_m.unwrap_or(0.),
            ),
            |leg| (leg.destination.clone(), leg.reversed, leg.end_chainage_m),
        );
        if start == node {
            return Err("Elegí otro punto del mapa".into());
        }
        let mut path = connected_path(graph, &start, node)?;
        if let Some(previous) = candidate.route.legs.last() {
            start = previous.start.clone();
            let mut prefix = previous.edges.clone();
            prefix.extend(path);
            path = prefix;
        } else {
            start_offset = candidate.route.start_offset_m.unwrap_or(0.);
        }
        if path.len() > 4096 || candidate.route.legs.len() >= 64 {
            return Err("El itinerario supera el límite de tramos".into());
        }
        let length = path
            .iter()
            .filter_map(|id| graph.edge(id))
            .map(|e| e.length_m)
            .sum::<f64>();
        if length <= start_offset + self.formation_length_m {
            return Err("El tramo es demasiado corto para esta formación".into());
        }
        if candidate.route.legs.is_empty() {
            candidate.route.stops.clear();
            candidate.route.destination = node.into();
        }
        let switches = switches_for_path(graph, &path);
        candidate
            .route
            .legs
            .push(openrailsrs_scenarios::model::RouteLeg {
                operations: vec![],
                service_id: None,
                not_before_s: None,
                start,
                destination: node.into(),
                edges: path,
                start_offset_m: start_offset,
                end_chainage_m: length,
                reversed,
                wait_s: 0.,
                stops: vec![],
                switches,
            });
        self.commit_itinerary(old.clone(), candidate)
    }
    pub fn itinerary_reverse(&mut self) -> Result<(), String> {
        let old = self.scenario.as_ref().ok_or("Abrí un servicio")?;
        let mut candidate = old.clone();
        let graph = self.graph.as_ref().ok_or("Ruta no disponible")?;
        let leg = candidate
            .route
            .legs
            .last_mut()
            .ok_or("Elegí dos puntos del mapa primero")?;
        let edges = leg
            .edges
            .iter()
            .rev()
            .map(|id| {
                if let Some(id) = id.strip_suffix("_r") {
                    id.to_string()
                } else {
                    format!("{id}_r")
                }
            })
            .collect::<Vec<_>>();
        if edges.iter().any(|e| graph.edge(e).is_none()) {
            return Err("El recorrido no tiene vías dirigidas para regresar por ese tramo".into());
        }
        let total = leg
            .edges
            .iter()
            .filter_map(|id| graph.edge(id))
            .map(|e| e.length_m)
            .sum::<f64>();
        let switches = switches_for_path(graph, &edges);
        let next = openrailsrs_scenarios::model::RouteLeg {
            operations: vec![],
            service_id: None,
            not_before_s: None,
            start: leg.destination.clone(),
            destination: leg.start.clone(),
            edges,
            start_offset_m: total - leg.end_chainage_m + self.formation_length_m,
            end_chainage_m: total,
            reversed: !leg.reversed,
            wait_s: 0.,
            stops: vec![],
            switches,
        };
        if candidate.route.legs.len() >= 64 {
            return Err("Límite de tramos alcanzado".into());
        }
        candidate.route.legs.push(next);
        self.commit_itinerary(old.clone(), candidate)
    }
    pub fn itinerary_wait(&mut self, delta: f64) -> Result<(), String> {
        let old = self.scenario.as_ref().ok_or("Abrí un servicio")?;
        let mut candidate = old.clone();
        let leg = candidate
            .route
            .legs
            .last_mut()
            .ok_or("Elegí un tramo primero")?;
        leg.wait_s = (leg.wait_s + delta).clamp(0., 1800.);
        self.commit_itinerary(old.clone(), candidate)
    }
    fn commit_itinerary(
        &mut self,
        old: ScenarioFile,
        candidate: ScenarioFile,
    ) -> Result<(), String> {
        validate_itinerary(
            self.graph.as_ref().ok_or("Ruta no disponible")?,
            &candidate,
            self.formation_length_m,
        )?;
        validate_times(&candidate)?;
        self.undo.push(old);
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.scenario = Some(candidate);
        self.status = "Itinerario propio actualizado; guardar valida conexiones y recursos".into();
        Ok(())
    }
    pub fn toggle_stop(&mut self, index: usize) -> Result<(), String> {
        let c = self
            .candidates
            .get(index)
            .ok_or("Estación desconocida")?
            .clone();
        let old = self.scenario.as_ref().ok_or("Abrí un servicio primero")?;
        let mut next = old.clone();
        if c.chainage_m <= self.candidates[self.origin].chainage_m
            || c.chainage_m >= self.candidates[self.destination].chainage_m
        {
            return Err("Elegí una estación intermedia; origen y destino se generan aparte".into());
        }
        if let Some(i) = next.route.stops.iter().position(|s| s.node == c.node) {
            next.route.stops.remove(i);
        } else {
            let after = (c.chainage_m - self.candidates[self.origin].chainage_m) / 15.;
            next.route.stops.push(StopDef {
                node: c.node,
                name: Some(c.name),
                offset_m: c.offset_m,
                arrive_s: after,
                depart_s: after + 30.,
                dwell_s: 30.,
                passengers_on: 0,
                passengers_off: 0,
            });
            next.route.stops.sort_by(|a, b| {
                self.candidates
                    .iter()
                    .find(|c| c.node == a.node)
                    .map_or(0., |c| c.chainage_m)
                    .total_cmp(
                        &self
                            .candidates
                            .iter()
                            .find(|c| c.node == b.node)
                            .map_or(0., |c| c.chainage_m),
                    )
            });
        }
        validate_times(&next)?;
        self.undo.push(old.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.scenario = Some(next);
        Ok(())
    }
    pub fn generate(&mut self) -> Result<(), String> {
        let start = self
            .candidates
            .get(self.origin)
            .ok_or("Origen desconocido")?;
        let end = self
            .candidates
            .get(self.destination)
            .ok_or("Destino desconocido")?;
        if end.chainage_m <= start.chainage_m + 10. {
            return Err(
                "El destino debe estar después del origen en este itinerario validado".into(),
            );
        }
        let mut scenario = self.template.clone().ok_or("No hay recorrido original")?;
        let probe = openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
            .map_err(|e| e.to_string())?;
        if end.chainage_m - start.chainage_m < probe.formation.length_m() {
            return Err("El recorrido es más corto que la formación".into());
        }
        let (edge, position) = openrailsrs_sim::path_data::PathData::position_at_odometer(
            &probe.state.path_edges,
            &probe.path_data.edges,
            start.chainage_m,
        )
        .ok_or("El origen no está conectado al recorrido")?;
        let first = probe
            .state
            .path_edges
            .iter()
            .position(|e| e == &edge)
            .ok_or("Vía inicial no disponible")?;
        let from = probe
            .graph
            .edge(&edge)
            .ok_or("Vía inicial inválida")?
            .from
            .0
            .clone();
        let last = probe
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(first)
            .find(|(_, id)| probe.graph.edge(id).is_some_and(|e| e.to.0 == end.node))
            .map(|(i, _)| i)
            .ok_or("El destino no está conectado en este sentido")?;
        scenario.route.start = from.clone();
        scenario.route.destination = end.node.clone();
        scenario.route.start_offset_m = Some(position);
        scenario.route.waypoints = std::iter::once(from)
            .chain(
                probe.state.path_edges[first..=last]
                    .iter()
                    .filter_map(|id| probe.graph.edge(id).map(|e| e.to.0.clone())),
            )
            .collect();
        let before = probe.path_data.edges[..first]
            .iter()
            .map(|e| e.length_m)
            .sum::<f64>();
        scenario.route.stops = self
            .candidates
            .iter()
            .filter(|c| {
                c.chainage_m > start.chainage_m + 0.1 && c.chainage_m <= end.chainage_m + 0.1
            })
            .enumerate()
            .map(|(i, c)| {
                let arrive = (c.chainage_m - start.chainage_m) / 15. + 30. * i as f64;
                StopDef {
                    node: c.node.clone(),
                    name: Some(c.name.clone()),
                    offset_m: c.offset_m,
                    arrive_s: arrive,
                    depart_s: arrive + 30.,
                    dwell_s: 30.,
                    passengers_on: 0,
                    passengers_off: 0,
                }
            })
            .collect();
        // Keep named stations on their authored nodes; their signed offsets are
        // measured from those nodes, independent of the path's new prefix.
        let _ = before;
        scenario.gameplay.shunting.clear();
        scenario.extra_trains.clear();
        scenario.simulation.rail_operations = default();
        scenario.train.announcements = default();
        scenario.scenario.name = format!("Servicio propio · {} → {}", start.name, end.name);
        let generated = openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
            .map_err(|e| e.to_string())?;
        validate_times(&scenario)?;
        validate_supplies(&generated)?;
        if let Some(old) = self.scenario.replace(scenario) {
            self.undo.push(old);
            if self.undo.len() > 64 {
                self.undo.remove(0);
            }
        }
        self.status="Recorrido conectado, longitud, formación y alimentación comprobados. Revisá horarios antes de guardar.".into();
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), String> {
        self.scenario = Some(self.undo.pop().ok_or("No hay cambios para deshacer")?);
        Ok(())
    }
    pub fn save(&mut self, data: &Path) -> Result<PathBuf, String> {
        let scenario = self.scenario.as_ref().ok_or("No hay servicio editable")?;
        validate_times(scenario)?;
        validate_itinerary(
            self.graph.as_ref().ok_or("Ruta no disponible")?,
            scenario,
            self.formation_length_m,
        )?;
        openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), scenario)
            .map_err(|e| e.to_string())?;
        let mut value = toml::Value::try_from(scenario).map_err(|e| e.to_string())?;
        if let Some(root) = &self.native_root {
            value.as_table_mut().unwrap().insert(
                "player_native_root".into(),
                toml::Value::String(root.to_string_lossy().into_owned()),
            );
        }
        let bytes = toml::to_string_pretty(&value).map_err(|e| e.to_string())?;
        let mut hash = 0xcbf29ce484222325u64;
        for b in bytes.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let path = data
            .join("custom-services")
            .join(format!("service-{hash:016x}.toml"));
        crate::player_settings::atomic_write(&path, bytes.as_bytes())?;
        self.status = format!("Guardado en {}. Elegilo en Nueva partida.", path.display());
        Ok(path)
    }
}
fn validate_itinerary(
    graph: &openrailsrs_track::TrackGraph,
    scenario: &ScenarioFile,
    train_length: f64,
) -> Result<(), String> {
    use openrailsrs_sim::path_data::PathData;
    let normalize = |(edge, position): (String, f64)| {
        let length = graph.edge(&edge).map_or(0., |e| e.length_m);
        if let Some(base) = edge.strip_suffix("_r") {
            (base.to_owned(), length - position)
        } else {
            (edge, position)
        }
    };
    for (i, leg) in scenario.route.legs.iter().enumerate() {
        let mut node = leg.start.as_str();
        let mut total = 0.;
        for id in &leg.edges {
            let edge = graph
                .edge(id)
                .ok_or("El itinerario contiene una vía desconocida")?;
            if edge.from.0 != node {
                return Err("El itinerario tiene vías desconectadas o en otro sentido".into());
            }
            node = &edge.to.0;
            total += edge.length_m;
        }
        if leg.edges.is_empty()
            || node != leg.destination
            || !(0. ..=1800.).contains(&leg.wait_s)
            || leg.start_offset_m < train_length
            || leg.end_chainage_m > total + 0.001
            || leg.end_chainage_m <= leg.start_offset_m + 1.
        {
            return Err(
                "El tramo no conserva destino, espera o espacio para toda la formación".into(),
            );
        }
        if let Some(previous) = i.checked_sub(1).and_then(|i| scenario.route.legs.get(i)) {
            let before = PathData::from_path(&previous.edges, graph);
            let after = PathData::from_path(&leg.edges, graph);
            for offset in [0., train_length / 2., train_length] {
                let old_position = PathData::position_at_odometer(
                    &previous.edges,
                    &before.edges,
                    previous.end_chainage_m - offset,
                )
                .map(normalize);
                let new_position = PathData::position_at_odometer(
                    &leg.edges,
                    &after.edges,
                    leg.start_offset_m
                        - if leg.reversed != previous.reversed {
                            train_length - offset
                        } else {
                            offset
                        },
                )
                .map(normalize);
                if !old_position
                    .zip(new_position)
                    .is_some_and(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() < 0.01)
                {
                    return Err("La inversión o continuación desplaza vehículos fuera de su vía; elegí un tramo que conserve toda la formación".into());
                }
            }
        }
    }
    Ok(())
}
fn switches_for_path(
    graph: &openrailsrs_track::TrackGraph,
    path: &[String],
) -> Vec<openrailsrs_scenarios::SwitchDef> {
    path.windows(2)
        .filter_map(|pair| {
            let node = &graph.edge(&pair[0])?.to.0;
            let openrailsrs_track::NodeKind::Switch {
                stem_edge,
                diverging_edge,
            } = &graph.node(node)?.kind
            else {
                return None;
            };
            let base = |s: &str| s.strip_suffix("_r").unwrap_or(s).to_owned();
            let position =
                if base(&pair[1]) == base(&stem_edge.0) || base(&pair[0]) == base(&stem_edge.0) {
                    openrailsrs_scenarios::SwitchPositionDef::Straight
                } else if base(&pair[1]) == base(&diverging_edge.0)
                    || base(&pair[0]) == base(&diverging_edge.0)
                {
                    openrailsrs_scenarios::SwitchPositionDef::Diverging
                } else {
                    return None;
                };
            Some(openrailsrs_scenarios::SwitchDef {
                node: node.clone(),
                position,
            })
        })
        .collect()
}
fn connected_path(
    graph: &openrailsrs_track::TrackGraph,
    start: &str,
    end: &str,
) -> Result<Vec<String>, String> {
    let mut todo = std::collections::VecDeque::from([start.to_string()]);
    let mut previous = std::collections::BTreeMap::<String, (String, String)>::new();
    let mut seen = std::collections::BTreeSet::from([start.to_string()]);
    while let Some(node) = todo.pop_front() {
        if node == end {
            let mut path = vec![];
            let mut n = end;
            while n != start {
                let (parent, edge) = previous.get(n).ok_or("Conexión incompleta")?;
                path.push(edge.clone());
                n = parent;
            }
            path.reverse();
            return Ok(path);
        }
        let mut edges = graph.outgoing_edges(&node).to_vec();
        edges.sort();
        for id in edges {
            if let Some(edge) = graph.edge(&id)
                && seen.insert(edge.to.0.clone())
            {
                previous.insert(edge.to.0.clone(), (node.clone(), id));
                todo.push_back(edge.to.0.clone());
            }
        }
        if seen.len() > 100000 {
            return Err("La ruta supera el límite del editor".into());
        }
    }
    Err("No hay una conexión dirigida entre esos puntos".into())
}

fn validate_times(s: &ScenarioFile) -> Result<(), String> {
    s.validate().map_err(|e| e.to_string())?;
    for leg in &s.route.legs {
        if leg
            .stops
            .iter()
            .any(|s| s.depart_s < s.arrive_s + s.dwell_s)
            || leg.stops.windows(2).any(|p| p[0].depart_s > p[1].arrive_s)
        {
            return Err("Horarios incompatibles dentro de un tramo".into());
        }
    }
    if s.route
        .stops
        .iter()
        .any(|s| s.depart_s + 1e-6 < s.arrive_s + s.dwell_s)
        || s.route
            .stops
            .windows(2)
            .any(|s| s[0].depart_s > s[1].arrive_s)
    {
        return Err("Horarios incompatibles: la salida debe incluir la parada mínima y ocurrir antes de la próxima llegada".into());
    }
    Ok(())
}
fn validate_supplies(generated: &openrailsrs_sim::LiveDriveSession) -> Result<(), String> {
    let config = &generated.physics.electric;
    for car in &config.cars {
        let mut compatible = false;
        let mut before = 0.;
        for (edge, data) in generated
            .state
            .path_edges
            .iter()
            .zip(&generated.path_data.edges)
        {
            let begin = if before < generated.start_chainage_m {
                (generated.start_chainage_m - before).clamp(0., data.length_m)
            } else {
                0.
            };
            let mut points = vec![begin, data.length_m];
            for section in &config.route.sections {
                if &section.edge == edge && section.end_m > begin {
                    points.push(section.start_m.clamp(begin, data.length_m));
                    points.push(section.end_m.clamp(begin, data.length_m));
                }
            }
            points.sort_by(f64::total_cmp);
            points.dedup();
            for interval in points.windows(2) {
                if interval[1] <= interval[0] + 1e-6 {
                    continue;
                }
                let supply = config.supply_at(
                    &generated.state,
                    &generated.path_data,
                    before + (interval[0] + interval[1]) * 0.5,
                );
                compatible |= supply.kind == car.params.pickup
                    && supply.voltage_v >= car.params.minimum_voltage_v
                    && car
                        .params
                        .maximum_voltage_v
                        .is_none_or(|v| supply.voltage_v <= v);
            }
            before += data.length_m;
        }
        if !compatible {
            return Err("La formación necesita alimentación compatible en las vías elegidas; una sección en otro recorrido no alcanza".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scenario() -> ScenarioFile {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/scenario.toml");
        let mut s = openrailsrs_scenarios::load_scenario(&source).unwrap();
        let base = source.parent().unwrap();
        s.route.path = base
            .join(&s.route.path)
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into();
        s.train.consist = base
            .join(&s.train.consist)
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into();
        s
    }
    #[test]
    fn connected_subservice_times_undo_persistence_and_invalid_order() {
        let mut builder = ServiceBuilder::default();
        let original = scenario();
        builder.load_scenario(original.clone(), None).unwrap();
        builder.origin = 1;
        builder.destination = 2;
        builder.generate().unwrap();
        let sub = builder.scenario.as_ref().unwrap();
        let probe = openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), sub).unwrap();
        assert_eq!(probe.head_chainage_m(), 500.);
        assert_eq!(probe.state.path_edges.len(), 2);
        assert_eq!(sub.route.start_offset_m, Some(500.));
        assert!(probe.head_chainage_m() >= probe.formation.length_m());
        assert_eq!(sub.route.destination, "b");
        let original_text = toml::to_string(&original).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let saved = builder.save(temp.path()).unwrap();
        let read = openrailsrs_scenarios::load_scenario(saved).unwrap();
        assert_eq!(read.route.destination, "b");
        assert!(builder.edit_stop(0, 2, 500.).is_err());
        builder.undo().unwrap();
        assert_eq!(builder.scenario.as_ref().unwrap().route.start, "a");
        assert_eq!(toml::to_string(&original).unwrap(), original_text);
        builder.origin = 2;
        builder.destination = 0;
        assert!(builder.generate().is_err());
    }
    #[test]
    fn graphical_itinerary_validates_all_legs_reversal_wait_undo_and_separate_save() {
        let mut original = scenario();
        let temp = tempfile::tempdir().unwrap();
        let source =
            std::fs::read_to_string(Path::new(&original.route.path).join("track.toml")).unwrap();
        let mut graph: toml::Value = toml::from_str(&source).unwrap();
        let edges = graph["edges"].as_array().unwrap().clone();
        for mut edge in edges {
            let id = edge["id"].as_str().unwrap().to_owned();
            let from = edge["from"].clone();
            let to = edge["to"].clone();
            edge["id"] = toml::Value::String(format!("{id}_r"));
            edge["from"] = to;
            edge["to"] = from;
            graph["edges"].as_array_mut().unwrap().push(edge);
        }
        std::fs::write(
            temp.path().join("track.toml"),
            toml::to_string(&graph).unwrap(),
        )
        .unwrap();
        original.route.path = temp.path().to_string_lossy().into_owned();
        original.route.start_offset_m = Some(80.);
        let mut editor = ServiceBuilder::default();
        editor.load_scenario(original.clone(), None).unwrap();
        editor.itinerary_point("mid").unwrap();
        editor.itinerary_wait(60.).unwrap();
        editor.itinerary_reverse().unwrap();
        editor.itinerary_point("unknown-node").unwrap_err();
        assert_eq!(editor.scenario.as_ref().unwrap().route.legs.len(), 2);
        let saved = editor.save(temp.path()).unwrap();
        let loaded = openrailsrs_scenarios::load_scenario(saved).unwrap();
        assert_eq!(loaded.route.legs[0].wait_s, 60.);
        assert!(loaded.route.legs[1].reversed);
        let mut session =
            openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &loaded).unwrap();
        for _ in 0..8000 {
            session.step_autodrive(0.05, 0.5, |_| {});
            if session.arrived {
                break;
            }
        }
        assert!(session.arrived, "{:?}", session.gameplay.phase);
        assert!(session.time_s() >= 60.);
        editor.undo().unwrap();
        assert_eq!(editor.scenario.as_ref().unwrap().route.legs.len(), 1);
        let before = toml::to_string(editor.scenario.as_ref().unwrap()).unwrap();
        assert!(editor.itinerary_wait(f64::NAN).is_err());
        assert_eq!(
            before,
            toml::to_string(editor.scenario.as_ref().unwrap()).unwrap()
        );
        let mut invalid = loaded.clone();
        invalid.route.legs[1].start_offset_m += 10.;
        assert!(
            validate_itinerary(
                editor.graph.as_ref().unwrap(),
                &invalid,
                editor.formation_length_m
            )
            .is_err()
        );
        invalid = loaded;
        invalid.route.legs[1].edges[0] = "missing".into();
        assert!(
            validate_itinerary(
                editor.graph.as_ref().unwrap(),
                &invalid,
                editor.formation_length_m
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("track.toml")).unwrap(),
            toml::to_string(&graph).unwrap()
        );
    }
}
