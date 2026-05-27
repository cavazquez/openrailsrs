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
            extension_m: 0.0,
            broken: false,
        }
    }

    /// Compute the current coupler force (N) given the relative velocity between
    /// the two vehicles it connects.
    pub fn force_n(&self, delta_v_mps: f64) -> f64 {
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
        // Damping force (acts whenever there is relative motion).
        let damp = self.damping_n_per_mps * delta_v_mps;
        spring + damp
    }
}

impl Default for CouplerState {
    fn default() -> Self {
        Self::freight()
    }
}

/// Maximum explicit-Euler sub-step for stiff coupler springs (s).
pub const MULTI_BODY_MAX_SUBSTEP_S: f64 = 0.05;

/// Sub-step count so each integration step is at most [`MULTI_BODY_MAX_SUBSTEP_S`].
pub fn multi_body_substep_count(dt: f64) -> usize {
    if dt <= 0.0 {
        return 1;
    }
    ((dt / MULTI_BODY_MAX_SUBSTEP_S).ceil() as usize).max(1)
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
) -> f64 {
    let n = vehicles.len();
    if n == 0 {
        return 0.0;
    }

    // Compute coupler forces.
    let mut coupler_forces = vec![0.0f64; n.saturating_sub(1)];
    for i in 0..couplers.len().min(n.saturating_sub(1)) {
        let dv = vehicles[i].velocity_mps - vehicles[i + 1].velocity_mps;
        coupler_forces[i] = couplers[i].force_n(dv);
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
    fn substep_count_scales_with_dt() {
        assert_eq!(multi_body_substep_count(0.01), 1);
        assert_eq!(multi_body_substep_count(0.05), 1);
        assert_eq!(multi_body_substep_count(0.051), 2);
        assert_eq!(multi_body_substep_count(1.0), 20);
    }

    #[test]
    fn large_dt_substeps_stay_bounded_with_velocity_slack() {
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
        let mut couplers = vec![CouplerState::freight()];
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
}
