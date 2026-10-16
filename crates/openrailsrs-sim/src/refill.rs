//! Native fuel pickup operation. Quantities are kg; the diesel tank remains litres.
use crate::{LiveDriveSession, diesel_operation::DIESEL_KG_PER_L};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RefillStation {
    pub id: String,
    pub pickup_type: u32,
    pub capacity_kg: f64,
    pub feed_kg_s: f64,
    pub opening_time_s: f64,
    pub speed_range_mps: [f64; 2],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RefillOperation {
    #[serde(default)]
    pub unloading: bool,
    #[serde(default)]
    pub delay_remaining_s: f64,
    pub station: RefillStation,
    pub vehicle: usize,
    /// Fraction of the operating pose, not the full looping animation.
    pub opening: f64,
    pub delivered_kg: f64,
    pub returning: bool,
}
impl RefillOperation {
    pub(crate) fn valid(&self, vehicles: usize) -> bool {
        let s = &self.station;
        self.vehicle < vehicles
            && !s.id.is_empty()
            && s.id.len() <= 256
            && !s.id.chars().any(char::is_control)
            && (1..=14).contains(&s.pickup_type)
            && self.delay_remaining_s.is_finite()
            && (0. ..=86400.).contains(&self.delay_remaining_s)
            && [
                s.capacity_kg,
                s.feed_kg_s,
                s.opening_time_s,
                self.delivered_kg,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0. && *v <= 1e9)
            && (0. ..=1.).contains(&self.opening)
            && s.speed_range_mps
                .iter()
                .all(|v| v.is_finite() && (0. ..=100.).contains(v))
            && s.speed_range_mps[0] <= s.speed_range_mps[1]
            && self.delivered_kg <= s.capacity_kg
    }
}

impl LiveDriveSession {
    fn refill_room_kg(&self, vehicle: usize, kind: u32) -> f64 {
        if vehicle >= self.formation.coupled_count {
            return 0.;
        }
        if let Some(c) = self
            .state
            .freight
            .iter()
            .find(|c| c.vehicle == vehicle && c.profile.pickup_types.contains(&kind))
        {
            return (c.profile.capacity_kg - c.load_kg).max(0.);
        }
        match kind {
            5 | 6
                if self.consist.vehicles[..=vehicle].iter().any(
                    |v| matches!(v, openrailsrs_train::Vehicle::Loco(l) if l.steam.is_some()),
                ) =>
            {
                match (&self.state.boiler_state, &self.physics.steam_params) {
                    (Some(b), _) if b.native_or161.is_some() => {
                        if kind == 5 {
                            (b.initial_tender_water_kg - b.tender_water_kg).max(0.)
                        } else {
                            (b.initial_coal_kg - b.coal_kg).max(0.)
                        }
                    }
                    (Some(b), Some(p)) if kind == 5 => {
                        (p.initial_water_kg - b.tender_water_kg).max(0.)
                    }
                    (Some(b), Some(p)) => (p.initial_coal_kg - b.coal_kg).max(0.),
                    _ => 0.,
                }
            }
            7 if self
                .state
                .vehicle_systems
                .iter()
                .any(|c| c.definition.vehicle == vehicle && !c.motors.is_empty()) =>
            {
                self.state
                    .vehicle_systems
                    .iter()
                    .find(|c| c.definition.vehicle == vehicle)
                    .unwrap()
                    .motors
                    .iter()
                    .zip(
                        &self
                            .state
                            .vehicle_systems
                            .iter()
                            .find(|c| c.definition.vehicle == vehicle)
                            .unwrap()
                            .definition
                            .motors,
                    )
                    .map(|(s, p)| (p.fuel_l - s.fuel_l).max(0.) * DIESEL_KG_PER_L)
                    .sum()
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
        self.begin_transfer(station, vehicle, distance_m, width_m, false)
    }
    pub fn begin_unload(
        &mut self,
        station: RefillStation,
        vehicle: usize,
        distance_m: f64,
        width_m: f64,
    ) -> Result<(), String> {
        self.begin_transfer(station, vehicle, distance_m, width_m, true)
    }
    fn begin_transfer(
        &mut self,
        station: RefillStation,
        vehicle: usize,
        distance_m: f64,
        width_m: f64,
        unloading: bool,
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
        let cargo = self.state.freight.iter().find(|c| {
            c.vehicle == vehicle && c.profile.pickup_types.contains(&station.pickup_type)
        });
        let room = if unloading {
            cargo.map_or(0., |c| c.load_kg)
        } else {
            self.refill_room_kg(vehicle, station.pickup_type)
        };
        if room <= 1e-6 {
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
            unloading,
            delay_remaining_s: if unloading {
                cargo.map_or(0., |c| c.profile.unloading_delay_s)
            } else {
                0.
            },
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
        let cargo = self.state.freight.iter().find(|c| {
            c.vehicle == op.vehicle && c.profile.pickup_types.contains(&op.station.pickup_type)
        });
        let room = if op.unloading {
            cargo.map_or(0., |c| c.load_kg)
        } else {
            self.refill_room_kg(op.vehicle, op.station.pickup_type)
        };
        let delay_dt = op.delay_remaining_s.min(dt - opening_dt);
        op.delay_remaining_s -= delay_dt;
        let mut delivered = (op.station.feed_kg_s * (dt - opening_dt - delay_dt))
            .min(remaining)
            .min(room);
        if let Some(c) = self.state.freight.iter_mut().find(|c| {
            c.vehicle == op.vehicle && c.profile.pickup_types.contains(&op.station.pickup_type)
        }) {
            if op.unloading {
                c.load_kg -= delivered;
                c.unloaded_kg += delivered;
            } else {
                c.load_kg += delivered;
                c.received_kg += delivered;
            }
        } else {
            match op.station.pickup_type {
                5 | 6 => {
                    if let Some(b) = &mut self.state.boiler_state {
                        let native = b.native_or161.is_some();
                        if native {
                            let supply = if op.station.pickup_type == 5 {
                                crate::native_steam::NativeSteamSupply::Water
                            } else {
                                crate::native_steam::NativeSteamSupply::Coal
                            };
                            match crate::native_steam::replenish_boiler(b, supply, delivered) {
                                Ok(accepted) => delivered = accepted,
                                Err(_) => {
                                    // Invalid native inventories cannot be
                                    // refilled through a legacy mirror. Stop
                                    // transferring without charging the source.
                                    delivered = 0.;
                                    op.returning = true;
                                }
                            }
                        } else if op.station.pickup_type == 5 {
                            b.tender_water_kg += delivered;
                        } else {
                            b.coal_kg += delivered;
                        }
                        let active = self.state.active_cab_vehicle.unwrap_or(0);
                        if native
                            && let Some(entry) = self
                                .state
                                .steam_boilers
                                .iter_mut()
                                .find(|entry| entry.vehicle == active)
                        {
                            // A same-tick save must see the authoritative
                            // native controller inventory and its mirror in
                            // both representations of the active boiler.
                            entry.boiler = b.clone();
                        }
                    } else {
                        delivered = 0.;
                        op.returning = true;
                    }
                }
                7 if self
                    .state
                    .vehicle_systems
                    .iter()
                    .any(|c| c.definition.vehicle == op.vehicle && !c.motors.is_empty()) =>
                {
                    let car = self
                        .state
                        .vehicle_systems
                        .iter_mut()
                        .find(|c| c.definition.vehicle == op.vehicle)
                        .unwrap();
                    let mut remaining = delivered / DIESEL_KG_PER_L;
                    for (s, p) in car.motors.iter_mut().zip(&car.definition.motors) {
                        let filled = remaining.min((p.fuel_l - s.fuel_l).max(0.));
                        s.fuel_l += filled;
                        s.refilled_l += filled;
                        remaining -= filled;
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
    fn native_steam(kind: u32) -> LiveDriveSession {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/physics_lab");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario_native_steam.toml"))
                .unwrap();
        let mut s = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        s.intake_points[0] = vec![openrailsrs_formats::IntakePoint {
            offset_m: -2.,
            width_m: 1.,
            pickup_type: kind,
        }];
        s.driver_throttle = 0.;
        s.driver_brake = 1.;
        s.steam_command(crate::steam::SteamCommand::Injector1)
            .unwrap();
        // Deplete the real native reservoirs through its normal thermal and
        // brake cycle. No observed native trajectory is an initialization.
        for _ in 0..400 {
            s.step_realtime(0.05, |_| {});
        }
        s
    }
    #[test]
    fn native_refill_updates_controllers_finite_supply_and_same_tick_save() {
        for kind in [5, 6] {
            let mut s = native_steam(kind);
            let b = s.state.boiler_state.as_mut().unwrap();
            // A recorded fusible-plug failure cannot be repaired by a fuel
            // installation. Keep the native state and its checked mirror equal.
            b.native_or161.as_mut().unwrap().FusiblePlugIsBlown = true;
            b.low_water_failure = true;
            s.state.steam_boilers[0].boiler = b.clone();
            let before = b.clone();
            let room = s.refill_room_kg(0, kind);
            assert!(room > 0., "native resource did not deplete: kind {kind}");
            s.begin_refill(station(kind), 0, 0., 1.).unwrap();
            s.tick_refill(3.);
            let after = s.state.boiler_state.as_ref().unwrap();
            let received = if kind == 5 {
                after.tender_water_kg - before.tender_water_kg
            } else {
                after.coal_kg - before.coal_kg
            };
            assert!(received > 0. && received <= room.min(4.));
            assert_eq!(s.state.refill_used_kg["1:2:3"], received);
            assert_eq!(s.refilling.as_ref().unwrap().delivered_kg, received);
            assert_eq!(s.state.steam_boilers[0].boiler, *after);
            let a = after.native_or161.as_ref().unwrap();
            let b = before.native_or161.as_ref().unwrap();
            assert_eq!(a.BoilerMassLB, b.BoilerMassLB);
            assert_eq!(a.BoilerHeatBTU, b.BoilerHeatBTU);
            assert_eq!(a.FireMassKG, b.FireMassKG);
            assert_eq!(a.BoilerPressurePSI, b.BoilerPressurePSI);
            assert!(a.FusiblePlugIsBlown && after.low_water_failure);
            assert!(if kind == 5 {
                a.WaterControllerValue > b.WaterControllerValue
                    && a.FuelControllerValue == b.FuelControllerValue
            } else {
                a.FuelControllerValue > b.FuelControllerValue
                    && a.WaterControllerValue == b.WaterControllerValue
            });
            let encoded = serde_json::to_vec(&s.snapshot()).unwrap();
            let mut restored = native_steam(kind);
            restored
                .restore_snapshot(serde_json::from_slice(&encoded).unwrap())
                .unwrap();
            assert_eq!(restored.state.boiler_state, s.state.boiler_state);
            assert_eq!(restored.state.steam_boilers, s.state.steam_boilers);
            assert_eq!(restored.state.refill_used_kg, s.state.refill_used_kg);
        }
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
        assert!(fresh.refilling.as_ref().unwrap().returning);
        fresh.tick_refill(1.0);
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
