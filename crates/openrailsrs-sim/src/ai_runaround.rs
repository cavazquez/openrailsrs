//! AI crews execute the same secured cuts, routed reversals and physical front
//! pickups as F9. An authored loop is a plan, never a relocation instruction.
use crate::{LiveDriveSession, LiveTraffic};
use openrailsrs_scenarios::model::{RouteLeg, TimetableAction, TimetableOperation};
impl LiveDriveSession {
    pub(crate) fn initialize_runaround_plan(&mut self) -> Result<(), String> {
        let Some(def) = self
            .state
            .yard_operations
            .definition
            .run_arounds
            .iter()
            .find(|r| r.service == self.service_id)
            .cloned()
        else {
            return Ok(());
        };
        if !self.physics.rack.vehicles.is_empty() {
            return Err(
                "El rodeo cambia la referencia de cremallera: requiere mapa por cada subtrayecto"
                    .into(),
            );
        }
        if def.keep >= self.formation.coupled_count || self.cab_vehicle() >= def.keep {
            return Err("El rodeo debe conservar una locomotora y una sección remolcada".into());
        }
        if def.legs[0].edges != self.state.path_edges
            || (def.legs[0].start_offset_m - self.head_chainage_m()).abs() > 0.001
            || def.legs[0].reversed != self.state.path_reversed
        {
            return Err("El plan de rodeo no coincide con la posición física inicial".into());
        }
        if !(def.legs[0].start_offset_m..=def.legs[0].end_chainage_m)
            .contains(&def.detach_chainage_m)
            || !(def.legs[2].start_offset_m..=def.legs[2].end_chainage_m)
                .contains(&def.pickup_chainage_m)
        {
            return Err("Marcador de corte o recogida fuera de su subtrayecto de rodeo".into());
        }
        let mut legs = vec![];
        for (i, l) in def.legs.iter().enumerate() {
            if l.edges.windows(2).any(|p| {
                self.graph
                    .edge(&p[0])
                    .zip(self.graph.edge(&p[1]))
                    .is_none_or(|(a, b)| a.to != b.from)
            }) {
                return Err("El rodeo salta entre vías".into());
            }
            let from = self
                .graph
                .edge(&l.edges[0])
                .ok_or("Entrada del rodeo inexistente")?
                .from
                .0
                .clone();
            let to = self
                .graph
                .edge(l.edges.last().unwrap())
                .ok_or("Salida del rodeo inexistente")?
                .to
                .0
                .clone();
            let length = l
                .edges
                .iter()
                .map(|id| {
                    self.graph
                        .edge(id)
                        .map(|e| e.length_m)
                        .ok_or("Tramo del rodeo inexistente")
                })
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .sum::<f64>();
            if l.end_chainage_m > length || l.start_offset_m > length {
                return Err("Marcador de rodeo fuera de la vía".into());
            }
            let operations = if i == 0 {
                vec![TimetableOperation {
                    chainage_m: def.detach_chainage_m,
                    section: def.section.clone(),
                    manual: false,
                    action: TimetableAction::Detach { keep: def.keep },
                }]
            } else if i == 2 {
                vec![TimetableOperation {
                    chainage_m: def.pickup_chainage_m,
                    section: def.section.clone(),
                    manual: false,
                    action: TimetableAction::PickUp {
                        count: self.formation.cars.len(),
                    },
                }]
            } else {
                vec![]
            };
            legs.push(RouteLeg {
                operations,
                service_id: None,
                not_before_s: None,
                start: from,
                destination: to,
                edges: l.edges.clone(),
                start_offset_m: l.start_offset_m,
                end_chainage_m: l.end_chainage_m,
                reversed: l.reversed,
                wait_s: 0.,
                stops: vec![],
                switches: runaround_switches(&self.graph, &l.edges)?,
            });
        }
        self.operational_legs = legs;
        self.operational_leg = 0;
        self.operational_operation = 0;
        self.operational_prefix_m = self.head_chainage_m();
        self.operational_start_m = self.head_chainage_m();
        self.state
            .yard_operations
            .run_arounds
            .entry(def.id)
            .or_default();
        Ok(())
    }
    pub(crate) fn yard_front_pickup(&self, section: &str) -> bool {
        self.state
            .yard_operations
            .definition
            .run_arounds
            .iter()
            .any(|r| {
                r.service == self.service_id
                    && r.section == section
                    && self
                        .state
                        .yard_operations
                        .run_arounds
                        .get(&r.id)
                        .is_some_and(|s| s.active)
            })
    }
    pub(crate) fn runaround_pickup_reference(&self, section: &str, count: usize) -> f64 {
        let head = self.head_chainage_m();
        if !self.yard_front_pickup(section)
            || self.state.path_reversed
            || self.formation.sections.iter().any(|s| s.id == section)
            || self.formation.coupled_count != count
        {
            return head;
        }
        let Some(def) = self
            .state
            .yard_operations
            .definition
            .run_arounds
            .iter()
            .find(|r| r.service == self.service_id && r.section == section)
        else {
            return head;
        };
        head - self
            .formation
            .cars
            .iter()
            .take(count.saturating_sub(def.keep))
            .map(|c| c.length_m)
            .sum::<f64>()
    }
    pub(crate) fn step_ai_runaround(&mut self, dt: f64) -> bool {
        let Some(def) = self
            .state
            .yard_operations
            .definition
            .run_arounds
            .iter()
            .find(|r| r.service == self.service_id)
            .cloned()
        else {
            return false;
        };
        if !self
            .state
            .yard_operations
            .run_arounds
            .get(&def.id)
            .is_some_and(|r| r.active)
        {
            self.driver_throttle = 0.;
            self.driver_brake = 1.;
            self.step_realtime(dt, |_| {});
            return true;
        }
        let Some(leg) = self.operational_legs.get(self.operational_leg) else {
            return false;
        };
        let target = leg
            .operations
            .get(self.operational_operation)
            .map_or(leg.end_chainage_m, |o| o.chainage_m);
        let remaining = target - self.head_chainage_m();
        self.driver_direction = if self.state.path_reversed { 0. } else { 1. };
        crate::mobile_portions::yard_precision_controls(
            self,
            target,
            if self.operational_leg == 2 { 1.2 } else { 3. },
        );
        let assistance = self.traffic_brake_assistance;
        if self.operational_leg == 2
            && remaining >= -0.05
            && self.formation.sections.iter().any(|s| s.id == def.section)
        {
            // The whole signal block contains our own waiting coaches. Use
            // their declared, physical contact reference from the entry of
            // this leg, rather than stopping at the preceding block boundary.
            // A third train, reservation or protected interval still blocks.
            let peer = format!("{} · sección {}", self.service_id, def.section);
            if self.portion_corridor_clear(&peer, target) {
                self.traffic_brake_assistance = false;
            }
        }
        self.step_realtime(dt, |_| {});
        self.traffic_brake_assistance = assistance;
        // This is an AI crew, even though the precision driver supplied the
        // physical handles. Authorise the same secured PAT reversal that an
        // automatic driver would; step_realtime alone deliberately waits for
        // a human to select the next reverser position.
        self.tick_operational_path(true);
        true
    }
}
impl LiveTraffic {
    pub fn start_ai_runaround(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let def = world
            .definition
            .run_arounds
            .iter()
            .find(|r| r.id == id)
            .ok_or("Plan de rodeo desconocido")?
            .clone();
        if world
            .run_arounds
            .get(id)
            .is_some_and(|r| r.active || r.completed)
        {
            return Err("El rodeo ya está en curso o terminó".into());
        }
        if self.remote_drivers.contains(&def.service) {
            return Err(
                "No se inicia una tripulación AI en un servicio con conductor humano de red".into(),
            );
        }
        let service = self
            .services
            .iter_mut()
            .find(|s| s.id == def.service)
            .ok_or("El plan requiere un servicio AI físico")?;
        if service.session.state.yard_operations.human_driven {
            return Err("El servicio tiene una autoridad humana de conducción".into());
        }
        if service.session.velocity_mps().abs() > 0.02 || service.session.driver_throttle > 0. {
            return Err("Detené la formación antes de iniciar su rodeo".into());
        }
        let mut prepared = service.session.clone_for_sections()?;
        for i in 0..def.keep {
            if prepared.formation.cars[i].powered {
                if !prepared.formation.cars[i].battery_on {
                    prepared.operate_car(i, crate::CarOperation::Battery)?;
                }
                if !prepared.formation.cars[i].power_on {
                    prepared.operate_car(i, crate::CarOperation::Power)?;
                }
                if prepared
                    .state
                    .diesel
                    .cars
                    .iter()
                    .any(|c| c.vehicle == i && !c.command_running)
                {
                    prepared.toggle_diesel_engine(i)?;
                }
            }
        }
        for car in &mut prepared.formation.cars[..def.keep] {
            car.handbrake = false;
        }
        prepared.sync_operating_brakes();
        service.session = prepared;
        service.departed = true;
        service.parked = false;
        let p = world.run_arounds.entry(id.into()).or_default();
        p.active = true;
        p.phase = 0;
        p.notice = "Tripulación AI: corte, cambios, rodeo y recogida frontal".into();
        world.notice = p.notice.clone();
        self.publish_yard_world(player, world);
        Ok(())
    }
    pub(crate) fn tick_ai_runarounds(&mut self, player: &mut LiveDriveSession) {
        let auto = player
            .state
            .yard_operations
            .definition
            .run_arounds
            .iter()
            .filter(|r| {
                r.automatic
                    && !player
                        .state
                        .yard_operations
                        .run_arounds
                        .get(&r.id)
                        .is_some_and(|s| s.active || s.completed)
            })
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        for id in auto {
            if let Err(e) = self.start_ai_runaround(player, &id) {
                player.state.yard_operations.notice = e;
            }
        }
        let mut world = player.state.yard_operations.clone();
        for d in &world.definition.run_arounds {
            if let Some(s) = self.services.iter().find(|s| s.id == d.service) {
                let p = world.run_arounds.entry(d.id.clone()).or_default();
                p.phase = s.session.operational_leg;
                if p.active
                    && s.session.arrived
                    && s.session.detached_sections().is_empty()
                    && s.session.formation.coupled_count == s.session.formation.cars.len()
                {
                    p.active = false;
                    p.completed = true;
                    p.notice = "Rodeo físico AI completado con todos sus vehículos".into();
                }
            }
        }
        self.publish_yard_world(player, world);
    }
}

pub(crate) fn runaround_switches(
    graph: &openrailsrs_track::TrackGraph,
    edges: &[String],
) -> Result<Vec<openrailsrs_scenarios::model::SwitchDef>, String> {
    use openrailsrs_scenarios::model::{SwitchDef, SwitchPositionDef};
    use openrailsrs_track::NodeKind;
    let mut result = Vec::<SwitchDef>::new();
    for pair in edges.windows(2) {
        let a = graph.edge(&pair[0]).ok_or("Vía de rodeo inexistente")?;
        let b = graph.edge(&pair[1]).ok_or("Vía de rodeo inexistente")?;
        let Some(node) = graph.node(&a.to.0) else {
            return Err("Cambio de rodeo inexistente".into());
        };
        if let NodeKind::Switch { diverging_edge, .. } = &node.kind {
            let diverges = pair
                .iter()
                .any(|id| id.trim_end_matches("_r") == diverging_edge.0.trim_end_matches("_r"));
            let position = if diverges {
                SwitchPositionDef::Diverging
            } else {
                SwitchPositionDef::Straight
            };
            if let Some(existing) = result.iter().find(|s| s.node == node.id.0) {
                if existing.position != position {
                    return Err("Un subtrayecto de rodeo exige dos posiciones del mismo cambio; dividilo antes de atravesarlo".into());
                }
            } else {
                result.push(SwitchDef {
                    node: node.id.0.clone(),
                    position,
                });
            }
            if a.to != b.from {
                return Err("Rodeo discontinuo".into());
            }
        }
    }
    Ok(result)
}
