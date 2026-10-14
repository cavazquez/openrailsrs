//! Manual, stationary rear-to-front coupling. Original service snapshots are
//! retained as reconstruction receipts, never as a second physical train.
use crate::{LiveDriveSession, LiveTraffic, SessionSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouplingReceipt {
    pub donor: String,
    pub before: SessionSnapshot,
    pub donor_before: SessionSnapshot,
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
    fn can_attach(&self, donor: &Self) -> Result<(), String> {
        let n = self.formation.cars.len();
        let m = donor.formation.cars.len();
        if n == 0
            || m == 0
            || n + m > 256
            || self.formation.coupled_count != n
            || donor.formation.coupled_count != m
        {
            return Err("Acoplá las secciones propias antes de unir dos formaciones completas (máximo 256 vehículos)".into());
        }
        if self.velocity_mps() > 0.1
            || donor.velocity_mps() > 0.1
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
        if self.state.path_reversed
            || donor.state.path_reversed
            || self.script_tcs.is_some()
            || donor.script_tcs.is_some()
            || self.physics.steam_params.is_some()
            || donor.physics.steam_params.is_some()
            || self.physics.native.is_some() != donor.physics.native.is_some()
            || self.physics.rail_adhesion.is_some() != donor.physics.rail_adhesion.is_some()
            || self.state.vehicles.is_empty() != donor.state.vehicles.is_empty()
        {
            return Err("Esta combinación requiere otro tipo de acople; disponible: extremo trasero a delantero, diésel/eléctrico con TCS Rust y dinámica compatible".into());
        }
        let rear = self
            .state
            .brake_system
            .cylinders
            .last()
            .ok_or("Falta el freno trasero")?;
        let front = donor
            .state
            .brake_system
            .cylinders
            .first()
            .ok_or("Falta el freno delantero")?;
        if !rear.connection_compatible(front) {
            return Err("Los equipos de freno de ambos extremos no son compatibles".into());
        }
        let end = self.position_at_head_offset(-self.formation.length_m());
        let head = donor.position_at_head_offset(0.);
        if !same_position(self, end, donor, head) {
            return Err(
                "Acercá el enganche trasero al delantero de la otra formación a menos de 1 m"
                    .into(),
            );
        }
        // All donor axles must remain on exactly their original physical track;
        // contact at a junction cannot teleport the trailing train to a branch.
        for i in 0..m {
            let offset = donor.formation.center_offset_m(i);
            if !same_position(
                self,
                self.position_at_head_offset(-self.formation.length_m() + offset),
                donor,
                donor.position_at_head_offset(offset),
            ) {
                return Err(
                    "El itinerario no conserva la posición de todos los vehículos a recoger".into(),
                );
            }
        }
        Ok(())
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
        let n = self.consist.vehicles.len();
        let length = self.formation.length_m();
        let anchor = self.head_chainage_m();
        let p = &mut self.original_physics;
        let q = donor.original_physics;
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
        donor.formation.cars[0].hose_connected = false;
        donor.formation.cars[0].front_cock_open = false;
        donor.formation.cars[0].train_bus_connected = false;
        self.formation.cars[n - 1].rear_cock_open = false;
        self.formation.cars.append(&mut donor.formation.cars);
        self.formation.coupled_count = self.formation.cars.len();
        let a = &mut self.state;
        let b = &mut donor.state;
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
            if let Some(original) = a.couplers.last().or(b.couplers.first()) {
                joint = original.clone();
                joint.extension_m = 0.;
                joint.broken = false;
            }
            a.couplers.push(joint);
            a.couplers.append(&mut b.couplers);
            // Car positions represent integrated travel, not absolute chainage.
            for car in &mut b.vehicles {
                car.position_m = anchor;
            }
            a.vehicles.append(&mut b.vehicles);
            a.vehicle_masses.append(&mut b.vehicle_masses);
        }
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
        self.own_track_reservations.clear();
        self.refilling = None;
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
    }
}
impl LiveTraffic {
    pub fn validate_coupling(&self, player: &LiveDriveSession, index: usize) -> Result<(), String> {
        if self.couplings.len() >= 16 {
            return Err("Máximo 16 acoples independientes por partida".into());
        }
        player.can_attach(
            &self
                .services
                .get(index)
                .ok_or("Formación desconocida")?
                .session,
        )
    }
    pub fn couple_service(
        &mut self,
        player: &mut LiveDriveSession,
        index: usize,
    ) -> Result<(), String> {
        if self.couplings.len() >= 16 {
            return Err("Máximo 16 acoples independientes por partida".into());
        }
        let donor = self.services.get(index).ok_or("Formación desconocida")?;
        player.can_attach(&donor.session)?;
        let receipt = CouplingReceipt {
            donor: donor.id.clone(),
            before: player.snapshot(),
            donor_before: donor.session.snapshot(),
        };
        let donor = self.services.remove(index);
        player.attach(donor.session);
        self.couplings.push(receipt);
        self.synchronize_occupancy(player);
        Ok(())
    }
    pub fn reconstruct_couplings(
        &mut self,
        player: &mut LiveDriveSession,
        receipts: &[CouplingReceipt],
    ) -> Result<(), String> {
        if receipts.len() > 16 || !self.couplings.is_empty() {
            return Err("Historial de acoples inválido".into());
        }
        for receipt in receipts {
            let index = self
                .services
                .iter()
                .position(|s| s.id == receipt.donor)
                .ok_or("Formación de acople no disponible")?;
            player.validate_snapshot(&receipt.before)?;
            self.services[index]
                .session
                .validate_snapshot(&receipt.donor_before)?;
            // Restore the actual pre-contact states before checking geometry.
            player.restore_snapshot(receipt.before.clone())?;
            self.services[index]
                .session
                .restore_snapshot(receipt.donor_before.clone())?;
            self.couple_service(player, index)?;
        }
        Ok(())
    }
}
