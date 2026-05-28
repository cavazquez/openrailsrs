//! Coupler-force (multi-body) dynamics — Fase 20.
//!
//! Each vehicle in the consist is modelled as an independent mass connected to
//! its neighbours by a spring-damper coupler with a small free-play gap.  This
//! reproduces the characteristic "jerk" felt when a long freight train starts
//! or stops: the slack in each coupler must be taken up sequentially.
//!
//! # Integration with `physics::step()`
//!
//! When `TrainSimState::vehicles` is non-empty, `step()` calls
//! [`multi_body_step()`] instead of the single-mass path.  The resulting
//! mean velocity is stored in `state.velocity_mps`; individual vehicle
//! velocities live in `state.vehicles`.
//!
//! When `state.vehicles` is empty the old single-mass path is used unchanged
//! (backwards-compatible).

/// Per-vehicle kinematic state.
#[derive(Clone, Debug)]
pub struct VehicleState {
    /// Current velocity (m/s).
    pub velocity_mps: f64,
    /// Cumulative distance travelled (m).
    pub position_m: f64,
}

/// Spring-damper coupler between two adjacent vehicles.
///
/// `extension_m` is positive when the coupler is in tension (vehicles pulling
/// apart) and negative when in compression.  Within ±`free_play_m` the spring
/// force is zero (slack).
#[derive(Clone, Debug)]
pub struct CouplerState {
    /// Spring stiffness (N/m). Typical value: 2e6 N/m.
    pub stiffness_n_per_m: f64,
    /// Viscous damping coefficient (N·s/m). Typical value: 1e5 N·s/m.
    pub damping_n_per_mps: f64,
    /// Free-play (slack) half-width in metres (±gap before spring engages).
    pub free_play_m: f64,
    /// Force at which the coupler breaks (N); 0 = unbreakable.
    pub break_force_n: f64,
    /// Saturated draft-gear force (N); 0 = no cap.
    pub max_force_n: f64,
    /// Current extension relative to nominal (m).
    pub extension_m: f64,
    /// Whether this coupler has failed.
    pub broken: bool,
}

impl CouplerState {
    /// Typical freight coupler with generous free-play.
    pub fn freight() -> Self {
        Self {
            stiffness_n_per_m: 2e6,
            damping_n_per_mps: 1e5,
            free_play_m: 0.05,
            break_force_n: 0.0,
            max_force_n: 0.0,
            extension_m: 0.0,
            broken: false,
        }
    }

    /// Rigid EMU gangway (tight coupler, almost no free-play).
    pub fn emu() -> Self {
        Self {
            stiffness_n_per_m: 5e6,
            damping_n_per_mps: 2e5,
            free_play_m: 0.005,
            break_force_n: 0.0,
            max_force_n: 0.0,
            extension_m: 0.0,
            broken: false,
        }
    }

    /// Passenger / Mk2: moderate slack, force-capped for stiff-brake stability.
    pub fn passenger() -> Self {
        Self {
            stiffness_n_per_m: 2.0e5,
            damping_n_per_mps: 8.0e3,
            free_play_m: 0.015,
            break_force_n: 0.0,
            max_force_n: 600_000.0,
            extension_m: 0.0,
            broken: false,
        }
    }

    /// Blue Pullman / tight passenger stock (Chiltern 8-car): low dissipation in coast.
    pub fn pullman() -> Self {
        Self {
            stiffness_n_per_m: 1.2e5,
            damping_n_per_mps: 4.0e3,
            free_play_m: 0.012,
            break_force_n: 0.0,
            max_force_n: 800_000.0,
            extension_m: 0.0,
            broken: false,
        }
    }

    /// Build from [`CouplerKind`].
    pub fn from_kind(kind: CouplerKind) -> Self {
        match kind {
            CouplerKind::Freight => Self::freight(),
            CouplerKind::Passenger => Self::passenger(),
            CouplerKind::Pullman => Self::pullman(),
        }
    }

    /// Compute the current coupler force (N) given the relative velocity between
    /// the two vehicles it connects.
    pub fn force_n(&self, delta_v_mps: f64) -> f64 {
        self.force_n_scaled(delta_v_mps, 1.0)
    }

    /// Like [`force_n`] but scales viscous damping (spring force unchanged).
    pub fn force_n_scaled(&self, delta_v_mps: f64, damping_scale: f64) -> f64 {
        if self.broken {
            return 0.0;
        }
        // Spring force (zero inside free-play band).
        let spring = if self.extension_m.abs() > self.free_play_m {
            let sign = self.extension_m.signum();
            self.stiffness_n_per_m * (self.extension_m - sign * self.free_play_m)
        } else {
            0.0
        };
        // Damping only when the spring is engaged (no dissipation in slack).
        let damp = if spring.abs() > 0.0 {
            self.damping_n_per_mps * delta_v_mps * damping_scale
        } else {
            0.0
        };
        let mut f = spring + damp;
        if self.max_force_n > 0.0 {
            f = f.clamp(-self.max_force_n, self.max_force_n);
        }
        f
    }
}

impl Default for CouplerState {
    fn default() -> Self {
        Self::freight()
    }
}

/// Coupler preset for multi-body consists (scenario `coupler_kind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CouplerKind {
    #[default]
    Freight,
    Passenger,
    Pullman,
}

impl CouplerKind {
    /// Parse scenario TOML value (`freight`, `passenger`, `pullman`, `mk2`, `emu`).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "pullman" => Self::Pullman,
            "passenger" | "emu" | "mk2" => Self::Passenger,
            _ => Self::Freight,
        }
    }
}

/// Maximum explicit-Euler sub-step for stiff coupler springs (s).
pub const MULTI_BODY_MAX_SUBSTEP_S: f64 = 0.05;

/// Sub-step count so each integration step is at most [`MULTI_BODY_MAX_SUBSTEP_S`].
pub fn multi_body_substep_count(dt: f64) -> usize {
    multi_body_substep_count_for_vehicles(dt, &[])
}

/// Like [`multi_body_substep_count`] but adds extra sub-steps when adjacent speeds diverge.
pub fn multi_body_substep_count_for_vehicles(dt: f64, vehicles: &[VehicleState]) -> usize {
    if dt <= 0.0 {
        return 1;
    }
    let base = ((dt / MULTI_BODY_MAX_SUBSTEP_S).ceil() as usize).max(1);
    let max_dv = vehicles
        .windows(2)
        .map(|pair| (pair[0].velocity_mps - pair[1].velocity_mps).abs())
        .fold(0.0_f64, f64::max);
    let scale = if max_dv > 15.0 {
        4
    } else if max_dv > 5.0 {
        2
    } else {
        1
    };
    base * scale
}

/// True when every coupler is inside its free-play band (no spring load).
pub fn couplers_all_in_slack(couplers: &[CouplerState]) -> bool {
    couplers
        .iter()
        .all(|c| c.extension_m.abs() <= c.free_play_m)
}

/// Mass-weighted mean of per-vehicle speeds (m/s).
pub fn mass_weighted_mean_velocity(vehicles: &[VehicleState], masses: &[f64]) -> f64 {
    let total_mass: f64 = masses.iter().sum::<f64>().max(1.0);
    masses
        .iter()
        .zip(vehicles.iter())
        .map(|(m, v)| m * v.velocity_mps)
        .sum::<f64>()
        / total_mass
}

/// Advance all vehicles by `dt` seconds.
///
/// - `vehicles` — mutable per-vehicle states (index 0 = locomotive / front).
/// - `couplers` — one coupler between each adjacent pair: `couplers[i]`
///   connects `vehicles[i]` and `vehicles[i+1]`.  Length must be
///   `vehicles.len() - 1` (or empty for a single vehicle).
/// - `f_motor` — tractive force applied to `vehicles[0]` (N).
/// - `brake_forces` — braking force per vehicle (N); must be same length as `vehicles`.
/// - `grade_resist` — combined grade + Davis resistance per vehicle (N).
/// - `masses` — mass of each vehicle (kg).
///
/// Returns the **mass-weighted mean velocity** (m/s) for `state.velocity_mps`.
pub fn multi_body_step(
    vehicles: &mut [VehicleState],
    couplers: &mut [CouplerState],
    f_motor: f64,
    brake_forces: &[f64],
    grade_resist: &[f64],
    masses: &[f64],
    dt: f64,
    damping_scale: f64,
) -> f64 {
    let n = vehicles.len();
    if n == 0 {
        return 0.0;
    }

    // Compute coupler forces.
    let mut coupler_forces = vec![0.0f64; n.saturating_sub(1)];
    for i in 0..couplers.len().min(n.saturating_sub(1)) {
        let dv = vehicles[i].velocity_mps - vehicles[i + 1].velocity_mps;
        coupler_forces[i] = couplers[i].force_n_scaled(dv, damping_scale);
    }

    // Advance each vehicle.
    let mut new_velocities = vec![0.0f64; n];
    for i in 0..n {
        // Net force on vehicle i.
        let f_coupler_fwd = if i > 0 { coupler_forces[i - 1] } else { 0.0 };
        let f_coupler_bwd = if i < n - 1 { coupler_forces[i] } else { 0.0 };

        let f_drive = if i == 0 { f_motor } else { 0.0 };
        let f_net = f_drive - brake_forces[i] - grade_resist[i] + f_coupler_fwd - f_coupler_bwd;

        let accel = f_net / masses[i].max(1.0);
        new_velocities[i] = (vehicles[i].velocity_mps + accel * dt).max(0.0);
    }

    // Update coupler extensions.
    for i in 0..couplers.len().min(n.saturating_sub(1)) {
        if couplers[i].broken {
            continue;
        }
        let dv = vehicles[i].velocity_mps - vehicles[i + 1].velocity_mps;
        couplers[i].extension_m += dv * dt;

        // Check break force.
        if couplers[i].break_force_n > 0.0 {
            let f = coupler_forces[i].abs();
            if f > couplers[i].break_force_n {
                couplers[i].broken = true;
            }
        }
    }

    // Apply new velocities and advance positions.
    for i in 0..n {
        let v_avg = 0.5 * (vehicles[i].velocity_mps + new_velocities[i]);
        vehicles[i].position_m += v_avg * dt;
        vehicles[i].velocity_mps = new_velocities[i];
    }

    // Mass-weighted mean velocity.
    mass_weighted_mean_velocity(vehicles, masses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passenger_preset_is_stable_under_one_second_step() {
        let mut vehicles = vec![
            VehicleState {
                velocity_mps: 10.0,
                position_m: 0.0,
            },
            VehicleState {
                velocity_mps: 0.0,
                position_m: 0.0,
            },
        ];
        let mut couplers = vec![CouplerState::passenger()];
        let masses = vec![50_000.0, 50_000.0];
        let brake = vec![0.0; 2];
        let resist = vec![800.0, 800.0];
        let n = multi_body_substep_count(1.0);
        let sub_dt = 1.0 / n as f64;
        for _ in 0..n {
            multi_body_step(
                &mut vehicles,
                &mut couplers,
                0.0,
                &brake,
                &resist,
                &masses,
                sub_dt,
                1.0,
            );
        }
        assert!(
            vehicles[0].velocity_mps.is_finite() && vehicles[0].velocity_mps < 15.0,
            "front v={}",
            vehicles[0].velocity_mps
        );
        assert!(
            vehicles[1].velocity_mps.is_finite() && vehicles[1].velocity_mps < 15.0,
            "rear v={}",
            vehicles[1].velocity_mps
        );
    }

    #[test]
    fn force_cap_limits_passenger_coupler() {
        let mut c = CouplerState::passenger();
        c.extension_m = 0.5;
        let f = c.force_n(50.0);
        assert!(f.abs() <= c.max_force_n + 1.0);
    }

    #[test]
    fn pullman_preset_survives_hard_brake_jerk() {
        let mut vehicles = vec![
            VehicleState {
                velocity_mps: 30.0,
                position_m: 0.0,
            },
            VehicleState {
                velocity_mps: 28.0,
                position_m: 0.0,
            },
        ];
        let mut couplers = vec![CouplerState::pullman()];
        let masses = vec![80_000.0, 50_000.0];
        let brake = vec![120_000.0, 80_000.0];
        let resist = vec![2_000.0, 1_500.0];
        let n = multi_body_substep_count_for_vehicles(1.0, &vehicles);
        let sub_dt = 1.0 / n as f64;
        for _ in 0..n {
            multi_body_step(
                &mut vehicles,
                &mut couplers,
                0.0,
                &brake,
                &resist,
                &masses,
                sub_dt,
                1.0,
            );
        }
        assert!(
            vehicles.iter().all(|v| v.velocity_mps.is_finite() && v.velocity_mps < 35.0),
            "front={} rear={}",
            vehicles[0].velocity_mps,
            vehicles[1].velocity_mps
        );
    }
}
