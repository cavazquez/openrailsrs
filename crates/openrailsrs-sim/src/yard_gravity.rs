//! Free rolling uses the ordinary vehicle gravity, Davis, brakes and couplers.
//! Retarders supply an opposing force. No command prescribes a wagon's speed.
use crate::operational_yards::YardContact;
use crate::{LiveDriveSession, LiveTraffic, path_data::PathData, state::TrainSimState};

/// Physics hook: call after the ordinary wheel-brake calculation/adhesion cap.
/// These forces act at a declared wayside retarder, not in the driver's valve.
pub fn apply_retarder_brakes(
    state: &mut TrainSimState,
    path: &PathData,
    lengths: &[f64],
    forces: &mut [f64],
) {
    for r in state.yard_operations.retarders.values_mut() {
        r.last_forces.clear();
        r.last_positions.clear();
    }
    if state.yard_operations.definition.retarders.is_empty() {
        return;
    }
    let head = path.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    let total: f64 = lengths.iter().sum();
    let mut prefix = 0.;
    for (i, length) in lengths.iter().enumerate() {
        let center = if state.path_reversed {
            head - total + prefix + length / 2.
        } else {
            head - prefix - length / 2.
        };
        prefix += length;
        let Some((edge, position)) =
            PathData::position_at_odometer(&state.path_edges, &path.edges, center)
        else {
            continue;
        };
        let native = if edge.ends_with("_r") {
            path.edges[state.path_edges.iter().position(|id| id == &edge).unwrap()].length_m
                - position
        } else {
            position
        };
        for def in &state.yard_operations.definition.retarders {
            if def.edge.trim_end_matches("_r") != edge.trim_end_matches("_r")
                || native < def.start_m
                || native > def.end_m
            {
                continue;
            }
            let Some(r) = state.yard_operations.retarders.get_mut(&def.id) else {
                continue;
            };
            let force = def.max_force_n * r.fraction;
            if let Some(brake) = forces.get_mut(i) {
                *brake += force;
                let speed = state
                    .vehicles
                    .get(i)
                    .map_or(state.velocity_mps, |v| v.velocity_mps)
                    .abs();
                r.last_forces.push((i, force, speed));
                r.last_positions.push((
                    i,
                    state
                        .vehicles
                        .get(i)
                        .map_or(state.odometer_m, |v| v.position_m),
                ));
            }
        }
    }
}
/// Physics hook after integration. The ledger uses the actual integrated
/// wheelset displacement; stopped wheels dissipate no mechanical work.
pub fn account_retarder_work(state: &mut TrainSimState, dt: f64) {
    if !dt.is_finite() || dt <= 0. {
        return;
    }
    for r in state.yard_operations.retarders.values_mut() {
        let mut work = 0.;
        for (i, force, _) in &r.last_forces {
            let after = state
                .vehicles
                .get(*i)
                .map_or(state.odometer_m, |v| v.position_m);
            if let Some((_, before)) = r.last_positions.iter().find(|(j, _)| j == i) {
                work += force * (after - before).abs();
            }
        }
        *r.absorbed_j
            .entry(state.yard_operations.actor_id.clone())
            .or_default() += work;
    }
}
impl LiveDriveSession {
    pub fn yard_free_rolling(&self) -> bool {
        self.state
            .yard_operations
            .free_rolling_services
            .contains(&self.service_id)
    }
    pub(crate) fn step_free_rolling(&mut self, dt: f64) {
        self.driver_throttle = 0.;
        let backwards = self
            .state
            .yard_operations
            .rolling_backwards
            .get(&self.service_id)
            .copied()
            .unwrap_or(false);
        self.driver_direction = if self.state.path_reversed != backwards {
            0.
        } else {
            1.
        };
        // Occupancy remains published, but insufficient physical brakes cannot
        // stop a runaway merely because a dispatcher or signal asks it to.
        let assistance = self.traffic_brake_assistance;
        self.traffic_brake_assistance = false;
        let before = self.velocity_mps().abs();
        self.step_realtime(dt, |_| {});
        self.traffic_brake_assistance = assistance;
        if self.arrived
            && before > 0.1
            && self.velocity_mps().abs() < 0.01
            && self.state.yard_operations.contacts.len() < 256
        {
            self.state.yard_operations.contacts.push(YardContact {
                time_s: self.time_s(),
                left: self.service_id.clone(),
                right: format!("Tope: {}", self.current_edge_id().unwrap_or("fin")),
                relative_speed_mps: before,
                impulse_ns: self.effective_mass_kg() * before,
                absorbed_j: 0.5 * self.effective_mass_kg() * before * before,
                unsafe_impact: true,
            });
            self.state.yard_operations.notice = "Impacto físico contra el final de vía".into();
        }
    }
}
impl LiveTraffic {
    pub fn release_gravity_train(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let def = world
            .definition
            .gravity_releases
            .iter()
            .find(|r| r.id == id)
            .ok_or("Incidente de gravedad desconocido")?
            .clone();
        if self.remote_drivers.contains(&def.service) {
            return Err(
                "El ejercicio no puede liberar una formación con conductor humano de red".into(),
            );
        }
        if world.released.contains(id) {
            return Err("El incidente ya se inició; restaurar no genera otro tren".into());
        }
        if player.time_s() < def.release_s {
            return Err("Todavía no se autorizó el incidente opcional".into());
        }
        let s = self
            .services
            .iter_mut()
            .find(|s| s.id == def.service)
            .ok_or("La formación del incidente no está presente")?;
        if !s.parked
            || s.session.velocity_mps().abs() > 0.02
            || s.session.formation.cars.iter().any(|c| c.powered)
        {
            return Err(
                "El ejercicio requiere vagones independientes detenidos, sin locomotora".into(),
            );
        }
        if def
            .initial_handbrakes
            .iter()
            .any(|i| *i >= s.session.formation.cars.len())
        {
            return Err(
                "El incidente referencia un freno de mano que no existe en su formación".into(),
            );
        }
        if s.session.head_chainage_m() < s.session.formation.length_m() {
            return Err("La formación del incidente no cabe en su vía inicial".into());
        }
        for (i, car) in s.session.formation.cars.iter_mut().enumerate() {
            car.handbrake = def.initial_handbrakes.contains(&i);
        }
        s.session.sync_operating_brakes();
        s.session.driver_brake = def.initial_brake_fraction;
        s.session.driver_throttle = 0.;
        s.parked = false;
        s.departed = true;
        world.released.insert(id.into());
        world.free_rolling_services.insert(def.service.clone());
        world.notice = format!(
            "{}: vagones libres con freno {:.0}% declarado; pendiente y física normales",
            def.service,
            100. * def.initial_brake_fraction
        );
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub fn set_yard_retarder(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
        fraction: f64,
    ) -> Result<(), String> {
        if !fraction.is_finite() || !(0. ..=1.).contains(&fraction) {
            return Err("Intensidad del retardador fuera de0–100%".into());
        }
        let mut world = player.state.yard_operations.clone();
        world
            .retarders
            .get_mut(id)
            .ok_or("Retardador desconocido")?
            .fraction = fraction;
        world.notice = format!("Retardador {id}: {:.0}%", 100. * fraction);
        self.publish_yard_world(player, world);
        Ok(())
    }
    pub fn protect_recovery(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let def = world
            .definition
            .recovery_exercises
            .iter()
            .find(|r| r.id == id)
            .ok_or("Ejercicio desconocido")?
            .clone();
        let now = player.time_s();
        let r = world.recovery.entry(id.into()).or_default();
        if r.protected_at_s.is_some() {
            return Ok(());
        }
        if now > def.protect_before_s {
            r.failed = Some("Protección colocada después del plazo declarado".into());
        }
        r.protected_at_s = Some(now);
        world.notice = format!(
            "Recuperación {id}: vía {} protegida para los demás servicios",
            def.protection_edge
        );
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub fn secure_recovery(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let def = world
            .definition
            .recovery_exercises
            .iter()
            .find(|r| r.id == id)
            .ok_or("Ejercicio desconocido")?
            .clone();
        let progress = world
            .recovery
            .get(id)
            .ok_or("Primero protegé la zona de recuperación")?;
        if progress.recovered_at_s.is_some() {
            return Ok(());
        }
        if progress.protected_at_s.is_none() || progress.failed.is_some() {
            return Err("La recuperación no tiene protección válida".into());
        }
        let s = self
            .services
            .iter_mut()
            .find(|s| s.id == def.service)
            .ok_or("Los vagones deben conservar su identidad independiente hasta asegurarlos")?;
        let footprint = s.session.own_track_occupancy();
        if s.session.velocity_mps().abs() > 0.02
            || footprint.is_empty()
            || footprint.iter().any(|p| {
                p.edge != def.stop_edge.trim_end_matches("_r")
                    || p.start_m < def.stop_start_m
                    || p.end_m > def.stop_end_m
            })
        {
            return Err(
                "Detené todos los vagones dentro de la zona declarada antes de asegurarlos".into(),
            );
        }
        if world.contacts.iter().any(|c| {
            (c.left == def.service || c.right == def.service)
                && c.relative_speed_mps > def.max_safe_contact_mps
        }) {
            return Err("La recuperación tuvo un impacto superior al límite declarado".into());
        }
        for car in &mut s.session.formation.cars {
            car.handbrake = true;
        }
        s.session.driver_brake = 1.;
        s.session.driver_throttle = 0.;
        s.session.sync_operating_brakes();
        s.parked = true;
        world.free_rolling_services.remove(&def.service);
        world.rolling_backwards.remove(&def.service);
        world.recovery.get_mut(id).unwrap().recovered_at_s = Some(player.time_s());
        world.notice = format!(
            "Recuperación {id} completada: vehículos detenidos, protegidos y con freno de mano"
        );
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub(crate) fn tick_yard_gravity(&mut self, player: &mut LiveDriveSession) {
        let due = player
            .state
            .yard_operations
            .definition
            .gravity_releases
            .iter()
            .filter(|r| {
                r.automatic
                    && player.time_s() >= r.release_s
                    && !player.state.yard_operations.released.contains(&r.id)
            })
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        for id in due {
            if let Err(e) = self.release_gravity_train(player, &id) {
                player.state.yard_operations.notice = e;
            }
        }
        // Merge physical impact ledgers without discarding any existing actor.
        let mut world = player.state.yard_operations.clone();
        for service in &self.services {
            for c in &service.session.state.yard_operations.contacts {
                if !world
                    .contacts
                    .iter()
                    .any(|p| p.time_s == c.time_s && p.left == c.left && p.right == c.right)
                {
                    world.contacts.push(c.clone());
                }
            }
        }
        if world.contacts.len() > 256 {
            world.contacts.drain(..world.contacts.len() - 256);
        }
        self.publish_yard_world(player, world);
    }
    pub(crate) fn yard_recovery_occupancy(
        &self,
        player: &LiveDriveSession,
    ) -> Vec<crate::native_signals::TrackOccupancy> {
        player
            .state
            .yard_operations
            .definition
            .recovery_exercises
            .iter()
            .filter_map(|d| {
                let r = player.state.yard_operations.recovery.get(&d.id)?;
                r.protected_at_s?;
                let edge = player.graph.edge(&d.protection_edge)?;
                Some(crate::native_signals::TrackOccupancy {
                    edge: d.protection_edge.trim_end_matches("_r").into(),
                    start_m: 0.,
                    end_m: edge.length_m,
                    owner: format!("Protección recuperación: {}", d.id),
                })
            })
            .collect()
    }
}

// Contact is a perfectly inelastic wheelset/stock impact extension. It uses
// current normalised track footprints, never screen coordinates. A small
// adaptive physics quantum approaches contact before applying its impulse;
// positions and odometers are not corrected or translated afterwards.
const CONTACT_CLEARANCE_M: f64 = 0.002;
const CONTACT_MIN_DT_S: f64 = 0.0001;
const CONTACT_ACCEL_BOUND_MPS2: f64 = 100.;
#[derive(Clone)]
struct ContactBody {
    id: String,
    edge: String,
    start: f64,
    end: f64,
    velocity: f64,
    mass: f64,
    held: bool,
}
fn path_axis(s: &LiveDriveSession, edge: &str) -> f64 {
    if s.state
        .path_edges
        .iter()
        .find(|id| id.trim_end_matches("_r") == edge)
        .is_some_and(|id| id.ends_with("_r"))
    {
        -1.
    } else {
        1.
    }
}
fn track_velocity(s: &LiveDriveSession, edge: &str) -> f64 {
    let backward = (s.driver_direction <= 0.25) != s.state.path_reversed;
    s.velocity_mps().abs() * path_axis(s, edge) * if backward { -1. } else { 1. }
}
fn contact_candidates(
    traffic: &LiveTraffic,
    player: &LiveDriveSession,
) -> Vec<(ContactBody, ContactBody, f64, f64)> {
    let actors = std::iter::once(player)
        .chain(
            traffic
                .services
                .iter()
                .filter(|s| s.departed)
                .map(|s| &s.session),
        )
        .collect::<Vec<_>>();
    let mut result = vec![];
    for (i, a) in actors.iter().enumerate() {
        for b in actors.iter().skip(i + 1) {
            if !a.yard_free_rolling() && !b.yard_free_rolling() {
                continue;
            }
            for x in a
                .own_track_occupancy()
                .iter()
                .filter(|p| p.owner == a.service_id)
            {
                for y in b
                    .own_track_occupancy()
                    .iter()
                    .filter(|p| p.owner == b.service_id && p.edge == x.edge)
                {
                    let body = |s: &LiveDriveSession, p: &crate::native_signals::TrackOccupancy| {
                        ContactBody {
                            id: s.service_id.clone(),
                            edge: p.edge.clone(),
                            start: p.start_m,
                            end: p.end_m,
                            velocity: track_velocity(s, &p.edge),
                            mass: s.effective_mass_kg(),
                            held: s.formation.cars[..s.formation.coupled_count]
                                .iter()
                                .all(|c| c.handbrake)
                                || s.state.brake_system.total_force_n(0.)
                                    > s.effective_mass_kg()
                                        * 9.81
                                        * s.path_data.edges[s.state.edge_index]
                                            .grade_at(s.state.pos_on_edge_m)
                                            .abs()
                                        / 100.
                                        + 1.,
                        }
                    };
                    let x = body(a, x);
                    let y = body(b, y);
                    let (left, right) = if x.start <= y.start { (x, y) } else { (y, x) };
                    let gap = right.start - left.end;
                    let closing = left.velocity - right.velocity;
                    if !(left.held
                        && right.held
                        && left.velocity.abs() < 1e-6
                        && right.velocity.abs() < 1e-6)
                    {
                        result.push((left, right, gap, closing));
                    }
                }
            }
        }
    }
    result
}
fn apply_contact_velocity(s: &mut LiveDriveSession, edge: &str, normal_velocity: f64) {
    let along = normal_velocity * path_axis(s, edge);
    let backwards = along < 0.;
    s.state.velocity_mps = along.abs();
    for v in &mut s.state.vehicles {
        v.velocity_mps = along.abs();
    }
    // Contact may set a stationary wagon in motion opposite to its original
    // path. Keep that physical direction until a driver takes authority.
    s.driver_direction = if s.state.path_reversed != backwards {
        0.
    } else {
        1.
    };
    s.state
        .yard_operations
        .rolling_backwards
        .insert(s.service_id.clone(), backwards);
    s.driver_throttle = 0.;
    s.state.throttle = 0.;
}
impl LiveTraffic {
    pub(crate) fn yard_contact_quantum(&self, player: &LiveDriveSession, max_dt: f64) -> f64 {
        if player
            .state
            .yard_operations
            .definition
            .gravity_releases
            .is_empty()
        {
            return max_dt;
        }
        contact_candidates(self, player)
            .iter()
            .fold(max_dt, |dt, (_, _, gap, closing)| {
                let reach =
                    closing.max(0.) * max_dt + 0.5 * CONTACT_ACCEL_BOUND_MPS2 * max_dt * max_dt;
                if *gap > CONTACT_CLEARANCE_M + reach {
                    dt
                } else {
                    dt.min(
                        ((gap - CONTACT_CLEARANCE_M)
                            / (closing.max(0.) + CONTACT_ACCEL_BOUND_MPS2 * max_dt))
                            .max(CONTACT_MIN_DT_S),
                    )
                }
            })
            .clamp(CONTACT_MIN_DT_S, max_dt)
    }
    pub(crate) fn resolve_yard_contacts(&mut self, player: &mut LiveDriveSession) {
        if player
            .state
            .yard_operations
            .definition
            .gravity_releases
            .is_empty()
        {
            return;
        }
        let mut world = player.state.yard_operations.clone();
        for (left, right, gap, closing) in contact_candidates(self, player) {
            let reach = CONTACT_CLEARANCE_M
                + closing * CONTACT_MIN_DT_S
                + 0.5 * CONTACT_ACCEL_BOUND_MPS2 * CONTACT_MIN_DT_S.powi(2);
            if gap > reach || closing <= 1e-8 {
                continue;
            }
            let total = left.mass + right.mass;
            if !total.is_finite() || total <= 0. {
                continue;
            }
            let after = (left.mass * left.velocity + right.mass * right.velocity) / total;
            let reduced = left.mass * right.mass / total;
            let impulse = reduced * closing;
            let energy = 0.5 * reduced * closing * closing;
            for (id, edge) in [(&left.id, &left.edge), (&right.id, &right.edge)] {
                let session = if id == &player.service_id {
                    Some(&mut *player)
                } else {
                    self.services.iter_mut().find(|s| &s.id == id).map(|s| {
                        s.parked = false;
                        s.departed = true;
                        &mut s.session
                    })
                };
                if let Some(s) = session {
                    apply_contact_velocity(s, edge, after);
                    // The emergency response commands physical brakes; the
                    // collision impulse itself never erases speed or mass.
                    s.driver_brake = 1.;
                    if closing > 0.5 {
                        s.state.brake_system.emergency = true;
                    }
                    world.free_rolling_services.insert(id.clone());
                    world
                        .rolling_backwards
                        .insert(id.clone(), after * path_axis(s, edge) < 0.);
                }
            }
            let unsafe_impact = closing > 0.5 || gap < -CONTACT_CLEARANCE_M;
            if let Some(previous) = world
                .contacts
                .iter_mut()
                .rev()
                .find(|p| p.left == left.id && p.right == right.id)
            {
                previous.impulse_ns += impulse;
                previous.absorbed_j += energy;
                previous.relative_speed_mps = previous.relative_speed_mps.max(closing);
                previous.unsafe_impact |= unsafe_impact;
            } else if world.contacts.len() < 256 {
                world.contacts.push(YardContact {
                    time_s: player.time_s(),
                    left: left.id.clone(),
                    right: right.id.clone(),
                    relative_speed_mps: closing,
                    impulse_ns: impulse,
                    absorbed_j: energy,
                    unsafe_impact,
                });
            }
            world.notice = format!(
                "Contacto {} / {}: {:.2} m/s, {:.0} N·s, {:.0} J absorbidos{}",
                left.id,
                right.id,
                closing,
                impulse,
                energy,
                if unsafe_impact {
                    "; impacto fuera de maniobra"
                } else {
                    ""
                }
            );
        }
        self.publish_yard_world(player, world);
    }
}

impl LiveTraffic {
    pub fn route_gravity_train(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
        index: usize,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let def = world
            .definition
            .gravity_releases
            .iter()
            .find(|r| r.id == id)
            .ok_or("Incidente desconocido")?;
        let route = def
            .routes
            .get(index)
            .ok_or("Vía de clasificación inexistente")?
            .clone();
        if world.released.contains(id) {
            return Err(
                "Elegí la vía con los vagones detenidos y asegurados antes de liberarlos".into(),
            );
        }
        let actor = self
            .services
            .iter_mut()
            .find(|s| s.id == def.service)
            .ok_or("Vagones de clasificación ausentes")?;
        if !actor.parked
            || actor.session.velocity_mps().abs() > 0.02
            || actor.session.formation.cars.iter().any(|c| !c.handbrake)
        {
            return Err("Asegurá los vagones antes de asignar su vía".into());
        }
        let mut prepared = actor.session.clone_for_sections()?;
        if !prepared.physics.rack.vehicles.is_empty() {
            return Err("La clasificación requiere un mapa de cremallera por alternativa".into());
        }
        let destination = route
            .edges
            .last()
            .and_then(|id| prepared.graph.edge(id))
            .ok_or("Clasificación sin destino")?
            .to
            .0
            .clone();
        let switches = crate::ai_runaround::runaround_switches(&prepared.graph, &route.edges)?;
        for switch in &switches {
            let desired = match switch.position {
                openrailsrs_scenarios::SwitchPositionDef::Straight => {
                    openrailsrs_track::SwitchPosition::Straight
                }
                openrailsrs_scenarios::SwitchPositionDef::Diverging => {
                    openrailsrs_track::SwitchPosition::Diverging
                }
            };
            if prepared
                .dispatcher
                .other_locks
                .iter()
                .any(|l| l.node == switch.node && l.position != desired)
            {
                return Err("El cambio de clasificación está reservado a otro servicio".into());
            }
            if prepared.graph.switch_position(&switch.node) != Some(desired) {
                let touched = prepared
                    .own_track_occupancy()
                    .into_iter()
                    .chain(prepared.external_track_occupancy.iter().cloned())
                    .any(|p| {
                        prepared.graph.edge(&p.edge).is_some_and(|e| {
                            (e.from.0 == switch.node && p.start_m < 5.)
                                || (e.to.0 == switch.node && p.end_m > e.length_m - 5.)
                        })
                    });
                if touched {
                    return Err(
                        "El cambio de clasificación está ocupado: no se moverá bajo ruedas".into(),
                    );
                }
                prepared
                    .graph
                    .set_switch(&switch.node, desired)
                    .map_err(|e| e.to_string())?;
            }
        }
        prepared.gameplay.destination_node = destination.clone();
        prepared.gameplay.destination = destination;
        prepared.adopt_dispatch_path(route.edges)?;
        actor.session = prepared;
        world.gravity_routes.insert(id.into(), index);
        world.notice = format!(
            "{}: vía {} asignada conservando todos los vagones en su posición",
            def.service, route.id
        );
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
}

impl LiveTraffic {
    /// Explicit crew order for an independent wagon portion. It does not cut a
    /// moving locomotive consist or bypass any ordinary uncoupling restriction.
    pub fn secure_gravity_train(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
    ) -> Result<(), String> {
        let mut world = player.state.yard_operations.clone();
        let def = world
            .definition
            .gravity_releases
            .iter()
            .find(|r| r.id == id)
            .ok_or("Formación de gravedad desconocida")?;
        if !world.released.contains(id) {
            return Err("La formación todavía no salió de su posición inicial".into());
        }
        let s = self
            .services
            .iter_mut()
            .find(|s| s.id == def.service)
            .ok_or("Vagones independientes ausentes")?;
        if s.session.velocity_mps().abs() > 0.02
            || s.session.driver_throttle > 0.
            || s.session.formation.cars.iter().any(|c| c.powered)
        {
            return Err("Detené todos los vagones sin locomotora antes de asegurarlos".into());
        }
        let footprint = s.session.own_track_occupancy();
        let zone = player
            .state
            .rail_operations
            .definition
            .yard_deliveries
            .iter()
            .find(|z| {
                z.service.as_deref() == Some(def.service.as_str())
                    && !footprint.is_empty()
                    && footprint.iter().all(|p| {
                        p.edge == z.edge.trim_end_matches("_r")
                            && p.start_m >= z.start_m
                            && p.end_m <= z.end_m
                    })
            });
        if zone.is_none() {
            return Err(
                "La formación completa debe quedar dentro de su vía de clasificación declarada"
                    .into(),
            );
        }
        let mut prepared = s.session.clone_for_sections()?;
        for i in 0..prepared.formation.coupled_count {
            if !prepared.formation.cars[i].handbrake {
                prepared.operate_car(i, crate::CarOperation::Handbrake)?;
            }
        }
        prepared.sync_operating_brakes();
        prepared.driver_brake = 1.;
        prepared.driver_throttle = 0.;
        prepared.driver_direction = 0.5;
        s.session = prepared;
        s.parked = true;
        world.free_rolling_services.remove(&def.service);
        world.rolling_backwards.remove(&def.service);
        world.notice = format!(
            "{}: clasificación asegurada con todos sus frenos de mano",
            def.service
        );
        self.publish_yard_world(player, world);
        self.synchronize_occupancy(player);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prescribed_contact_conserves_momentum_loses_only_inelastic_energy_and_never_repositions_stock()
     {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/operational_yards");
        let mut def =
            openrailsrs_scenarios::load_scenario(dir.join("scenario_gravity.toml")).unwrap();
        let obstacle:openrailsrs_scenarios::TrainEntryDef=toml::from_str("id='obstacle'\nconsist='consists/wagon.con'\nstart='crest'\ndestination='road_a_end'\nstart_offset_m=95.001\nparked=true\noutput_csv='obstacle.csv'").unwrap();
        def.extra_trains.push(obstacle);
        let mut player = LiveDriveSession::from_scenario(&dir, &def).unwrap();
        let mut traffic = LiveTraffic::from_scenario(&dir, &def).unwrap();
        traffic
            .release_gravity_train(&mut player, "release-wagons")
            .unwrap();
        traffic.services[0].session.state.velocity_mps = 2.;
        for b in &mut traffic.services[0].session.state.vehicles {
            b.velocity_mps = 2.;
        }
        let poses = traffic
            .services
            .iter()
            .map(|s| s.session.head_chainage_m())
            .collect::<Vec<_>>();
        let masses = traffic
            .services
            .iter()
            .map(|s| s.session.effective_mass_kg())
            .collect::<Vec<_>>();
        let before_p = masses[0] * 2.;
        let before_ke = 0.5 * masses[0] * 4.;
        traffic.resolve_yard_contacts(&mut player);
        assert_eq!(
            traffic
                .services
                .iter()
                .map(|s| s.session.head_chainage_m())
                .collect::<Vec<_>>(),
            poses
        );
        let after_p = traffic
            .services
            .iter()
            .zip(&masses)
            .map(|(s, m)| s.session.velocity_mps() * m)
            .sum::<f64>();
        let after_ke = traffic
            .services
            .iter()
            .zip(&masses)
            .map(|(s, m)| 0.5 * m * s.session.velocity_mps().powi(2))
            .sum::<f64>();
        assert!((before_p - after_p).abs() < 1e-6);
        let impact = &player.state.yard_operations.contacts[0];
        assert!((before_ke - after_ke - impact.absorbed_j).abs() < 1e-6);
        assert!(impact.impulse_ns > 0.);
        assert!(
            traffic
                .services
                .iter()
                .all(|s| s.session.state.brake_system.emergency)
        );
    }
}
