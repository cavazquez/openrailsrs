//! Optional live wheel/rail contact. Headless reference runs keep their authored
//! environment unless a caller explicitly supplies weather. No rendering types.
use crate::{SimError, native_dynamics::NativeAxleState, state::TrainSimState};
use openrailsrs_formats::NativeVehiclePhysics;
use openrailsrs_train::{Consist, Vehicle};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailWeather {
    #[default]
    Dry,
    Rain,
    Storm,
    Snow,
    Fog,
}
impl RailWeather {
    // Normalized gameplay presets within OR 1.6.1's 0.5–1 weather range.
    // They do not infer ice or convert provider mm/h to OR particle density.
    pub fn factor(self) -> f64 {
        match self {
            Self::Dry => 1.,
            Self::Rain | Self::Fog => 0.6,
            Self::Storm => 0.8,
            Self::Snow => 0.5,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Dry => "Seca",
            Self::Rain | Self::Storm => "Lluvia",
            Self::Snow => "Nieve",
            Self::Fog => "Niebla",
        }
    }
    fn sand_multiplier(self) -> f64 {
        match self {
            Self::Rain | Self::Storm => 1.25,
            Self::Snow => 1.5,
            Self::Dry | Self::Fog => 1.4,
        }
    }
}

#[derive(Clone)]
pub struct RailVehicle {
    pub profile: NativeVehiclePhysics,
    pub mass_kg: f64,
    pub powered: bool,
    pub enabled: bool,
    pub steam: bool,
}
#[derive(Clone, Default)]
pub struct RailAdhesionConfig {
    pub vehicles: Vec<RailVehicle>,
}
impl RailAdhesionConfig {
    pub fn load(path: &Path, base: &Path, consist: &Consist) -> Result<Self, SimError> {
        let masses: Vec<_> = consist
            .vehicles
            .iter()
            .map(|v| match v {
                Vehicle::Loco(l) => l.mass_kg,
                Vehicle::Wagon(w) => w.mass_kg,
            })
            .collect();
        let profiles = openrailsrs_train::load_consist_native_parameters(path, base, &masses)?;
        Ok(Self {
            vehicles: profiles
                .into_iter()
                .zip(&consist.vehicles)
                .zip(masses)
                .map(|((mut profile, v), mass_kg)| {
                    let (powered, steam) = match v {
                        Vehicle::Loco(l) => {
                            if let Some(s) = &l.steam {
                                profile.wheel_radius_m = s.driving_wheel_radius_m;
                            }
                            (l.max_tractive_effort_n > 0., l.steam.is_some())
                        }
                        Vehicle::Wagon(_) => (false, false),
                    };
                    RailVehicle {
                        profile,
                        mass_kg,
                        powered,
                        enabled: powered,
                        steam,
                    }
                })
                .collect(),
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RailCarState {
    pub sand_m3: f64,
    pub consumed_sand_m3: f64,
    pub using_sand: bool,
    pub wheel_speed_mps: f64,
    /// Signed extra rolling distance, relative to the car's path motion.
    pub slip_distance_m: f64,
    pub previous_slip_distance_m: f64,
    pub slipping: bool,
    pub factor: f64,
    pub requested_force_n: f64,
    pub rail_force_n: f64,
    #[serde(skip)]
    start_odometer_m: f64,
    #[serde(skip)]
    start_speed_mps: f64,
    #[serde(skip)]
    integrated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RailAdhesionState {
    pub weather: RailWeather,
    pub weather_factor: f64,
    /// Optional continuous target supplied by a live environment. Old saves
    /// and reference callers retain the authored category presets.
    #[serde(default)]
    pub weather_target_factor: Option<f64>,
    pub sander_command: bool,
    pub backwards: bool,
    pub cars: Vec<RailCarState>,
}
impl RailAdhesionState {
    pub fn new(config: &RailAdhesionConfig, weather: RailWeather, speed: f64) -> Self {
        Self {
            weather,
            weather_factor: weather.factor(),
            weather_target_factor: None,
            sander_command: false,
            backwards: false,
            cars: config
                .vehicles
                .iter()
                .map(|v| RailCarState {
                    sand_m3: if v.powered {
                        v.profile.sander.capacity_m3
                    } else {
                        0.
                    },
                    wheel_speed_mps: speed,
                    factor: weather.factor(),
                    ..Default::default()
                })
                .collect(),
        }
    }
    pub fn valid_for(&self, config: &RailAdhesionConfig) -> bool {
        (0.5..=1.).contains(&self.weather_factor)
            && self
                .weather_target_factor
                .is_none_or(|factor| (0.5..=1.).contains(&factor))
            && self.cars.len() == config.vehicles.len()
            && self.cars.iter().zip(&config.vehicles).all(|(c, v)| {
                c.sand_m3.is_finite()
                    && c.sand_m3 >= 0.
                    && c.sand_m3
                        <= if v.powered {
                            v.profile.sander.capacity_m3
                        } else {
                            0.
                        }
                    && c.consumed_sand_m3.is_finite()
                    && c.consumed_sand_m3 >= 0.
                    && (c.sand_m3 + c.consumed_sand_m3
                        - if v.powered {
                            v.profile.sander.capacity_m3
                        } else {
                            0.
                        })
                    .abs()
                        < 1.0e-8
                    && c.wheel_speed_mps.is_finite()
                    && c.wheel_speed_mps.abs() <= 1000.
                    && c.slip_distance_m.is_finite()
                    && c.slip_distance_m.abs() <= 1.0e9
                    && c.previous_slip_distance_m.is_finite()
                    && c.previous_slip_distance_m.abs() <= 1.0e9
                    && (0.05..=2.5).contains(&c.factor)
                    && c.requested_force_n.is_finite()
                    && c.rail_force_n.is_finite()
            })
    }
    pub fn slipping(&self) -> bool {
        self.cars.iter().any(|c| c.slipping)
    }
    pub fn sander_status(&self) -> &'static str {
        if !self.sander_command {
            "Apagado"
        } else if self.cars.iter().any(|c| c.using_sand) {
            "Aplicando"
        } else if self.cars.iter().all(|c| c.sand_m3 <= 0.) {
            "Sin arena"
        } else {
            "Sin aplicación"
        }
    }
}

/// Prepare grip and finite supplies on the fixed simulation clock. A zero tick
/// refreshes conditions without consuming sand or changing the surface.
pub(crate) fn prepare(state: &mut TrainSimState, config: &RailAdhesionConfig, base: f64, dt: f64) {
    let Some(rail) = &mut state.rail_adhesion else {
        return;
    };
    let target = rail
        .weather_target_factor
        .unwrap_or_else(|| rail.weather.factor());
    let tau = if target < rail.weather_factor {
        12.
    } else {
        90.
    };
    rail.weather_factor += (target - rail.weather_factor) * (1. - (-dt / tau).exp());
    for (i, (car, v)) in rail.cars.iter_mut().zip(&config.vehicles).enumerate() {
        let speed = state
            .vehicles
            .get(i)
            .map_or(state.velocity_mps, |v| v.velocity_mps)
            .abs();
        let s = &v.profile.sander;
        let rate = if rail.backwards {
            s.reverse_m3_s
        } else {
            s.forward_m3_s
        };
        let required = rate * dt;
        car.using_sand = v.enabled
            && rail.sander_command
            && car.sand_m3 > 0.
            && speed < s.max_speed_mps
            && rate > 0.;
        let supplied = if car.using_sand && required > 0. {
            (car.sand_m3 / required).min(1.)
        } else {
            1.
        };
        let speed_effect = if s.effect_up_to_mps > 0. {
            (1. - 0.5 * speed / s.effect_up_to_mps).max(0.)
        } else {
            1.
        };
        let multiplier = if car.using_sand {
            1. + (rail.weather.sand_multiplier() * speed_effect - 1.).max(0.) * supplied
        } else {
            1.
        };
        car.factor = (base * rail.weather_factor * multiplier).clamp(0.05, 2.5);
        if car.using_sand {
            let delivered = required.min(car.sand_m3);
            car.sand_m3 -= delivered;
            car.consumed_sand_m3 += delivered;
        }
        car.requested_force_n = 0.;
        car.rail_force_n = 0.;
        car.slipping = false;
        car.start_odometer_m = state.odometer_m;
        car.start_speed_mps = speed;
        car.previous_slip_distance_m = car.slip_distance_m;
        car.integrated = false;
        if !v.enabled {
            car.wheel_speed_mps = speed;
        }
    }
}

pub(crate) fn factor(state: &TrainSimState, vehicle: usize, fallback: f64) -> f64 {
    state
        .rail_adhesion
        .as_ref()
        .and_then(|r| r.cars.get(vehicle))
        .map_or(fallback, |c| c.factor)
}

/// Record the native diesel axle after its existing Pacha step. Steam and
/// fallback electric stock use the same solver once weather contact is enabled.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record(
    state: &mut TrainSimState,
    vehicle: usize,
    speed: f64,
    wheel_speed: f64,
    requested: f64,
    force: f64,
    dt: f64,
) {
    let Some(rail) = &mut state.rail_adhesion else {
        return;
    };
    let Some(c) = rail.cars.get_mut(vehicle) else {
        return;
    };
    c.previous_slip_distance_m = c.slip_distance_m;
    c.slip_distance_m += (0.5 * (c.wheel_speed_mps + wheel_speed) - speed)
        * dt
        * if rail.backwards { -1. } else { 1. };
    c.wheel_speed_mps = wheel_speed;
    c.integrated = true;
    c.slipping = NativeAxleState {
        speed_mps: wheel_speed,
    }
    .wheel_slipping(speed, c.factor);
    c.requested_force_n = requested;
    c.rail_force_n = force;
}

/// Body and wheel phases share the actual integrated path distance. Using only
/// the speed at the start of a tick would invent slip whenever the train accelerates.
pub(crate) fn finish(state: &mut TrainSimState, dt: f64) {
    let Some(rail) = &mut state.rail_adhesion else {
        return;
    };
    let direction = if rail.backwards { -1. } else { 1. };
    for car in &mut rail.cars {
        if car.integrated {
            car.slip_distance_m -=
                (state.odometer_m - car.start_odometer_m - car.start_speed_mps * dt) * direction;
            car.integrated = false;
        }
    }
}

pub(crate) fn transmit(
    state: &mut TrainSimState,
    config: &RailAdhesionConfig,
    vehicle: usize,
    speed: f64,
    requested: f64,
    brake: f64,
    dt: f64,
) -> f64 {
    let Some(rail) = &state.rail_adhesion else {
        return requested;
    };
    let (Some(car), Some(v)) = (rail.cars.get(vehicle), config.vehicles.get(vehicle)) else {
        return requested;
    };
    if !v.enabled {
        return 0.;
    }
    let mut axle = NativeAxleState {
        speed_mps: car.wheel_speed_mps,
    };
    let force = axle.step(
        &v.profile,
        v.mass_kg,
        v.profile.driven_mass_kg,
        speed,
        requested,
        brake,
        car.factor,
        dt,
    );
    record(state, vehicle, speed, axle.speed_mps, requested, force, dt);
    force
}

/// Curtius–Kniffler wheel/rail limit, separate from brake shoe friction.
pub fn friction(speed_mps: f64, factor: f64) -> f64 {
    factor * (7.5 / (speed_mps.abs() * 3.6 + 44.) + 0.161)
}

pub(crate) fn cap_brakes(state: &TrainSimState, config: &RailAdhesionConfig, forces: &mut [f64]) {
    if state.rail_adhesion.is_none() {
        return;
    }
    for (i, (force, v)) in forces.iter_mut().zip(&config.vehicles).enumerate() {
        let speed = state
            .vehicles
            .get(i)
            .map_or(state.velocity_mps, |v| v.velocity_mps);
        *force = force.min(v.mass_kg * 9.80665 * friction(speed, factor(state, i, 1.)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> RailAdhesionConfig {
        let ast = openrailsrs_formats::parse_vehicle_text(
            "Wagon ( demo WheelRadius ( 0.5334m ) ) Engine ( demo NumWheels ( 1 ) )",
        )
        .unwrap();
        RailAdhesionConfig {
            vehicles: vec![RailVehicle {
                profile: openrailsrs_formats::parse_native_vehicle_physics(&ast, true, 67000.),
                mass_kg: 67000.,
                powered: true,
                enabled: true,
                steam: false,
            }],
        }
    }
    fn state(c: &RailAdhesionConfig, weather: RailWeather, sand: bool) -> TrainSimState {
        let mut s = TrainSimState::new(vec![]);
        s.velocity_mps = 10.;
        let mut rail = RailAdhesionState::new(c, weather, 10.);
        rail.sander_command = sand;
        s.rail_adhesion = Some(rail);
        s
    }
    #[test]
    fn low_grip_spins_the_physical_axle_and_sand_restores_transmitted_force() {
        let c = config();
        let run = |weather, sand| {
            let mut s = state(&c, weather, sand);
            for _ in 0..100 {
                prepare(&mut s, &c, 1., 0.05);
                transmit(&mut s, &c, 0, 10., 100000., 0., 0.05);
            }
            s.rail_adhesion.unwrap().cars.remove(0)
        };
        let dry = run(RailWeather::Dry, false);
        let snow = run(RailWeather::Snow, false);
        let sanded = run(RailWeather::Snow, true);
        assert!(snow.slipping && snow.wheel_speed_mps > 15., "snow {snow:?}");
        assert!(dry.wheel_speed_mps < snow.wheel_speed_mps, "dry {dry:?}");
        assert!(
            sanded.wheel_speed_mps < snow.wheel_speed_mps,
            "sand {sanded:?}"
        );
        assert!(sanded.rail_force_n > snow.rail_force_n);
        assert!(sanded.consumed_sand_m3 > 0. && dry.consumed_sand_m3 == 0.);
        assert!(snow.slip_distance_m > sanded.slip_distance_m);
    }
    #[test]
    fn supply_is_finite_speed_and_direction_gate_delivery_and_zero_time_is_free() {
        let mut c = config();
        c.vehicles[0].profile.sander.capacity_m3 = 0.0001;
        c.vehicles[0].profile.sander.forward_m3_s = 0.0001;
        let mut s = state(&c, RailWeather::Snow, true);
        prepare(&mut s, &c, 1., 0.);
        assert_eq!(s.rail_adhesion.as_ref().unwrap().cars[0].sand_m3, 0.0001);
        s.velocity_mps = 30.;
        prepare(&mut s, &c, 1., 1.);
        assert!(!s.rail_adhesion.as_ref().unwrap().cars[0].using_sand);
        s.velocity_mps = 10.;
        s.rail_adhesion.as_mut().unwrap().backwards = true;
        prepare(&mut s, &c, 1., 1.);
        assert!(!s.rail_adhesion.as_ref().unwrap().cars[0].using_sand);
        s.rail_adhesion.as_mut().unwrap().backwards = false;
        prepare(&mut s, &c, 1., 2.);
        let r = s.rail_adhesion.as_ref().unwrap();
        assert_eq!(r.cars[0].sand_m3, 0.);
        assert_eq!(r.cars[0].consumed_sand_m3, 0.0001);
        assert!(
            (r.cars[0].factor - 0.625).abs() < 1e-12,
            "last interval only has half its demand"
        );
        assert!(r.valid_for(&c));
        prepare(&mut s, &c, 1., 0.05);
        let r = s.rail_adhesion.as_ref().unwrap();
        assert_eq!(r.sander_status(), "Sin arena");
        assert_eq!(r.cars[0].factor, 0.5);
    }
    #[test]
    fn drying_is_gradual_and_uses_simulation_time_independently_of_frame_size() {
        let c = config();
        let mut whole = state(&c, RailWeather::Snow, false);
        whole.rail_adhesion.as_mut().unwrap().weather = RailWeather::Dry;
        let mut divided = whole.clone();
        prepare(&mut whole, &c, 1., 90.);
        for _ in 0..20 {
            prepare(&mut divided, &c, 1., 4.5);
        }
        let a = whole.rail_adhesion.unwrap().weather_factor;
        let b = divided.rail_adhesion.as_ref().unwrap().weather_factor;
        assert!((a - b).abs() < 1e-12 && a > 0.5 && a < 1.);
        let before = divided.rail_adhesion.as_ref().unwrap().weather_factor;
        prepare(&mut divided, &c, 1., 0.);
        assert_eq!(divided.rail_adhesion.unwrap().weather_factor, before);
    }
    #[test]
    fn weather_caps_wheel_rail_braking_separately_from_shoe_pressure() {
        let c = config();
        let mut dry = state(&c, RailWeather::Dry, false);
        let mut snow = state(&c, RailWeather::Snow, false);
        prepare(&mut dry, &c, 1., 0.05);
        prepare(&mut snow, &c, 1., 0.05);
        let mut d = [1.0e6];
        let mut s = d;
        cap_brakes(&dry, &c, &mut d);
        cap_brakes(&snow, &c, &mut s);
        assert!((s[0] * 2. - d[0]).abs() < 1e-8);
        assert!(d[0] < 1.0e6);
        assert_eq!(snow.brake_system.cylinders.len(), 0);
    }

    #[test]
    fn continuous_target_controls_brake_contact_and_survives_old_saves() {
        let c = config();
        let run = |target| {
            let mut s = state(&c, RailWeather::Rain, false);
            s.rail_adhesion.as_mut().unwrap().weather_target_factor = Some(target);
            prepare(&mut s, &c, 1., 600.);
            let mut force = [1e6];
            cap_brakes(&s, &c, &mut force);
            assert!(s.rail_adhesion.as_ref().unwrap().valid_for(&c));
            force[0]
        };
        assert!(run(0.928) > run(0.752));
        assert!(run(0.752) > run(0.6));
        let mut rail = RailAdhesionState::new(&c, RailWeather::Snow, 0.);
        let mut old = serde_json::to_value(&rail).unwrap();
        old.as_object_mut().unwrap().remove("weather_target_factor");
        rail = serde_json::from_value(old).unwrap();
        assert_eq!(rail.weather_target_factor, None);
        assert!(rail.valid_for(&c));
        rail.weather_target_factor = Some(f64::NAN);
        assert!(!rail.valid_for(&c));
    }

    #[test]
    fn wheel_phase_subtracts_integrated_body_travel_in_both_directions() {
        let c = config();
        for backwards in [false, true] {
            let mut s = state(&c, RailWeather::Dry, false);
            s.rail_adhesion.as_mut().unwrap().backwards = backwards;
            prepare(&mut s, &c, 1., 0.5);
            record(&mut s, 0, 10., 12., 0., 0., 0.5);
            s.odometer_m += 5.5; // Trapezoidal body motion from 10 to 12 m/s.
            finish(&mut s, 0.5);
            assert_eq!(
                s.rail_adhesion.as_ref().unwrap().cars[0].slip_distance_m,
                0.
            );
            prepare(&mut s, &c, 1., 0.5);
            record(&mut s, 0, 10., 14., 0., 0., 0.5);
            s.odometer_m += 5.5;
            finish(&mut s, 0.5);
            assert_eq!(
                s.rail_adhesion.as_ref().unwrap().cars[0].slip_distance_m,
                if backwards { -1. } else { 1. }
            );
        }
    }
}
