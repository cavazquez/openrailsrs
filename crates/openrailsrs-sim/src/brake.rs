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
    /// Apply/release ramp rate (N/s) when increasing cylinder force.
    apply_ramp_rate_n_per_s: f64,
    /// Slower ramp when exhausting the cylinder (OR pipe recharge is gradual).
    release_ramp_rate_n_per_s: f64,
    /// Train-air latch: lap release does not reduce wagon cylinder command until driver hits 0.
    latched_command: f64,
}

impl BrakeCylinder {
    /// Create a new charged (released) cylinder.
    pub fn new(position_m: f64, max_force_n: f64, ep_instant: bool) -> Self {
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
}

impl BrakeSystem {
    /// Build a system from `(position_m, max_force_n, ep_instant)` triples.
    pub fn from_vehicles(vehicles: &[(f64, f64, bool)], pipe_speed_mps: f64) -> Self {
        Self::from_vehicles_with_options(vehicles, pipe_speed_mps, false)
    }

    /// Build a system with optional train-air lap-release hold (see `train_air_lap_hold`).
    pub fn from_vehicles_with_options(
        vehicles: &[(f64, f64, bool)],
        pipe_speed_mps: f64,
        train_air_lap_hold: bool,
    ) -> Self {
        let cylinders = vehicles
            .iter()
            .map(|&(pos, force, ep)| BrakeCylinder::new(pos, force, ep))
            .collect();
        Self {
            cylinders,
            pipe_speed_mps,
            prev_command: 0.0,
            train_air_lap_hold,
        }
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

            // Ramp toward target; exhausting a cylinder is slower than applying.
            let delta = if cyl.current_force_n > target {
                let rate = if command <= 0.0 && was_latched && self.train_air_lap_hold {
                    // Activity brake bleed after lap release (Chiltern ~3 s train-air dump).
                    cyl.max_force_n / 3.0
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

    /// Sum of all cylinder forces at the current instant (N).
    pub fn total_force_n(&self) -> f64 {
        self.cylinders.iter().map(|c| c.current_force_n).sum()
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
        }
    }
}
