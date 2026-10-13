//! Declared traction storage and source selection. A vehicle uses one source per tick.
use openrailsrs_scenarios::vehicle_systems::{TractionBatteryDef, TractionSource};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TractionEnergyState {
    pub selection: TractionSource,
    pub active: Option<TractionSource>,
    pub energy_j: f64,
    pub discharged_j: f64,
    pub charged_j: f64,
    pub losses_j: f64,
    pub mechanical_j: f64,
    pub charging_w: f64,
}
impl TractionEnergyState {
    pub fn new(p: Option<&TractionBatteryDef>) -> Self {
        Self {
            energy_j: p.map_or(0., |p| p.capacity_kwh * 3_600_000. * p.initial_fraction),
            ..Default::default()
        }
    }
    pub fn valid(&self, p: Option<&TractionBatteryDef>) -> bool {
        [
            self.energy_j,
            self.discharged_j,
            self.charged_j,
            self.losses_j,
            self.mechanical_j,
            self.charging_w,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && p.map_or(self.energy_j == 0., |p| {
                self.energy_j <= p.capacity_kwh * 3_600_000. + 1e-6
                    && (self.energy_j + self.discharged_j
                        - self.charged_j
                        - p.capacity_kwh * 3_600_000. * p.initial_fraction)
                        .abs()
                        < 1e-4
            })
    }
    pub fn discharge(&mut self, p: &TractionBatteryDef, mechanical_j: f64) -> f64 {
        let available = (self.energy_j - p.capacity_kwh * 3_600_000. * p.minimum_fraction).max(0.);
        let taken = (mechanical_j.max(0.) / p.efficiency).min(available);
        self.energy_j -= taken;
        self.discharged_j += taken;
        self.mechanical_j += taken * p.efficiency;
        self.losses_j += taken * (1. - p.efficiency);
        taken * p.efficiency
    }
    pub fn charge(&mut self, p: &TractionBatteryDef, input_j: f64) -> f64 {
        let capacity = p.capacity_kwh * 3_600_000.;
        let taken = (input_j.max(0.) * p.efficiency).min((capacity - self.energy_j).max(0.));
        self.energy_j += taken;
        self.charged_j += taken;
        self.losses_j += taken * (1. / p.efficiency - 1.);
        taken / p.efficiency
    }
}
pub fn prepare(state: &mut crate::TrainSimState, count: usize) {
    for car in &mut state.vehicle_systems {
        let i = car.definition.vehicle;
        if i >= count {
            car.energy.active = None;
            continue;
        }
        let line = state
            .electric
            .cars
            .iter()
            .any(|c| c.vehicle == i && c.main_power)
            && state.power_supply.power_available(i);
        let diesel = car
            .motors
            .iter()
            .any(|m| m.running && !m.isolated && m.fuel_l > 0. && m.fault.is_none())
            || state.diesel.cars.iter().any(|c| {
                c.vehicle == i
                    && c.phase == crate::diesel_operation::EnginePhase::Running
                    && c.command_running
                    && c.fuel_l > 0.
            }) && state.power_supply.power_available(i);
        let controls = state
            .power_supply
            .cars
            .iter()
            .find(|c| c.vehicle == i)
            .is_none_or(|c| c.supply.cab);
        let diesel = diesel && controls;
        let battery = controls
            && car.definition.battery.as_ref().is_some_and(|p| {
                car.energy.energy_j > p.capacity_kwh * 3_600_000. * p.minimum_fraction + 1e-6
            });
        car.energy.active = match car.energy.selection {
            TractionSource::Auto => {
                if line {
                    Some(TractionSource::Line)
                } else if diesel {
                    Some(TractionSource::Diesel)
                } else if battery {
                    Some(TractionSource::Battery)
                } else {
                    None
                }
            }
            TractionSource::Line => line.then_some(TractionSource::Line),
            TractionSource::Diesel => diesel.then_some(TractionSource::Diesel),
            TractionSource::Battery => battery.then_some(TractionSource::Battery),
        };
    }
}
pub fn available(state: &crate::TrainSimState, vehicle: usize) -> bool {
    if let Some(car) = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == vehicle && c.definition.battery.is_some())
    {
        car.energy.active.is_some()
    } else if let Some(car) = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == vehicle && !c.motors.is_empty())
    {
        car.motors
            .iter()
            .any(|m| m.running && !m.isolated && m.fuel_l > 0. && m.fault.is_none())
            && state.power_supply.power_available(vehicle)
    } else {
        state.electric.power_available(vehicle)
            && state.diesel.power_available(vehicle)
            && state.power_supply.power_available(vehicle)
    }
}
pub fn limit_motor(
    state: &crate::TrainSimState,
    vehicle: usize,
    force: f64,
    speed: f64,
    dt: f64,
    train: &crate::physics::TrainPhysics,
    path: &crate::path_data::PathData,
) -> f64 {
    let Some(car) = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == vehicle)
    else {
        return force;
    };
    let Some(p) = &car.definition.battery else {
        return force;
    };
    if car.energy.active == Some(TractionSource::Battery) {
        let energy =
            (car.energy.energy_j - p.capacity_kwh * 3_600_000. * p.minimum_fraction).max(0.);
        let power = p.maximum_discharge_w.min(energy / dt.max(1e-6)) * p.efficiency;
        // Upper bound on end speed includes all positive traction and downhill
        // gravity. Bounding work before applying force prevents empty storage
        // from delivering unaccounted mechanical energy during acceleration.
        let mass = crate::consumable_mass::total_mass_kg(train, state).max(1.);
        let downhill = path
            .edges
            .iter()
            .flat_map(|e| {
                std::iter::once(e.grade_percent.abs() / 100.).chain(
                    e.profile
                        .grades
                        .iter()
                        .map(|g| g.grade_percent.abs() / 100.),
                )
            })
            .fold(0., f64::max);
        let maximum_force = train.max_tractive_effort_n.max(force)
            + train
                .electric
                .fallback_cars
                .iter()
                .map(|(_, c)| c.points.iter().map(|p| p.1).fold(0., f64::max))
                .sum::<f64>()
            + train
                .diesel_engines
                .iter()
                .map(|e| {
                    e.notch_curves
                        .iter()
                        .flat_map(|(_, c)| &c.points)
                        .map(|p| p.1)
                        .fold(e.max_force_n, f64::max)
                })
                .sum::<f64>();
        let end = speed.abs() + dt * (maximum_force / mass + 9.80665 * downhill);
        force.min(power / end.max(0.5))
    } else if car.energy.active.is_some() {
        force
    } else {
        0.
    }
}
pub fn account(state: &mut crate::TrainSimState, speed: f64, dt: f64) {
    for car in &mut state.vehicle_systems {
        let Some(p) = &car.definition.battery else {
            continue;
        };
        let force = state
            .rail_force_n
            .get(car.definition.vehicle)
            .copied()
            .unwrap_or(0.)
            .max(0.);
        if car.energy.active == Some(TractionSource::Battery) {
            let work = force * speed.abs() * dt;
            let delivered = car.energy.discharge(p, work);
            debug_assert!(
                delivered + 1e-5 >= work,
                "traction storage under-accounted mechanical work"
            );
            // The common counter already included mechanical work. Add only
            // conversion losses, so battery traction is counted once.
            state.cumulative_energy_j += delivered * (1. / p.efficiency - 1.);
            car.energy.charging_w = 0.;
        } else if car.energy.active == Some(TractionSource::Line) {
            let drawn = car.energy.charge(p, p.maximum_charge_w * dt);
            car.energy.charging_w = if dt > 0. { drawn / dt } else { 0. };
            state.cumulative_energy_j += drawn;
        } else {
            car.energy.charging_w = 0.;
        }
    }
}
impl crate::LiveDriveSession {
    pub fn select_traction_source(
        &mut self,
        vehicle: usize,
        source: TractionSource,
    ) -> Result<(), String> {
        if self.driver_throttle > 0.001 {
            return Err("Cerrá el regulador antes de cambiar de fuente".into());
        }
        let c = self
            .state
            .vehicle_systems
            .iter_mut()
            .find(|c| c.definition.vehicle == vehicle && c.definition.battery.is_some())
            .ok_or("El vehículo no declara batería de tracción o propulsión dual")?;
        c.energy.selection = source;
        prepare(&mut self.state, self.formation.coupled_count);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn params() -> TractionBatteryDef {
        TractionBatteryDef {
            capacity_kwh: 1.,
            initial_fraction: 0.5,
            maximum_discharge_w: 100000.,
            maximum_charge_w: 50000.,
            efficiency: 0.9,
            minimum_fraction: 0.1,
        }
    }
    #[test]
    fn bounded_storage_conserves_energy_at_depletion_full_charge_and_restore() {
        let p = params();
        let mut s = TractionEnergyState::new(Some(&p));
        for _ in 0..100 {
            s.discharge(&p, 100000.);
        }
        assert!((s.energy_j - 360000.).abs() < 1e-6);
        assert!(s.valid(Some(&p)));
        assert!((s.mechanical_j + s.losses_j - s.discharged_j).abs() < 1e-6);
        let mut b: TractionEnergyState =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        for _ in 0..100 {
            s.charge(&p, 50000.);
            b.charge(&p, 50000.);
        }
        assert_eq!(s, b);
        assert!((s.energy_j - 3600000.).abs() < 1e-6);
        assert!(s.valid(Some(&p)));
    }
}
