//! Rotating and translating decks carry stopped physical formations. A route
//! changes only after the animated deck reaches an authored, compatible port.
use crate::operational_yards::{CarrierState, YardWorldState};
use crate::{LiveDriveSession, LiveTraffic, SessionSnapshot, path_data::PathData};
use openrailsrs_scenarios::operational_yards::{MovingDeviceDef, MovingDeviceKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MovingDevicePose {
    pub id: String,
    pub kind: MovingDeviceKind,
    pub center_world: [f64; 3],
    pub heading_rad: f64,
    pub deck_length_m: f64,
    pub deck_width_m: f64,
    pub moving: bool,
    pub aligned_port: usize,
    pub target_port: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct YardVehiclePose {
    pub center_world: [f64; 3],
    pub heading_rad: f64,
    pub device: String,
}
fn pose(d: &MovingDeviceDef, position: f64) -> MovingDevicePose {
    let angle = if d.kind == MovingDeviceKind::Turntable {
        position
    } else {
        d.heading_rad
    };
    let lateral = if d.kind == MovingDeviceKind::TransferTable {
        position
    } else {
        0.
    };
    MovingDevicePose {
        id: d.id.clone(),
        kind: d.kind,
        center_world: [
            d.center_x_m - lateral * angle.sin(),
            d.elevation_m,
            d.center_z_m + lateral * angle.cos(),
        ],
        heading_rad: angle,
        deck_length_m: d.deck_length_m,
        deck_width_m: d.deck_width_m,
        moving: false,
        aligned_port: 0,
        target_port: None,
    }
}
fn path_valid(s: &LiveDriveSession, path: &[String]) -> bool {
    !path.is_empty()
        && path.iter().all(|id| s.graph.edge(id).is_some())
        && path.windows(2).all(|p| {
            s.graph
                .edge(&p[0])
                .zip(s.graph.edge(&p[1]))
                .is_some_and(|(a, b)| a.to == b.from)
        })
}
impl LiveDriveSession {
    pub fn initialize_yard_operations(&mut self) -> Result<(), String> {
        self.state.yard_operations.actor_id = self.service_id.clone();
        let world = &self.state.yard_operations;
        if !world.definition.valid() {
            return Err("Definición del patio operativo inválida".into());
        }
        for d in &world.definition.moving_devices {
            for p in &d.ports {
                if !path_valid(self, &p.path) {
                    return Err(format!("Mesa {}: puerto {} sin vía continua", d.id, p.id));
                }
                let e = self.graph.edge(&p.path[0]).unwrap();
                let a = self.graph.node(&e.from.0).ok_or("Puerto sin origen")?;
                let b = self.graph.node(&e.to.0).ok_or("Puerto sin destino")?;
                let placement = pose(d, p.position);
                let expected_a = [
                    placement.center_world[0] - d.deck_length_m / 2. * placement.heading_rad.cos(),
                    placement.center_world[2] - d.deck_length_m / 2. * placement.heading_rad.sin(),
                ];
                let expected_b = [
                    placement.center_world[0] + d.deck_length_m / 2. * placement.heading_rad.cos(),
                    placement.center_world[2] + d.deck_length_m / 2. * placement.heading_rad.sin(),
                ];
                if (e.length_m - d.deck_length_m).abs() > 0.001
                    || (a.x_m - expected_a[0]).hypot(a.y_m - expected_a[1]) > 0.05
                    || (b.x_m - expected_b[0]).hypot(b.y_m - expected_b[1]) > 0.05
                {
                    return Err(format!(
                        "Mesa {}: geometría del puerto {} no coincide con su tablero",
                        d.id, p.id
                    ));
                }
            }
        }
        for p in &world.definition.pools {
            if self
                .graph
                .edge(&p.edge)
                .is_none_or(|e| p.end_m > e.length_m)
            {
                return Err(format!("Pool {}: intervalo de depósito fuera de vía", p.id));
            }
        }
        for r in &world.definition.rendezvous {
            if self
                .graph
                .edge(&r.edge)
                .is_none_or(|e| r.position_m > e.length_m)
            {
                return Err(format!(
                    "Encuentro {}: marcador fuera de su vector original",
                    r.id
                ));
            }
        }
        for r in &world.definition.retarders {
            let e = self
                .graph
                .edge(&r.edge)
                .ok_or_else(|| format!("Retardador {}: vía inexistente", r.id))?;
            if r.end_m > e.length_m {
                return Err(format!("Retardador {} fuera de vía", r.id));
            }
        }
        for r in &world.definition.recovery_exercises {
            if self.graph.edge(&r.protection_edge).is_none()
                || self
                    .graph
                    .edge(&r.stop_edge)
                    .is_none_or(|e| r.stop_end_m > e.length_m)
            {
                return Err(format!("Recuperación {}: zona fuera de vía", r.id));
            }
        }
        self.initialize_runaround_plan()?;
        Ok(())
    }
    pub fn yard_device_poses(&self) -> Vec<MovingDevicePose> {
        self.state
            .yard_operations
            .definition
            .moving_devices
            .iter()
            .filter_map(|d| {
                let state = self.state.yard_operations.devices.get(&d.id)?;
                let mut p = pose(d, state.position);
                p.moving = state.target_port.is_some();
                p.aligned_port = state.aligned_port;
                p.target_port = state.target_port;
                Some(p)
            })
            .collect()
    }
    pub fn yard_vehicle_pose(&self, index: usize) -> Option<YardVehiclePose> {
        let world = &self.state.yard_operations;
        let id = self.vehicle_identities().get(index)?.clone();
        for d in &world.definition.moving_devices {
            let state = world.devices.get(&d.id)?;
            let Some(c) = &state.carrier else { continue };
            if state.target_port.is_none()
                || c.service != self.service_id
                || !c.vehicle_ids.contains(&id)
            {
                continue;
            }
            let p = pose(d, state.position);
            let local = c.head_on_deck_m
                + (if c.vehicle_reversed {
                    -self.formation.length_m() - self.formation.center_offset_m(index)
                } else {
                    self.formation.center_offset_m(index)
                })
                - d.deck_length_m / 2.;
            return Some(YardVehiclePose {
                center_world: [
                    p.center_world[0] + local * p.heading_rad.cos(),
                    p.center_world[1],
                    p.center_world[2] + local * p.heading_rad.sin(),
                ],
                heading_rad: p.heading_rad
                    + if c.vehicle_reversed {
                        std::f64::consts::PI
                    } else {
                        0.
                    },
                device: d.id.clone(),
            });
        }
        None
    }
    pub fn yard_driving_blocked(&self) -> bool {
        self.state.yard_operations.devices.values().any(|d| {
            d.target_port.is_some()
                && d.carrier
                    .as_ref()
                    .is_some_and(|c| c.service == self.service_id)
        })
    }
    pub(crate) fn yard_device_occupancy(&self) -> Vec<crate::native_signals::TrackOccupancy> {
        let world = &self.state.yard_operations;
        let mut out = vec![];
        for d in &world.definition.moving_devices {
            let Some(s) = world.devices.get(&d.id) else {
                continue;
            };
            for (i, p) in d.ports.iter().enumerate() {
                if s.target_port.is_none() && i == s.aligned_port {
                    continue;
                }
                let Some(e) = self.graph.edge(&p.path[0]) else {
                    continue;
                };
                out.push(crate::native_signals::TrackOccupancy {
                    edge: e.id.0.trim_end_matches("_r").into(),
                    start_m: 0.,
                    end_m: e.length_m,
                    owner: format!("Mesa: {}", d.id),
                });
            }
        }
        out
    }
    fn deck_head(&self, bridge: &str) -> Option<(f64, bool)> {
        let mut prefix = 0.;
        for (i, id) in self.state.path_edges.iter().enumerate() {
            let length = self.path_data.edges.get(i)?.length_m;
            if id.trim_end_matches("_r") == bridge.trim_end_matches("_r") {
                let head = self.head_chainage_m() - prefix;
                let reversed = id.ends_with("_r") != bridge.ends_with("_r");
                return (head - self.formation.length_m() >= 0. && head <= length).then_some((
                    if reversed {
                        length - head + self.formation.length_m()
                    } else {
                        head
                    },
                    self.state.path_reversed ^ reversed,
                ));
            }
            prefix += length;
        }
        None
    }
    fn validate_device_departure(&self, path: &[String], head: f64) -> Result<(), String> {
        if !self.physics.rack.vehicles.is_empty() {
            return Err(
                "La mesa requiere un mapa de cremallera por puerto antes de transportar este tren"
                    .into(),
            );
        }
        if !path_valid(self, path)
            || !self.operational_legs.is_empty()
            || self.formation.coupled_count != self.formation.cars.len()
        {
            return Err(
                "La mesa requiere una formación completa y una salida continua sin PAT pendiente"
                    .into(),
            );
        }
        if path
            .last()
            .and_then(|id| self.graph.edge(id))
            .is_none_or(|e| e.to.0 != self.gameplay.destination_node)
        {
            return Err("El puerto de salida no conserva el destino del servicio".into());
        }
        let mut before = 0.;
        for target in self
            .gameplay
            .stop_targets
            .iter()
            .skip(self.gameplay.next_stop_idx)
        {
            let mut found = None;
            let mut distance = 0.;
            for id in path {
                let e = self.graph.edge(id).unwrap();
                distance += e.length_m;
                if e.to.0 == target.node_id {
                    found = Some(
                        distance
                            + self
                                .stop_offsets
                                .get(&target.node_id)
                                .copied()
                                .unwrap_or(0.),
                    );
                    break;
                }
            }
            let at = found.ok_or("La salida de la mesa omite una parada pendiente")?;
            if at < head || at < before {
                return Err("La salida de la mesa altera el orden de paradas".into());
            }
            before = at;
        }
        Ok(())
    }
    fn adopt_device_port(
        &mut self,
        path: Vec<String>,
        head: f64,
        reversed: bool,
    ) -> Result<(), String> {
        self.validate_device_departure(&path, head)?;
        let mut chainages = std::collections::HashMap::new();
        let mut total = 0.;
        for id in &path {
            let e = self.graph.edge(id).unwrap();
            total += e.length_m;
            chainages.insert(e.to.0.clone(), total);
        }
        for target in self
            .gameplay
            .stop_targets
            .iter_mut()
            .skip(self.gameplay.next_stop_idx)
        {
            target.cum_dist_m = chainages[&target.node_id]
                + self
                    .stop_offsets
                    .get(&target.node_id)
                    .copied()
                    .unwrap_or(0.);
        }
        self.path_data = PathData::from_path(&path, &self.graph);
        self.state.path_edges = path;
        self.state.path_reversed = reversed;
        let odometer = self.state.odometer_m;
        crate::live_drive::apply_start_offset(&mut self.state, &self.path_data, head);
        self.state.odometer_m = odometer;
        self.previous_render_chainage_m = head;
        self.own_track_reservations.clear();
        self.dispatcher.own_locks.clear();
        self.native_signals.needs_refresh = true;
        Ok(())
    }
    pub(crate) fn validate_yard_snapshot(&self, saved: &SessionSnapshot) -> Result<(), String> {
        if (!saved.state.yard_operations.definition.is_empty()
            && saved.state.yard_operations.actor_id != self.service_id)
            || !saved
                .state
                .yard_operations
                .valid_for(&self.state.yard_operations.definition)
        {
            return Err("Estado de patio guardado incompatible".into());
        }
        for state in saved.state.yard_operations.devices.values() {
            if let Some(c) = &state.carrier {
                if c.service == self.service_id {
                    let length = saved
                        .formation
                        .cars
                        .iter()
                        .map(|car| car.length_m)
                        .sum::<f64>();
                    if saved.formation.coupled_count != saved.formation.cars.len()
                        || !length.is_finite()
                        || length <= 0.
                        || c.head_on_deck_m < length
                    {
                        return Err("La formación guardada no cabe completa sobre la mesa".into());
                    }
                }
                if c.service == self.service_id
                    && c.vehicle_ids
                        != if saved.formation.vehicle_ids.is_empty() {
                            (0..saved.formation.cars.len())
                                .map(|i| format!("{}/car/{i}", self.service_id))
                                .collect::<Vec<_>>()
                        } else {
                            saved.formation.vehicle_ids.clone()
                        }
                {
                    return Err("La mesa guardada transporta otra formación".into());
                }
            }
        }
        Ok(())
    }
}
impl LiveTraffic {
    pub fn request_moving_device(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
        target: usize,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let d = world
            .definition
            .moving_devices
            .iter()
            .find(|d| d.id == id)
            .ok_or("Mesa desconocida")?
            .clone();
        let s = world.devices.get(id).ok_or("Mesa sin estado")?;
        if target >= d.ports.len() || s.target_port.is_some() {
            return Err("Puerto inexistente o mesa en movimiento".into());
        }
        if target == s.aligned_port {
            return Ok(());
        }
        let bridge = &d.ports[s.aligned_port].path[0];
        let mut carrier = None;
        for session in std::iter::once(&*player).chain(
            self.services
                .iter()
                .filter(|s| s.departed)
                .map(|s| &s.session),
        ) {
            let overlaps = session
                .own_track_occupancy()
                .iter()
                .any(|p| p.edge == bridge.trim_end_matches("_r"));
            if !overlaps {
                continue;
            }
            if carrier.is_some() {
                return Err("Dos formaciones ocupan la mesa".into());
            }
            let (head, vehicle_reversed) = session
                .deck_head(bridge)
                .ok_or("La formación no está completamente dentro del tablero")?;
            if session.velocity_mps().abs() > 0.02
                || session.driver_throttle > 0.
                || session.driver_brake < 0.5
                || session.formation.length_m() > d.deck_length_m
                || session.effective_mass_kg() > d.max_mass_kg
            {
                return Err(
                    "Detené y frená la formación completa dentro de la capacidad de la mesa".into(),
                );
            }
            session.validate_device_departure(&d.ports[target].path, head)?;
            carrier = Some(CarrierState {
                service: session.service_id.clone(),
                vehicle_ids: session.vehicle_identities(),
                source_port: s.aligned_port,
                head_on_deck_m: head,
                vehicle_reversed,
            });
        }
        // A switch lock leading across a deck cannot be bypassed by moving it.
        for session in std::iter::once(&*player).chain(self.services.iter().map(|s| &s.session)) {
            if carrier
                .as_ref()
                .is_some_and(|c| c.service == session.service_id)
            {
                continue;
            }
            if session.own_track_reservations.iter().any(|g| {
                d.ports
                    .iter()
                    .any(|p| p.path[0].trim_end_matches("_r") == g.edge)
            }) {
                return Err("La mesa tiene una reserva de circulación ajena".into());
            }
        }
        let state = world.devices.get_mut(id).unwrap();
        state.target_port = Some(target);
        state.carrier = carrier;
        state.started_s = player.time_s();
        world.notice = format!(
            "Mesa {id}: traslado físico al puerto {}",
            d.ports[target].id
        );
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub(crate) fn advance_moving_devices(
        &mut self,
        player: &mut LiveDriveSession,
        world: &mut YardWorldState,
        dt: f64,
    ) {
        for d in world.definition.moving_devices.clone() {
            let Some(s) = world.devices.get_mut(&d.id) else {
                continue;
            };
            let Some(target) = s.target_port else {
                continue;
            };
            let destination = d.ports[target].position;
            let delta = destination - s.position;
            let travel = d.speed * dt;
            if delta.abs() > travel {
                s.position += delta.signum() * travel;
                continue;
            }
            s.position = destination;
            if let Some(carrier) = &s.carrier {
                let session = if carrier.service == player.service_id {
                    Some(&mut *player)
                } else {
                    self.services
                        .iter_mut()
                        .find(|v| v.id == carrier.service)
                        .map(|v| &mut v.session)
                };
                let Some(session) = session else {
                    world.notice =
                        "Se perdió la identidad física transportada; mesa bloqueada".into();
                    continue;
                };
                if session.vehicle_identities() != carrier.vehicle_ids {
                    world.notice = "Cambió la formación durante el traslado; mesa bloqueada".into();
                    continue;
                }
                if let Err(error) = session.adopt_device_port(
                    d.ports[target].path.clone(),
                    carrier.head_on_deck_m,
                    carrier.vehicle_reversed,
                ) {
                    world.notice = error;
                    continue;
                }
            }
            s.aligned_port = target;
            s.target_port = None;
            s.carrier = None;
            world.notice = format!(
                "Mesa {} alineada y enclavada en {}",
                d.id, d.ports[target].id
            );
        }
    }
}
