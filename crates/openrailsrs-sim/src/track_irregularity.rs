//! Deterministic two-rail roughness and unilateral vertical contact. This is a
//! Rust extension; OR camera vibration is deliberately not a force reference.
use openrailsrs_train::bogie_geometry::{IrregularitySection, VerticalSuspension};
use serde::{Deserialize, Serialize};
const G: f64 = 9.80665;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuarterCar {
    pub body_z_m: f64,
    pub body_velocity_mps: f64,
    pub axle_z_m: f64,
    pub axle_velocity_mps: f64,
    pub normal_force_n: f64,
    pub reference_load_n: f64,
    pub track_height_m: f64,
    pub suspension_force_n: f64,
    pub longitudinal_force_n: f64,
    pub bump_force_n: f64,
    pub contact_lost: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VerticalSupportState {
    pub wheels: [QuarterCar; 2],
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VerticalVehicleState {
    pub supports: [VerticalSupportState; 2],
    pub enabled_override: Option<bool>,
    pub configured_enabled: bool,
}
fn hash(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
/// Height and d(height)/d(chainage). A smooth envelope prevents a new section
/// boundary from injecting an unbounded velocity/impulse. No RNG state or
/// frame/render order participates in this function.
pub fn sample(sections: &[IrregularitySection], chainage: f64, right_rail: bool) -> (f64, f64) {
    let Some(s) = sections
        .iter()
        .find(|s| chainage >= s.start_chainage_m && chainage <= s.end_chainage_m)
    else {
        return (0., 0.);
    };
    let from_start = chainage - s.start_chainage_m;
    let to_end = s.end_chainage_m - chainage;
    let edge = from_start.min(to_end);
    let (window, derivative) = if edge < s.transition_m {
        let angle = std::f64::consts::PI * edge / s.transition_m;
        (
            0.5 - 0.5 * angle.cos(),
            0.5 * std::f64::consts::PI / s.transition_m
                * angle.sin()
                * if from_start < to_end { 1. } else { -1. },
        )
    } else {
        (1., 0.)
    };
    let mut height = 0.;
    let mut gradient = 0.;
    for (i, &[wavelength, amplitude]) in s.spectrum.iter().enumerate() {
        let key = hash(
            s.seed
                ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15)
                ^ if right_rail { 0xa0761d6478bd642f } else { 0 },
        );
        let phase = (key >> 11) as f64 / ((1_u64 << 53) as f64) * std::f64::consts::TAU;
        let k = std::f64::consts::TAU / wavelength;
        let angle = k * from_start + phase;
        height += amplitude * angle.sin();
        gradient += amplitude * k * angle.cos();
    }
    (height * window, gradient * window + height * derivative)
}
impl VerticalVehicleState {
    pub fn new(p: &VerticalSuspension) -> Self {
        Self {
            configured_enabled: p.enabled,
            ..Default::default()
        }
    }
    pub fn enabled(&self, p: &VerticalSuspension) -> bool {
        self.enabled_override.unwrap_or(p.enabled)
    }
    pub fn reverse(&mut self) {
        self.supports.swap(0, 1);
        for s in &mut self.supports {
            s.wheels.swap(0, 1);
        }
    }
    pub fn valid_for(&self, p: &VerticalSuspension) -> bool {
        self.configured_enabled == p.enabled
            && self.supports.iter().flat_map(|s| &s.wheels).all(|s| {
                [
                    s.body_z_m,
                    s.body_velocity_mps,
                    s.axle_z_m,
                    s.axle_velocity_mps,
                    s.normal_force_n,
                    s.reference_load_n,
                    s.track_height_m,
                    s.suspension_force_n,
                    s.bump_force_n,
                    s.longitudinal_force_n,
                ]
                .iter()
                .all(|v| v.is_finite())
                    && s.normal_force_n >= 0.
                    && s.reference_load_n >= 0.
                    && s.body_z_m.abs() < 100.
                    && s.axle_z_m.abs() < 100.
                    && s.body_velocity_mps.abs() < 1000.
                    && s.axle_velocity_mps.abs() < 1000.
            })
    }
    pub fn normal_factor(&self) -> f64 {
        let load = self
            .supports
            .iter()
            .flat_map(|s| &s.wheels)
            .map(|s| s.normal_force_n)
            .sum::<f64>();
        let static_load = self
            .supports
            .iter()
            .flat_map(|s| &s.wheels)
            .map(|s| s.reference_load_n)
            .sum::<f64>();
        if static_load > 0. {
            (load / static_load).clamp(0., 5.)
        } else {
            1.
        }
    }
}
fn solve_wheel(
    s: &mut QuarterCar,
    p: &VerticalSuspension,
    mass: f64,
    road: f64,
    road_velocity: f64,
    dt: f64,
) {
    let sprung = (mass * p.sprung_mass_fraction).max(1.);
    let unsprung = (mass - sprung).max(1.);
    let delta = s.body_z_m - s.axle_z_m;
    let bump = if delta.abs() > p.travel_m {
        p.bump_stop_n_m
    } else {
        0.
    };
    let spring =
        p.spring_n_m / 2. * delta + bump / 2. * (delta.abs() - p.travel_m).max(0.) * delta.signum();
    let stiffness = (p.spring_n_m + bump) / 2.;
    let damping = p.damper_ns_m / 2.;
    let static_load = mass * G;
    let solve = |contact: bool| {
        let kt = if contact {
            p.contact_spring_n_m / 2.
        } else {
            0.
        };
        let ct = if contact {
            p.contact_damper_ns_m / 2.
        } else {
            0.
        };
        let a = sprung + dt * damping + dt * dt * stiffness;
        let b = -dt * damping - dt * dt * stiffness;
        let d = unsprung + dt * (damping + ct) + dt * dt * (stiffness + kt);
        let r1 = sprung * s.body_velocity_mps - dt * spring;
        let r2 = unsprung * s.axle_velocity_mps
            + dt * spring
            + dt * if contact {
                kt * (road - s.axle_z_m) + ct * road_velocity
            } else {
                -static_load
            };
        let det = a * d - b * b;
        ((r1 * d - b * r2) / det, (a * r2 - b * r1) / det)
    };
    let (mut body_velocity, mut axle_velocity) = solve(true);
    let mut normal = static_load
        + p.contact_spring_n_m / 2. * (road - s.axle_z_m - dt * axle_velocity)
        + p.contact_damper_ns_m / 2. * (road_velocity - axle_velocity);
    if normal < 0. {
        (body_velocity, axle_velocity) = solve(false);
        normal = 0.;
    }
    s.body_velocity_mps = body_velocity;
    s.axle_velocity_mps = axle_velocity;
    s.body_z_m += body_velocity * dt;
    s.axle_z_m += axle_velocity * dt;
    s.normal_force_n = normal;
    s.reference_load_n = static_load;
    s.track_height_m = road;
    s.suspension_force_n = spring + damping * (body_velocity - axle_velocity);
    s.bump_force_n = bump / 2. * (delta.abs() - p.travel_m).max(0.) * delta.signum();
    s.contact_lost = normal <= static_load * 0.001;
}
fn advance_support(
    s: &mut VerticalSupportState,
    p: &VerticalSuspension,
    mass: f64,
    coordinate: f64,
    signed_speed: f64,
    axle_span: f64,
    dt: f64,
) {
    let count = (dt / 0.002).ceil().clamp(1., 5000.) as usize;
    let h = dt / count as f64;
    let mut longitudinal = [0.; 2];
    for step in 0..count {
        let x = coordinate + signed_speed * h * (step + 1) as f64;
        for (rail, total) in longitudinal.iter_mut().enumerate() {
            let a = sample(&p.sections, x - axle_span / 2., rail == 1);
            let b = sample(&p.sections, x + axle_span / 2., rail == 1);
            solve_wheel(
                &mut s.wheels[rail],
                p,
                mass / 2.,
                (a.0 + b.0) / 2.,
                (a.1 + b.1) / 2. * signed_speed,
                h,
            );
            *total += s.wheels[rail].normal_force_n * (a.1 + b.1) / 2. / count as f64;
        }
    }
    for (wheel, force) in s.wheels.iter_mut().zip(longitudinal) {
        wheel.longitudinal_force_n = force;
    }
}
pub fn advance_train(
    state: &mut crate::TrainSimState,
    train: &crate::physics::TrainPhysics,
    path: &crate::path_data::PathData,
    centers: &[f64],
    dt: f64,
) {
    if !dt.is_finite() || dt <= 0. {
        return;
    }
    let mut masses = vec![];
    for (i, p) in train.bogies.iter().enumerate() {
        let base = state.vehicle_masses.get(i).copied().unwrap_or(p.mass_kg);
        masses.push(
            crate::consumable_mass::vehicle_mass_kg(train, state, i, base)
                + state.extra_mass_kg / train.bogies.len().max(1) as f64,
        );
    }
    let mut shared = std::collections::BTreeSet::new();
    for &[a, b] in &state.articulated_joints {
        if b >= train.bogies.len() || b >= state.bogies.len() || a >= b {
            continue;
        }
        let (Some(pa), Some(pb)) = (
            &train.bogies[a].vertical_suspension,
            &train.bogies[b].vertical_suspension,
        ) else {
            continue;
        };
        let sa = state.bogies[a]
            .vertical
            .get_or_insert_with(|| VerticalVehicleState::new(pa));
        if !sa.enabled(pa) {
            continue;
        }
        let mut combined = pa.clone();
        combined.spring_n_m += pb.spring_n_m;
        combined.damper_ns_m += pb.damper_ns_m;
        combined.contact_spring_n_m += pb.contact_spring_n_m;
        combined.contact_damper_ns_m += pb.contact_damper_ns_m;
        combined.bump_stop_n_m += pb.bump_stop_n_m;
        let ma = support_mass(&train.bogies[a], masses[a], 1);
        let mb = support_mass(&train.bogies[b], masses[b], 0);
        let direction = if state.path_reversed { -1. } else { 1. };
        let x = centers[a] + train.bogies[a].support_offsets_m[1] * direction;
        let x = if state.path_reversed {
            path.total_length_m() - x
        } else {
            x
        };
        let speed = state
            .vehicles
            .get(a)
            .map_or(state.velocity_mps, |v| v.velocity_mps)
            * if state.driver_reverser <= 0.25 {
                -1.
            } else {
                1.
            };
        advance_support(
            &mut sa.supports[1],
            &combined,
            ma + mb,
            x,
            speed,
            train.bogies[a].rigid_wheelbase_m,
            dt,
        );
        let original = sa.supports[1].clone();
        for (i, support, mass) in [(a, 1, ma), (b, 0, mb)] {
            let slot = state.bogies[i]
                .vertical
                .get_or_insert_with(|| VerticalVehicleState::new(pa));
            slot.supports[support] = original.clone();
            for wheel in &mut slot.supports[support].wheels {
                wheel.normal_force_n *= mass / (ma + mb);
                wheel.reference_load_n *= mass / (ma + mb);
                wheel.suspension_force_n *= mass / (ma + mb);
                wheel.bump_force_n *= mass / (ma + mb);
                wheel.longitudinal_force_n *= mass / (ma + mb);
            }
            shared.insert((i, support));
        }
    }
    for (i, p) in train.bogies.iter().enumerate() {
        let Some(model) = &p.vertical_suspension else {
            continue;
        };
        let Some(bogie) = state.bogies.get_mut(i) else {
            continue;
        };
        let vertical = bogie
            .vertical
            .get_or_insert_with(|| VerticalVehicleState::new(model));
        if !vertical.enabled(model) {
            continue;
        }
        let direction = if state.path_reversed { -1. } else { 1. };
        let speed = state
            .vehicles
            .get(i)
            .map_or(state.velocity_mps, |v| v.velocity_mps)
            * if state.driver_reverser <= 0.25 {
                -1.
            } else {
                1.
            };
        for support in 0..2 {
            if !shared.contains(&(i, support)) {
                let x = centers[i] + p.support_offsets_m[support] * direction;
                let x = if state.path_reversed {
                    path.total_length_m() - x
                } else {
                    x
                };
                advance_support(
                    &mut vertical.supports[support],
                    model,
                    support_mass(p, masses[i], support),
                    x,
                    speed,
                    p.rigid_wheelbase_m,
                    dt,
                );
            }
            let left = vertical.supports[support].wheels[0].normal_force_n;
            let right = vertical.supports[support].wheels[1].normal_force_n;
            let lateral = &mut bogie.supports[support];
            let transfer = (lateral.right_wheel_load_n - lateral.left_wheel_load_n) / 2.;
            lateral.left_wheel_load_n = (left - transfer).clamp(0., left + right);
            lateral.right_wheel_load_n = left + right - lateral.left_wheel_load_n;
            lateral.wheel_unloaded |= lateral.left_wheel_load_n.min(lateral.right_wheel_load_n)
                <= support_mass(p, masses[i], support) * G * 0.001;
        }
    }
}
/// Load scales the existing contact law, not the weather/friction coefficient.
pub fn normal_factor(state: &crate::TrainSimState, vehicle: usize) -> f64 {
    state
        .bogies
        .get(vehicle)
        .and_then(|s| s.vertical.as_ref())
        .filter(|s| s.enabled_override.unwrap_or(s.configured_enabled))
        .map_or(1., VerticalVehicleState::normal_factor)
}
pub fn longitudinal_forces(state: &crate::TrainSimState) -> Vec<f64> {
    let direction = if state.driver_reverser <= 0.25 {
        -1.
    } else {
        1.
    };
    state
        .bogies
        .iter()
        .map(|s| {
            s.vertical
                .as_ref()
                .filter(|s| s.enabled_override.unwrap_or(s.configured_enabled))
                .map_or(0., |s| {
                    s.supports
                        .iter()
                        .flat_map(|s| &s.wheels)
                        .map(|w| w.longitudinal_force_n * direction)
                        .sum()
                })
        })
        .collect()
}
fn support_mass(
    p: &openrailsrs_train::bogie_geometry::VehicleBogieModel,
    mass: f64,
    support: usize,
) -> f64 {
    mass * if support == 0 {
        -p.support_offsets_m[1]
    } else {
        p.support_offsets_m[0]
    } / (p.support_offsets_m[0] - p.support_offsets_m[1])
}
