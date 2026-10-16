//! Native detach/static and pickup operations reuse the physical F9 machinery.
use crate::LiveDriveSession;
use openrailsrs_scenarios::model::TimetableAction;

impl LiveDriveSession {
    pub(crate) fn tick_timetable_operation(&mut self, automatic: bool) {
        let Some(operation) = self
            .operational_legs
            .get(self.operational_leg)
            .and_then(|leg| leg.operations.get(self.operational_operation))
            .cloned()
        else {
            return;
        };
        let section = self
            .formation
            .sections
            .iter()
            .find(|s| s.id == operation.section);
        let operation_head = if matches!(operation.action, TimetableAction::Detach { .. }) {
            section.map_or(self.head_chainage_m(), |s| {
                s.anchor_m
                    + if s.reversed {
                        self.formation.cars[s.start..s.start + s.count]
                            .iter()
                            .map(|c| c.length_m)
                            .sum::<f64>()
                    } else {
                        self.formation.length_m()
                    }
            })
        } else if let TimetableAction::PickUp { count } = operation.action {
            self.runaround_pickup_reference(&operation.section, count)
        } else {
            self.head_chainage_m()
        };
        if self.velocity_mps().abs() > 0.1 || (operation_head - operation.chainage_m).abs() > 20.0 {
            return;
        }
        let automatic =
            (automatic || !operation.manual) && !self.state.yard_operations.human_driven;
        let finished = match operation.action {
            TimetableAction::Detach { keep } => {
                if automatic
                    && !self
                        .formation
                        .sections
                        .iter()
                        .any(|s| s.id == operation.section)
                    && keep > 0
                    && keep < self.formation.coupled_count
                {
                    for car in &mut self.formation.cars[keep..self.formation.coupled_count] {
                        car.handbrake = true;
                    }
                    if self
                        .uncouple_after_named(keep - 1, &operation.section)
                        .is_err()
                    {
                        return;
                    }
                }
                self.formation
                    .sections
                    .iter()
                    .find(|s| s.id == operation.section)
                    .is_some_and(|s| {
                        self.formation.coupled_count == keep
                            && s.start == keep
                            && (self.head_chainage_m()
                                - (s.anchor_m
                                    + if s.reversed {
                                        0.
                                    } else {
                                        self.formation.length_m()
                                    }))
                            .abs()
                                <= 1.
                    })
            }
            TimetableAction::JoinService { .. } | TimetableAction::SplitService { .. } => false,
            TimetableAction::PickUp { count } => {
                if automatic
                    && self
                        .formation
                        .sections
                        .iter()
                        .any(|s| s.id == operation.section)
                {
                    if self.yard_front_pickup(&operation.section)
                        && (self.velocity_mps().abs() > 0.005
                            || (self.head_chainage_m() - operation.chainage_m).abs() > 0.008)
                    {
                        return;
                    }
                    let end = if self.yard_front_pickup(&operation.section) {
                        crate::sections::CouplingEnd::Front
                    } else {
                        crate::sections::CouplingEnd::Rear
                    };
                    if self.recouple_section(&operation.section, end).is_err() {
                        return;
                    }
                    for car in &mut self.formation.cars[..self.formation.coupled_count] {
                        car.handbrake = false;
                        car.hose_connected = true;
                        car.front_cock_open = true;
                        car.rear_cock_open = true;
                        car.train_bus_connected = true;
                    }
                    self.sync_operating_brakes();
                }
                !self
                    .formation
                    .sections
                    .iter()
                    .any(|s| s.id == operation.section)
                    && self.formation.coupled_count == count
                    && self.formation.cars[..count].iter().all(|c| {
                        !c.handbrake && c.hose_connected && c.front_cock_open && c.rear_cock_open
                    })
            }
        };
        if finished {
            self.operational_operation += 1;
        }
    }
}

impl crate::LiveTraffic {
    pub(crate) fn tick_timetable_transfers(
        &mut self,
        player: &mut LiveDriveSession,
        automatic: bool,
    ) {
        use crate::sections::CouplingEnd;
        let Some(operation) = player
            .operational_legs
            .get(player.operational_leg)
            .and_then(|l| l.operations.get(player.operational_operation))
            .cloned()
        else {
            return;
        };
        if player.velocity_mps().abs() > 0.1
            || (player.head_chainage_m() - operation.chainage_m).abs() > 20.
            || operation.manual && !automatic
        {
            return;
        }
        let result = match operation.action {
            TimetableAction::JoinService { service, front } => {
                if player.state.yard_operations.human_driven
                    || self.remote_drivers.contains(&player.service_id)
                    || self.remote_drivers.contains(&service)
                {
                    player.state.rail_operations.notice =
                        "Transferencia pendiente: la formación tiene un conductor humano de red"
                            .into();
                    return;
                }
                if player
                    .state
                    .yard_operations
                    .definition
                    .rendezvous
                    .iter()
                    .any(|r| {
                        r.receiver == player.service_id
                            && r.donor.eq_ignore_ascii_case(&service)
                            && r.receiver_leg
                                .is_none_or(|leg| leg == player.operational_leg)
                            && (player.time_s() < r.not_before_s
                                || player
                                    .state
                                    .yard_operations
                                    .rendezvous
                                    .get(&r.id)
                                    .is_some_and(|m| m.failed))
                    })
                {
                    player.state.rail_operations.notice =
                        "El encuentro no está autorizado o venció; no se recoge su porción".into();
                    return;
                }
                let Some(index) = self
                    .services
                    .iter()
                    .position(|s| s.id.eq_ignore_ascii_case(&service))
                else {
                    player.state.rail_operations.notice =
                        format!("Esperando la porción del servicio {service}");
                    return;
                };
                if !self.services[index].departed && !self.services[index].parked {
                    player.state.rail_operations.notice =
                        format!("Esperando la llegada del servicio {service}");
                    return;
                }
                let ends = match front {
                    Some(true) => vec![CouplingEnd::Front],
                    Some(false) => vec![CouplingEnd::Rear],
                    None => vec![CouplingEnd::Rear, CouplingEnd::Front],
                };
                let mut last = Err("No hay contacto físico entre los servicios".into());
                for end in ends {
                    let strict = player
                        .state
                        .yard_operations
                        .definition
                        .rendezvous
                        .iter()
                        .any(|r| {
                            r.receiver == player.service_id
                                && r.donor.eq_ignore_ascii_case(&service)
                                && r.receiver_leg
                                    .is_none_or(|leg| leg == player.operational_leg)
                        });
                    if strict && !player.strict_portion_contact(&self.services[index].session, end)
                    {
                        last=Err("Esperando contacto de la porción a menos de 8 mm y ambas formaciones detenidas".into());
                        continue;
                    }
                    let mut donor = match self.services[index].session.clone_for_sections() {
                        Ok(d) => d,
                        Err(e) => {
                            last = Err(e);
                            break;
                        }
                    };
                    if donor.velocity_mps().abs() > 0.1 || donor.driver_throttle != 0. {
                        last = Err("Esperando a que el servicio a recoger se detenga".into());
                        break;
                    }
                    for car in &mut donor.formation.cars {
                        car.handbrake = true;
                    }
                    donor.sync_operating_brakes();
                    if let Err(error) = player.can_attach_at(&donor, end) {
                        last = Err(error);
                        continue;
                    }
                    let own_count = player.formation.coupled_count;
                    let donor_count = donor.formation.coupled_count;
                    let original = std::mem::replace(&mut self.services[index].session, donor);
                    last = self.couple_service_at(player, index, end);
                    if last.is_err() {
                        self.services[index].session = original;
                    }
                    if last.is_ok() {
                        let boundary = if end == CouplingEnd::Rear {
                            own_count - 1
                        } else {
                            donor_count - 1
                        };
                        player.formation.cars[boundary].rear_cock_open = true;
                        player.formation.cars[boundary + 1].front_cock_open = true;
                        player.formation.cars[boundary + 1].hose_connected = true;
                        player.formation.cars[boundary + 1].train_bus_connected = true;
                        let donor_range = if end == CouplingEnd::Rear {
                            own_count..own_count + donor_count
                        } else {
                            0..donor_count
                        };
                        for car in &mut player.formation.cars[donor_range] {
                            car.handbrake = false;
                        }
                        player.sync_operating_brakes();
                    }
                    break;
                }
                last
            }
            TimetableAction::SplitService { service } => {
                if player.state.yard_operations.human_driven
                    || self.remote_drivers.contains(&player.service_id)
                    || self.remote_drivers.contains(&service)
                {
                    player.state.rail_operations.notice =
                        "División pendiente: la formación tiene un conductor humano de red".into();
                    return;
                }
                // The declared operation includes the crew securing its portion.
                // Prepare that state separately so a failed split cannot alter
                // the player's handbrakes or lose either physical train.
                let mut prepared = match player.clone_for_sections() {
                    Ok(s) => s,
                    Err(error) => {
                        player.state.rail_operations.notice = error;
                        return;
                    }
                };
                let source = self.couplings.iter().rev().find(|r| {
                    r.donor == service && r.kind == crate::independent_coupling::TransferKind::Join
                });
                if let Some(source) = source {
                    let ids = if source.donor_before.formation.vehicle_ids.is_empty() {
                        (0..source.donor_before.formation.cars.len())
                            .map(|i| format!("{service}/car/{i}"))
                            .collect::<Vec<_>>()
                    } else {
                        source.donor_before.formation.vehicle_ids.clone()
                    };
                    let inventory = prepared.vehicle_identities();
                    for (i, id) in inventory.iter().enumerate() {
                        if ids.contains(id) {
                            prepared.formation.cars[i].handbrake = true;
                        }
                    }
                    prepared.sync_operating_brakes();
                }
                match self.split_service(&mut prepared, &service) {
                    Ok(()) => {
                        *player = prepared;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            _ => return,
        };
        if let Err(error) = result {
            player.state.rail_operations.notice = error;
        }
    }
}
impl LiveDriveSession {
    pub(crate) fn complete_service_operation(
        &mut self,
        id: &str,
        joining: bool,
        operation_head: f64,
    ) {
        let Some(operation) = self
            .operational_legs
            .get(self.operational_leg)
            .and_then(|l| l.operations.get(self.operational_operation))
        else {
            return;
        };
        if (operation_head - operation.chainage_m).abs() > 20. {
            return;
        }
        let matches = match &operation.action {
            TimetableAction::JoinService { service, .. } => {
                joining && service.eq_ignore_ascii_case(id)
            }
            TimetableAction::SplitService { service } => {
                !joining && service.eq_ignore_ascii_case(id)
            }
            _ => false,
        };
        if matches {
            self.operational_operation += 1;
        }
    }
}
