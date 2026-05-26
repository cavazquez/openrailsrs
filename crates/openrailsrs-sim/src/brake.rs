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
    /// Apply/release ramp rate (N/s).
    ramp_rate_n_per_s: f64,
}

impl BrakeCylinder {
    /// Create a new charged (released) cylinder.
    pub fn new(position_m: f64, max_force_n: f64, ep_instant: bool) -> Self {
        // EP locomotive brakes ramp faster (~0.15 s); train air ~0.5 s.
        let ramp_time_s = if ep_instant { 0.15 } else { 0.5 };
        let ramp_rate_n_per_s = max_force_n / ramp_time_s;
        Self {
            position_m,
            max_force_n,
            ep_instant,
            state: BrakeState::Charged,
            current_force_n: 0.0,
            time_pending_s: 0.0,
            ramp_rate_n_per_s,
        }
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
}

impl BrakeSystem {
    /// Build a system from `(position_m, max_force_n, ep_instant)` triples.
    pub fn from_vehicles(vehicles: &[(f64, f64, bool)], pipe_speed_mps: f64) -> Self {
        let cylinders = vehicles
            .iter()
            .map(|&(pos, force, ep)| BrakeCylinder::new(pos, force, ep))
            .collect();
        Self {
            cylinders,
            pipe_speed_mps,
            prev_command: 0.0,
        }
    }

    /// Advance the brake system by `dt` seconds given driver `command` in [0, 1].
    ///
    /// - `command == 0.0` → release.
    /// - `command > 0.0`  → apply at `command * max_force_n`.
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

            let target = if applying {
                command * cyl.max_force_n
            } else {
                0.0
            };

            // Ramp toward target.
            let delta = cyl.ramp_rate_n_per_s * dt;
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

    /// Sum of all cylinder forces at the current instant (N).
    pub fn total_force_n(&self) -> f64 {
        self.cylinders.iter().map(|c| c.current_force_n).sum()
    }

    /// Maximum total force when fully applied (N).
    pub fn max_total_force_n(&self) -> f64 {
        self.cylinders.iter().map(|c| c.max_force_n).sum()
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
        }
    }
}
