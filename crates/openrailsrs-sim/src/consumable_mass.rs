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
        (Some(c), Some(s)) => diesel_mass_kg(base, c.params.capacity_l, s.fuel_l),
        _ => base,
    }
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
                    (s.fuel_l.clamp(0.0, c.params.capacity_l) - c.params.capacity_l)
                        * DIESEL_KG_PER_L
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
    (train.mass_kg + state.extra_mass_kg + delta + declared).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
