//! Ideal shared-bogie longitudinal constraints. Projection conserves group
//! momentum; lateral support states and risk indications live in bogie_dynamics.
pub(crate) fn constrain(
    vehicles: &mut [crate::VehicleState],
    couplers: &mut [crate::CouplerState],
    joints: &[[usize; 2]],
    masses: &[f64],
) {
    let mut first = 0;
    while first < vehicles.len() {
        let mut last = first;
        while last + 1 < vehicles.len() && joints.contains(&[last, last + 1]) {
            last += 1;
        }
        if last > first {
            let mass = (first..=last)
                .map(|i| masses.get(i).copied().unwrap_or(1.).max(1.))
                .sum::<f64>();
            let velocity = (first..=last)
                .map(|i| vehicles[i].velocity_mps * masses.get(i).copied().unwrap_or(1.).max(1.))
                .sum::<f64>()
                / mass;
            let position = (first..=last)
                .map(|i| vehicles[i].position_m * masses.get(i).copied().unwrap_or(1.).max(1.))
                .sum::<f64>()
                / mass;
            for vehicle in &mut vehicles[first..=last] {
                vehicle.velocity_mps = velocity;
                vehicle.position_m = position;
            }
            for i in first..last {
                if let Some(c) = couplers.get_mut(i) {
                    c.extension_m = 0.;
                }
            }
        }
        first = last + 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_bogie_group_conserves_momentum_and_does_not_constrain_a_separate_vehicle() {
        let mut s = crate::TrainSimState::new(vec![]);
        s.articulated_joints = vec![[0, 1], [1, 2]];
        s.vehicles = (0..4)
            .map(|i| crate::VehicleState {
                position_m: i as f64,
                velocity_mps: 2. * i as f64,
            })
            .collect();
        let masses = [10000., 20000., 30000., 40000.];
        let before = (0..3)
            .map(|i| s.vehicles[i].velocity_mps * masses[i])
            .sum::<f64>();
        constrain(
            &mut s.vehicles,
            &mut s.couplers,
            &s.articulated_joints,
            &masses,
        );
        assert!((s.vehicles[0].velocity_mps * 60000. - before).abs() < 1e-6);
        assert_eq!(s.vehicles[0].velocity_mps, s.vehicles[2].velocity_mps);
        assert_eq!(s.vehicles[3].velocity_mps, 6.);
    }
}
