//! Pools reserve existing, parked physical stock. Checkout never creates a
//! second train or replenishes fuel, pressure, sand, cargo or maintenance.
use crate::{CarOperation, LiveDriveSession, LiveTraffic};
use openrailsrs_scenarios::operational_yards::MaterialPoolDef;
fn inside(session: &LiveDriveSession, pool: &MaterialPoolDef) -> bool {
    let parts = session.own_track_occupancy();
    !parts.is_empty()
        && parts.iter().all(|p| {
            p.edge == pool.edge.trim_end_matches("_r")
                && p.start_m >= pool.start_m
                && p.end_m <= pool.end_m
        })
}
impl LiveTraffic {
    pub fn reserve_pool_material(
        &mut self,
        player: &mut LiveDriveSession,
        pool_id: &str,
        job_id: &str,
    ) -> Result<String, String> {
        let mut world = player.state.yard_operations.clone();
        let pool = world
            .definition
            .pools
            .iter()
            .find(|p| p.id == pool_id)
            .ok_or("Pool desconocido")?
            .clone();
        let job = pool
            .jobs
            .iter()
            .find(|j| j.id == job_id)
            .ok_or("Pedido de material inexistente")?;
        if world
            .completed_pool_jobs
            .contains(&(pool_id.into(), job_id.into()))
        {
            return Err("El pedido ya fue servido; reservá otro pedido del horario".into());
        }
        if player.time_s() > job.deadline_s {
            return Err("Venció el pedido de material".into());
        }
        if let Some((member, _)) = world
            .pool_members
            .iter()
            .find(|(_, s)| s.pool == pool_id && s.reserved_job.as_deref() == Some(job_id))
        {
            return Ok(member.clone());
        }
        let member = pool
            .members
            .iter()
            .find(|id| {
                !self.remote_drivers.contains(*id)
                    && world
                        .pool_members
                        .get(*id)
                        .is_some_and(|s| s.reserved_job.is_none() && !s.dispatched)
                    && self
                        .services
                        .iter()
                        .find(|s| &s.id == *id)
                        .is_some_and(|s| {
                            s.parked
                                && s.session.velocity_mps().abs() <= 0.02
                                && (job.min_vehicles..=job.max_vehicles)
                                    .contains(&s.session.formation.coupled_count)
                                && s.session.formation.cars.iter().all(|c| c.handbrake)
                                && inside(&s.session, &pool)
                        })
            })
            .ok_or("Pool sin material físico disponible y asegurado para este pedido")?
            .clone();
        let s = world.pool_members.get_mut(&member).unwrap();
        s.reserved_job = Some(job_id.into());
        s.reserved_at_s = Some(player.time_s());
        world.notice = format!("Pool {pool_id}: {member} reservado para {job_id}");
        self.publish_yard_world(player, world);
        Ok(member)
    }
    pub fn dispatch_pool_material(
        &mut self,
        player: &mut LiveDriveSession,
        member: &str,
    ) -> Result<(), String> {
        if self.remote_drivers.contains(member) {
            return Err(
                "El material tiene conductor humano de red; no se reasigna por el pool".into(),
            );
        }
        let mut world = player.state.yard_operations.clone();
        let record = world
            .pool_members
            .get(member)
            .ok_or("Material ajeno a los pools")?
            .clone();
        if record.dispatched {
            return Err("Este material ya está asignado y en servicio".into());
        }
        let pool = world
            .definition
            .pools
            .iter()
            .find(|p| p.id == record.pool)
            .unwrap();
        let job = pool
            .jobs
            .iter()
            .find(|j| Some(j.id.as_str()) == record.reserved_job.as_deref())
            .ok_or("Reservá el material antes de despacharlo")?;
        if player.time_s() < job.not_before_s || player.time_s() > job.deadline_s {
            return Err("El pedido aún no está autorizado o ya venció".into());
        }
        let i = self
            .services
            .iter()
            .position(|s| s.id == member)
            .ok_or("El material reservado no está presente físicamente")?;
        if !self.services[i].parked || !inside(&self.services[i].session, pool) {
            return Err("El material salió del depósito antes de su despacho".into());
        }
        let mut prepared = self.services[i].session.clone_for_sections()?;
        prepared.prepare_pool_itinerary()?;
        prepared.driver_throttle = 0.;
        prepared.driver_brake = 1.;
        for index in 0..prepared.formation.coupled_count {
            if prepared.formation.cars[index].powered {
                if !prepared.formation.cars[index].battery_on {
                    prepared.operate_car(index, CarOperation::Battery)?;
                }
                if !prepared.formation.cars[index].power_on {
                    prepared.operate_car(index, CarOperation::Power)?;
                }
                if prepared
                    .state
                    .diesel
                    .cars
                    .iter()
                    .any(|c| c.vehicle == index && !c.command_running)
                {
                    prepared.toggle_diesel_engine(index)?;
                }
            }
            if prepared.formation.cars[index].handbrake {
                prepared.operate_car(index, CarOperation::Handbrake)?;
            }
        }
        let now = player.time_s();
        let offset = (now - record.last_departure_s).max(0.);
        prepared.state.time.0 = now;
        // The physical actor receives a new departure, not a replacement state.
        for stop in prepared
            .gameplay
            .stop_targets
            .iter_mut()
            .skip(prepared.gameplay.next_stop_idx)
        {
            stop.arrive_s += offset;
            stop.depart_s += offset;
        }
        for leg in &mut prepared.operational_legs {
            for stop in &mut leg.stops {
                stop.arrive_s += offset;
                stop.depart_s += offset;
            }
            if let Some(t) = &mut leg.not_before_s {
                *t += offset;
            }
        }
        let s = &mut self.services[i];
        s.session = prepared;
        s.parked = false;
        s.departed = true;
        s.departure_s = now;
        let record = world.pool_members.get_mut(member).unwrap();
        record.dispatched = true;
        record.checkout_s = Some(now);
        record.last_departure_s = now;
        world.notice = format!("{member}: material del pool despachado sin duplicar vehículos");
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub fn return_pool_material(
        &mut self,
        player: &mut LiveDriveSession,
        member: &str,
    ) -> Result<(), String> {
        if self.remote_drivers.contains(member) {
            return Err("El material tiene conductor humano de red; no se retira su puesto".into());
        }
        let mut world = player.state.yard_operations.clone();
        let record = world
            .pool_members
            .get(member)
            .ok_or("Material ajeno al pool")?
            .clone();
        let pool = world
            .definition
            .pools
            .iter()
            .find(|p| p.id == record.pool)
            .unwrap();
        if !record.dispatched {
            return Err("Este material no está despachado".into());
        }
        let s = self
            .services
            .iter_mut()
            .find(|s| s.id == member)
            .ok_or("El servicio debe devolver su formación completa al depósito")?;
        if !s.session.arrived {
            return Err(
                "Completá el recorrido físico del pedido antes de devolver su material".into(),
            );
        }
        if s.session.velocity_mps().abs() > 0.02
            || !inside(&s.session, pool)
            || s.session.formation.coupled_count != s.session.formation.cars.len()
        {
            return Err("Detené y asegurá todos los vehículos dentro de su vía de depósito".into());
        }
        let mut prepared = s.session.clone_for_sections()?;
        // Returning a stopped service is a crew instruction to isolate its
        // driving controls and apply handbrakes, not an alteration of its pose.
        // AI may retain its last handle demand after the itinerary ends.
        prepared.driver_throttle = 0.;
        prepared.driver_brake = 1.;
        prepared.driver_direction = 0.5;
        for index in 0..prepared.formation.coupled_count {
            if !prepared.formation.cars[index].handbrake {
                prepared.operate_car(index, CarOperation::Handbrake)?;
            }
        }
        prepared.sync_operating_brakes();
        s.session = prepared;
        s.parked = true;
        s.session.driver_throttle = 0.;
        s.session.driver_brake = 1.;
        s.session.driver_direction = 0.5;
        if let Some(job) = &record.reserved_job {
            world
                .completed_pool_jobs
                .insert((record.pool.clone(), job.clone()));
        }
        let record = world.pool_members.get_mut(member).unwrap();
        record.dispatched = false;
        record.reserved_job = None;
        record.reserved_at_s = None;
        record.checkout_s = None;
        record.returns += 1;
        world.notice = format!("{member} devuelto: vehículos, consumibles y estado conservados");
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
}

impl LiveDriveSession {
    pub(crate) fn step_pool_service(&mut self, dt: f64) -> bool {
        if !self
            .state
            .yard_operations
            .pool_members
            .get(&self.service_id)
            .is_some_and(|m| m.dispatched)
        {
            return false;
        }
        let Some(leg) = self.operational_legs.get(self.operational_leg) else {
            return false;
        };
        let target = leg
            .operations
            .get(self.operational_operation)
            .map_or(leg.end_chainage_m, |o| o.chainage_m);
        crate::mobile_portions::yard_precision_controls(self, target, 3.);
        self.step_realtime(dt, |_| {});
        self.tick_operational_path(true);
        true
    }
    fn prepare_pool_itinerary(&mut self) -> Result<(), String> {
        if !self.arrived && self.operational_leg == 0 {
            return Ok(());
        }
        let first=self.operational_legs.first().cloned().ok_or("El material volvió sin un itinerario de salida repetible; prepará el siguiente servicio antes de reservarlo")?;
        if !self.physics.rack.vehicles.is_empty() {
            return Err("El despacho repetido requiere mapa de cremallera por subtrayecto".into());
        }
        let pd = crate::path_data::PathData::from_path(&first.edges, &self.graph);
        let normalize = |s: &Self, (id, p): (String, f64)| {
            let n = s.graph.edge(&id).map_or(0., |e| e.length_m);
            (
                id.trim_end_matches("_r").to_string(),
                if id.ends_with("_r") { n - p } else { p },
            )
        };
        let source = self
            .position_at_head_offset(if first.reversed {
                -self.formation.length_m()
            } else {
                0.
            })
            .map(|p| normalize(self, p))
            .ok_or("Extremo del material fuera de vía")?;
        let mut before = 0.;
        let mut anchor = None;
        for (id, e) in first.edges.iter().zip(&pd.edges) {
            if id.trim_end_matches("_r") == source.0 {
                if anchor.is_some() {
                    return Err("Salida repetida ambigua: el mismo vector aparece dos veces".into());
                }
                anchor = Some(
                    before
                        + if id.ends_with("_r") {
                            e.length_m - source.1
                        } else {
                            source.1
                        },
                );
            }
            before += e.length_m;
        }
        let anchor =
            anchor.ok_or("El material no volvió a la vía física de salida del depósito")?;
        for i in 0..self.formation.coupled_count {
            let offset = self.formation.center_offset_m(i);
            let old = self
                .position_at_head_offset(offset)
                .map(|p| normalize(self, p));
            let new = crate::path_data::PathData::position_at_odometer(
                &first.edges,
                &pd.edges,
                anchor
                    + if first.reversed {
                        -self.formation.length_m() - offset
                    } else {
                        offset
                    },
            )
            .map(|p| normalize(self, p));
            if !old
                .zip(new)
                .is_some_and(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() < 0.001)
            {
                return Err(
                    "El nuevo despacho desplazaría vehículos: volvé físicamente al depósito".into(),
                );
            }
        }
        self.path_data = pd;
        self.state.path_edges = first.edges;
        self.state.path_reversed = first.reversed;
        let odometer = self.state.odometer_m;
        crate::live_drive::apply_start_offset(&mut self.state, &self.path_data, anchor);
        self.state.odometer_m = odometer;
        self.operational_leg = 0;
        self.operational_operation = 0;
        self.operational_prefix_m = anchor;
        self.operational_start_m = anchor;
        self.operational_wait_started = None;
        self.previous_render_chainage_m = anchor;
        self.arrived = false;
        let mut gameplay = crate::LiveGameplay::new(
            self.gameplay.destination_node.clone(),
            self.gameplay.penalty_per_second_late,
            self.gameplay.stop_targets.clone(),
        );
        gameplay.shunting_steps = self.gameplay.shunting_steps.clone();
        gameplay.quick_station_practice = self.gameplay.quick_station_practice;
        gameplay.missed_stop_policy = self.gameplay.missed_stop_policy;
        self.gameplay = gameplay;
        self.state.pat_operations_completed.clear();
        self.own_track_reservations.clear();
        self.dispatcher.own_locks.clear();
        self.native_signals.needs_refresh = true;
        Ok(())
    }
}
