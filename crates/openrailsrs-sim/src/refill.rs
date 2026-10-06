//! Native fuel pickup operation. Quantities are kg; the diesel tank remains litres.
use crate::{LiveDriveSession, diesel_operation::DIESEL_KG_PER_L};

#[derive(Clone, Debug, PartialEq)]
pub struct RefillStation {
    pub id: String,
    pub pickup_type: u32,
    pub capacity_kg: f64,
    pub feed_kg_s: f64,
    pub opening_time_s: f64,
    pub speed_range_mps: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct RefillOperation {
    pub station: RefillStation,
    pub vehicle: usize,
    /// Fraction of the operating pose, not the full looping animation.
    pub opening: f64,
    pub delivered_kg: f64,
    pub returning: bool,
}

impl LiveDriveSession {
    fn refill_room_kg(&self, vehicle: usize, kind: u32) -> f64 {
        if vehicle >= self.formation.coupled_count {
            return 0.;
        }
        match kind {
            5 | 6
                if self.consist.vehicles[..=vehicle].iter().any(
                    |v| matches!(v, openrailsrs_train::Vehicle::Loco(l) if l.steam.is_some()),
                ) =>
            {
                match (&self.state.boiler_state, &self.physics.steam_params) {
                    (Some(b), Some(p)) if kind == 5 => {
                        (p.initial_water_kg - b.tender_water_kg).max(0.)
                    }
                    (Some(b), Some(p)) => (p.initial_coal_kg - b.coal_kg).max(0.),
                    _ => 0.,
                }
            }
            7 => self
                .state
                .diesel
                .car(vehicle)
                .and_then(|s| {
                    self.physics
                        .diesel
                        .cars
                        .iter()
                        .find(|c| c.vehicle == vehicle)
                        .map(|c| (c.params.capacity_l - s.fuel_l).max(0.) * DIESEL_KG_PER_L)
                })
                .unwrap_or(0.),
            _ => 0.,
        }
    }

    pub fn begin_refill(
        &mut self,
        station: RefillStation,
        vehicle: usize,
        distance_m: f64,
        width_m: f64,
    ) -> Result<(), String> {
        if self.refilling.is_some() {
            return Err("El abastecedor todavía está conectado o regresando".into());
        }
        if station.id.is_empty()
            || station.id.len() > 128
            || ![
                station.capacity_kg,
                station.feed_kg_s,
                station.opening_time_s,
                station.speed_range_mps[0],
                station.speed_range_mps[1],
                distance_m,
                width_m,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            || station.capacity_kg <= 0.
            || station.feed_kg_s <= 0.
            || station.speed_range_mps[0] > station.speed_range_mps[1]
            || width_m <= 0.
            || distance_m > 2.5 + width_m / 2.
            || !self.intake_points.get(vehicle).is_some_and(|p| {
                p.iter()
                    .any(|p| p.pickup_type == station.pickup_type && p.width_m == width_m)
            })
        {
            return Err("La toma del vehículo no coincide con este abastecedor".into());
        }
        if !self.refill_speed_valid(&station) || self.driver_throttle > 0.001 {
            return Err("Detené el tren y cerrá el regulador para abastecer".into());
        }
        if self.refill_room_kg(vehicle, station.pickup_type) <= 1e-6 {
            return Err("El depósito está completo o no admite ese combustible".into());
        }
        if self
            .state
            .refill_used_kg
            .get(&station.id)
            .copied()
            .unwrap_or(0.)
            >= station.capacity_kg
        {
            return Err("El abastecedor está agotado".into());
        }
        self.refilling = Some(RefillOperation {
            station,
            vehicle,
            opening: 0.,
            delivered_kg: 0.,
            returning: false,
        });
        Ok(())
    }

    fn refill_speed_valid(&self, station: &RefillStation) -> bool {
        let speed = self.state.velocity_mps.abs();
        // OR permits stationary pickups only at rest; tolerate solver residuals.
        speed + 0.1 >= station.speed_range_mps[0] && speed <= station.speed_range_mps[1].max(0.1)
    }

    pub fn cancel_refill(&mut self) {
        if let Some(op) = &mut self.refilling {
            op.returning = true;
        }
    }

    pub(crate) fn tick_refill(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let Some(mut op) = self.refilling.take() else {
            return;
        };
        op.returning |= !self.refill_speed_valid(&op.station)
            || self.driver_throttle > 0.001
            || op.vehicle >= self.formation.coupled_count;
        let duration = op.station.opening_time_s.max(0.001);
        if op.returning {
            op.opening = (op.opening - dt / duration).max(0.);
            if op.opening > 0. {
                self.refilling = Some(op);
            }
            return;
        }
        let opening_dt = ((1. - op.opening) * duration).min(dt);
        op.opening = (op.opening + opening_dt / duration).min(1.);
        let remaining = (op.station.capacity_kg
            - self
                .state
                .refill_used_kg
                .get(&op.station.id)
                .copied()
                .unwrap_or(0.))
        .max(0.);
        let room = self.refill_room_kg(op.vehicle, op.station.pickup_type);
        let delivered = (op.station.feed_kg_s * (dt - opening_dt))
            .min(remaining)
            .min(room);
        match op.station.pickup_type {
            5 => {
                if let Some(b) = &mut self.state.boiler_state {
                    b.tender_water_kg += delivered;
                }
            }
            6 => {
                if let Some(b) = &mut self.state.boiler_state {
                    b.coal_kg += delivered;
                }
            }
            7 => {
                if let Some(s) = self
                    .state
                    .diesel
                    .cars
                    .iter_mut()
                    .find(|s| s.vehicle == op.vehicle)
                {
                    s.fuel_l += delivered / DIESEL_KG_PER_L;
                    s.refilled_l += delivered / DIESEL_KG_PER_L;
                }
            }
            _ => (),
        }
        if delivered > 0. {
            *self
                .state
                .refill_used_kg
                .entry(op.station.id.clone())
                .or_default() += delivered;
            op.delivered_kg += delivered;
        }
        op.returning |= remaining <= delivered + 1e-6 || room <= delivered + 1e-6;
        self.refilling = Some(op);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(file: &str, kind: u32) -> LiveDriveSession {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation")
            .join(file);
        let scenario = openrailsrs_scenarios::load_scenario(&p).unwrap();
        let mut s = LiveDriveSession::from_scenario(p.parent().unwrap(), &scenario).unwrap();
        s.intake_points[0] = vec![openrailsrs_formats::IntakePoint {
            offset_m: -2.,
            width_m: 1.,
            pickup_type: kind,
        }];
        s
    }
    fn station(kind: u32) -> RefillStation {
        RefillStation {
            id: "1:2:3".into(),
            pickup_type: kind,
            capacity_kg: 4.,
            feed_kg_s: 2.,
            opening_time_s: 1.,
            speed_range_mps: [0., 0.],
        }
    }
    fn diesel() -> LiveDriveSession {
        let mut s = session("scenario.toml", 7);
        let d = &mut s.state.diesel.cars[0];
        d.fuel_l -= 10.;
        d.consumed_l = 10.;
        s
    }
    #[test]
    fn finite_diesel_supply_waits_for_connection_and_preserves_consumption_and_save() {
        let mut s = diesel();
        let fuel = s.state.diesel.cars[0].fuel_l;
        s.begin_refill(station(7), 0, 3., 1.).unwrap();
        s.tick_refill(0.5);
        assert_eq!(s.state.diesel.cars[0].fuel_l, fuel);
        s.tick_refill(2.5);
        assert!((s.state.diesel.cars[0].fuel_l - fuel - 4. / DIESEL_KG_PER_L).abs() < 1e-9);
        assert_eq!(s.state.diesel.cars[0].consumed_l, 10.);
        assert!(s.state.diesel.valid_for(&s.physics.diesel));
        assert_eq!(s.state.refill_used_kg["1:2:3"], 4.);
        assert!(s.refilling.as_ref().unwrap().returning);
        let saved = s.snapshot();
        let mut fresh = session("scenario.toml", 7);
        fresh.restore_snapshot(saved).unwrap();
        assert!(fresh.refilling.is_none());
        assert_eq!(fresh.state.refill_used_kg["1:2:3"], 4.);
        assert!(
            fresh
                .begin_refill(station(7), 0, 0., 1.)
                .unwrap_err()
                .contains("agotado")
        );
    }
    #[test]
    fn filling_is_partition_independent_and_stops_if_train_moves() {
        let mut a = diesel();
        let mut b = diesel();
        for s in [&mut a, &mut b] {
            s.begin_refill(station(7), 0, 0., 1.).unwrap();
        }
        a.tick_refill(2.);
        for _ in 0..40 {
            b.tick_refill(0.05);
        }
        assert!((a.state.diesel.cars[0].fuel_l - b.state.diesel.cars[0].fuel_l).abs() < 1e-9);
        let fuel = a.state.diesel.cars[0].fuel_l;
        a.state.velocity_mps = 0.2;
        a.tick_refill(0.5);
        assert_eq!(a.state.diesel.cars[0].fuel_l, fuel);
        assert!(a.refilling.as_ref().unwrap().returning);
        a.tick_refill(0.5);
        assert!(a.refilling.is_none());
    }
    #[test]
    fn water_fills_only_tender_without_repairing_boiler_and_cancel_does_not_transfer() {
        let mut s = session("scenario_steam.toml", 5);
        let b = s.state.boiler_state.as_mut().unwrap();
        b.tender_water_kg -= 10.;
        b.low_water_failure = true;
        let boiler = b.water_kg;
        let tender = b.tender_water_kg;
        s.begin_refill(station(5), 0, 0., 1.).unwrap();
        s.tick_refill(2.);
        let b = s.state.boiler_state.as_ref().unwrap();
        assert_eq!(b.water_kg, boiler);
        assert_eq!(b.tender_water_kg, tender + 2.);
        assert!(b.low_water_failure);
        s.cancel_refill();
        s.tick_refill(5.);
        assert!(s.refilling.is_none());
        assert_eq!(
            s.state.boiler_state.as_ref().unwrap().tender_water_kg,
            tender + 2.
        );
    }
    #[test]
    fn rejects_bad_profile_distance_missing_intake_and_full_tank_without_mutation() {
        let mut s = diesel();
        assert!(s.begin_refill(station(5), 0, 0., 1.).is_err());
        assert!(s.begin_refill(station(7), 0, 3.01, 1.).is_err());
        assert!(s.begin_refill(station(7), 99, 0., 1.).is_err());
        let mut invalid = station(7);
        invalid.feed_kg_s = f64::NAN;
        assert!(s.begin_refill(invalid, 0, 0., 1.).is_err());
        assert!(s.refilling.is_none());
        assert!(s.state.refill_used_kg.is_empty());
        s.state.diesel.cars[0].fuel_l += 10.;
        s.state.diesel.cars[0].consumed_l = 0.;
        assert!(s.begin_refill(station(7), 0, 0., 1.).is_err());
    }
}
