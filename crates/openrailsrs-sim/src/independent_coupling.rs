//! Manual, stationary rear-to-front coupling. Original service snapshots are
//! retained as reconstruction receipts, never as a second physical train.
use crate::path_data::PathData;
use crate::sections::CouplingEnd;
use crate::{LiveDriveSession, LiveTraffic, SessionSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouplingReceipt {
    /// The physical service receiving/executing this chronological operation.
    /// None identifies legacy receipts whose control swaps were stored apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    pub donor: String,
    pub before: SessionSnapshot,
    pub donor_before: SessionSnapshot,
    #[serde(default)]
    pub end: CouplingEnd,
    #[serde(default)]
    pub kind: TransferKind,
    #[serde(default)]
    pub departure_s: f64,
    #[serde(default)]
    pub departed: bool,
    #[serde(default)]
    pub parked: bool,
    #[serde(default)]
    pub donor_legs: Vec<openrailsrs_scenarios::model::RouteLeg>,
    #[serde(default)]
    pub donor_clock_start_s: f64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    #[default]
    Join,
    Split,
}
fn normalize(session: &LiveDriveSession, at: (String, f64)) -> Option<(String, f64)> {
    let edge = session.graph.edge(&at.0)?;
    let reverse = at.0.ends_with("_r");
    Some((
        at.0.strip_suffix("_r").unwrap_or(&at.0).into(),
        if reverse { edge.length_m - at.1 } else { at.1 },
    ))
}
fn same_position(
    a: &LiveDriveSession,
    left: Option<(String, f64)>,
    b: &LiveDriveSession,
    right: Option<(String, f64)>,
) -> bool {
    left.and_then(|p| normalize(a, p))
        .zip(right.and_then(|p| normalize(b, p)))
        .is_some_and(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() <= 1.)
}
impl LiveDriveSession {
    pub(crate) fn can_attach_at(&self, donor: &Self, end: CouplingEnd) -> Result<f64, String> {
        let n = self.formation.coupled_count;
        let m = donor.formation.coupled_count;
        if n == 0
            || m == 0
            || n + m > 256
            || n != self.formation.cars.len()
            || m != donor.formation.cars.len()
        {
            return Err("Recogé las secciones propias antes de unir formaciones independientes (máximo 256 vehículos)".into());
        }
        if self.velocity_mps().abs() > 0.1
            || donor.velocity_mps().abs() > 0.1
            || self.driver_throttle != 0.
            || donor.driver_throttle != 0.
        {
            return Err("Detené ambas formaciones y quitá la tracción antes del contacto".into());
        }
        if !donor.formation.cars.iter().all(|c| c.handbrake) {
            return Err(
                "Asegurá con freno de mano todos los vehículos de la formación a recoger".into(),
            );
        }
        if self.script_tcs.is_some()
            || donor.script_tcs.is_some()
            || self.physics.native.is_some() != donor.physics.native.is_some()
            || self.physics.rail_adhesion.is_some() != donor.physics.rail_adhesion.is_some()
            || self.state.vehicles.is_empty() != donor.state.vehicles.is_empty()
        {
            return Err("El acople requiere TCS Rust y modos de dinámica compatibles".into());
        }
        if donor.dispatcher.own_locks.iter().any(|lock| {
            self.graph.switch_position(&lock.node) != Some(lock.position)
                || self
                    .dispatcher
                    .own_locks
                    .iter()
                    .any(|own| own.node == lock.node && own.position != lock.position)
        }) {
            return Err(
                "Las reservas de ambas formaciones exigen posiciones incompatibles del cambio"
                    .into(),
            );
        }
        let own_i = if end == CouplingEnd::Rear { n - 1 } else { 0 };
        let donor_i = if end == CouplingEnd::Rear { 0 } else { m - 1 };
        if !self.state.brake_system.cylinders[own_i]
            .connection_compatible(&donor.state.brake_system.cylinders[donor_i])
        {
            return Err("Los equipos de freno de ambos extremos no son compatibles".into());
        }
        let a = self.formation.length_m();
        let b = donor.formation.length_m();
        let full = a + b;
        let anchor = self.head_chainage_m()
            + match (end, self.state.path_reversed) {
                (CouplingEnd::Rear, true) | (CouplingEnd::Front, false) => b,
                _ => 0.,
            };
        for i in 0..m {
            let offset = donor.formation.center_offset_m(i);
            let combined = offset - if end == CouplingEnd::Rear { a } else { 0. };
            let next = PathData::position_at_odometer(
                &self.state.path_edges,
                &self.path_data.edges,
                anchor
                    + if self.state.path_reversed {
                        -full - combined
                    } else {
                        combined
                    },
            );
            let before = donor.presentation_position_for_car(offset, offset, 0.);
            if !same_position(self, next, donor, before) {
                return Err("Acercá los enganches a menos de 1 m: el itinerario debe conservar la posición de todos los vehículos a recoger".into());
            }
        }
        Ok(anchor)
    }
    fn attach(&mut self, mut donor: Self) {
        // A parked donor may never have advanced a simulation tick. Initialize
        // its physical supply at the current state before merging configurations,
        // so an immediate save remains valid even while the game is paused.
        for train in [&mut *self, &mut donor] {
            if !train.state.power_supply.initialized {
                crate::power_supply::advance(
                    &mut train.state,
                    &train.original_physics.power_supply,
                );
            }
        }
        self.normalize_boiler_inventory();
        donor.normalize_boiler_inventory();
        self.remember_native_vehicle_states();
        donor.remember_native_vehicle_states();
        let mut ids = self.vehicle_ids();
        ids.extend(donor.vehicle_ids());
        let n = self.consist.vehicles.len();
        let length = self.formation.length_m();
        let p = &mut self.original_physics;
        let q = donor.original_physics;
        p.bogies.extend(q.bogies);
        p.mass_kg += q.mass_kg;
        p.max_power_w += q.max_power_w;
        p.max_tractive_effort_n += q.max_tractive_effort_n;
        p.max_brake_n += q.max_brake_n;
        p.davis = p.davis.sum(&q.davis);
        p.vehicle_davis.extend(q.vehicle_davis);
        p.vehicle_lengths_m.extend(q.vehicle_lengths_m);
        p.diesel_vehicle_indices
            .extend(q.diesel_vehicle_indices.into_iter().map(|i| i + n));
        p.diesel_engines.extend(q.diesel_engines);
        for mut c in q.diesel.cars {
            c.vehicle += n;
            p.diesel.cars.push(c);
        }
        for mut c in q.electric.cars {
            c.vehicle += n;
            c.offset_m -= length;
            p.electric.cars.push(c);
        }
        p.electric.fallback_cars.extend(
            q.electric
                .fallback_cars
                .into_iter()
                .map(|(i, c)| (i + n, c)),
        );
        p.electric.length_m += q.electric.length_m;
        for mut c in q.power_supply.cars {
            c.vehicle += n;
            p.power_supply.cars.push(c);
        }
        p.power_supply
            .passenger_loads
            .extend(q.power_supply.passenger_loads);
        p.power_supply.loads.auxiliary_w += q.power_supply.loads.auxiliary_w;
        p.power_supply.loads.continuous_w += q.power_supply.loads.continuous_w;
        p.power_supply.loads.heating_w += q.power_supply.loads.heating_w;
        p.power_supply.loads.cooling_w += q.power_supply.loads.cooling_w;
        p.power_supply
            .bus_connected
            .extend(q.power_supply.bus_connected);
        if let (Some(a), Some(b)) = (&mut p.native, q.native) {
            a.vehicles.extend(b.vehicles);
            a.vehicle_masses_kg.extend(b.vehicle_masses_kg);
        }
        if let (Some(a), Some(b)) = (&mut p.rail_adhesion, q.rail_adhesion) {
            a.vehicles.extend(b.vehicles);
        }
        self.consist.vehicles.append(&mut donor.consist.vehicles);
        self.consist.davis = self.consist.davis.sum(&donor.consist.davis);
        self.curve_parameters.append(&mut donor.curve_parameters);
        self.intake_points.append(&mut donor.intake_points);
        for mut c in donor.original_vehicle_systems {
            c.vehicle += n;
            self.original_vehicle_systems.push(c);
        }
        for mut c in donor.original_cab_equipment {
            c.vehicle += n;
            self.original_cab_equipment.push(c);
        }
        self.formation.cars.append(&mut donor.formation.cars);
        self.formation.coupled_count = self.formation.cars.len();
        self.formation.vehicle_ids = ids;
        self.formation
            .native_bearing_c
            .extend(donor.formation.native_bearing_c.clone());
        self.formation
            .native_axles
            .extend(donor.formation.native_axles.clone());
        let a = &mut self.state;
        let b = &mut donor.state;
        for mut car in b.freight.drain(..) {
            car.vehicle += n;
            a.freight.push(car);
        }
        a.bogies.append(&mut b.bogies);
        for mut boiler in b.steam_boilers.drain(..) {
            boiler.vehicle += n;
            a.steam_boilers.push(boiler);
        }
        for c in &mut b.brake_system.cylinders {
            c.position_m += length;
        }
        a.brake_system
            .cylinders
            .append(&mut b.brake_system.cylinders);
        for mut circuit in b.brake_system.circuits.drain(..) {
            circuit.vehicle += n;
            a.brake_system.circuits.push(circuit);
        }
        if !a.vehicles.is_empty() {
            let mut joint = crate::CouplerState::passenger();
            if let Some(original) = a
                .couplers
                .last()
                .or(b.couplers.first())
                .filter(|joint| joint.native_profile.is_none())
            {
                joint = original.clone();
                joint.extension_m = 0.;
                joint.broken = false;
                joint.memory = Default::default();
            }
            if let (Some(leading), Some(trailing)) = (
                self.authored_couplers.last(),
                donor.authored_couplers.first(),
            ) && let Some(authored) = crate::CouplerState::from_vehicle_ends(leading, trailing)
            {
                joint = authored;
            }
            self.original_couplers.push(joint.clone());
            self.original_couplers.append(&mut donor.original_couplers);
            a.couplers.push(joint);
            a.couplers.append(&mut b.couplers);
            // Translate the donor's travel origin without flattening its
            // internal deflections or invalidating persistent gear references.
            let target = a.vehicles.last().map_or(0., |car| car.position_m);
            let shift = target - b.vehicles.first().map_or(0., |car| car.position_m);
            for car in &mut b.vehicles {
                car.position_m += shift;
            }
            a.vehicles.append(&mut b.vehicles);
            a.vehicle_masses.append(&mut b.vehicle_masses);
        }
        self.authored_couplers.append(&mut donor.authored_couplers);
        a.diesel_rpm.append(&mut b.diesel_rpm);
        a.diesel_run_up.append(&mut b.diesel_run_up);
        a.diesel_motor_heat.append(&mut b.diesel_motor_heat);
        a.diesel_traction_force_n
            .append(&mut b.diesel_traction_force_n);
        a.diesel_average_force_n
            .append(&mut b.diesel_average_force_n);
        a.diesel_apparent_throttle
            .append(&mut b.diesel_apparent_throttle);
        a.motor_force_n.append(&mut b.motor_force_n);
        a.rail_force_n.append(&mut b.rail_force_n);
        for mut c in b.diesel.cars.drain(..) {
            c.vehicle += n;
            a.diesel.cars.push(c);
        }
        for mut c in b.electric.cars.drain(..) {
            c.vehicle += n;
            a.electric.cars.push(c);
        }
        for mut c in b.power_supply.cars.drain(..) {
            c.vehicle += n;
            a.power_supply.cars.push(c);
        }
        a.power_supply.energy_j += b.power_supply.energy_j;
        a.power_supply.auxiliary_w += b.power_supply.auxiliary_w;
        a.power_supply.hotel_w += b.power_supply.hotel_w;
        a.power_supply.bus_source.clear();
        a.power_supply.source_hotel_w.clear();
        for mut c in b.vehicle_systems.drain(..) {
            c.definition.vehicle += n;
            a.vehicle_systems.push(c);
        }
        for mut c in b.cab_equipment.drain(..) {
            c.config.vehicle += n;
            a.cab_equipment.push(c);
        }
        a.articulated_joints
            .extend(b.articulated_joints.iter().map(|p| [p[0] + n, p[1] + n]));
        if let (Some(a), Some(b)) = (&mut a.native_dynamics, &mut b.native_dynamics) {
            a.bearing_c.append(&mut b.bearing_c);
            a.axles.append(&mut b.axles);
        }
        if let (Some(a), Some(b)) = (&mut a.rail_adhesion, &mut b.rail_adhesion) {
            a.cars.append(&mut b.cars);
        }
        a.passengers = a.passengers.saturating_add(b.passengers);
        a.extra_mass_kg += b.extra_mass_kg;
        a.cumulative_energy_j += b.cumulative_energy_j;
        a.regen_energy_j += b.regen_energy_j;
        a.fuel_consumption_g += b.fuel_consumption_g;
        self.content_signature = format!("{}+{}", self.content_signature, donor.content_signature);
        for mut grant in donor.own_track_reservations {
            grant.owner = self.service_id.clone();
            self.own_track_reservations.push(grant);
        }
        self.own_track_reservations
            .sort_by(|a, b| a.edge.cmp(&b.edge).then(a.start_m.total_cmp(&b.start_m)));
        let mut grants: Vec<crate::native_signals::TrackOccupancy> = Vec::new();
        for grant in self.own_track_reservations.drain(..) {
            if let Some(last) = grants.last_mut()
                && last.edge == grant.edge
                && grant.start_m <= last.end_m + 1e-8
            {
                last.end_m = last.end_m.max(grant.end_m);
            } else {
                grants.push(grant);
            }
        }
        self.own_track_reservations = grants;
        for mut lock in donor.dispatcher.own_locks {
            lock.owner = self.service_id.clone();
            if !self
                .dispatcher
                .own_locks
                .iter()
                .any(|l| l.node == lock.node)
            {
                self.dispatcher.own_locks.push(lock);
            }
        }
        self.refilling = None;
        self.align_native_vehicle_states();
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
    }
}
impl LiveTraffic {
    pub fn validate_coupling(&self, player: &LiveDriveSession, index: usize) -> Result<(), String> {
        self.validate_coupling_at(player, index, CouplingEnd::Rear)
    }
    pub fn validate_coupling_at(
        &self,
        player: &LiveDriveSession,
        index: usize,
        end: CouplingEnd,
    ) -> Result<(), String> {
        if self.couplings.len() >= 64 {
            return Err("Máximo 64 transferencias de formaciones por partida".into());
        }
        let donor = self.services.get(index).ok_or("Formación desconocida")?;
        if !donor.departed && !donor.parked {
            // At its actual departure a stopped actor can be collected before
            // the first scheduler quantum. Future actors remain unavailable;
            // an occupied spawn cannot become physical just by clicking F7.
            let due = player.time_s() + 1e-9 >= donor.departure_s;
            let stock = donor.session.own_track_occupancy();
            let blocked = self.services.iter().enumerate().any(|(i, other)| {
                i != index
                    && other.departed
                    && other.session.own_track_occupancy().iter().any(|occupied| {
                        stock
                            .iter()
                            .any(|s| crate::track_reservations::overlaps(s, occupied))
                    })
            });
            if !due || blocked {
                return Err("El servicio todavía no está presente en una vía libre".into());
            }
        }
        player.can_attach_at(&donor.session, end).map(|_| ())
    }
    pub fn couple_service(
        &mut self,
        player: &mut LiveDriveSession,
        index: usize,
    ) -> Result<(), String> {
        self.couple_service_at(player, index, CouplingEnd::Rear)
    }
    pub fn couple_service_at(
        &mut self,
        player: &mut LiveDriveSession,
        index: usize,
        end: CouplingEnd,
    ) -> Result<(), String> {
        self.validate_coupling_at(player, index, end)?;
        let donor = &self.services[index];
        let anchor = player.can_attach_at(&donor.session, end)?;
        let n = player.formation.coupled_count;
        let m = donor.session.formation.coupled_count;
        let receipt = CouplingReceipt {
            receiver: Some(player.service_id.clone()),
            donor: donor.id.clone(),
            before: player.snapshot(),
            donor_before: donor.session.snapshot(),
            end,
            kind: TransferKind::Join,
            departure_s: donor.departure_s,
            departed: donor.departed || donor.parked || player.time_s() + 1e-9 >= donor.departure_s,
            parked: donor.parked,
            donor_legs: donor.session.operational_legs.clone(),
            donor_clock_start_s: donor.session.service_clock_start_s,
        };
        let mut combined = player.clone_for_sections()?;
        let mut selected = donor.session.clone_for_sections()?;
        // Public TrafficService construction predates persistent car identities:
        // its session can still carry the default player label. Assign source
        // identities from the actual service, without relabeling already-owned
        // cars after a prior transfer. The original receipt remains replayable.
        if selected.formation.vehicle_ids.is_empty() {
            let previous = selected.vehicle_ids();
            let ids: Vec<_> = (0..m).map(|i| format!("{}/car/{i}", donor.id)).collect();
            let rename = |id: &String| {
                previous
                    .iter()
                    .position(|p| p == id)
                    .map(|i| ids[i].clone())
            };
            selected.formation.native_bearing_c = selected
                .formation
                .native_bearing_c
                .drain()
                .filter_map(|(id, value)| rename(&id).map(|id| (id, value)))
                .collect();
            selected.formation.native_axles = selected
                .formation
                .native_axles
                .drain()
                .filter_map(|(id, value)| rename(&id).map(|id| (id, value)))
                .collect();
            selected.formation.vehicle_ids = ids;
        }
        combined.attach(selected);
        if end == CouplingEnd::Front {
            let order: Vec<_> = (n..n + m).chain(0..n).collect();
            combined.permute_catalog(&order, n + m)?;
        }
        let boundary = if end == CouplingEnd::Rear {
            n - 1
        } else {
            m - 1
        };
        combined.formation.cars[boundary].rear_cock_open = false;
        combined.formation.cars[boundary + 1].front_cock_open = false;
        combined.formation.cars[boundary + 1].hose_connected = false;
        combined.formation.cars[boundary + 1].train_bus_connected = false;
        let odometer = combined.state.odometer_m;
        crate::live_drive::apply_start_offset(&mut combined.state, &combined.path_data, anchor);
        combined.state.odometer_m = odometer;
        combined.previous_render_chainage_m = anchor;
        combined.sync_operating_brakes();
        combined.validate_snapshot(&combined.snapshot())?;
        combined.complete_service_operation(&receipt.donor, true, player.head_chainage_m());
        *player = combined;
        self.services.remove(index);
        self.couplings.push(receipt);
        self.synchronize_occupancy(player);
        Ok(())
    }
    /// Reconstruct identities in operation order, then select the saved driver.
    /// Legacy files keep their original all-swaps-before-couplings convention.
    pub fn reconstruct_service_history(
        &mut self,
        player: &mut LiveDriveSession,
        controls: &[String],
        controlled: &str,
        receipts: &[CouplingReceipt],
    ) -> Result<(), String> {
        if controls.len() > 128 {
            return Err("Historial de conductores demasiado largo".into());
        }
        let chronological = receipts.iter().any(|r| r.receiver.is_some());
        if !chronological {
            for id in controls {
                let index = self
                    .services
                    .iter()
                    .position(|s| &s.id == id)
                    .ok_or("El servicio controlado ya no existe")?;
                self.restore_control(player, index)?;
            }
        }
        self.reconstruct_couplings(player, receipts)?;
        if player.service_id != controlled {
            let index = self
                .services
                .iter()
                .position(|s| s.id == controlled)
                .ok_or("El conductor final no tiene un servicio físico")?;
            self.restore_control(player, index)?;
        }
        self.control_history = controls.to_vec();
        Ok(())
    }

    pub fn reconstruct_couplings(
        &mut self,
        player: &mut LiveDriveSession,
        receipts: &[CouplingReceipt],
    ) -> Result<(), String> {
        if receipts.len() > 64 || !self.couplings.is_empty() {
            return Err("Historial de acoples inválido".into());
        }
        for receipt in receipts {
            let receiver = receipt.receiver.as_deref();
            if receiver.is_some_and(|id| {
                id.is_empty() || id.len() > 256 || id.chars().any(char::is_control)
            }) {
                return Err("Receptor del historial de porciones inválido".into());
            }
            if let Some(id) = receiver {
                if player.service_id != id {
                    let index = self
                        .services
                        .iter()
                        .position(|s| s.id == id)
                        .ok_or("El receptor del acople no existe en ese punto del historial")?;
                    self.restore_control(player, index)?;
                }
            } else if player.content_signature != receipt.before.content_signature {
                // A legacy prefix can precede new chronological receipts after
                // loading an older save. Its original content signature must
                // identify exactly one actor; guessing a similarly named train
                // would duplicate or replace physical stock.
                let indices: Vec<_> = self
                    .services
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| {
                        (s.session.content_signature == receipt.before.content_signature)
                            .then_some(i)
                    })
                    .collect();
                if indices.len() != 1 {
                    return Err("El receptor del historial antiguo es ambiguo o falta".into());
                }
                self.restore_control(player, indices[0])?;
            }
            if receipt.kind == TransferKind::Split {
                player.validate_snapshot(&receipt.before)?;
                player.restore_snapshot(receipt.before.clone())?;
                self.split_service(player, &receipt.donor)?;
                continue;
            }
            let index = self
                .services
                .iter()
                .position(|s| s.id == receipt.donor)
                .ok_or("Formación de acople no disponible")?;
            player.validate_snapshot(&receipt.before)?;
            self.services[index]
                .session
                .validate_snapshot(&receipt.donor_before)?;
            player.restore_snapshot(receipt.before.clone())?;
            self.services[index]
                .session
                .restore_snapshot(receipt.donor_before.clone())?;
            self.couple_service_at(player, index, receipt.end)?;
        }
        Ok(())
    }
}

impl LiveTraffic {
    /// Return a complete joined foreign portion to its original service. Only
    /// an end portion can be removed: closing a gap between two remaining
    /// groups would relocate vehicles and is deliberately rejected.
    pub fn split_service(
        &mut self,
        player: &mut LiveDriveSession,
        service_id: &str,
    ) -> Result<(), String> {
        if self.couplings.len() >= 64 || self.services.iter().any(|s| s.id == service_id) {
            return Err(
                "El servicio ya está activo o se alcanzó el límite de transferencias".into(),
            );
        }
        if player.velocity_mps().abs() > 0.1
            || player.driver_throttle != 0.
            || player.formation.coupled_count != player.formation.cars.len()
        {
            return Err(
                "Detené la formación completa y quitá la tracción antes de dividir servicios"
                    .into(),
            );
        }
        let source = self
            .couplings
            .iter()
            .rev()
            .find(|r| r.donor == service_id && r.kind == TransferKind::Join)
            .cloned()
            .ok_or("No existe una porción de ese servicio en la formación")?;
        let foreign_ids = if source.donor_before.formation.vehicle_ids.is_empty() {
            (0..source.donor_before.formation.cars.len())
                .map(|i| format!("{service_id}/car/{i}"))
                .collect::<Vec<_>>()
        } else {
            source.donor_before.formation.vehicle_ids.clone()
        };
        let ids = player.vehicle_ids();
        let indices: Vec<_> = foreign_ids
            .iter()
            .map(|id| {
                ids.iter()
                    .position(|v| v == id)
                    .ok_or("Falta un vehículo de la porción ajena")
            })
            .collect::<Result<_, _>>()?;
        if indices.is_empty()
            || indices.len() == ids.len()
            || indices.windows(2).any(|p| p[1] != p[0] + 1)
            || !(indices[0] == 0 || indices.last() == Some(&(ids.len() - 1)))
        {
            return Err(
                "La porción ajena debe estar completa y en un extremo de la formación".into(),
            );
        }
        if indices.iter().any(|&i| !player.formation.cars[i].handbrake) {
            return Err(
                "Asegurá con freno de mano todos los vehículos del servicio a separar".into(),
            );
        }
        if indices.contains(&player.cab_vehicle()) {
            return Err("Cambiá a una cabina de la porción que vas a conservar".into());
        }
        let keep: Vec<_> = (0..ids.len()).filter(|i| !indices.contains(i)).collect();
        let selected_length: f64 = indices
            .iter()
            .map(|&i| player.formation.cars[i].length_m)
            .sum();
        let before = player.snapshot();
        let mut foreign = player.clone_for_sections()?;
        foreign.normalize_boiler_inventory();
        let mut retained = player.clone_for_sections()?;
        retained.normalize_boiler_inventory();
        let fraction = indices
            .iter()
            .map(|&i| player.formation.cars[i].mass_kg)
            .sum::<f64>()
            / player.formation.cars.iter().map(|c| c.mass_kg).sum::<f64>();
        foreign.project_portion(&indices)?;
        retained.project_portion(&keep)?;
        for (a, b, original) in [
            (
                &mut foreign.state.cumulative_energy_j,
                &mut retained.state.cumulative_energy_j,
                player.state.cumulative_energy_j,
            ),
            (
                &mut foreign.state.regen_energy_j,
                &mut retained.state.regen_energy_j,
                player.state.regen_energy_j,
            ),
            (
                &mut foreign.state.fuel_consumption_g,
                &mut retained.state.fuel_consumption_g,
                player.state.fuel_consumption_g,
            ),
            (
                &mut foreign.state.extra_mass_kg,
                &mut retained.state.extra_mass_kg,
                player.state.extra_mass_kg,
            ),
            (
                &mut foreign.state.power_supply.energy_j,
                &mut retained.state.power_supply.energy_j,
                player.state.power_supply.energy_j,
            ),
        ] {
            *a = original * fraction;
            *b = original - *a;
        }
        foreign.state.passengers = (f64::from(player.state.passengers) * fraction).round() as u32;
        retained.state.passengers = player.state.passengers - foreign.state.passengers;
        foreign.service_id = service_id.into();
        foreign.scenario_name = service_id.into();
        foreign.content_signature = source.donor_before.content_signature.clone();
        foreign.operational_legs = source.donor_legs.clone();
        foreign.operational_leg = source.donor_before.operational_leg;
        foreign.operational_operation = source.donor_before.operational_operation;
        foreign.operational_prefix_m = source.donor_before.operational_prefix_m;
        foreign.operational_start_m = source.donor_before.operational_start_m;
        foreign.operational_wait_started = source.donor_before.operational_wait_started;
        foreign.gameplay = source.donor_before.gameplay.clone();
        // These are service objectives and messages, not vehicle equipment.
        // A returned foreign portion resumes its own activity instead of
        // inheriting the player's ACT, announcements or driving evaluation.
        foreign.state.activity = source.donor_before.state.activity.clone();
        foreign.state.learning = source.donor_before.state.learning.clone();
        foreign.state.announcements = source.donor_before.state.announcements.clone();
        foreign.state.driving_report = source.donor_before.state.driving_report.clone();
        foreign.state.depot = source.donor_before.state.depot.clone();
        foreign.state.rail_operations = source.donor_before.state.rail_operations.clone();
        foreign.service_clock_start_s = source.donor_clock_start_s;
        foreign.state.path_edges = source.donor_before.state.path_edges.clone();
        foreign.state.path_reversed = source.donor_before.state.path_reversed;
        foreign.path_data = PathData::from_path(&foreign.state.path_edges, &foreign.graph);
        // Locate the actual coupler on the original service path, and require
        // every vehicle to occupy its original physical track after projection.
        let first_offset = player.formation.offset_m(indices[0]);
        let physical_front = player
            .position_at_head_offset(first_offset)
            .ok_or("La porción está fuera de su vía")?;
        let target = normalize(player, physical_front).ok_or("Posición de porción inválida")?;
        let mut before_m = 0.;
        let mut anchors = Vec::new();
        for (id, e) in foreign
            .state
            .path_edges
            .iter()
            .zip(&foreign.path_data.edges)
        {
            if id.strip_suffix("_r").unwrap_or(id) == target.0 {
                anchors.push(
                    before_m
                        + if id.ends_with("_r") {
                            e.length_m - target.1
                        } else {
                            target.1
                        }
                        + if foreign.state.path_reversed {
                            selected_length
                        } else {
                            0.
                        },
                );
            }
            before_m += e.length_m;
        }
        let mut found = None;
        for anchor in anchors {
            crate::live_drive::apply_start_offset(&mut foreign.state, &foreign.path_data, anchor);
            if indices.iter().enumerate().all(|(i, &old)| {
                same_position(
                    player,
                    player.position_at_head_offset(player.formation.center_offset_m(old)),
                    &foreign,
                    foreign.position_at_head_offset(foreign.formation.center_offset_m(i)),
                )
            }) {
                found = Some(anchor);
                break;
            }
        }
        let anchor = found.ok_or(
            "El itinerario del servicio separado no conserva la vía de todos sus vehículos",
        )?;
        foreign.state.odometer_m = player.state.odometer_m;
        foreign.previous_render_chainage_m = anchor;
        let removed_front = indices[0] == 0;
        let retained_anchor = player.head_chainage_m()
            - if removed_front != player.state.path_reversed {
                selected_length
            } else {
                0.
            };
        crate::live_drive::apply_start_offset(
            &mut retained.state,
            &retained.path_data,
            retained_anchor,
        );
        retained.state.odometer_m = player.state.odometer_m;
        retained.previous_render_chainage_m = retained_anchor;
        retained.content_signature = format!("{}-portion:{}", player.content_signature, service_id);
        foreign.driver_throttle = 0.;
        foreign.driver_brake = 1.;
        foreign.driver_direction = 0.5;
        foreign.state.active_cab_vehicle = foreign.formation.cars.iter().position(|c| c.powered);
        foreign.formation.cars[0].front_cock_open = false;
        foreign.formation.cars[0].hose_connected = false;
        foreign.formation.cars.last_mut().unwrap().rear_cock_open = false;
        if removed_front {
            retained.formation.cars[0].front_cock_open = false;
            retained.formation.cars[0].hose_connected = false;
        } else {
            retained.formation.cars.last_mut().unwrap().rear_cock_open = false;
        }
        // Existing authorities are partitioned by original owner and current
        // route; no removed donor retains a second occupation or stale grant.
        let footprints = foreign.own_track_occupancy();
        foreign.own_track_reservations.clear();
        retained.own_track_reservations.clear();
        for grant in &player.own_track_reservations {
            let mut remaining = vec![(grant.start_m, grant.end_m)];
            for occupied in footprints.iter().filter(|p| p.edge == grant.edge) {
                let a = grant.start_m.max(occupied.start_m);
                let b = grant.end_m.min(occupied.end_m);
                if b <= a {
                    continue;
                }
                let mut transferred = grant.clone();
                transferred.start_m = a;
                transferred.end_m = b;
                transferred.owner = service_id.into();
                foreign.own_track_reservations.push(transferred);
                remaining = remaining
                    .into_iter()
                    .flat_map(|(left, right)| {
                        if b <= left || a >= right {
                            vec![(left, right)]
                        } else {
                            let mut spans = Vec::new();
                            if left < a {
                                spans.push((left, a));
                            }
                            if b < right {
                                spans.push((b, right));
                            }
                            spans
                        }
                    })
                    .collect();
            }
            for (a, b) in remaining {
                let mut kept = grant.clone();
                kept.start_m = a;
                kept.end_m = b;
                retained.own_track_reservations.push(kept);
            }
        }
        for grants in [
            &mut foreign.own_track_reservations,
            &mut retained.own_track_reservations,
        ] {
            grants.sort_by(|a, b| a.edge.cmp(&b.edge).then(a.start_m.total_cmp(&b.start_m)));
            let mut merged: Vec<crate::native_signals::TrackOccupancy> = Vec::new();
            for grant in grants.drain(..) {
                if let Some(last) = merged.last_mut()
                    && last.edge == grant.edge
                    && last.owner == grant.owner
                    && grant.start_m <= last.end_m + 1e-8
                {
                    last.end_m = last.end_m.max(grant.end_m);
                } else {
                    merged.push(grant);
                }
            }
            *grants = merged;
        }
        foreign.dispatcher.own_locks.clear();
        retained.dispatcher.own_locks.clear();
        foreign.rebuild_formation_physics();
        foreign.sync_operating_brakes();
        retained.rebuild_formation_physics();
        retained.sync_operating_brakes();
        foreign.validate_snapshot(&foreign.snapshot())?;
        retained.validate_snapshot(&retained.snapshot())?;
        let receipt = CouplingReceipt {
            receiver: Some(player.service_id.clone()),
            kind: TransferKind::Split,
            before,
            donor_before: foreign.snapshot(),
            ..source.clone()
        };
        retained.complete_service_operation(service_id, false, player.head_chainage_m());
        *player = retained;
        self.services.push(crate::TrafficService {
            parked: source.parked,
            id: service_id.into(),
            departure_s: source.departure_s,
            session: foreign,
            departed: source.departed,
        });
        self.couplings.push(receipt);
        self.synchronize_occupancy(player);
        Ok(())
    }
}
