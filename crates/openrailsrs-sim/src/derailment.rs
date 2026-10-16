//! Optional, persistent derailment consequence. The existing native risk
//! indication stays unchanged; sustained contact/Nadal failures and off-rail
//! drag are explicitly Rust extensions. No terrain collision is simulated.
use openrailsrs_train::bogie_geometry::{DerailmentLimits, VehicleBogieModel};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerailmentCause {
    NativeFlangeClimb,
    SustainedNadalExcess,
    WheelUnloading,
}
impl DerailmentCause {
    pub fn label(self) -> &'static str {
        match self {
            Self::NativeFlangeClimb => "Trepado de pestaña (riesgo nativo)",
            Self::SustainedNadalExcess => "Fuerza lateral sostenida sobre el límite de Nadal",
            Self::WheelUnloading => "Descarga de rueda sostenida",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DerailmentState {
    pub enabled_override: Option<bool>,
    pub cause: Option<DerailmentCause>,
    pub event_time_s: Option<f64>,
    pub event_chainage_m: Option<f64>,
    pub event_speed_mps: f64,
    pub lateral_force_n: f64,
    pub minimum_wheel_load_n: f64,
    pub nadal_threshold: f64,
    pub lateral_ratio: f64,
    pub nadal_elapsed_s: f64,
    pub unloading_elapsed_s: f64,
    pub offrail_distance_m: f64,
}
impl DerailmentState {
    pub fn enabled(&self, p: &DerailmentLimits) -> bool {
        self.enabled_override.unwrap_or(p.enabled)
    }
    pub fn active(&self, p: &DerailmentLimits) -> bool {
        self.enabled(p) && self.cause.is_some()
    }
    pub fn valid(&self) -> bool {
        [
            self.event_speed_mps,
            self.lateral_force_n,
            self.minimum_wheel_load_n,
            self.nadal_threshold,
            self.lateral_ratio,
            self.nadal_elapsed_s,
            self.unloading_elapsed_s,
            self.offrail_distance_m,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.)
            && self.event_time_s.is_none_or(|v| v.is_finite() && v >= 0.)
            && self.event_chainage_m.is_none_or(|v| v.is_finite())
            && (self.cause.is_some() == self.event_time_s.is_some())
            && (self.cause.is_some() == self.event_chainage_m.is_some())
    }
}
#[allow(clippy::too_many_arguments)]
pub fn advance(
    s: &mut DerailmentState,
    p: &VehicleBogieModel,
    supports: &[crate::bogie_dynamics::SupportState; 2],
    native: &crate::bogie_dynamics::NativeLateralState,
    speed_mps: f64,
    time_s: f64,
    chainage_m: f64,
    friction: f64,
    dt: f64,
) {
    let Some(limits) = &p.derailment else { return };
    if !dt.is_finite() || dt <= 0. || !s.enabled(limits) {
        return;
    }
    if s.cause.is_some() {
        s.offrail_distance_m += speed_mps.abs() * dt;
        return;
    }
    let tangent = p.flange_angle_rad.tan();
    let nadal =
        ((tangent - friction) / (1. + friction * tangent)).max(0.) * limits.nadal_multiplier;
    let mut worst = (0., 0., 0.);
    let mut minimum_wheel_load_n = f64::INFINITY;
    let mut unloaded = false;
    for support in supports {
        let normal = support.left_wheel_load_n.min(support.right_wheel_load_n);
        minimum_wheel_load_n = minimum_wheel_load_n.min(normal);
        let reference = (support.left_wheel_load_n + support.right_wheel_load_n).max(1.) / 2.;
        let lateral = support.rail_reaction_n.abs();
        let ratio = lateral / (2. * normal).max(1.);
        if ratio > worst.0 {
            worst = (ratio, lateral, normal);
        }
        unloaded |= normal <= reference * limits.unloading_fraction || support.wheel_unloaded;
    }
    if speed_mps.abs() < limits.minimum_speed_mps {
        s.nadal_elapsed_s = 0.;
        s.unloading_elapsed_s = 0.;
        return;
    }
    s.nadal_elapsed_s = if worst.0 > nadal {
        s.nadal_elapsed_s + dt
    } else {
        0.
    };
    s.unloading_elapsed_s = if unloaded {
        s.unloading_elapsed_s + dt
    } else {
        0.
    };
    let cause = if native.possible && native.has_derailed {
        Some(DerailmentCause::NativeFlangeClimb)
    } else if s.unloading_elapsed_s >= limits.persistence_s {
        Some(DerailmentCause::WheelUnloading)
    } else if s.nadal_elapsed_s >= limits.persistence_s {
        Some(DerailmentCause::SustainedNadalExcess)
    } else {
        None
    };
    if let Some(cause) = cause {
        s.cause = Some(cause);
        s.event_time_s = Some(time_s);
        s.event_chainage_m = Some(chainage_m);
        s.event_speed_mps = speed_mps.abs();
        s.lateral_force_n = worst.1;
        s.minimum_wheel_load_n = minimum_wheel_load_n;
        s.nadal_threshold = nadal;
        s.lateral_ratio = worst.0;
    }
}
pub fn any_active(state: &crate::TrainSimState, train: &crate::physics::TrainPhysics) -> bool {
    state.bogies.iter().zip(&train.bogies).any(|(s, p)| {
        s.derailment
            .as_ref()
            .zip(p.derailment.as_ref())
            .is_some_and(|(s, p)| s.active(p))
    })
}
pub fn drag_forces(state: &crate::TrainSimState, train: &crate::physics::TrainPhysics) -> Vec<f64> {
    train
        .bogies
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if state
                .bogies
                .get(i)
                .and_then(|s| s.derailment.as_ref())
                .zip(p.derailment.as_ref())
                .is_some_and(|(s, p)| s.active(p))
            {
                let mass = state.vehicle_masses.get(i).copied().unwrap_or(p.mass_kg);
                crate::consumable_mass::vehicle_mass_kg(train, state, i, mass)
                    * 9.80665
                    * p.derailment.as_ref().unwrap().offrail_drag_mu
            } else {
                0.
            }
        })
        .collect()
}
impl crate::LiveDriveSession {
    /// Disabling consequences preserves the recorded cause. Re-enabling a
    /// derailed saved train cannot silently erase its failure history.
    pub fn set_derailment_enabled(&mut self, enabled: bool) {
        for (s, p) in self.state.bogies.iter_mut().zip(&self.physics.bogies) {
            if p.derailment.is_some() {
                s.derailment
                    .get_or_insert_with(Default::default)
                    .enabled_override = Some(enabled);
            }
        }
    }
    pub fn derailment_events(&self) -> Vec<(usize, &DerailmentState)> {
        self.state
            .bogies
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.derailment
                    .as_ref()
                    .filter(|s| s.cause.is_some())
                    .map(|s| (i, s))
            })
            .collect()
    }
    pub fn set_vertical_irregularities_enabled(&mut self, enabled: bool) {
        for (s, p) in self.state.bogies.iter_mut().zip(&self.physics.bogies) {
            if let Some(model) = &p.vertical_suspension {
                s.vertical
                    .get_or_insert_with(|| {
                        crate::track_irregularity::VerticalVehicleState::new(model)
                    })
                    .enabled_override = Some(enabled);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bogie_dynamics::{NativeLateralState, SupportState};

    #[test]
    fn unloading_event_reports_the_minimum_across_both_supports() {
        let mut model = VehicleBogieModel::for_length(20.);
        model.derailment = Some(DerailmentLimits {
            enabled: true,
            minimum_speed_mps: 1.,
            nadal_multiplier: 1.,
            unloading_fraction: 0.1,
            persistence_s: 0.1,
            offrail_drag_mu: 0.3,
        });
        let unloaded = SupportState {
            left_wheel_load_n: 0.,
            right_wheel_load_n: 200.,
            rail_reaction_n: 0.,
            wheel_unloaded: true,
            ..Default::default()
        };
        let lateral = SupportState {
            left_wheel_load_n: 100.,
            right_wheel_load_n: 100.,
            rail_reaction_n: 100.,
            ..Default::default()
        };
        for supports in [
            [unloaded.clone(), lateral.clone()],
            [lateral.clone(), unloaded.clone()],
        ] {
            let mut state = DerailmentState::default();
            advance(
                &mut state,
                &model,
                &supports,
                &NativeLateralState::default(),
                2.,
                0.1,
                30.,
                0.2,
                0.1,
            );
            assert_eq!(state.cause, Some(DerailmentCause::WheelUnloading));
            assert_eq!(state.minimum_wheel_load_n, 0.);
            // The separate maximum Nadal-ratio diagnostic is unchanged.
            assert_eq!(state.lateral_force_n, 100.);
            assert_eq!(state.lateral_ratio, 0.5);
            assert!(state.valid());
            let restored: DerailmentState =
                serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
            assert_eq!(restored, state);
        }
    }
}
