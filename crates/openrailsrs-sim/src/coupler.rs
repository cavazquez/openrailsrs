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

use openrailsrs_train::coupler_profile::{CouplerCurve, CouplerEndProfile, VehicleCouplerProfile};
use serde::{Deserialize, Serialize};

/// Immutable authored draft gears at one joint; compression also models its
/// buffers. There is deliberately no additional parallel buffer spring.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeCouplerProfile {
    pub tension: CouplerCurve,
    pub compression: CouplerCurve,
    pub warning_force_n: f64,
    pub advanced: bool,
    #[serde(default)]
    pub rigid: bool,
    pub sources: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CouplerBranch {
    #[default]
    Slack,
    Loading,
    Unloading,
}

/// Persist the turning point and energy ledger through saves. The loading
/// envelope comes from OR's authored zones; the return-point law and energy
/// ledger are a passive Rust constitutive extension, not an OR oracle.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CouplerMemory {
    pub branch: CouplerBranch,
    pub previous_extension_m: f64,
    pub previous_force_n: f64,
    /// Actual rigid constraint reaction, independent of the elastic envelope.
    pub constraint_force_n: f64,
    pub reversal_extension_m: f64,
    pub reversal_force_n: f64,
    pub stored_energy_j: f64,
    pub absorbed_energy_j: f64,
    pub compression_absorbed_energy_j: f64,
    pub last_impulse_ns: f64,
    pub last_impact_energy_j: f64,
    pub impact_out_of_range: bool,
    pub impact_out_of_range_count: u64,
    pub warning_exceeded: bool,
    pub position_reference_m: Option<f64>,
    /// Axis used by the positive travel coordinates. None is an uninitialized
    /// link (or an old forward-axis save); force/history always remain physical.
    pub position_orientation: Option<f64>,
}
/// Per-vehicle kinematic state.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_profile: Option<NativeCouplerProfile>,
    #[serde(default)]
    pub memory: CouplerMemory,
}

impl CouplerState {
    /// A restore may change kinematics/history, never the author's hardware.
    /// Source paths are informational and can change when a library is moved.
    pub fn valid_for(&self, expected: &Self) -> bool {
        let hardware = [
            self.stiffness_n_per_m,
            self.damping_n_per_mps,
            self.free_play_m,
            self.break_force_n,
            self.max_force_n,
        ];
        let required = [
            expected.stiffness_n_per_m,
            expected.damping_n_per_mps,
            expected.free_play_m,
            expected.break_force_n,
            expected.max_force_n,
        ];
        if hardware != required
            || hardware.iter().any(|v| !v.is_finite() || *v < 0.)
            || !self.extension_m.is_finite()
            || self.extension_m.abs() > 1e6
        {
            return false;
        }
        let (max_force, maximum_stored_energy, lower_history, upper_history) =
            match (&self.native_profile, &expected.native_profile) {
                (None, None) => (
                    self.max_force_n.max(self.break_force_n).max(1e12),
                    1e18,
                    -1e6,
                    1e6,
                ),
                (Some(actual), Some(authored))
                    if actual.tension == authored.tension
                        && actual.compression == authored.compression
                        && actual.warning_force_n == authored.warning_force_n
                        && actual.advanced == authored.advanced
                        && actual.rigid == authored.rigid
                        && actual.tension.valid()
                        && actual.compression.valid()
                        && actual.warning_force_n.is_finite()
                        && actual.warning_force_n >= 0. =>
                {
                    if self.extension_m < -actual.compression.stroke_m() - 1e-6
                        || (!self.broken && self.extension_m > actual.tension.stroke_m() + 1e-6)
                    {
                        return false;
                    }
                    let maximum_force = actual
                        .tension
                        .max_force_n()
                        .max(actual.compression.max_force_n());
                    (
                        if actual.rigid {
                            maximum_force.max(if self.break_force_n > 0. {
                                self.break_force_n
                            } else {
                                1e12
                            })
                        } else {
                            maximum_force
                        },
                        maximum_force
                            * actual.tension.stroke_m().max(actual.compression.stroke_m()),
                        -actual.compression.stroke_m() - 1e-6,
                        if self.broken {
                            1e6
                        } else {
                            actual.tension.stroke_m() + 1e-6
                        },
                    )
                }
                _ => return false,
            };
        let m = &self.memory;
        [
            m.previous_extension_m,
            m.previous_force_n,
            m.reversal_extension_m,
            m.reversal_force_n,
            m.last_impulse_ns,
            m.constraint_force_n,
        ]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1e15)
            && [
                m.stored_energy_j,
                m.absorbed_energy_j,
                m.compression_absorbed_energy_j,
                m.last_impact_energy_j,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0. && *v <= 1e18)
            && m.compression_absorbed_energy_j <= m.absorbed_energy_j + 1e-6
            && m.stored_energy_j <= maximum_stored_energy + 1e-6
            && (lower_history..=upper_history).contains(&m.previous_extension_m)
            && (lower_history..=upper_history).contains(&m.reversal_extension_m)
            && m.previous_force_n.abs() <= max_force + 1e-6
            && m.reversal_force_n.abs() <= max_force + 1e-6
            && m.position_reference_m
                .is_none_or(|v| v.is_finite() && v.abs() <= 1e9)
            && m.position_orientation.is_none_or(|v| v == 1. || v == -1.)
    }
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
            native_profile: None,
            memory: CouplerMemory::default(),
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
            native_profile: None,
            memory: CouplerMemory::default(),
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
            native_profile: None,
            memory: CouplerMemory::default(),
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
            native_profile: None,
            memory: CouplerMemory::default(),
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

    /// Connect the leading vehicle's rear to the following vehicle's front.
    /// Missing author data uses explicit OR defaults, and two missing ends
    /// return None so the scenario's legacy preset remains unchanged.
    pub fn from_vehicle_ends(
        leading: &VehicleCouplerProfile,
        trailing: &VehicleCouplerProfile,
    ) -> Option<Self> {
        let a = leading.rear_in_consist();
        let b = trailing.front_in_consist();
        if a.is_none() && b.is_none() {
            return None;
        }
        let default = CouplerEndProfile::unauthored();
        let a = a.unwrap_or(&default);
        let b = b.unwrap_or(&default);
        let tension = CouplerCurve::in_series(&a.tension, &b.tension);
        let compression = CouplerCurve::in_series(&a.compression, &b.compression);
        let mut out = Self::freight();
        out.free_play_m = tension.slack_m().min(compression.slack_m());
        out.stiffness_n_per_m = tension
            .maximum_stiffness_npm()
            .max(compression.maximum_stiffness_npm());
        out.damping_n_per_mps = 0.; // OR's native force solver does not read MSTS Damping.
        out.max_force_n = tension.max_force_n().max(compression.max_force_n());
        out.break_force_n = a.break_force_n.min(b.break_force_n);
        out.native_profile = Some(NativeCouplerProfile {
            tension,
            compression,
            warning_force_n: a.warning_force_n.min(b.warning_force_n),
            advanced: a.advanced || b.advanced,
            rigid: a.rigid && b.rigid,
            sources: vec![a.source.clone(), b.source.clone()],
        });
        Some(out)
    }

    /// Stateful passive loading/unloading. Stiff unloading follows the largest
    /// authored tangent from the last turning point and rejoins the envelope;
    /// it can release less energy than loading supplied, never create energy.
    /// This Rust return-point extension is tested separately from native OR.
    pub fn advance_force_n(&mut self, delta_v_mps: f64, dt: f64, damping_scale: f64) -> f64 {
        let Some(profile) = &self.native_profile else {
            return self.force_n_scaled(delta_v_mps, damping_scale);
        };
        if self.broken && self.extension_m >= 0. {
            self.memory.absorbed_energy_j += self.memory.stored_energy_j;
            self.memory.stored_energy_j = 0.;
            self.memory.previous_force_n = 0.;
            self.memory.previous_extension_m = self.extension_m;
            self.memory.branch = CouplerBranch::Slack;
            return 0.;
        }
        let compression = self.extension_m < 0.;
        let sign = if compression { -1. } else { 1. };
        let curve = if compression {
            &profile.compression
        } else {
            &profile.tension
        };
        let x = self.extension_m.abs();
        let old_x = self.memory.previous_extension_m.abs();
        let same_side = self.extension_m * self.memory.previous_extension_m >= 0.;
        let envelope = curve.force_n(x);
        let branch = if envelope == 0. {
            CouplerBranch::Slack
        } else if x + 1e-12 < old_x && same_side {
            CouplerBranch::Unloading
        } else {
            CouplerBranch::Loading
        };
        if !same_side || branch == CouplerBranch::Slack {
            let lost = self.memory.stored_energy_j;
            self.memory.absorbed_energy_j += lost;
            if self.memory.previous_extension_m < 0. {
                self.memory.compression_absorbed_energy_j += lost;
            }
            self.memory.stored_energy_j = 0.;
            self.memory.reversal_extension_m = self.extension_m;
            self.memory.reversal_force_n = 0.;
        } else if branch != self.memory.branch {
            self.memory.reversal_extension_m = self.memory.previous_extension_m;
            self.memory.reversal_force_n = self.memory.previous_force_n;
        }
        let spring = if profile.advanced && self.memory.branch != CouplerBranch::Slack && same_side
        {
            let anchor_x = self.memory.reversal_extension_m.abs();
            let anchor_f = self.memory.reversal_force_n.abs();
            (anchor_f + curve.maximum_stiffness_npm() * (x - anchor_x)).clamp(0., envelope)
        } else {
            envelope
        };
        let damping = if envelope > 0. {
            self.damping_n_per_mps * delta_v_mps * damping_scale.max(0.)
        } else {
            0.
        };
        let force = (sign * spring + damping).clamp(
            -profile.compression.max_force_n(),
            profile.tension.max_force_n(),
        );
        let work = 0.5
            * (self.memory.previous_force_n + force)
            * (self.extension_m - self.memory.previous_extension_m);
        let heat = (damping * delta_v_mps * dt.max(0.)).max(0.);
        self.memory.stored_energy_j = (self.memory.stored_energy_j + work - heat).max(0.);
        self.memory.absorbed_energy_j += heat;
        if compression {
            self.memory.compression_absorbed_energy_j += heat;
        }
        // Once the returning gear has no restoring load, residual hysteresis
        // work became heat, rather than remaining fictitious stored energy.
        if spring <= 1e-9 && branch == CouplerBranch::Unloading {
            let lost = self.memory.stored_energy_j;
            self.memory.absorbed_energy_j += lost;
            if compression {
                self.memory.compression_absorbed_energy_j += lost;
            }
            self.memory.stored_energy_j = 0.;
        }
        self.memory.branch = branch;
        self.memory.previous_extension_m = self.extension_m;
        self.memory.previous_force_n = force;
        self.memory.warning_exceeded = force.abs() > profile.warning_force_n;
        if self.break_force_n > 0. && force.abs() > self.break_force_n {
            self.broken = true;
        }
        force
    }

    /// Compute the current coupler force (N) given the relative velocity between
    /// the two vehicles it connects.
    pub fn force_n(&self, delta_v_mps: f64) -> f64 {
        self.force_n_scaled(delta_v_mps, 1.0)
    }

    /// Like [`force_n`] but scales viscous damping (spring force unchanged).
    pub fn force_n_scaled(&self, delta_v_mps: f64, damping_scale: f64) -> f64 {
        if let Some(profile) = &self.native_profile {
            if self.broken && self.extension_m >= 0. {
                return 0.;
            }
            return if self.extension_m < 0. {
                -profile.compression.force_n(-self.extension_m)
            } else {
                profile.tension.force_n(self.extension_m)
            };
        }
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
    couplers.iter().all(|c| match &c.native_profile {
        Some(p) if c.extension_m < 0. => -c.extension_m <= p.compression.slack_m(),
        Some(p) => c.extension_m <= p.tension.slack_m(),
        None => c.extension_m.abs() <= c.free_play_m,
    })
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ContactImpulse {
    pub leading_velocity_mps: f64,
    pub trailing_velocity_mps: f64,
    /// Positive for tension, negative for compression; N·s, not N.
    pub impulse_ns: f64,
    pub absorbed_energy_j: f64,
}

/// A hard stroke stop removes only the closing/separating relative motion.
/// Absolute momentum is conserved. Compare velocities/impulse with OR's real
/// AddCouplerImpulseForces; the explicit energy ledger is a Rust extension.
pub fn inelastic_contact(
    leading_mass: f64,
    trailing_mass: f64,
    leading_velocity: f64,
    trailing_velocity: f64,
) -> ContactImpulse {
    let total = leading_mass + trailing_mass;
    let relative = leading_velocity - trailing_velocity;
    let reduced = leading_mass * trailing_mass / total;
    let common = (leading_mass * leading_velocity + trailing_mass * trailing_velocity) / total;
    ContactImpulse {
        leading_velocity_mps: common,
        trailing_velocity_mps: common,
        impulse_ns: reduced * relative,
        absorbed_energy_j: 0.5 * reduced * relative * relative,
    }
}

/// Active-face two-body acceleration constraint, in the tension-positive Rust
/// convention. OR's CouplerForceU has the opposite sign. Used by diagnostics
/// and exact DLL force comparisons; it is not an additional spring force.
pub fn equilibrating_coupler_force_n(
    leading_mass: f64,
    trailing_mass: f64,
    leading_force: f64,
    trailing_force: f64,
) -> f64 {
    (trailing_mass * leading_force - leading_mass * trailing_force) / (leading_mass + trailing_mass)
}

/// The active-face form of OR's acceleration constraint. A force cannot pull
/// through a compression face, push through a tension face, or act in slack.
#[allow(clippy::too_many_arguments)]
pub fn active_face_constraint_force_n(
    leading_mass: f64,
    trailing_mass: f64,
    leading_force: f64,
    trailing_force: f64,
    extension: f64,
    tension_slack: f64,
    compression_slack: f64,
) -> f64 {
    let force =
        equilibrating_coupler_force_n(leading_mass, trailing_mass, leading_force, trailing_force);
    if (extension >= tension_slack && force > 0.) || (extension <= -compression_slack && force < 0.)
    {
        force
    } else {
        0.
    }
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
/// With authored native profiles, include Davis/rolling friction in
/// `brake_forces` and pass only signed grade force in `grade_resist`. This
/// separates dissipative impulses from gravity and prevents friction from
/// driving a stopped train backwards. Legacy presets retain their old inputs.
///
/// Returns the **mass-weighted mean velocity** (m/s) for `state.velocity_mps`.
#[allow(clippy::too_many_arguments)]
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
    let mut motor_forces = vec![0.0; vehicles.len()];
    if let Some(first) = motor_forces.first_mut() {
        *first = f_motor;
    }
    multi_body_step_distributed(
        vehicles,
        couplers,
        &motor_forces,
        brake_forces,
        grade_resist,
        masses,
        dt,
        damping_scale,
    )
}

/// Apply each powered vehicle's force at its own position in the formation.
#[allow(clippy::too_many_arguments)]
pub fn multi_body_step_distributed(
    vehicles: &mut [VehicleState],
    couplers: &mut [CouplerState],
    motor_forces: &[f64],
    brake_forces: &[f64],
    grade_resist: &[f64],
    masses: &[f64],
    dt: f64,
    damping_scale: f64,
) -> f64 {
    multi_body_step_distributed_oriented(
        vehicles,
        couplers,
        motor_forces,
        brake_forces,
        grade_resist,
        masses,
        dt,
        damping_scale,
        1.,
    )
}

/// Native profiles retain physical tension/compression while LiveDrive may
/// integrate positive travel on a reversed virtual axis. Switch axes only with
/// every body stopped (within 0.1 m/s); a moving train retains its previous axis.
/// The legacy-only path keeps its established inputs and behavior.
#[allow(clippy::too_many_arguments)]
pub fn multi_body_step_distributed_oriented(
    vehicles: &mut [VehicleState],
    couplers: &mut [CouplerState],
    motor_forces: &[f64],
    brake_forces: &[f64],
    grade_resist: &[f64],
    masses: &[f64],
    dt: f64,
    damping_scale: f64,
    orientation: f64,
) -> f64 {
    let n = vehicles.len();
    if n == 0 {
        return 0.0;
    }
    if couplers.iter().any(|c| c.native_profile.is_some()) {
        return native_multi_body_step(
            vehicles,
            couplers,
            motor_forces,
            brake_forces,
            grade_resist,
            masses,
            dt,
            damping_scale,
            orientation,
        );
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

        let f_drive = motor_forces.get(i).copied().unwrap_or(0.0);
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

/// Native profiles may be much stiffer than the old gameplay presets. Resolve
/// their frequency locally instead of weakening the author's spring or raising
/// a global frame rate. Legacy-only formations keep their original integrator.
#[allow(clippy::too_many_arguments)]
fn native_multi_body_step(
    vehicles: &mut [VehicleState],
    couplers: &mut [CouplerState],
    motor: &[f64],
    brakes: &[f64],
    resistance: &[f64],
    masses: &[f64],
    dt: f64,
    damping_scale: f64,
    requested_orientation: f64,
) -> f64 {
    let n = vehicles.len();
    if !dt.is_finite()
        || dt <= 0.
        || masses.len() != n
        || brakes.len() != n
        || resistance.len() != n
        || masses.iter().any(|m| !m.is_finite() || *m <= 0.)
        || !matches!(requested_orientation, 1. | -1.)
    {
        return mass_weighted_mean_velocity(vehicles, masses);
    }
    let links = couplers.len().min(n.saturating_sub(1));
    let old_orientation = couplers.iter().find_map(|c| {
        c.memory
            .position_reference_m
            .map(|_| c.memory.position_orientation.unwrap_or(1.))
    });
    let orientation = if old_orientation.is_some_and(|old| old != requested_orientation)
        && vehicles.iter().any(|v| v.velocity_mps.abs() > 0.1)
    {
        old_orientation.unwrap()
    } else {
        requested_orientation
    };
    let mut maximum_step = 0.01_f64;
    for i in 0..links {
        if let Some(p) = &couplers[i].native_profile {
            let k = p
                .tension
                .maximum_stiffness_npm()
                .max(p.compression.maximum_stiffness_npm());
            let reduced = masses[i] * masses[i + 1] / (masses[i] + masses[i + 1]);
            if k > 0. && !p.rigid {
                maximum_step = maximum_step.min(0.15 * (reduced / k).sqrt());
            }
        }
        let reference = vehicles[i].position_m
            - vehicles[i + 1].position_m
            - orientation * couplers[i].extension_m;
        if couplers[i].memory.position_reference_m.is_none()
            || couplers[i].memory.position_orientation.unwrap_or(1.) != orientation
        {
            couplers[i].memory.position_reference_m = Some(reference);
        }
        couplers[i].memory.position_orientation = Some(orientation);
    }
    let steps = (dt / maximum_step.max(0.00001)).ceil().clamp(1., 100_000.) as usize;
    let h = dt / steps as f64;
    let mut forces = vec![0.; n];
    let mut rigid_start_force = vec![0.; links];
    let mut contact_impulse_ns = vec![0.; links];
    for i in 0..links {
        if couplers[i]
            .native_profile
            .as_ref()
            .is_some_and(|p| !p.rigid)
            && couplers[i].memory.previous_extension_m != couplers[i].extension_m
        {
            let dv = orientation * (vehicles[i].velocity_mps - vehicles[i + 1].velocity_mps);
            couplers[i].advance_force_n(dv, 0., damping_scale);
        }
    }
    for _ in 0..steps {
        collect_native_forces(
            vehicles,
            couplers,
            motor,
            resistance,
            masses,
            damping_scale,
            0.,
            false,
            orientation,
            &mut forces,
        );
        for i in 0..links {
            rigid_start_force[i] = couplers[i].memory.constraint_force_n;
            contact_impulse_ns[i] = 0.;
        }
        // Velocity Verlet: two half kicks around the position drift. Holding
        // an old spring force for the whole velocity update creates energy in
        // stiff buffers; the end kick uses the new force and history instead.
        for i in 0..n {
            let free_velocity = vehicles[i].velocity_mps + forces[i] / masses[i] * h * 0.5;
            vehicles[i].velocity_mps = free_velocity.signum()
                * (free_velocity.abs() - brakes[i].max(0.) / masses[i] * h * 0.5).max(0.);
            vehicles[i].position_m += vehicles[i].velocity_mps * h;
        }
        // Mass-weighted position projection enforces finite travel. Recompute
        // all adjacent gaps after each projection, including long formations.
        for _ in 0..(links.max(1) * 8) {
            let mut maximum_error = 0_f64;
            for i in 0..links {
                let Some(p) = &couplers[i].native_profile else {
                    continue;
                };
                let reference = couplers[i].memory.position_reference_m.unwrap_or(0.);
                let x =
                    orientation * (vehicles[i].position_m - vehicles[i + 1].position_m - reference);
                let lower = -p.compression.stroke_m();
                let upper = p.tension.stroke_m();
                let bounded = if couplers[i].broken {
                    x.max(lower)
                } else {
                    x.clamp(lower, upper)
                };
                let error = x - bounded;
                maximum_error = maximum_error.max(error.abs());
                if error.abs() <= 1e-10 {
                    continue;
                }
                let total = masses[i] + masses[i + 1];
                vehicles[i].position_m -= orientation * error * masses[i + 1] / total;
                vehicles[i + 1].position_m += orientation * error * masses[i] / total;
                let dv = orientation * (vehicles[i].velocity_mps - vehicles[i + 1].velocity_mps);
                if (error > 0. && dv > 0.) || (error < 0. && dv < 0.) {
                    let response = inelastic_contact(
                        masses[i],
                        masses[i + 1],
                        vehicles[i].velocity_mps,
                        vehicles[i + 1].velocity_mps,
                    );
                    let capacity = if error < 0. {
                        p.compression.elastic_energy_j(p.compression.stroke_m())
                    } else {
                        p.tension.elastic_energy_j(p.tension.stroke_m())
                    };
                    vehicles[i].velocity_mps = response.leading_velocity_mps;
                    vehicles[i + 1].velocity_mps = response.trailing_velocity_mps;
                    let warning_force = p.warning_force_n;
                    let memory = &mut couplers[i].memory;
                    memory.last_impulse_ns = orientation * response.impulse_ns;
                    contact_impulse_ns[i] += orientation * response.impulse_ns;
                    memory.last_impact_energy_j = response.absorbed_energy_j;
                    memory.absorbed_energy_j += response.absorbed_energy_j;
                    if error < 0. {
                        memory.compression_absorbed_energy_j += response.absorbed_energy_j;
                    }
                    memory.impact_out_of_range = response.absorbed_energy_j > capacity + 1e-6;
                    if memory.impact_out_of_range {
                        memory.impact_out_of_range_count =
                            memory.impact_out_of_range_count.saturating_add(1);
                    }
                    memory.warning_exceeded |= response.impulse_ns.abs() / h > warning_force;
                    // A fractured draft connection still has physical buffers.
                    if error > 0.
                        && couplers[i].break_force_n > 0.
                        && response.impulse_ns.abs() / h > couplers[i].break_force_n
                    {
                        couplers[i].broken = true;
                    }
                }
            }
            if maximum_error < 1e-8 {
                break;
            }
        }
        for i in 0..links {
            couplers[i].extension_m = orientation
                * (vehicles[i].position_m
                    - vehicles[i + 1].position_m
                    - couplers[i].memory.position_reference_m.unwrap_or(0.));
        }
        collect_native_forces(
            vehicles,
            couplers,
            motor,
            resistance,
            masses,
            damping_scale,
            h,
            true,
            orientation,
            &mut forces,
        );
        for i in 0..n {
            let free_velocity = vehicles[i].velocity_mps + forces[i] / masses[i] * h * 0.5;
            vehicles[i].velocity_mps = free_velocity.signum()
                * (free_velocity.abs() - brakes[i].max(0.) / masses[i] * h * 0.5).max(0.);
        }
        for i in 0..links {
            let Some(profile) = couplers[i].native_profile.as_ref().filter(|p| p.rigid) else {
                continue;
            };
            // Constraint reaction includes its continuous half kicks and any
            // stroke contact impulse. This also captures unequal braking in
            // mixed rigid/elastic formations without inventing spring load.
            let reaction = 0.5 * (rigid_start_force[i] + couplers[i].memory.constraint_force_n)
                + contact_impulse_ns[i] / h;
            let peak = reaction.abs().max((contact_impulse_ns[i] / h).abs());
            let warning_exceeded = peak > profile.warning_force_n;
            couplers[i].memory.constraint_force_n = reaction;
            couplers[i].memory.warning_exceeded = warning_exceeded;
            if couplers[i].break_force_n > 0. && peak > couplers[i].break_force_n {
                couplers[i].broken = true;
            }
        }
    }
    mass_weighted_mean_velocity(vehicles, masses)
}

#[allow(clippy::too_many_arguments)]
fn collect_native_forces(
    vehicles: &[VehicleState],
    couplers: &mut [CouplerState],
    motor: &[f64],
    resistance: &[f64],
    masses: &[f64],
    damping_scale: f64,
    dt: f64,
    advance_history: bool,
    orientation: f64,
    forces: &mut [f64],
) {
    for (i, force) in forces.iter_mut().enumerate() {
        // Dissipative brake/rolling impulses are applied separately from the
        // signed external drive and gravity forces.
        *force = motor.get(i).copied().unwrap_or(0.) - resistance[i];
    }
    for i in 0..couplers.len().min(vehicles.len().saturating_sub(1)) {
        let dv = orientation * (vehicles[i].velocity_mps - vehicles[i + 1].velocity_mps);
        let f = if let Some(p) = couplers[i].native_profile.as_ref().filter(|p| p.rigid) {
            let f = if couplers[i].broken && couplers[i].extension_m >= 0. {
                0.
            } else {
                active_face_constraint_force_n(
                    masses[i],
                    masses[i + 1],
                    orientation * forces[i],
                    orientation * forces[i + 1],
                    couplers[i].extension_m,
                    p.tension.slack_m(),
                    p.compression.slack_m(),
                )
            };
            let bounded = f.clamp(-p.compression.max_force_n(), p.tension.max_force_n());
            couplers[i].memory.constraint_force_n = f;
            couplers[i].memory.previous_force_n = bounded;
            couplers[i].memory.previous_extension_m = couplers[i].extension_m;
            f
        } else if couplers[i].native_profile.is_none() || advance_history {
            couplers[i].advance_force_n(dv, dt, damping_scale)
        } else if couplers[i].broken && couplers[i].extension_m >= 0. {
            0.
        } else {
            couplers[i].memory.previous_force_n
        };
        forces[i] -= orientation * f;
        forces[i + 1] += orientation * f;
    }
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
            vehicles
                .iter()
                .all(|v| v.velocity_mps.is_finite() && v.velocity_mps < 35.0),
            "front={} rear={}",
            vehicles[0].velocity_mps,
            vehicles[1].velocity_mps
        );
    }
}
