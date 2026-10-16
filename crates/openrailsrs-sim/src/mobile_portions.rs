//! Rendezvous handshakes stop both real actors at their couplers before a
//! transfer. A missing train, incompatible brake or timeout remains diagnostic.
use crate::{LiveDriveSession, LiveTraffic, sections::CouplingEnd};
impl LiveTraffic {
    pub(crate) fn with_yard_receiver<T>(
        &mut self,
        player: &mut LiveDriveSession,
        id: &str,
        run: impl FnOnce(&mut Self, &mut LiveDriveSession) -> Result<T, String>,
    ) -> Result<T, String> {
        if id == player.service_id {
            return run(self, player);
        }
        let i = self
            .services
            .iter()
            .position(|s| s.id == id)
            .ok_or("Receptor de la porción desconocido")?;
        let human = player.service_id.clone();
        let flags = (
            self.services[i].departure_s,
            self.services[i].departed,
            self.services[i].parked,
        );
        std::mem::swap(player, &mut self.services[i].session);
        self.services[i].id = human.clone();
        let result = run(self, player);
        let slot = self
            .services
            .iter()
            .position(|s| s.id == human)
            .expect("a yard transfer never consumes the player's service");
        std::mem::swap(player, &mut self.services[slot].session);
        self.services[slot].id = id.into();
        self.services[slot].departure_s = flags.0;
        self.services[slot].departed = flags.1;
        self.services[slot].parked = flags.2;
        result
    }
    fn collect_meeting(
        &mut self,
        player: &mut LiveDriveSession,
        receiver: &str,
        donor: &str,
        end: CouplingEnd,
    ) -> Result<(), String> {
        let human = |id: &str| {
            (id == player.service_id && player.state.yard_operations.human_driven)
                || self
                    .services
                    .iter()
                    .any(|s| s.id == id && s.session.state.yard_operations.human_driven)
        };
        if self.remote_drivers.contains(receiver)
            || self.remote_drivers.contains(donor)
            || human(receiver)
            || human(donor)
        {
            return Err("El encuentro requiere autorización del conductor humano; no se modificará su formación de red".into());
        }
        if donor == player.service_id && receiver != player.service_id {
            return Err(
                "El servicio humano debe autorizar su entrega cambiando de conductor".into(),
            );
        }
        self.with_yard_receiver(player, receiver, |traffic, receiver| {
            let i = traffic
                .services
                .iter()
                .position(|s| s.id == donor)
                .ok_or("Esperando al servicio de la porción")?;
            if !traffic.services[i].departed
                || traffic.services[i].session.velocity_mps().abs() > 0.02
            {
                return Err("Esperando que llegue y se detenga la porción móvil".into());
            }
            if !receiver.strict_portion_contact(&traffic.services[i].session, end) {
                return Err(
                    "Esperando contacto a menos de 8 mm y ambas formaciones detenidas".into(),
                );
            }
            let mut prepared = traffic.services[i].session.clone_for_sections()?;
            if prepared.driver_throttle > 0. {
                return Err("La porción todavía tiene tracción aplicada".into());
            }
            for car in &mut prepared.formation.cars {
                car.handbrake = true;
            }
            prepared.sync_operating_brakes();
            receiver.can_attach_at(&prepared, end)?;
            let own = receiver.formation.coupled_count;
            let incoming = prepared.formation.coupled_count;
            let original = std::mem::replace(&mut traffic.services[i].session, prepared);
            if let Err(error) = traffic.couple_service_at(receiver, i, end) {
                traffic.services[i].session = original;
                return Err(error);
            }
            let boundary = if end == CouplingEnd::Rear {
                own - 1
            } else {
                incoming - 1
            };
            receiver.formation.cars[boundary].rear_cock_open = true;
            receiver.formation.cars[boundary + 1].front_cock_open = true;
            receiver.formation.cars[boundary + 1].hose_connected = true;
            receiver.formation.cars[boundary + 1].train_bus_connected = true;
            let range = if end == CouplingEnd::Rear {
                own..own + incoming
            } else {
                0..incoming
            };
            for car in &mut receiver.formation.cars[range] {
                car.handbrake = false;
            }
            receiver.sync_operating_brakes();
            receiver.rebuild_formation_physics();
            Ok(())
        })
    }
    pub(crate) fn tick_mobile_meetings(&mut self, player: &mut LiveDriveSession) {
        let definitions = player.state.yard_operations.definition.rendezvous.clone();
        for def in definitions {
            if let Some(leg) = def.receiver_leg {
                let current = if def.receiver == player.service_id {
                    Some(player.operational_leg)
                } else {
                    self.services
                        .iter()
                        .find(|s| s.id == def.receiver)
                        .map(|s| s.session.operational_leg)
                };
                if current != Some(leg) {
                    continue;
                }
            }
            let existing = player.state.yard_operations.rendezvous.get(&def.id);
            if existing.is_some_and(|m| {
                m.complete
                    || m.failed
                    || m.last_attempt_s.is_some_and(|t| player.time_s() - t < 0.5)
            }) || player.time_s() < def.not_before_s
            {
                continue;
            }
            let mut world = player.state.yard_operations.clone();
            let now = player.time_s();
            let progress = world.rendezvous.entry(def.id.clone()).or_default();
            let start = *progress.waiting_since_s.get_or_insert(now);
            progress.last_attempt_s = Some(now);
            if self.couplings.iter().any(|r| {
                r.donor == def.donor
                    && r.receiver.as_deref() == Some(&def.receiver)
                    && r.kind == crate::independent_coupling::TransferKind::Join
            }) {
                progress.complete = true;
                progress.notice = "Porción recibida con contacto físico".into();
                self.publish_yard_world(player, world);
                continue;
            }
            if now - start > def.max_wait_s {
                progress.failed = true;
                progress.notice = format!(
                    "Encuentro {}: venció la espera; no se creó material sustituto",
                    def.id
                );
                world.notice = progress.notice.clone();
                self.publish_yard_world(player, world);
                continue;
            }
            let present = std::iter::once(&*player)
                .chain(
                    self.services
                        .iter()
                        .filter(|s| s.departed)
                        .map(|s| &s.session),
                )
                .filter(|s| s.service_id == def.receiver || s.service_id == def.donor)
                .filter(|s| {
                    s.own_track_occupancy()
                        .iter()
                        .any(|p| p.edge == def.edge.trim_end_matches("_r"))
                })
                .count()
                == 2;
            if !present {
                progress.notice = "Esperando ambos servicios en la vía del encuentro".into();
                world.notice = progress.notice.clone();
                self.publish_yard_world(player, world);
                continue;
            }
            let end = if def.front {
                CouplingEnd::Front
            } else {
                CouplingEnd::Rear
            };
            let result = self.collect_meeting(player, &def.receiver, &def.donor, end);
            let progress = world.rendezvous.get_mut(&def.id).unwrap();
            match result {
                Ok(()) => {
                    progress.complete = true;
                    progress.notice = "Encuentro y transferencia completados".into();
                }
                Err(error) => progress.notice = error,
            }
            world.notice = progress.notice.clone();
            self.publish_yard_world(player, world);
        }
        // Native timetable operations also execute for AI receivers. Their
        // receipts keep the actual chronological receiver ID across save/load.
        let receivers = self
            .services
            .iter()
            .filter(|s| {
                s.departed
                    && !s.parked
                    && !self.remote_drivers.contains(&s.id)
                    && !s.session.state.yard_operations.human_driven
            })
            .filter_map(|s| {
                s.session
                    .operational_legs
                    .get(s.session.operational_leg)
                    .and_then(|l| l.operations.get(s.session.operational_operation))
                    .filter(|o| {
                        matches!(
                            o.action,
                            openrailsrs_scenarios::model::TimetableAction::JoinService { .. }
                                | openrailsrs_scenarios::model::TimetableAction::SplitService { .. }
                        )
                    })
                    .map(|_| s.id.clone())
            })
            .collect::<Vec<_>>();
        for receiver in receivers {
            let _ = self.with_yard_receiver(player, &receiver, |traffic, session| {
                traffic.tick_timetable_transfers(session, true);
                Ok(())
            });
        }
        // A native TT join may have consumed its donor above, after the
        // rendezvous polling pass. Commit the matching handshake in this same
        // coordinator tick, so a save immediately after contact is complete.
        let mut world = player.state.yard_operations.clone();
        let mut changed = false;
        for def in &world.definition.rendezvous {
            if self.couplings.iter().any(|r| {
                r.donor == def.donor
                    && r.receiver.as_deref() == Some(&def.receiver)
                    && r.kind == crate::independent_coupling::TransferKind::Join
            }) {
                let progress = world.rendezvous.entry(def.id.clone()).or_default();
                if !progress.complete && !progress.failed {
                    progress.complete = true;
                    progress.notice = "Porción recibida con contacto físico".into();
                    world.notice = progress.notice.clone();
                    changed = true;
                }
            }
        }
        if changed {
            self.publish_yard_world(player, world);
        }
    }
}
impl LiveDriveSession {
    pub(crate) fn meeting_head_target(&self) -> Option<f64> {
        let world = &self.state.yard_operations;
        for def in &world.definition.rendezvous {
            if world
                .rendezvous
                .get(&def.id)
                .is_some_and(|m| m.complete || m.failed)
                || self.time_s() < def.not_before_s
            {
                continue;
            }
            if def.receiver != self.service_id && def.donor != self.service_id {
                continue;
            }
            if def.receiver == self.service_id
                && def.receiver_leg.is_some_and(|l| l != self.operational_leg)
            {
                continue;
            }
            if def.donor == self.service_id && self.meeting_receiver_length(&def.receiver).is_none()
            {
                continue;
            }
            let mut before = 0.;
            for id in &self.state.path_edges {
                let e = self.graph.edge(id)?;
                if id.trim_end_matches("_r") == def.edge.trim_end_matches("_r") {
                    let target = if def.donor == self.service_id {
                        def.position_m
                            + if def.front {
                                self.formation.length_m()
                            } else {
                                -self.meeting_receiver_length(&def.receiver)?
                            }
                    } else {
                        def.position_m
                    };
                    return Some(
                        before
                            + if id.ends_with("_r") {
                                e.length_m - target
                            } else {
                                target
                            },
                    );
                }
                before += e.length_m;
            }
        }
        None
    }
    fn meeting_receiver_length(&self, id: &str) -> Option<f64> {
        // Only actual published stock can establish a coupling reference.
        let length: f64 = self
            .external_track_occupancy
            .iter()
            .filter(|p| p.owner == id)
            .map(|p| p.end_m - p.start_m)
            .sum();
        (length > 0.).then_some(length)
    }
    pub(crate) fn step_meeting_approach(&mut self, dt: f64) -> bool {
        let Some(target) = self.meeting_head_target() else {
            return false;
        };
        if matches!(
            self.gameplay.phase,
            crate::ServicePhase::Boarding | crate::ServicePhase::ReadyToDepart
        ) {
            self.step_autodrive(dt, 0.25, |_| {});
            return true;
        }
        let remaining = target - self.head_chainage_m();
        yard_precision_controls(self, target, 2.);
        // A bounded, physically compatible coupling approach replaces only the
        // conservative stop-12m-before-an-occupied-block driving assistance.
        let assistance = self.traffic_brake_assistance;
        if remaining <= self.formation.length_m() + 20. && remaining >= -0.05 {
            let peer = self
                .state
                .yard_operations
                .definition
                .rendezvous
                .iter()
                .find(|d| d.receiver == self.service_id || d.donor == self.service_id)
                .map(|d| {
                    if d.receiver == self.service_id {
                        d.donor.as_str()
                    } else {
                        d.receiver.as_str()
                    }
                });
            if peer.is_some_and(|id| self.portion_corridor_clear(id, target)) {
                self.traffic_brake_assistance = false;
            }
        }
        self.step_realtime(dt, |_| {});
        self.traffic_brake_assistance = assistance;
        true
    }
}

impl LiveDriveSession {
    /// An authorised meeting replaces the conservative block margin only for
    /// its declared peer. Every third train, work, deck or protection remains a
    /// blocking footprint; native red signals and TCS are still authoritative.
    pub(crate) fn portion_corridor_clear(&self, peer: &str, target: f64) -> bool {
        let mut prefix = 0.;
        let head = self.head_chainage_m();
        let limit = target + self.formation.length_m() + 1.;
        for (id, e) in self.state.path_edges.iter().zip(&self.path_data.edges) {
            let base = id.trim_end_matches("_r");
            if prefix + e.length_m >= head
                && prefix <= limit
                && self.edge_is_occupied(id, &self.external_occupancy)
                && !self.external_track_occupancy.iter().any(|p| p.edge == base)
            {
                return false;
            }
            for p in self
                .external_track_occupancy
                .iter()
                .chain(&self.external_track_reservations)
                .filter(|p| p.edge == base && p.owner != peer && p.owner != self.service_id)
            {
                let (a, b) = if id.ends_with("_r") {
                    (e.length_m - p.end_m, e.length_m - p.start_m)
                } else {
                    (p.start_m, p.end_m)
                };
                if prefix + b >= head - 1. && prefix + a <= limit {
                    return false;
                }
            }
            prefix += e.length_m;
            if prefix > limit {
                break;
            }
        }
        true
    }
}

/// Reusable physical crew controller. Only throttle/brake/reverser are changed;
/// neither the remaining distance nor a stopped pose is written into state.
pub(crate) fn yard_precision_controls(s: &mut LiveDriveSession, target: f64, max_speed: f64) {
    let distance = target - s.head_chainage_m();
    let speed = s.velocity_mps().abs();
    s.driver_direction = if s.state.path_reversed { 0. } else { 1. };
    if distance < -0.01 {
        s.driver_throttle = 0.;
        s.driver_brake = 1.;
        s.state.yard_operations.notice =
            "La tripulación pasó el punto de contacto: requiere una maniobra física de corrección"
                .into();
        return;
    }
    if distance.abs() < 0.008 && speed <= 0.005 {
        s.driver_throttle = 0.;
        s.driver_brake = 1.;
        return;
    }
    let cap = (2. * 0.08 * (distance - 0.001).max(0.))
        .sqrt()
        .min(max_speed);
    let band = (cap * 0.1).min(0.002);
    if speed > cap + band {
        s.driver_throttle = 0.;
        s.driver_brake = if cap < 0.01 { 0.6 } else { 0.25 };
    } else if speed < cap - band {
        s.driver_throttle = if distance < 0.5 {
            0.012
        } else if distance < 5. {
            0.04
        } else {
            0.18
        };
        s.driver_brake = 0.;
    } else {
        s.driver_throttle = 0.;
        s.driver_brake = 0.;
    }
}
impl LiveDriveSession {
    pub(crate) fn strict_portion_contact(&self, other: &Self, end: CouplingEnd) -> bool {
        if self.velocity_mps().abs() > 0.005 || other.velocity_mps().abs() > 0.005 {
            return false;
        }
        let normalize = |s: &Self, p: Option<(String, f64)>| {
            p.and_then(|(id, p)| {
                s.graph.edge(&id).map(|e| {
                    (
                        id.trim_end_matches("_r").to_string(),
                        if id.ends_with("_r") {
                            e.length_m - p
                        } else {
                            p
                        },
                    )
                })
            })
        };
        let own = normalize(
            self,
            self.position_at_head_offset(if end == CouplingEnd::Rear {
                -self.formation.length_m()
            } else {
                0.
            }),
        );
        let donor = normalize(
            other,
            other.position_at_head_offset(if end == CouplingEnd::Front {
                -other.formation.length_m()
            } else {
                0.
            }),
        );
        own.zip(donor)
            .is_some_and(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() <= 0.008)
    }
}
