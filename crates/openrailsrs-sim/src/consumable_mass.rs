//! Effective reserve mass. Authored full-tank masses remain immutable; fuel
//! changes inertia without changing positions, velocities or snapshot identity.
use crate::{TrainSimState, diesel_operation::DIESEL_KG_PER_L, physics::TrainPhysics};

pub fn diesel_mass_kg(full_mass_kg: f64, full_l: f64, remaining_l: f64) -> f64 {
    if !full_mass_kg.is_finite() || !full_l.is_finite() || !remaining_l.is_finite() {
        return full_mass_kg;
    }
    let dry = (full_mass_kg - full_l.max(0.0) * DIESEL_KG_PER_L).max(1.0);
    (full_mass_kg - (full_l.max(0.0) - remaining_l.clamp(0.0, full_l.max(0.0))) * DIESEL_KG_PER_L)
        .clamp(dry, full_mass_kg.max(dry))
}

pub fn vehicle_mass_kg(
    train: &TrainPhysics,
    state: &TrainSimState,
    index: usize,
    base: f64,
) -> f64 {
    let base = (base + steam_mass_delta(state, index)).max(1.);
    if let Some(cargo) = state.freight.iter().find(|c| c.vehicle == index) {
        return (base + cargo.mass_delta_kg()).max(1.);
    }
    if let Some(c) = state
        .vehicle_systems
        .iter()
        .find(|c| c.definition.vehicle == index && !c.motors.is_empty())
    {
        return diesel_mass_kg(
            base,
            c.definition.motors.iter().map(|p| p.fuel_l).sum(),
            c.motors.iter().map(|s| s.fuel_l).sum(),
        );
    }
    let config = train.diesel.cars.iter().find(|c| c.vehicle == index);
    let fuel = state.diesel.car(index);
    match (config, fuel) {
        (Some(c), Some(s)) => {
            legacy_diesel_mass_kg(train, index, base, c.params.capacity_l, s.fuel_l)
        }
        _ => base,
    }
}

fn legacy_diesel_mass_kg(
    train: &TrainPhysics,
    index: usize,
    base: f64,
    capacity_l: f64,
    fuel_l: f64,
) -> f64 {
    let inventory = if let Some(native) = &train.native {
        let Some(profile) = native
            .vehicles
            .get(index)
            .and_then(|v| v.inventory_mass.as_ref())
        else {
            // MSTSWagon.UpdateLocomotiveLoadPhysics returns without changing
            // MassKG when ContinuousFreightAnimationsPresent is false.
            return base;
        };
        Some(profile)
    } else {
        None
    };
    let mass = diesel_mass_kg(base, capacity_l, fuel_l);
    inventory.map_or(mass, |bounds| {
        // Preserve the existing reserve-mass extension for continuous stock,
        // inside its authored native bounds. This does not claim the full OR
        // heating-water/sand inventory and load-physics interpolation model.
        mass.clamp(bounds.empty_kg, f64::from(bounds.full_kg as f32 * 1.02_f32))
    })
}

pub fn total_mass_kg(train: &TrainPhysics, state: &TrainSimState) -> f64 {
    let delta: f64 = train
        .diesel
        .cars
        .iter()
        .filter_map(|c| {
            state
                .diesel
                .car(c.vehicle)
                .filter(|_| {
                    !state
                        .vehicle_systems
                        .iter()
                        .any(|s| s.definition.vehicle == c.vehicle && !s.motors.is_empty())
                })
                .map(|s| {
                    if let Some(native) = &train.native {
                        let Some(&base) = native.vehicle_masses_kg.get(c.vehicle) else {
                            return 0.;
                        };
                        legacy_diesel_mass_kg(train, c.vehicle, base, c.params.capacity_l, s.fuel_l)
                            - base
                    } else {
                        (s.fuel_l.clamp(0.0, c.params.capacity_l) - c.params.capacity_l)
                            * DIESEL_KG_PER_L
                    }
                })
        })
        .sum();
    let declared: f64 = state
        .vehicle_systems
        .iter()
        .filter(|c| c.definition.vehicle < train.vehicle_lengths_m.len())
        .flat_map(|c| &c.motors)
        .map(|m| (m.refilled_l - m.consumed_l) * DIESEL_KG_PER_L)
        .sum();
    let freight = state
        .freight
        .iter()
        .filter(|c| c.vehicle < train.vehicle_lengths_m.len())
        .map(|c| c.mass_delta_kg())
        .sum::<f64>();
    let steam = (0..train.vehicle_lengths_m.len())
        .map(|i| steam_mass_delta(state, i))
        .sum::<f64>();
    (train.mass_kg + state.extra_mass_kg + delta + declared + freight + steam).max(1.0)
}

fn steam_mass_delta(state: &TrainSimState, index: usize) -> f64 {
    let boiler = state
        .steam_boilers
        .iter()
        .find(|b| b.vehicle == index)
        .map(|b| &b.boiler)
        .or_else(|| {
            (state.steam_boilers.is_empty() && index == state.active_cab_vehicle.unwrap_or(0))
                .then_some(state.boiler_state.as_ref())
                .flatten()
        });
    let Some(b) = boiler else { return 0. };
    let Some(t) = &b.thermodynamics else {
        return 0.;
    }; // Preserve the legacy mass path.
    // Filling the fire transfers coal inside the locomotive. HP exhaust stored
    // in a compound receiver remains aboard until the LP engine exhausts it.
    t.total_mass_kg - t.initial_mass_kg
        + t.receiver_mass_kg
        + b.tender_water_kg
        + b.coal_kg
        + b.fire_mass_kg
        - (b.initial_tender_water_kg + b.initial_coal_kg + b.fire_capacity_kg * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(file: &str) -> crate::LiveDriveSession {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation")
            .join(file);
        let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        scenario.simulation.native_physics = Some(Default::default());
        crate::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
    }

    #[test]
    fn native_stock_without_continuous_freight_keeps_mass_while_own_inventory_changes() {
        let mut s = session("scenario_two_diesel.toml");
        let native = s.physics.native.as_ref().unwrap();
        assert!(native.vehicles.iter().all(|p| p.inventory_mass.is_none()));
        let index = s.physics.diesel.cars[0].vehicle;
        let base = native.vehicle_masses_kg[index];
        let before = total_mass_kg(&s.physics, &s.state);
        s.state.diesel.cars[0].fuel_l -= 10.;
        s.state.diesel.cars[0].consumed_l += 10.;
        assert_eq!(vehicle_mass_kg(&s.physics, &s.state, index, base), base);
        assert_eq!(total_mass_kg(&s.physics, &s.state), before);
        // The explicit own model continues to account for consumable inertia.
        s.physics.native = None;
        assert!(
            (vehicle_mass_kg(&s.physics, &s.state, index, base) - (base - 10. * DIESEL_KG_PER_L))
                .abs()
                < 1e-9
        );
        assert!(
            (total_mass_kg(&s.physics, &s.state) - (before - 10. * DIESEL_KG_PER_L)).abs() < 1e-9
        );
    }

    #[test]
    fn continuous_stock_keeps_existing_inventory_extension_inside_authored_bounds() {
        let mut s = session("scenario_two_diesel.toml");
        let index = s.physics.diesel.cars[0].vehicle;
        let base = s.physics.native.as_ref().unwrap().vehicle_masses_kg[index];
        s.physics.native.as_mut().unwrap().vehicles[index].inventory_mass = Some(
            openrailsrs_formats::typed::native_physics::NativeInventoryMass {
                empty_kg: base - 1.,
                full_kg: base,
            },
        );
        let before = total_mass_kg(&s.physics, &s.state);
        s.state.diesel.cars[0].fuel_l -= 10.;
        s.state.diesel.cars[0].consumed_l += 10.;
        assert_eq!(
            vehicle_mass_kg(&s.physics, &s.state, index, base),
            base - 1.
        );
        assert_eq!(total_mass_kg(&s.physics, &s.state), before - 1.);
    }

    #[test]
    fn declared_vehicle_runtime_motors_retain_mass_changes_with_native_axles() {
        let mut s = session("scenario_equipment.toml");
        let car = s
            .state
            .vehicle_systems
            .iter()
            .find(|c| !c.motors.is_empty())
            .unwrap();
        let index = car.definition.vehicle;
        let base = s.physics.native.as_ref().unwrap().vehicle_masses_kg[index];
        let before_vehicle = vehicle_mass_kg(&s.physics, &s.state, index, base);
        let before_total = total_mass_kg(&s.physics, &s.state);
        let car = s
            .state
            .vehicle_systems
            .iter_mut()
            .find(|c| c.definition.vehicle == index)
            .unwrap();
        car.motors[0].fuel_l -= 10.;
        car.motors[0].consumed_l += 10.;
        assert!(
            (vehicle_mass_kg(&s.physics, &s.state, index, base)
                - (before_vehicle - 10. * DIESEL_KG_PER_L))
                .abs()
                < 1e-9
        );
        assert!(
            (total_mass_kg(&s.physics, &s.state) - (before_total - 10. * DIESEL_KG_PER_L)).abs()
                < 1e-9
        );
    }

    #[test]
    fn reserve_mass_matches_original_diesel_density_and_refill_is_reversible() {
        let full = 80_000.0;
        assert_eq!(diesel_mass_kg(full, 5_000.0, 5_000.0), full);
        assert!((diesel_mass_kg(full, 5_000.0, 0.0) - 75_746.0).abs() < 1e-9);
        assert!((diesel_mass_kg(full, 5_000.0, 2_500.0) - 77_873.0).abs() < 1e-9);
        assert_eq!(diesel_mass_kg(full, 5_000.0, 6_000.0), full);
        assert_eq!(diesel_mass_kg(full, 0.0, 0.0), full);
    }

    #[test]
    fn reserve_consumption_does_not_accumulate_roundoff_or_negative_mass() {
        let masses: Vec<_> = (0..=1000)
            .map(|i| diesel_mass_kg(20_000.0, 1_000.0, f64::from(i)))
            .collect();
        assert!(
            masses
                .windows(2)
                .all(|p| (p[1] - p[0] - DIESEL_KG_PER_L).abs() < 1e-8)
        );
        assert_eq!(diesel_mass_kg(10.0, 10_000.0, 0.0), 1.0);
    }
    #[test]
    fn fuel_mass_and_driven_weight_match_the_pinned_native_oracle() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../oracles/openrails-consumable-mass.json"
        ))
        .unwrap();
        assert_eq!(reference["version"], "1.6.1");
        let full = reference["full_mass_kg"].as_f64().unwrap();
        let capacity = reference["capacity_l"].as_f64().unwrap();
        for p in reference["points"].as_array().unwrap() {
            let mass = diesel_mass_kg(full, capacity, p["fuel_l"].as_f64().unwrap());
            assert!((mass - p["mass_kg"].as_f64().unwrap()).abs() <= 0.02);
            assert!((36000. * mass / full - p["driven_mass_kg"].as_f64().unwrap()).abs() <= 0.02);
        }
    }
}
