//! Depot work is a timed, finite transfer into actual engine circuits.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DepotState {
    pub active: Option<(String, f64)>,
    pub used_oil_l: std::collections::BTreeMap<String, f64>,
    pub used_coolant_l: std::collections::BTreeMap<String, f64>,
    pub completed: std::collections::BTreeSet<String>,
}
impl DepotState {
    pub fn valid(&self) -> bool {
        self.active
            .as_ref()
            .is_none_or(|(id, t)| id.len() <= 256 && t.is_finite() && *t >= 0.)
            && self.used_oil_l.len() <= 64
            && self.used_coolant_l.len() <= 64
            && self.completed.len() <= 64
            && self
                .used_oil_l
                .values()
                .chain(self.used_coolant_l.values())
                .all(|v| v.is_finite() && (0. ..=10000.).contains(v))
    }
}
impl crate::LiveDriveSession {
    fn depot_safe(&self, id: &str) -> bool {
        let Some(def) = self
            .state
            .rail_operations
            .definition
            .depots
            .iter()
            .find(|d| d.id == id)
        else {
            return false;
        };
        self.current_edge_id() == Some(def.edge.as_str())
            && (def.start_m..=def.end_m).contains(&self.pos_on_edge_m())
            && self.velocity_mps() < 0.1
            && self.driver_throttle == 0.
            && (0.25..0.75).contains(&self.driver_direction)
            && self
                .formation
                .cars
                .iter()
                .take(self.formation.coupled_count)
                .all(|c| c.handbrake)
            && self
                .state
                .vehicle_systems
                .iter()
                .flat_map(|v| &v.motors)
                .all(|m| !m.running && m.rpm < 20. && m.oil_c < 80. && m.coolant_c < 80.)
    }
    pub fn begin_depot_work(&mut self, id: &str) -> Result<(), String> {
        if !self.depot_safe(id) {
            return Err("Estacioná en el depósito, poné neutro, asegurá todos los frenos de mano y detené/enfriá los motores".into());
        }
        if !self
            .state
            .vehicle_systems
            .iter()
            .any(|v| v.definition.diesel_circuits.is_some())
        {
            return Err(
                "Esta formación no declara circuitos de aceite y refrigerante mantenibles".into(),
            );
        }
        if self.state.depot.active.is_some() {
            return Err("Hay un mantenimiento en curso".into());
        }
        self.state.depot.active = Some((id.into(), 0.));
        Ok(())
    }
    pub(crate) fn tick_depot_work(&mut self, dt: f64) {
        let Some((id, elapsed)) = self.state.depot.active.clone() else {
            return;
        };
        if !self.depot_safe(&id) {
            self.state.depot.active = None;
            return;
        }
        let Some(def) = self
            .state
            .rail_operations
            .definition
            .depots
            .iter()
            .find(|d| d.id == id)
            .cloned()
        else {
            self.state.depot.active = None;
            return;
        };
        if elapsed + dt < def.maintenance_s {
            self.state.depot.active = Some((id, elapsed + dt));
            return;
        }
        let mut oil =
            (def.oil_l - self.state.depot.used_oil_l.get(&id).copied().unwrap_or(0.)).max(0.);
        let mut coolant = (def.coolant_l
            - self
                .state
                .depot
                .used_coolant_l
                .get(&id)
                .copied()
                .unwrap_or(0.))
        .max(0.);
        for vehicle in &mut self.state.vehicle_systems {
            let Some(circuits) = &vehicle.definition.diesel_circuits else {
                continue;
            };
            for motor in &mut vehicle.motors {
                let added = (circuits.oil_capacity_l - motor.oil_l).max(0.).min(oil);
                motor.oil_l += added;
                oil -= added;
                *self.state.depot.used_oil_l.entry(id.clone()).or_default() += added;
                let added = (circuits.coolant_capacity_l - motor.coolant_l)
                    .max(0.)
                    .min(coolant);
                motor.coolant_l += added;
                coolant -= added;
                *self
                    .state
                    .depot
                    .used_coolant_l
                    .entry(id.clone())
                    .or_default() += added;
                if motor.oil_l >= circuits.oil_capacity_l * 0.95
                    && motor.coolant_l >= circuits.coolant_capacity_l * 0.95
                {
                    motor.fault = None;
                    motor.isolated = false;
                }
            }
        }
        self.state.depot.completed.insert(id);
        self.state.depot.active = None;
    }
    pub fn depot_status(&self) -> Option<String> {
        self.state
            .rail_operations
            .definition
            .depots
            .first()
            .map(|d| {
                if let Some((id, elapsed)) = &self.state.depot.active {
                    format!(
                        "Depósito {id} · mantenimiento {:.0}/{:.0} s",
                        elapsed, d.maintenance_s
                    )
                } else {
                    format!(
                        "Depósito {} · batería {} · tubería {:.2} bar · motores {}",
                        d.id,
                        if self.formation.cars.iter().any(|c| c.battery_on) {
                            "conectada"
                        } else {
                            "desconectada"
                        },
                        self.state
                            .brake_system
                            .cylinders
                            .first()
                            .and_then(|c| c.pipe_pressure_bar())
                            .unwrap_or(0.),
                        if self
                            .state
                            .vehicle_systems
                            .iter()
                            .flat_map(|v| &v.motors)
                            .any(|m| m.fault.is_some())
                        {
                            "con avería"
                        } else {
                            "revisados"
                        }
                    )
                }
            })
    }
}
