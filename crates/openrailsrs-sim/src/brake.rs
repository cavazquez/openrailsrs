//! Westinghouse-style air-brake propagation model — Fase 19.
//!
//! Each vehicle in the consist has its own brake cylinder.  When the driver
//! applies the brake (`command > 0`), the brake pipe pressure drop travels at
//! `pipe_speed_mps` (~200 m/s) from the front of the train to the rear.  The
//! cylinder at position `p` starts applying after `p / pipe_speed_mps` seconds.
//!
//! Releasing works in reverse: the pressure is restored from the front and
//! cylinders release in front-to-back order.
//!
//! For a single-vehicle consist (locomotive only), the propagation delay is
//! zero and the behaviour is identical to the previous instantaneous model.

use openrailsrs_formats::{BrakeShoeFrictionCurve, resolve_brake_shoe_curve};
use openrailsrs_train::{Consist, Vehicle};

/// State of one brake cylinder.
#[derive(Clone, Debug, PartialEq)]
pub enum BrakeState {
    /// Pipe fully charged; no braking force.
    Charged,
    /// Pressure is dropping; force is ramping up.
    Applying,
    /// Fully applied; maximum braking force.
    Applied,
    /// Pressure recovering; force ramping down.
    Releasing,
}

/// One brake cylinder, representing a single vehicle's brakes.
#[derive(Clone, Debug)]
pub struct BrakeCylinder {
    /// Distance from the front of the train (metres).
    pub position_m: f64,
    /// Maximum braking force this cylinder can produce (N).
    pub max_force_n: f64,
    /// Electro-pneumatic (locomotive): responds instantly to driver command.
    pub ep_instant: bool,
    /// Current state.
    pub state: BrakeState,
    /// Currently applied force (N).
    pub current_force_n: f64,
    /// Seconds of pipe-signal travel still to cover before this cylinder reacts.
    time_pending_s: f64,
    /// Apply/release ramp rate (N/s) when increasing cylinder force.
    apply_ramp_rate_n_per_s: f64,
    /// Slower ramp when exhausting the cylinder (OR pipe recharge is gradual).
    release_ramp_rate_n_per_s: f64,
    /// Train-air latch: lap release does not reduce wagon cylinder command until driver hits 0.
    latched_command: f64,
    /// μ(v)/μ(0) curve for OR-P6b (identity = constant force vs speed).
    shoe_friction: BrakeShoeFrictionCurve,
    /// Vehicle mass for OR-P6c skid cap (kg).
    mass_kg: f64,
    /// Wheel-rail adhesion μ; 0 disables skid limit on this cylinder.
    skid_adhesion_mu: f64,
}

/// Per-vehicle brake cylinder specification for [`BrakeSystem`].
#[derive(Clone, Debug)]
pub struct BrakeVehicleSpec {
    pub position_m: f64,
    pub max_force_n: f64,
    pub ep_instant: bool,
    pub shoe_friction: BrakeShoeFrictionCurve,
    pub mass_kg: f64,
    /// Wheel-rail μ cap when skid limit enabled; 0 = no cap (OR-P6c off).
    pub skid_adhesion_mu: f64,
}

/// OR `Train.WagonCoefficientFriction` dry default for brake adhesion cap.
pub const OR_DEFAULT_BRAKE_ADHESION_MU: f64 = 0.25;

const DEFAULT_VEHICLE_LENGTH_M: f64 = 15.0;

/// Build per-vehicle brake specs from a loaded consist.
pub fn vehicle_specs_from_consist(
    consist: &Consist,
    shoe_speed_factor: bool,
    skid_limit: bool,
) -> Vec<BrakeVehicleSpec> {
    let mut pos = 0.0_f64;
    consist
        .vehicles
        .iter()
        .map(|v| {
            let cylinder_pos = pos;
            let length_m = match v {
                Vehicle::Loco(l) => {
                    if l.length_m > 0.0 {
                        l.length_m
                    } else {
                        DEFAULT_VEHICLE_LENGTH_M
                    }
                }
                Vehicle::Wagon(w) => {
                    if w.length_m > 0.0 {
                        w.length_m
                    } else {
                        DEFAULT_VEHICLE_LENGTH_M
                    }
                }
            };
            pos += length_m;
            let (force_n, ep, shoe_type, user_curve, mass_kg) = match v {
                Vehicle::Loco(l) => (
                    l.max_brake_force_n,
                    true,
                    &l.brake_shoe_type,
                    &l.brake_shoe_friction,
                    l.mass_kg,
                ),
                Vehicle::Wagon(w) => (
                    w.max_brake_force_n,
                    false,
                    &w.brake_shoe_type,
                    &w.brake_shoe_friction,
                    w.mass_kg,
                ),
            };
            let shoe_friction = if shoe_speed_factor {
                resolve_brake_shoe_curve(shoe_type, user_curve)
            } else {
                BrakeShoeFrictionCurve::identity()
            };
            let skid_adhesion_mu = if skid_limit {
                OR_DEFAULT_BRAKE_ADHESION_MU
            } else {
                0.0
            };
            BrakeVehicleSpec {
                position_m: cylinder_pos,
                max_force_n: force_n,
                ep_instant: ep,
                shoe_friction,
                mass_kg,
                skid_adhesion_mu,
            }
        })
        .collect()
}

impl BrakeCylinder {
    /// Create a new charged (released) cylinder.
    pub fn new(
        position_m: f64,
        max_force_n: f64,
        ep_instant: bool,
        shoe_friction: openrailsrs_formats::BrakeShoeFrictionCurve,
        mass_kg: f64,
        skid_adhesion_mu: f64,
    ) -> Self {
        // Release: EP ~2.5 s; wagons ~8 s (OR pipe recharge). Fast bleed on full release when lap-hold enabled.
        let (apply_time_s, release_time_s) = if ep_instant { (0.15, 2.5) } else { (0.5, 8.0) };
        Self {
            position_m,
            max_force_n,
            ep_instant,
            state: BrakeState::Charged,
            current_force_n: 0.0,
            time_pending_s: 0.0,
            apply_ramp_rate_n_per_s: max_force_n / apply_time_s,
            release_ramp_rate_n_per_s: max_force_n / release_time_s,
            latched_command: 0.0,
            shoe_friction,
            mass_kg,
            skid_adhesion_mu,
        }
    }

    /// Wheel-rim braking force after shoe μ(v) and optional skid adhesion cap.
    pub fn effective_force_n(&self, speed_mps: f64) -> f64 {
        let shoe = self.current_force_n * self.shoe_friction.speed_factor(speed_mps);
        if self.skid_adhesion_mu > 0.0 && self.mass_kg > 0.0 {
            shoe.min(self.mass_kg * 9.81 * self.skid_adhesion_mu)
        } else {
            shoe
        }
    }

    /// Driver command this cylinder responds to (EP follows handle; train air holds during lap release).
    fn effective_command(&mut self, command: f64, lap_hold: bool) -> f64 {
        if self.ep_instant || !lap_hold {
            return command;
        }
        // Train air: ignore lap release until driver reaches full release (command = 0).
        if command <= 0.0 {
            self.latched_command = 0.0;
            return 0.0;
        }
        if command > self.latched_command {
            self.latched_command = command;
        }
        self.latched_command
    }
}

/// Whole-train brake system: a collection of cylinders fed by a single pipe.
#[derive(Clone, Debug)]
pub struct BrakeSystem {
    pub cylinders: Vec<BrakeCylinder>,
    /// Speed at which the brake-pipe pressure change propagates (m/s).
    pub pipe_speed_mps: f64,
    /// Magnitude of the previous command (0.0 = released).
    prev_command: f64,
    /// Hold wagon cylinders at peak application during lap release (Chiltern activity start).
    train_air_lap_hold: bool,
    /// Seconds to dump train-air after driver full release when lap-hold is enabled.
    train_air_full_release_s: f64,
}

impl BrakeSystem {
    /// Build a system from `(position_m, max_force_n, ep_instant)` triples.
    pub fn from_vehicles(vehicles: &[(f64, f64, bool)], pipe_speed_mps: f64) -> Self {
        Self::from_vehicles_with_options(vehicles, pipe_speed_mps, false)
    }

    /// Build a system with optional train-air lap-release hold (see `train_air_lap_hold`).
    pub fn from_vehicle_specs(
        vehicles: &[BrakeVehicleSpec],
        pipe_speed_mps: f64,
        train_air_lap_hold: bool,
        train_air_full_release_s: f64,
    ) -> Self {
        let cylinders = vehicles
            .iter()
            .map(|v| {
                BrakeCylinder::new(
                    v.position_m,
                    v.max_force_n,
                    v.ep_instant,
                    v.shoe_friction.clone(),
                    v.mass_kg,
                    v.skid_adhesion_mu,
                )
            })
            .collect();
        Self {
            cylinders,
            pipe_speed_mps,
            prev_command: 0.0,
            train_air_lap_hold,
            train_air_full_release_s: train_air_full_release_s.max(0.5),
        }
    }

    /// Backward-compatible builder (identity μ(v) curve).
    pub fn from_vehicles_with_options(
        vehicles: &[(f64, f64, bool)],
        pipe_speed_mps: f64,
        train_air_lap_hold: bool,
    ) -> Self {
        Self::from_vehicles_with_release(vehicles, pipe_speed_mps, train_air_lap_hold, 3.0)
    }

    /// Build with lap-hold and full-release bleed duration (s).
    pub fn from_vehicles_with_release(
        vehicles: &[(f64, f64, bool)],
        pipe_speed_mps: f64,
        train_air_lap_hold: bool,
        train_air_full_release_s: f64,
    ) -> Self {
        let specs: Vec<BrakeVehicleSpec> = vehicles
            .iter()
            .map(|&(pos, force, ep)| BrakeVehicleSpec {
                position_m: pos,
                max_force_n: force,
                ep_instant: ep,
                shoe_friction: BrakeShoeFrictionCurve::identity(),
                mass_kg: 0.0,
                skid_adhesion_mu: 0.0,
            })
            .collect();
        Self::from_vehicle_specs(
            &specs,
            pipe_speed_mps,
            train_air_lap_hold,
            train_air_full_release_s,
        )
    }

    /// Advance the brake system by `dt` seconds given driver `command` in [0, 1].
    ///
    /// - `command == 0.0` → release.
    /// - `command > 0.0`  → apply at `command * max_force_n` (EP) or latched train-air level (wagons).
    pub fn step(&mut self, command: f64, dt: f64) {
        let command = command.clamp(0.0, 1.0);
        let applying = command > 0.0;
        let command_changed = (command - self.prev_command).abs() > 1e-6;

        if command_changed {
            for cyl in &mut self.cylinders {
                let travel_s = if cyl.ep_instant {
                    0.0
                } else {
                    cyl.position_m / self.pipe_speed_mps.max(1.0)
                };
                cyl.time_pending_s = travel_s;
                cyl.state = if applying {
                    BrakeState::Applying
                } else {
                    BrakeState::Releasing
                };
            }
            self.prev_command = command;
        }

        for cyl in &mut self.cylinders {
            // Drain pending travel time.
            if cyl.time_pending_s > 0.0 {
                cyl.time_pending_s = (cyl.time_pending_s - dt).max(0.0);
                if cyl.time_pending_s > 0.0 {
                    // Signal hasn't arrived yet; hold current force.
                    continue;
                }
            }

            let was_latched = !cyl.ep_instant && cyl.latched_command > 0.0;
            let eff = cyl.effective_command(command, self.train_air_lap_hold);
            let target = eff * cyl.max_force_n;

            // Release: EP + latched train-air bleed at `train_air_full_release_s` when lap-hold is on
            // (OR pipe exhaust / BC bleed); otherwise use per-cylinder default release ramp.
            let delta = if cyl.current_force_n > target {
                let rate = if command <= 0.0
                    && self.train_air_lap_hold
                    && (cyl.ep_instant || was_latched)
                {
                    cyl.max_force_n / self.train_air_full_release_s
                } else {
                    cyl.release_ramp_rate_n_per_s
                };
                rate * dt
            } else {
                cyl.apply_ramp_rate_n_per_s * dt
            };
            if cyl.current_force_n < target {
                cyl.current_force_n = (cyl.current_force_n + delta).min(target);
                cyl.state = if cyl.current_force_n >= target {
                    BrakeState::Applied
                } else {
                    BrakeState::Applying
                };
            } else if cyl.current_force_n > target {
                cyl.current_force_n = (cyl.current_force_n - delta).max(target);
                cyl.state = if cyl.current_force_n <= target {
                    BrakeState::Charged
                } else {
                    BrakeState::Releasing
                };
            }
        }
    }

    /// Sum of all cylinder forces at the current instant (N), scaled by μ(v).
    pub fn total_force_n(&self, speed_mps: f64) -> f64 {
        self.cylinders
            .iter()
            .map(|c| c.effective_force_n(speed_mps))
            .sum()
    }

    /// Per-cylinder wheel-rim force (N) after shoe μ(v) scaling.
    pub fn cylinder_forces_n(&self, speed_mps: f64) -> Vec<f64> {
        self.cylinders
            .iter()
            .map(|c| c.effective_force_n(speed_mps))
            .collect()
    }

    /// Maximum total force when fully applied (N).
    pub fn max_total_force_n(&self) -> f64 {
        self.cylinders.iter().map(|c| c.max_force_n).sum()
    }

    /// Set cylinders to steady state for `command` (activity start with brakes already set).
    pub fn precharge(&mut self, command: f64) {
        let command = command.clamp(0.0, 1.0);
        for cyl in &mut self.cylinders {
            cyl.latched_command = if cyl.ep_instant || !self.train_air_lap_hold {
                0.0
            } else {
                command
            };
            cyl.current_force_n =
                cyl.effective_command(command, self.train_air_lap_hold) * cyl.max_force_n;
            cyl.time_pending_s = 0.0;
            cyl.state = if command > 0.0 {
                BrakeState::Applied
            } else {
                BrakeState::Charged
            };
        }
        self.prev_command = command;
    }
}

impl Default for BrakeSystem {
    /// Empty system — `physics::step()` falls back to the instantaneous scalar model
    /// when no cylinders are registered.  Use [`BrakeSystem::from_vehicles`] to
    /// enable the detailed propagation model.
    fn default() -> Self {
        Self {
            cylinders: Vec::new(),
            pipe_speed_mps: 200.0,
            prev_command: 0.0,
            train_air_lap_hold: false,
            train_air_full_release_s: 3.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::BrakeShoeFrictionCurve;

    #[test]
    fn skid_limit_caps_brake_at_mass_times_adhesion() {
        let mass_kg = 30_000.0;
        let max_force = 500_000.0;
        let mu = OR_DEFAULT_BRAKE_ADHESION_MU;
        let cap = mass_kg * 9.81 * mu;

        let mut cyl = BrakeCylinder::new(
            0.0,
            max_force,
            true,
            BrakeShoeFrictionCurve::identity(),
            mass_kg,
            mu,
        );
        cyl.current_force_n = max_force;

        let effective = cyl.effective_force_n(0.0);
        assert!(
            (effective - cap).abs() < 1.0,
            "expected cap {cap}, got {effective}"
        );
        assert!(effective < max_force * 0.5);
    }
}
