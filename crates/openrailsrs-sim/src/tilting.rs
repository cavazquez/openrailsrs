//! Physical body tilt; independent of camera comfort and render frame rate.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TiltState {
    pub angle_rad: f64,
    pub demanded_rad: f64,
}
impl TiltState {
    pub fn valid(&self) -> bool {
        [self.angle_rad, self.demanded_rad]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 0.2)
    }
    pub fn advance(
        &mut self,
        p: &openrailsrs_scenarios::vehicle_systems::TiltDef,
        curvature: f64,
        cant: f64,
        speed: f64,
        dt: f64,
    ) {
        if dt <= 0. {
            return;
        }
        self.demanded_rad = if speed > p.minimum_speed_mps {
            ((speed * speed * curvature.abs() / 9.80665).min(0.99).asin() - cant.abs())
                .max(0.)
                .min(p.maximum_rad)
                * p.compensation
                * curvature.signum()
        } else {
            0.
        };
        if p.native_response {
            let speed = speed as f32;
            let radius = (1. / curvature.abs()) as f32;
            let demand = if speed > 50_f32 / 3.6 && curvature != 0. {
                ((speed * speed / (9.80665_f32 * radius)).min(0.99).asin() - (cant.abs() as f32))
                    .clamp(0., 8_f32.to_radians())
                    * (curvature.signum() as f32)
            } else {
                0.
            };
            let angle = self.angle_rad as f32;
            self.angle_rad = f64::from(angle + (demand - angle) * (dt as f32));
            self.demanded_rad = f64::from(demand);
            return;
        }
        let delta = (self.demanded_rad - self.angle_rad) * (1. - (-dt / p.time_constant_s).exp());
        self.angle_rad += (delta).clamp(-p.max_rate_rad_s * dt, p.max_rate_rad_s * dt);
    }
}
pub fn advance(
    state: &mut crate::TrainSimState,
    path: &crate::path_data::PathData,
    lengths: &[f64],
    dt: f64,
) {
    let head = path.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
    for c in &mut state.vehicle_systems {
        let i = c.definition.vehicle;
        let Some(length) = lengths.get(i) else {
            continue;
        };
        let Some(p) = &c.definition.tilt else {
            continue;
        };
        let center = head - lengths.iter().take(i).sum::<f64>() - length * 0.5;
        let (curvature, cant) = path.curve_at_chainage(center);
        c.tilt
            .advance(p, curvature, cant, state.velocity_mps.abs(), dt);
    }
}
