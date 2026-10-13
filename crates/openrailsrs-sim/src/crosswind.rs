//! Rail constraints balance lateral force; a sprung body receives its roll moment.
//! No lateral weather force is subtracted from longitudinal traction or braking.
use openrailsrs_scenarios::vehicle_systems::CrosswindDef;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindRollState {
    pub lateral_force_n: f64,
    pub moment_nm: f64,
    pub angle_rad: f64,
    pub angular_velocity_rad_s: f64,
}
impl WindRollState {
    pub fn valid(&self) -> bool {
        [
            self.lateral_force_n,
            self.moment_nm,
            self.angle_rad,
            self.angular_velocity_rad_s,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.angle_rad.abs() <= 0.2
            && self.angular_velocity_rad_s.abs() <= 20.
    }
    pub fn advance(
        &mut self,
        p: &CrosswindDef,
        wind: [f64; 2],
        forward: [f64; 2],
        speed: f64,
        dt: f64,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let along = wind[0] * forward[0] + wind[1] * forward[1] - speed;
        let lateral = -wind[0] * forward[1] + wind[1] * forward[0];
        self.lateral_force_n =
            0.5 * 1.225 * p.area_m2 * p.lateral_coefficient * lateral * lateral.hypot(along);
        self.moment_nm = self.lateral_force_n * p.center_height_m;
        let equilibrium = self.moment_nm / p.roll_stiffness_nm_rad;
        let omega = (p.roll_stiffness_nm_rad / p.roll_inertia_kg_m2).sqrt();
        let zeta =
            p.roll_damping_nm_s / (2. * (p.roll_stiffness_nm_rad * p.roll_inertia_kg_m2).sqrt());
        let (y, v) = (self.angle_rad - equilibrium, self.angular_velocity_rad_s);
        let (next, velocity) = if (zeta - 1.).abs() < 1e-5 {
            let c = v + omega * y;
            let decay = (-omega * dt).exp();
            ((y + c * dt) * decay, (v - omega * c * dt) * decay)
        } else if zeta < 1. {
            let wd = omega * (1. - zeta * zeta).sqrt();
            let b = (v + zeta * omega * y) / wd;
            let (sin, cos) = (wd * dt).sin_cos();
            let decay = (-zeta * omega * dt).exp();
            let next = (y * cos + b * sin) * decay;
            (
                next,
                (-zeta * omega * (y * cos + b * sin) + wd * (-y * sin + b * cos)) * decay,
            )
        } else {
            let root = (zeta * zeta - 1.).sqrt();
            let r1 = -omega / (zeta + root);
            let r2 = -omega * (zeta + root);
            let a = (v - r2 * y) / (r1 - r2);
            let b = y - a;
            (
                a * (r1 * dt).exp() + b * (r2 * dt).exp(),
                a * r1 * (r1 * dt).exp() + b * r2 * (r2 * dt).exp(),
            )
        };
        self.angle_rad = (equilibrium + next).clamp(-0.2, 0.2);
        self.angular_velocity_rad_s = if (equilibrium + next).abs() > 0.2 {
            0.
        } else {
            velocity.clamp(-20., 20.)
        };
    }
}
pub fn advance(
    state: &mut crate::TrainSimState,
    path: &crate::path_data::PathData,
    lengths: &[f64],
    dt: f64,
) {
    let Some(input) = state.surface_conditions.as_ref().map(|s| s.weather) else {
        return;
    };
    let head = path.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    for car in &mut state.vehicle_systems {
        let i = car.definition.vehicle;
        let Some(length) = lengths.get(i) else {
            continue;
        };
        let center = head - lengths.iter().take(i).sum::<f64>() - length * 0.5;
        let Some(forward) = path.forward_at_chainage(center) else {
            continue;
        };
        if let Some(p) = &car.definition.crosswind {
            car.crosswind
                .advance(p, input.wind_mps, forward, state.velocity_mps, dt);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn params() -> CrosswindDef {
        CrosswindDef {
            area_m2: 60.,
            lateral_coefficient: 1.2,
            center_height_m: 2.,
            roll_stiffness_nm_rad: 2e6,
            roll_damping_nm_s: 200000.,
            roll_inertia_kg_m2: 80000.,
        }
    }
    #[test]
    fn analytic_sprung_response_matches_grouped_ticks_and_keeps_the_same_equilibrium() {
        let p = params();
        let mut a = WindRollState::default();
        let mut b = a.clone();
        for _ in 0..20 {
            a.advance(&p, [0., 25.], [1., 0.], 20., 1.);
        }
        for _ in 0..400 {
            b.advance(&p, [0., 25.], [1., 0.], 20., 0.05);
        }
        assert!((a.angle_rad - b.angle_rad).abs() < 1e-10);
        assert!((a.angle_rad - a.moment_nm / p.roll_stiffness_nm_rad).abs() < 1e-5);
        assert!(a.valid());
        let snapshot = a.clone();
        a.advance(&p, [0., 25.], [1., 0.], 20., 0.);
        assert_eq!(a, snapshot);
        let mut along = WindRollState::default();
        along.advance(&p, [25., 0.], [1., 0.], 20., 1.);
        assert_eq!(along.lateral_force_n, 0.);
        assert_eq!(along.angle_rad, 0.);
    }
}
