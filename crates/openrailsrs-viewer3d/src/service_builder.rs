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
        if !scenario.route.legs.is_empty() {
            return Err("Elegí un PAT simple para el editor de servicios. Este editor trabaja sobre un itinerario existente en un único sentido; no edita inversiones ni maniobras.".into());
        }
        let probe = openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
            .map_err(|e| e.to_string())?;
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

fn validate_times(s: &ScenarioFile) -> Result<(), String> {
    s.validate().map_err(|e| e.to_string())?;
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
}
