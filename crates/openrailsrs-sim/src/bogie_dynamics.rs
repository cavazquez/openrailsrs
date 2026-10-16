//! Mechanical bogie states. Rendering neither advances nor writes this module.
//!
//! `NativeLateralState` is the OR 1.6.1 quasi-static risk indication, including
//! its lead-car behaviour. `SupportState` is a separate, opt-in Rust lateral
//! spring/damper and hard-stop model. It adds no duplicate longitudinal curve
//! resistance and does not pretend to simulate an off-rail vehicle.
use openrailsrs_train::bogie_geometry::{LateralSuspension, VehicleBogieModel};
use serde::{Deserialize, Serialize};

const G: f64 = 9.80665;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NativeLateralState {
    pub vertical_force_n: f64,
    pub lateral_force_n: f64,
    pub derailment_coefficient: f64,
    pub nadal_coefficient: f64,
    pub angle_of_attack_rad: f64,
    pub climb_distance_m: f64,
    pub elapsed_s: f64,
    pub possible: bool,
    pub expected: bool,
    pub has_derailed: bool,
    pub buff_force_exceeded: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeLateralInput {
    pub mass_kg: f64,
    pub speed_mps: f64,
    pub radius_m: f64,
    pub cant_angle_rad: f64,
    pub friction: f64,
    pub ahead_force_n: Option<f64>,
    pub coupler_angle_rad: f64,
    pub buff: bool,
    /// Zero is supported by the original method's explicit fallback branch.
    pub axles: u32,
    pub bogies: u32,
}

impl NativeLateralState {
    /// Source port of TrainCar.UpdateTrainDerailmentRisk's lateral branch.
    /// Coupler angles are inputs: visual matrices/hose animation are excluded.
    pub fn advance(&mut self, p: &VehicleBogieModel, i: NativeLateralInput, dt: f64) {
        if i.radius_m == 0. {
            self.vertical_force_n = 0.;
            self.lateral_force_n = 0.;
            self.derailment_coefficient = 0.;
            self.expected = false;
            self.possible = false;
            self.elapsed_s = 0.;
            self.has_derailed = false;
        } else {
            let g = G as f32;
            let mass = i.mass_kg as f32;
            let radius = i.radius_m as f32;
            let speed = i.speed_mps as f32;
            let axles = i.axles as f32;
            let wheels = axles * 2.;
            let a = if wheels != 0. {
                mass / wheels * g
            } else {
                mass * g
            };
            let b1 = if axles != 0. { mass / axles } else { mass };
            let b2 = g * (i.cant_angle_rad as f32 as f64).sin() as f32;
            let b3 = (speed.abs() as f64).powi(2) as f32 / radius;
            let b4 = p.cog_height_m as f32 / p.gauge_m as f32;
            let vertical = a + b1 * (b3 - b2) * b4;
            if let Some(force) = i.ahead_force_n {
                let mut aa =
                    (force as f32).abs() * (i.coupler_angle_rad.abs() as f32 as f64).sin() as f32;
                if i.bogies != 0 {
                    aa /= i.bogies as f32;
                }
                self.lateral_force_n = (aa + b1 * (b3 - b2)).abs() as f64;
            }
            self.vertical_force_n = vertical as f64;
            self.derailment_coefficient = ((self.lateral_force_n as f32 / vertical)
                * if i.buff { 1.77f32 } else { 1.34f32 })
                as f64;
            let tangent = (p.flange_angle_rad as f32 as f64).tan() as f32;
            self.nadal_coefficient =
                ((tangent - i.friction as f32) / (1. + i.friction as f32 * tangent)) as f64;
            self.angle_of_attack_rad =
                ((2. * p.rigid_wheelbase_m as f32 / radius) as f64).asin() as f32 as f64;
            let attack_mrad = self.angle_of_attack_rad as f32 * 1000.;
            // Preserve Single/Double boundaries of the original empirical
            // climb-distance expression, rather than fitting captured values.
            let degrees = p.flange_angle_rad as f32 * 57.29578f32;
            let inches = p.flange_height_m as f32 * (1f32 / 0.0254f32);
            let a1 = (100f32 / (-1.9128f32 * degrees + 146.56f32) + 3.1f32) * inches;
            let a2 = 1.0
                / (-0.0092f32 as f64 * (degrees as f64).powi(2) + (1.2125f32 * degrees) as f64
                    - 39.031f32 as f64)
                + 1.23f32 as f64;
            let pa = a1 as f64 + a2;
            let b1 = (10f32 / (-21.157f32 * inches + 2.1052f32) + 0.05f32) * degrees;
            let b2 = 10f32 / (0.2688f32 * inches - 0.0266f32) - 5f32;
            let pb = b1 + b2;
            self.climb_distance_m = (((pa * pb as f64 * inches as f64)
                / (attack_mrad + pb * inches) as f64) as f32
                * 0.3048f32) as f64;
            self.possible = self.derailment_coefficient > self.nadal_coefficient;
            let derail_time = self.climb_distance_m as f32 / speed.abs();
            if self.possible && self.elapsed_s as f32 > derail_time {
                self.expected = true;
                self.has_derailed = true;
            } else if self.possible {
                self.elapsed_s = (self.elapsed_s as f32 + dt as f32) as f64;
            } else {
                self.elapsed_s = 0.;
                self.has_derailed = false;
            }
            if speed.abs() < 0.01 {
                self.expected = false;
                self.possible = false;
                self.elapsed_s = 0.;
                self.has_derailed = false;
            }
        }
        self.buff_force_exceeded = self.lateral_force_n > self.vertical_force_n;
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SupportState {
    pub lateral_offset_m: f64,
    pub lateral_velocity_mps: f64,
    pub lateral_acceleration_mps2: f64,
    pub requested_force_n: f64,
    pub rail_reaction_n: f64,
    pub hard_stop_force_n: f64,
    /// Rail sides in the body's formation frame; positive lateral load
    /// transfers normal force toward the right rail, also on reverse curves.
    pub left_wheel_load_n: f64,
    pub right_wheel_load_n: f64,
    pub angle_of_attack_rad: f64,
    pub travel_limit: bool,
    pub attack_limit: bool,
    pub wheel_unloaded: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VehicleBogieState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical: Option<crate::track_irregularity::VerticalVehicleState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derailment: Option<crate::derailment::DerailmentState>,
    pub supports: [SupportState; 2],
    pub native: NativeLateralState,
    /// Original asin(2*wheelbase/radius) has no real value on tighter curves.
    /// Preserve the last finite indication and explicitly flag that domain.
    pub native_domain_valid: bool,
}
impl VehicleBogieState {
    pub fn from_model(p: &VehicleBogieModel) -> Self {
        Self {
            vertical: p
                .vertical_suspension
                .as_ref()
                .map(crate::track_irregularity::VerticalVehicleState::new),
            derailment: p.derailment.as_ref().map(|_| Default::default()),
            ..Default::default()
        }
    }
    pub fn reverse(&mut self) {
        if let Some(v) = &mut self.vertical {
            v.reverse();
        }
        self.supports.swap(0, 1);
        for s in &mut self.supports {
            std::mem::swap(&mut s.left_wheel_load_n, &mut s.right_wheel_load_n);
            s.lateral_offset_m = -s.lateral_offset_m;
            s.lateral_velocity_mps = -s.lateral_velocity_mps;
            s.lateral_acceleration_mps2 = -s.lateral_acceleration_mps2;
            s.requested_force_n = -s.requested_force_n;
            s.rail_reaction_n = -s.rail_reaction_n;
            s.hard_stop_force_n = -s.hard_stop_force_n;
            s.angle_of_attack_rad = -s.angle_of_attack_rad;
        }
    }
    pub fn valid_for(&self, p: &VehicleBogieModel) -> bool {
        let finite = [
            self.native.vertical_force_n,
            self.native.lateral_force_n,
            self.native.derailment_coefficient,
            self.native.nadal_coefficient,
            self.native.angle_of_attack_rad,
            self.native.climb_distance_m,
            self.native.elapsed_s,
        ]
        .iter()
        .all(|x| x.is_finite());
        let optional = match (&self.vertical, &p.vertical_suspension) {
            (Some(s), Some(p)) => s.valid_for(p),
            (None, None) => true,
            _ => false,
        } && match (&self.derailment, &p.derailment) {
            (Some(s), Some(_)) => s.valid(),
            (None, None) => true,
            _ => false,
        };
        finite
            && optional
            && self.native.elapsed_s >= 0.
            && self.supports.iter().all(|s| {
                [
                    s.lateral_offset_m,
                    s.lateral_velocity_mps,
                    s.lateral_acceleration_mps2,
                    s.requested_force_n,
                    s.rail_reaction_n,
                    s.hard_stop_force_n,
                    s.left_wheel_load_n,
                    s.right_wheel_load_n,
                    s.angle_of_attack_rad,
                ]
                .iter()
                .all(|x| x.is_finite())
                    && s.left_wheel_load_n >= 0.
                    && s.right_wheel_load_n >= 0.
                    && s.lateral_offset_m.abs()
                        <= p.suspension.as_ref().map_or(0., |p| p.travel_m) + 1e-9
                    && s.lateral_velocity_mps.abs() <= 100.
            })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SupportInput {
    pub mass_kg: f64,
    pub lateral_acceleration_mps2: f64,
    pub coupler_lateral_n: f64,
    pub curvature_per_m: f64,
}

/// Implicit spring/damper step, bounded by a unilateral hard stop. This is an
/// explicit Rust extension. The reaction is lateral; existing longitudinal
/// curve resistance remains solely in native_dynamics.
pub fn advance_support(s: &mut SupportState, p: &VehicleBogieModel, i: SupportInput, dt: f64) {
    if dt <= 0. || !dt.is_finite() || i.mass_kg <= 0. {
        return;
    }
    let force = i.mass_kg * i.lateral_acceleration_mps2 + i.coupler_lateral_n;
    s.lateral_acceleration_mps2 = i.lateral_acceleration_mps2;
    s.requested_force_n = force;
    s.angle_of_attack_rad = (2. * p.rigid_wheelbase_m * i.curvature_per_m)
        .clamp(-1., 1.)
        .asin();
    s.hard_stop_force_n = 0.;
    s.travel_limit = false;
    if let Some(c) = &p.suspension {
        let velocity = (s.lateral_velocity_mps
            + dt * (force - c.stiffness_npm * s.lateral_offset_m) / i.mass_kg)
            / (1. + dt * c.damping_nspm / i.mass_kg + dt * dt * c.stiffness_npm / i.mass_kg);
        let position = s.lateral_offset_m + dt * velocity;
        s.lateral_offset_m = position.clamp(-c.travel_m, c.travel_m);
        s.lateral_velocity_mps = velocity;
        if position.abs() > c.travel_m {
            s.travel_limit = true;
            s.lateral_velocity_mps = 0.;
            s.hard_stop_force_n =
                (force.abs() - c.stiffness_npm * c.travel_m).max(0.) * position.signum();
        }
        s.rail_reaction_n = c.stiffness_npm * s.lateral_offset_m
            + c.damping_nspm * s.lateral_velocity_mps
            + s.hard_stop_force_n;
        s.attack_limit = s.angle_of_attack_rad.abs() > c.maximum_attack_rad;
    } else {
        s.lateral_offset_m = 0.;
        s.lateral_velocity_mps = 0.;
        s.rail_reaction_n = force;
        s.attack_limit = false;
    }
    let transfer = force * p.cog_height_m / p.gauge_m;
    let static_half = i.mass_kg * G / 2.;
    let supported_transfer = transfer.clamp(-static_half, static_half);
    s.left_wheel_load_n = static_half - supported_transfer;
    s.right_wheel_load_n = static_half + supported_transfer;
    s.wheel_unloaded = transfer.abs() >= static_half;
}

/// One physical shared support: combine both bodies' masses and lateral load,
/// advance once, then allocate reaction/load by supported mass. Both body
/// references retain identical displacement, never two independent wheelsets.
pub fn advance_shared(
    a: &mut SupportState,
    b: &mut SupportState,
    pa: &VehicleBogieModel,
    pb: &VehicleBogieModel,
    ia: SupportInput,
    ib: SupportInput,
    dt: f64,
) {
    let mass = ia.mass_kg + ib.mass_kg;
    if mass <= 0. {
        return;
    }
    let mut p = pa.clone();
    p.cog_height_m = (pa.cog_height_m * ia.mass_kg + pb.cog_height_m * ib.mass_kg) / mass;
    p.gauge_m = pa.gauge_m;
    p.rigid_wheelbase_m = pa.rigid_wheelbase_m.max(pb.rigid_wheelbase_m);
    p.suspension = match (&pa.suspension, &pb.suspension) {
        (Some(a), Some(b)) => Some(LateralSuspension {
            stiffness_npm: a.stiffness_npm + b.stiffness_npm,
            damping_nspm: a.damping_nspm + b.damping_nspm,
            travel_m: a.travel_m.min(b.travel_m),
            maximum_attack_rad: a.maximum_attack_rad.min(b.maximum_attack_rad),
        }),
        _ => None,
    };
    let input = SupportInput {
        mass_kg: mass,
        lateral_acceleration_mps2: (ia.mass_kg * ia.lateral_acceleration_mps2
            + ib.mass_kg * ib.lateral_acceleration_mps2)
            / mass,
        coupler_lateral_n: ia.coupler_lateral_n + ib.coupler_lateral_n,
        curvature_per_m: (ia.curvature_per_m * ia.mass_kg + ib.curvature_per_m * ib.mass_kg) / mass,
    };
    advance_support(a, &p, input, dt);
    *b = a.clone();
    for (s, fraction) in [(a, ia.mass_kg / mass), (b, ib.mass_kg / mass)] {
        s.requested_force_n *= fraction;
        s.rail_reaction_n *= fraction;
        s.hard_stop_force_n *= fraction;
        s.left_wheel_load_n *= fraction;
        s.right_wheel_load_n *= fraction;
    }
}

pub fn apply_overrides(
    models: &mut [VehicleBogieModel],
    definitions: &[openrailsrs_scenarios::bogie_dynamics::BogieDynamicsDef],
) -> Result<Vec<[usize; 2]>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut joints = vec![];
    for d in definitions {
        if !d.valid() || !seen.insert(d.vehicle) || d.vehicle >= models.len() {
            return Err("Configuración mecánica de bogies inválida o duplicada".into());
        }
        let p = &mut models[d.vehicle];
        if let Some(offsets) = d.support_offsets_m {
            p.support_offsets_m = offsets;
        }
        if let Some(s) = &d.lateral_suspension {
            p.suspension = Some(LateralSuspension {
                stiffness_npm: s.stiffness_npm,
                damping_nspm: s.damping_nspm,
                travel_m: s.travel_m,
                maximum_attack_rad: s.maximum_attack_rad,
            });
        }
        if let Some(v) = &d.vertical_suspension {
            p.vertical_suspension = Some(openrailsrs_train::bogie_geometry::VerticalSuspension {
                enabled: v.enabled,
                sprung_mass_fraction: v.sprung_mass_fraction,
                spring_n_m: v.spring_n_m,
                damper_ns_m: v.damper_ns_m,
                contact_spring_n_m: v.contact_spring_n_m,
                contact_damper_ns_m: v.contact_damper_ns_m,
                travel_m: v.travel_m,
                bump_stop_n_m: v.bump_stop_n_m,
                sections: v
                    .sections
                    .iter()
                    .map(|s| openrailsrs_train::bogie_geometry::IrregularitySection {
                        start_chainage_m: s.start_chainage_m,
                        end_chainage_m: s.end_chainage_m,
                        transition_m: s.transition_m,
                        seed: s.seed,
                        spectrum: s.spectrum.clone(),
                    })
                    .collect(),
            });
        }
        if let Some(v) = &d.derailment {
            p.derailment = Some(openrailsrs_train::bogie_geometry::DerailmentLimits {
                enabled: v.enabled,
                minimum_speed_mps: v.minimum_speed_mps,
                nadal_multiplier: v.nadal_multiplier,
                unloading_fraction: v.unloading_fraction,
                persistence_s: v.persistence_s,
                offrail_drag_mu: v.offrail_drag_mu,
            });
        }
        if !p.valid() {
            return Err("Parámetros mecánicos de bogie inválidos".into());
        }
        if let Some(other) = d.shared_with {
            if other >= models.len() {
                return Err("El apoyo compartido requiere dos cuerpos existentes".into());
            }
            let joint = [d.vehicle.min(other), d.vehicle.max(other)];
            if !joints.contains(&joint) {
                joints.push(joint);
            }
        }
    }
    for &[a, b] in &joints {
        if models[a].vertical_suspension != models[b].vertical_suspension {
            return Err(
                "Un apoyo compartido requiere el mismo modelo vertical y espectro de vía".into(),
            );
        }
        if (models[a].gauge_m - models[b].gauge_m).abs() > 1e-6 {
            return Err("Un apoyo compartido requiere la misma trocha en ambos cuerpos".into());
        }
        let separation = (models[a].body_length_m + models[b].body_length_m) / 2.;
        if (models[a].support_offsets_m[1] + separation - models[b].support_offsets_m[0]).abs()
            > 0.001
        {
            return Err(
                "Las posiciones de ambos cuerpos deben describir el mismo apoyo compartido".into(),
            );
        }
    }
    Ok(joints)
}

/// Called by the physics tick, once per simulation step. Supports are sampled
/// independently, so a transition or reverse curve can load only one bogie.
pub fn advance_train(
    state: &mut crate::state::TrainSimState,
    train: &crate::physics::TrainPhysics,
    path: &crate::path_data::PathData,
    centers: &[f64],
    dt: f64,
) {
    let models = &train.bogies;
    if models.is_empty() || models.len() != centers.len() {
        return;
    }
    state.bogies.resize_with(models.len(), Default::default);
    let direction = if state.path_reversed { -1. } else { 1. };
    let mut inputs = vec![[SupportInput::default(); 2]; models.len()];
    for (index, (p, &center)) in models.iter().zip(centers).enumerate() {
        let speed = state
            .vehicles
            .get(index)
            .map_or(state.velocity_mps, |v| v.velocity_mps);
        let base_mass = state
            .vehicle_masses
            .get(index)
            .copied()
            .unwrap_or(p.mass_kg);
        let mass = crate::consumable_mass::vehicle_mass_kg(train, state, index, base_mass)
            + state.extra_mass_kg / models.len() as f64;
        let span = p.support_offsets_m[0] - p.support_offsets_m[1];
        let fractions = [
            -p.support_offsets_m[1] / span,
            p.support_offsets_m[0] / span,
        ];
        let mut radius = [0.; 2];
        let mut cant = [0.; 2];
        for support in 0..2 {
            let at = center + p.support_offsets_m[support] * direction;
            let (curvature, visual_roll) = path.curve_at_chainage(at);
            cant[support] = path
                .physical_cant_at_chainage(at)
                .unwrap_or(visual_roll.sin().abs() * p.gauge_m);
            let signed_cant = cant[support] * curvature.signum();
            let lateral = (speed * speed * curvature - G * signed_cant / p.gauge_m) * direction;
            radius[support] = if curvature.abs() > 1e-12 {
                1. / curvature.abs()
            } else {
                0.
            };
            inputs[index][support] = SupportInput {
                mass_kg: mass * fractions[support],
                lateral_acceleration_mps2: lateral,
                coupler_lateral_n: 0.,
                curvature_per_m: curvature * direction,
            };
        }
        let radius = if radius.contains(&0.) {
            0.
        } else {
            (radius[0] + radius[1]) / 2.
        };
        let ahead = index.checked_sub(1).and_then(|j| state.couplers.get(j));
        let native_force = ahead.map(|c| c.force_n(0.).abs());
        let own_coupler = state.couplers.get(index);
        let buff = own_coupler.is_some_and(|c| c.force_n(0.) < 0. && c.extension_m < 0.);
        // Geometry supplies a force direction; the renderer's clamped yaw does
        // not enter it. The dedicated native fixture prescribes this angle.
        let angle = if radius > 0. {
            train.vehicle_lengths_m[index] / (2. * radius)
        } else {
            0.
        };
        let coupler_lateral = native_force.unwrap_or(0.) * angle.sin();
        inputs[index][0].coupler_lateral_n =
            coupler_lateral * inputs[index][0].curvature_per_m.signum();
        state.bogies[index].native_domain_valid =
            radius == 0. || radius >= 2. * p.rigid_wheelbase_m;
        if state.bogies[index].native_domain_valid {
            state.bogies[index].native.advance(
                p,
                NativeLateralInput {
                    mass_kg: mass,
                    speed_mps: speed,
                    radius_m: radius,
                    cant_angle_rad: (((cant[0] + cant[1]) / 2.) / p.gauge_m)
                        .clamp(0., 1.)
                        .asin(),
                    friction: state
                        .rail_adhesion
                        .as_ref()
                        .map_or(0.35, |a| 0.35 * a.weather_factor),
                    ahead_force_n: native_force,
                    coupler_angle_rad: angle,
                    buff,
                    axles: p.axles,
                    bogies: p.bogies,
                },
                dt,
            );
        }
    }
    let mut shared = std::collections::BTreeSet::new();
    for &[a, b] in &state.articulated_joints {
        if a >= b || b >= models.len() {
            continue;
        }
        let (left, right) = state.bogies.split_at_mut(b);
        advance_shared(
            &mut left[a].supports[1],
            &mut right[0].supports[0],
            &models[a],
            &models[b],
            inputs[a][1],
            inputs[b][0],
            dt,
        );
        shared.insert((a, 1));
        shared.insert((b, 0));
    }
    for (index, model) in models.iter().enumerate() {
        for (support, &input) in inputs[index].iter().enumerate() {
            if !shared.contains(&(index, support)) {
                advance_support(&mut state.bogies[index].supports[support], model, input, dt);
            }
        }
    }
    crate::track_irregularity::advance_train(state, train, path, centers, dt);
    let now = state.time_s();
    let friction = state
        .rail_adhesion
        .as_ref()
        .map_or(0.35, |s| 0.35 * s.weather_factor);
    for (i, p) in models.iter().enumerate() {
        if p.derailment.is_none() {
            continue;
        }
        let speed = state
            .vehicles
            .get(i)
            .map_or(state.velocity_mps, |v| v.velocity_mps);
        let s = &mut state.bogies[i];
        crate::derailment::advance(
            s.derailment.get_or_insert_with(Default::default),
            p,
            &s.supports,
            &s.native,
            speed,
            now,
            if state.path_reversed {
                path.total_length_m() - centers[i]
            } else {
                centers[i]
            },
            friction,
            dt,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> VehicleBogieModel {
        let mut p = VehicleBogieModel::for_length(20.);
        p.suspension = Some(LateralSuspension {
            stiffness_npm: 1_000_000.,
            damping_nspm: 100_000.,
            travel_m: 0.03,
            maximum_attack_rad: 0.04,
        });
        p
    }
    #[test]
    fn equilibrium_forces_and_hard_stop_obey_each_models_limits() {
        let p = model();
        let mut s = SupportState::default();
        let i = SupportInput {
            mass_kg: 20_000.,
            lateral_acceleration_mps2: 1.,
            curvature_per_m: 1. / 300.,
            ..Default::default()
        };
        for _ in 0..1000 {
            advance_support(&mut s, &p, i, 0.01);
        }
        assert!((s.lateral_offset_m - 0.02).abs() < 1e-9);
        assert!((s.rail_reaction_n - 20_000.).abs() < 0.01);
        assert!(!s.travel_limit);
        for _ in 0..100 {
            advance_support(
                &mut s,
                &p,
                SupportInput {
                    lateral_acceleration_mps2: 4.,
                    curvature_per_m: 1. / 60.,
                    ..i
                },
                0.01,
            );
        }
        assert_eq!(s.lateral_offset_m, 0.03);
        assert!(s.travel_limit && s.attack_limit && s.wheel_unloaded);
        assert!((s.rail_reaction_n - 80_000.).abs() < 0.01);
        assert!((s.left_wheel_load_n + s.right_wheel_load_n - i.mass_kg * G).abs() < 1e-8);
    }
    #[test]
    fn zero_centre_of_gravity_retains_finite_static_wheel_loads_on_a_curve() {
        let mut p = model();
        p.cog_height_m = 0.;
        assert!(p.valid());
        let mut support = SupportState::default();
        advance_support(
            &mut support,
            &p,
            SupportInput {
                mass_kg: 20_000.,
                lateral_acceleration_mps2: 4.,
                curvature_per_m: 1. / 100.,
                ..Default::default()
            },
            0.05,
        );
        assert_eq!(support.left_wheel_load_n, 20_000. * G / 2.);
        assert_eq!(support.right_wheel_load_n, support.left_wheel_load_n);
        assert!(!support.wheel_unloaded);
        assert!(support.rail_reaction_n.is_finite());
        let mut native = NativeLateralState::default();
        native.advance(
            &p,
            NativeLateralInput {
                mass_kg: 40_000.,
                speed_mps: 20.,
                radius_m: 100.,
                cant_angle_rad: 0.03,
                friction: 0.3,
                ahead_force_n: Some(0.),
                coupler_angle_rad: 0.,
                buff: false,
                axles: 4,
                bogies: 2,
            },
            0.05,
        );
        assert_eq!(native.vertical_force_n, (40_000_f32 / 8. * G as f32) as f64);
        assert!(native.derailment_coefficient.is_finite());
    }
    #[test]
    fn shared_support_counts_load_once_and_retains_one_displacement() {
        let p = model();
        let mut a = SupportState::default();
        let mut b = SupportState::default();
        let ia = SupportInput {
            mass_kg: 10_000.,
            lateral_acceleration_mps2: 1.,
            ..Default::default()
        };
        let ib = SupportInput {
            mass_kg: 20_000.,
            lateral_acceleration_mps2: 1.,
            ..Default::default()
        };
        for _ in 0..1000 {
            advance_shared(&mut a, &mut b, &p, &p, ia, ib, 0.01);
        }
        assert_eq!(a.lateral_offset_m, b.lateral_offset_m);
        assert!((a.lateral_offset_m - 0.015).abs() < 1e-9);
        assert!((a.rail_reaction_n + b.rail_reaction_n - 30_000.).abs() < 0.01);
        assert!(
            (a.left_wheel_load_n
                + a.right_wheel_load_n
                + b.left_wheel_load_n
                + b.right_wheel_load_n
                - 30_000. * G)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn reverse_curves_change_lateral_sign_and_body_reversal_swaps_rails() {
        let p = model();
        let i = SupportInput {
            mass_kg: 20_000.,
            lateral_acceleration_mps2: 0.6,
            curvature_per_m: 1. / 180.,
            ..Default::default()
        };
        let mut left = SupportState::default();
        let mut right = SupportState::default();
        for _ in 0..200 {
            advance_support(&mut left, &p, i, 0.05);
            advance_support(
                &mut right,
                &p,
                SupportInput {
                    lateral_acceleration_mps2: -i.lateral_acceleration_mps2,
                    curvature_per_m: -i.curvature_per_m,
                    ..i
                },
                0.05,
            );
        }
        assert_eq!(left.lateral_offset_m, -right.lateral_offset_m);
        assert_eq!(left.rail_reaction_n, -right.rail_reaction_n);
        assert_eq!(left.left_wheel_load_n, right.right_wheel_load_n);
        let mut state = VehicleBogieState {
            supports: [left.clone(), right.clone()],
            ..Default::default()
        };
        let original = state.clone();
        state.reverse();
        assert_eq!(
            state.supports[0].left_wheel_load_n,
            right.right_wheel_load_n
        );
        state.reverse();
        assert_eq!(state, original);
    }

    #[test]
    fn shared_support_requires_same_gauge_and_coincident_geometry() {
        use openrailsrs_scenarios::bogie_dynamics::BogieDynamicsDef;
        let mut models = vec![model(), model()];
        let def = BogieDynamicsDef {
            vehicle: 0,
            support_offsets_m: Some([10., -10.]),
            lateral_suspension: None,
            shared_with: Some(1),
            vertical_suspension: None,
            derailment: None,
        };
        assert!(apply_overrides(&mut models, std::slice::from_ref(&def)).is_err());
        models[1].support_offsets_m = [10., -10.];
        assert_eq!(
            apply_overrides(&mut models, std::slice::from_ref(&def)).unwrap(),
            vec![[0, 1]]
        );
        models[1].gauge_m = 1.;
        assert!(apply_overrides(&mut models, &[def]).is_err());
    }
}
