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

use crate::legacy_ep_brake::LegacyEpState;
use crate::native_ep_brake::{NativeAirState, NativeEpState, PSI_TO_BAR};
use crate::native_vacuum_brake::NativeVacuumState;
use openrailsrs_formats::{BrakeShoeFrictionCurve, resolve_brake_shoe_curve};
use openrailsrs_train::{Consist, Vehicle};
use serde::{Deserialize, Serialize};

/// State of one brake cylinder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrakeCylinder {
    #[serde(default)]
    automatic_pipe_bar: Option<f64>,
    #[serde(default = "default_full_pressure_bar")]
    pub full_pressure_bar: f64,
    #[serde(default)]
    authored_response: bool,
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
    #[serde(default)]
    independent_force_n: f64,
    /// Parking brake force, independent of train air.
    #[serde(default)]
    pub handbrake_force_n: f64,
    /// An open disconnected hose vents this vehicle and everything behind it.
    #[serde(default)]
    pub air_vented: bool,
    /// Closed angle cocks retain cylinder pressure and isolate driver commands.
    #[serde(default)]
    pub air_isolated: bool,
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
    /// Legacy OR curve on nominal MaxBrakeForce, resolved from the content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_legacy_shoe_curve: Option<BrakeShoeFrictionCurve>,
    /// Vehicle mass for OR-P6c skid cap (kg).
    mass_kg: f64,
    /// Wheel-rail adhesion μ; 0 disables skid limit on this cylinder.
    skid_adhesion_mu: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_ep: Option<NativeEpState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_air: Option<NativeAirState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_vacuum: Option<NativeVacuumState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_ep: Option<LegacyEpState>,
    #[serde(default)]
    transfer_only: bool,
    #[serde(default)]
    native_air_skid: bool,
    /// Car.Update renews these after the locomotive's adhesion tick. Native
    /// Pacha therefore consumes the previous shoe retard, including at start.
    #[serde(default)]
    native_retard_last_n: f64,
    #[serde(default)]
    native_rail_brake_last_n: f64,
}

/// Per-vehicle brake cylinder specification for [`BrakeSystem`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrakeVehicleSpec {
    pub profile: openrailsrs_formats::VehicleBrakeProfile,
    pub position_m: f64,
    pub max_force_n: f64,
    pub ep_instant: bool,
    pub shoe_friction: BrakeShoeFrictionCurve,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_legacy_shoe_curve: Option<BrakeShoeFrictionCurve>,
    pub mass_kg: f64,
    /// Wheel-rail μ cap when skid limit enabled; 0 = no cap (OR-P6c off).
    pub skid_adhesion_mu: f64,
}

/// Normalize only inactive, newly recognized plain-valve candidates before
/// computing the historical Debug-based content signature. This preserves
/// old EP-led saves; active native equipment remains part of the identity.
pub fn legacy_brake_signature_consist(consist: &Consist) -> Consist {
    let mut normalized = consist.clone();
    let active = consist.vehicles.first().is_some_and(|v| match v {
        Vehicle::Loco(l) => plain_train_driver(&l.brake_profile),
        Vehicle::Wagon(_) => false,
    });
    if !active {
        for v in &mut normalized.vehicles {
            let p = match v {
                Vehicle::Loco(l) => &mut l.brake_profile,
                Vehicle::Wagon(w) => &mut w.brake_profile,
            };
            if p.native_air.as_ref().is_some_and(|a| a.native_plain) {
                p.native_air = None;
            }
        }
    }
    normalized
}
fn plain_train_driver(p: &openrailsrs_formats::VehicleBrakeProfile) -> bool {
    p.native_air.as_ref().is_some_and(|a| {
        a.native_plain
            && a.controller
                .notched
                .as_ref()
                .is_some_and(|n| n.main_reservoir.is_some())
    }) && p.native_ep.is_none()
        && p.legacy_ep.is_none()
}
impl BrakeCylinder {
    pub(crate) fn connection_compatible(&self, other: &Self) -> bool {
        self.native_vacuum.is_some() == other.native_vacuum.is_some()
            && (self.full_pressure_bar - other.full_pressure_bar).abs() <= 0.01
    }
}

/// OR `Train.WagonCoefficientFriction` dry default for brake adhesion cap.
pub const OR_DEFAULT_BRAKE_ADHESION_MU: f64 = 0.25;

const DEFAULT_VEHICLE_LENGTH_M: f64 = 15.0;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PneumaticEmergencyStatus {
    pub pipe_psi: f64,
    pub auxiliary_psi: f64,
    pub emergency_reservoir_psi: f64,
    pub control_psi: f64,
    pub cylinder_psi: f64,
    pub valve: String,
    pub dump_open: bool,
}
fn default_full_pressure_bar() -> f64 {
    4.5
}

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
            let (force_n, ep, shoe_type, user_curve, mass_kg, profile) = match v {
                Vehicle::Loco(l) => (
                    l.max_brake_force_n,
                    true,
                    &l.brake_shoe_type,
                    &l.brake_shoe_friction,
                    l.mass_kg,
                    &l.brake_profile,
                ),
                Vehicle::Wagon(w) => (
                    w.max_brake_force_n,
                    false,
                    &w.brake_shoe_type,
                    &w.brake_shoe_friction,
                    w.mass_kg,
                    &w.brake_profile,
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
            let mut profile = profile.clone();
            if let Some(e) = profile
                .native_air
                .as_mut()
                .and_then(|p| p.emergency.as_mut())
                && !e.pipe_volume_authored
            {
                e.pipe_volume_m3 =
                    0.032f64.powi(2) * std::f64::consts::PI / 4. * (length_m + 1.).max(5.);
            }
            if let Some(ep) = profile.legacy_ep.as_mut() {
                ep.pipe_volume_m3.get_or_insert(
                    0.032f64.powi(2) * std::f64::consts::PI / 4. * (length_m + 1.).max(5.),
                );
            }
            if let Some(vacuum) = profile.native_vacuum.as_mut() {
                vacuum.engine = matches!(v, Vehicle::Loco(_));
                vacuum.steam = matches!(v, Vehicle::Loco(l) if l.steam.is_some());
                vacuum.pipe_volume_m3.get_or_insert(
                    0.05f64.powi(2) * std::f64::consts::PI / 4. * (length_m + 1.).max(5.),
                );
            }
            let native_legacy_shoe_curve = profile
                .native_air
                .as_ref()
                .filter(|p| p.native_plain && p.cylinder.shoe_type.is_empty())
                .and(user_curve.clone());
            let ep_instant = profile.electro_pneumatic().unwrap_or(ep);
            BrakeVehicleSpec {
                native_legacy_shoe_curve,
                profile,
                position_m: cylinder_pos,
                max_force_n: force_n,
                ep_instant,
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
            full_pressure_bar: default_full_pressure_bar(),
            authored_response: false,
            position_m,
            max_force_n,
            ep_instant,
            state: BrakeState::Charged,
            current_force_n: 0.0,
            independent_force_n: 0.0,
            handbrake_force_n: 0.0,
            air_vented: false,
            automatic_pipe_bar: Some(5.0),
            air_isolated: false,
            time_pending_s: 0.0,
            apply_ramp_rate_n_per_s: max_force_n / apply_time_s,
            release_ramp_rate_n_per_s: max_force_n / release_time_s,
            latched_command: 0.0,
            shoe_friction,
            native_legacy_shoe_curve: None,
            mass_kg,
            skid_adhesion_mu,
            native_ep: None,
            native_air: None,
            native_vacuum: None,
            legacy_ep: None,
            transfer_only: false,
            native_air_skid: false,
            native_retard_last_n: 0.,
            native_rail_brake_last_n: 0.,
        }
    }

    /// Shoe retard force before wheel skid, as supplied to the native axle.
    pub fn retard_force_n(&self, speed_mps: f64) -> f64 {
        // TrainCar.GetBrakeShoeFrictionFactor (OR 1.6.1): legacy
        // MaxBrakeForce already includes a nominal CoF of 0.2. This is not
        // Karwatzki's absolute shoe CoF, nor the optional generic P6 curve.
        let native_legacy = self
            .native_air
            .as_ref()
            .is_some_and(|a| a.profile.native_plain && a.cylinder.profile.shoe_type.is_empty());
        let coefficient = if native_legacy {
            self.native_legacy_shoe_curve.as_ref().map_or_else(
                || {
                    let speed_kph = speed_mps.abs() as f32 * 3.6;
                    f64::from(((7.6 / (speed_kph + 17.5) + 0.07) / 0.2).clamp(0.01, 1.))
                },
                |curve| curve.native_legacy_speed_factor(speed_mps),
            )
        } else {
            self.native_air
                .as_ref()
                .map(|air| &air.cylinder)
                .or(self.native_ep.as_ref())
                .and_then(|ep| {
                    ep.friction_coefficient(
                        self.current_force_n.max(self.independent_force_n),
                        speed_mps,
                    )
                })
                .unwrap_or_else(|| self.shoe_friction.speed_factor(speed_mps))
        };
        (self.current_force_n.max(self.independent_force_n) * coefficient)
            .max(self.handbrake_force_n)
    }

    /// Wheel-rim braking force after shoe friction and the current skid state.
    pub fn effective_force_n(&self, speed_mps: f64) -> f64 {
        let shoe = self.retard_force_n(speed_mps);
        if self.native_air.is_some() {
            // OR TrainCar.UpdateBrakeSlideCalculation derives a diesel's skid
            // from axle slip; AirSinglePipe uses 0.08 at the shoe while sliding.
            return if self.native_air_skid {
                self.current_force_n.max(self.independent_force_n) * 0.08
            } else {
                shoe
            };
        }
        if self.native_air.is_none() && self.skid_adhesion_mu > 0.0 && self.mass_kg > 0.0 {
            shoe.min(self.mass_kg * 9.81 * self.skid_adhesion_mu)
        } else {
            shoe
        }
    }

    pub(crate) fn set_native_air_skid(&mut self, skid: bool) {
        self.native_air_skid = self.native_air.is_some()
            && skid
            && self.current_force_n.max(self.independent_force_n) > 25.;
    }
    /// WSP vents service pressure; independent and hand brakes remain separate.
    pub fn limit_service_fraction(&mut self, fraction: f64) {
        let factor = fraction.clamp(0., 1.);
        if let Some(c) = &mut self.native_ep {
            c.vent_fraction(factor);
        }
        if let Some(c) = &mut self.native_air {
            c.cylinder.vent_fraction(factor);
        }
        if let Some(c) = &mut self.legacy_ep {
            c.pressure_psi *= factor;
            c.auto_psi *= factor;
        }
        self.current_force_n *= factor;
    }
    pub fn pipe_pressure_bar(&self) -> Option<f64> {
        if let Some(vacuum) = &self.native_vacuum {
            return Some(
                (crate::native_vacuum_brake::ATMOSPHERE_PSI - vacuum.pipe_psi).max(0.) * PSI_TO_BAR,
            );
        }
        self.native_air
            .as_ref()
            .map(|air| air.pipe_psi * PSI_TO_BAR)
            .or(self.automatic_pipe_bar)
    }

    pub fn is_vacuum(&self) -> bool {
        self.native_vacuum.is_some()
    }

    pub fn pressure_bar(&self) -> f64 {
        if let Some(a) = &self.native_air
            && a.emergency.is_some()
        {
            return a.cylinder.pressure_psi.max(a.independent_psi) * PSI_TO_BAR;
        }
        self.service_pressure_bar()
            .max(self.independent_force_n / self.max_force_n.max(1.0) * self.full_pressure_bar)
    }
    pub(crate) fn independent_state_valid(&self) -> bool {
        let independent_max = self
            .native_air
            .as_ref()
            .filter(|a| a.emergency.is_some())
            .map_or(self.max_force_n, |a| {
                self.max_force_n
                    * ((self.full_pressure_bar / PSI_TO_BAR - a.cylinder.profile.spring_psi)
                        / (a.cylinder.profile.reference_psi - a.cylinder.profile.spring_psi))
                        .max(0.)
            });
        self.independent_force_n.is_finite()
            && self.native_retard_last_n.is_finite()
            && self.native_retard_last_n >= 0.
            && self.native_rail_brake_last_n.is_finite()
            && self.native_rail_brake_last_n >= 0.
            && (0.0..=independent_max + 1e-6).contains(&self.independent_force_n)
            && self
                .automatic_pipe_bar
                .is_none_or(|pressure| pressure.is_finite() && (0.0..=10.0).contains(&pressure))
            && self
                .native_vacuum
                .as_ref()
                .is_none_or(NativeVacuumState::valid)
            && self.native_air.as_ref().is_none_or(NativeAirState::valid)
    }

    /// Native hardware and pipe capacity are fixed by the loaded formation.
    /// A save may preserve pressure, but cannot replace its brake equipment.
    pub(crate) fn vacuum_hardware_compatible(&self, expected: &Self) -> bool {
        let air = match (&self.native_air, &expected.native_air) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                serde_json::to_value(&a.profile).ok() == serde_json::to_value(&b.profile).ok()
            }
            _ => false,
        };
        air && self.native_legacy_shoe_curve == expected.native_legacy_shoe_curve
            && match (&self.native_vacuum, &expected.native_vacuum) {
                (None, None) => true,
                (Some(saved), Some(expected)) => {
                    // Older saves predate role metadata. Roles only select pipe 3
                    // at construction; the resolved controller is checked below.
                    let mut profile = saved.profile.clone();
                    profile.engine = expected.profile.engine;
                    profile.steam = expected.profile.steam;
                    profile.tender = expected.profile.tender;
                    profile == expected.profile
                        && (saved.pipe_volume_m3 - expected.pipe_volume_m3).abs() < 1e-9
                }
                _ => false,
            }
    }
    pub fn emergency_pneumatic_status(&self) -> Option<PneumaticEmergencyStatus> {
        let a = self.native_air.as_ref()?;
        let e = a.emergency.as_ref()?;
        Some(PneumaticEmergencyStatus {
            pipe_psi: a.pipe_psi,
            auxiliary_psi: a.cylinder.auxiliary_pressure_psi(),
            emergency_reservoir_psi: e.reservoir_psi,
            control_psi: e.control_psi,
            cylinder_psi: a.cylinder.pressure_psi.max(a.independent_psi),
            valve: format!("{:?}", e.valve),
            dump_open: e.dump_open,
        })
    }
    /// Initialize a released native system from the train's starting EQ
    /// pressure, as AirSinglePipe.Initialize(false, max, fullService, true).
    /// Subsequent pipe/reservoir evolution is not prescribed by this operation.
    pub fn initialize_released_pneumatic_pressure_psi(
        &mut self,
        equalizing_psi: f64,
    ) -> Result<(), String> {
        if !equalizing_psi.is_finite() || !(0. ..=200.).contains(&equalizing_psi) {
            return Err("Presión inicial de tubería neumática inválida".into());
        }
        let a = self
            .native_air
            .as_mut()
            .filter(|a| a.emergency.is_some())
            .ok_or("Freno sin válvula de emergencia avanzada")?;
        a.initialize_released(equalizing_psi);
        self.automatic_pipe_bar = Some(equalizing_psi * PSI_TO_BAR);
        self.independent_force_n = 0.;
        self.current_force_n = a.cylinder.shoe_force_n(self.max_force_n);
        self.state = BrakeState::Charged;
        self.time_pending_s = 0.;
        Ok(())
    }
    /// Subsystem oracle input: prescribe only the external pipe boundary. All
    /// valve, reservoir, cylinder and timer states evolve in the runtime model.
    pub fn advance_prescribed_pneumatic_boundary(
        &mut self,
        pipe_psi: f64,
        dt: f64,
    ) -> Result<(), String> {
        if !pipe_psi.is_finite() || !(0. ..=200.).contains(&pipe_psi) || !dt.is_finite() || dt <= 0.
        {
            return Err("Condición de borde neumática inválida".into());
        }
        let a = self
            .native_air
            .as_mut()
            .filter(|a| a.emergency.is_some())
            .ok_or("Freno sin válvula de emergencia avanzada")?;
        a.pipe_psi = pipe_psi;
        a.step_with_emergency(0., false, dt);
        self.current_force_n = a.cylinder.shoe_force_n(self.max_force_n);
        Ok(())
    }
    /// Test/reference pipe 3 input, never a replacement for the live driver's
    /// finite independent circuit. Used only by prescribed-boundary oracles.
    pub fn prescribe_independent_pressure_psi(&mut self, psi: f64) -> Result<(), String> {
        if !psi.is_finite() || !(0. ..=200.).contains(&psi) {
            return Err("Presión de tubería 3 inválida".into());
        }
        let a = self
            .native_air
            .as_mut()
            .filter(|a| a.emergency.is_some())
            .ok_or("Freno sin modelo avanzado")?;
        a.independent_psi = psi;
        Ok(())
    }
    fn service_pressure_bar(&self) -> f64 {
        if let Some(ep) = &self.legacy_ep {
            return ep.pressure_psi * PSI_TO_BAR;
        }
        if let Some(vacuum) = &self.native_vacuum {
            return (crate::native_vacuum_brake::ATMOSPHERE_PSI - vacuum.cylinder_psi).max(0.)
                * PSI_TO_BAR;
        }
        self.native_air
            .as_ref()
            .map(|air| &air.cylinder)
            .or(self.native_ep.as_ref())
            .map_or_else(
                || {
                    (self.current_force_n / self.max_force_n.max(1.)).clamp(0., 1.)
                        * self.full_pressure_bar
                },
                |ep| ep.pressure_psi.max(0.) * PSI_TO_BAR,
            )
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrakeSystem {
    #[serde(default)]
    pub circuits: Vec<crate::brake_circuit::AirCircuit>,
    #[serde(default)]
    pub independent_command: f64,
    #[serde(default)]
    pub emergency: bool,
    pub cylinders: Vec<BrakeCylinder>,
    /// Speed at which the brake-pipe pressure change propagates (m/s).
    pub pipe_speed_mps: f64,
    /// Magnitude of the previous command (0.0 = released).
    prev_command: f64,
    /// Hold wagon cylinders at peak application during lap release (Chiltern activity start).
    train_air_lap_hold: bool,
    /// Seconds to dump train-air after driver full release when lap-hold is enabled.
    train_air_full_release_s: f64,
    /// EP-only service wire does not reduce the pneumatic pipe pressure.
    #[serde(default)]
    ep_only_wire: bool,
}

impl BrakeSystem {
    // Paste these methods INSIDE impl BrakeSystem in brake.rs (private fields
    // remain private; no native_steam access expansion is required).
    // Root owns this integration. Only called for an explicitly selected native
    // steam lead at catalog position0; other brake paths retain their callsites.
    fn native_steam_vacuum_cells(
        &self,
    ) -> Result<Vec<crate::native_steam::NativeSteamVacuumCar>, String> {
        self.cylinders
            .iter()
            .map(|c| {
                let v = c
                    .native_vacuum
                    .as_ref()
                    .ok_or("Native steam requires vacuum equipment on every coupled car")?;
                if v.profile.independent.is_some() {
                    return Err(
                        "Independent vacuum is outside this bounded native steam profile".into(),
                    );
                }
                Ok(crate::native_steam::NativeSteamVacuumCar {
                    pipe_psi: v.pipe_psi as f32,
                    cylinder_psi: v.cylinder_psi as f32,
                    reservoir_psi: v.reservoir_psi as f32,
                    pipe_volume_m3: v.pipe_volume_m3 as f32,
                    cylinder_volume_m3: v.profile.cylinder_volume_m3 as f32,
                    reservoir_volume_m3: v.profile.reservoir_volume_m3 as f32,
                    max_force_psi: v.profile.max_force_psi as f32,
                    application_psi_s: v.profile.application_psi_s as f32,
                    release_psi_s: v.profile.release_psi_s as f32,
                    direct_admission: v.profile.direct_admission,
                    isolated: c.air_isolated,
                    vented: c.air_vented,
                    bleed: false,
                    cylinder_fraction: v.brake_cyl_fraction as f32,
                })
            })
            .collect()
    }
    fn set_native_steam_vacuum_cells(
        &mut self,
        cells: &[crate::native_steam::NativeSteamVacuumCar],
    ) {
        for (c, cell) in self.cylinders.iter_mut().zip(cells) {
            let v = c
                .native_vacuum
                .as_mut()
                .expect("prevalidated native vacuum equipment");
            v.pipe_psi = cell.pipe_psi as f64;
            v.cylinder_psi = cell.cylinder_psi as f64;
            v.reservoir_psi = cell.reservoir_psi as f64;
            v.brake_cyl_fraction = cell.cylinder_fraction as f64;
            c.current_force_n = ((c.max_force_n as f32) * cell.shoe_fraction()) as f64;
            c.time_pending_s = 0.;
            c.state = if cell.shoe_fraction() > 0. {
                BrakeState::Applied
            } else {
                BrakeState::Charged
            };
        }
    }
    pub fn initialize_native_steam_vacuum(
        &mut self,
        native: &mut crate::native_steam::NativeSteamState,
    ) -> Result<(), String> {
        let mut cells = self.native_steam_vacuum_cells()?;
        if cells.is_empty() {
            return Err("Native steam requires a coupled vacuum brake network".into());
        }
        native.initialize_vacuum_network(&mut cells);
        self.set_native_steam_vacuum_cells(&cells);
        self.prev_command = native.hardware.source.initial_train_brake as f64;
        Ok(())
    }
    pub fn step_native_steam_vacuum(
        &mut self,
        native: &mut crate::native_steam::NativeSteamState,
        command: f64,
        dt: f64,
        height_m: f64,
    ) -> Result<(), String> {
        let mut cells = self.native_steam_vacuum_cells()?;
        native.advance_vacuum_network(
            &mut cells,
            command as f32,
            self.emergency,
            height_m as f32,
            dt as f32,
        )?;
        self.set_native_steam_vacuum_cells(&cells);
        self.prev_command = command;
        Ok(())
    }

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
        // Original plain-valve metadata is a candidate until a compatible
        // locomotive supplies the train brake controller. An EP-led formation
        // must keep its established service-wire path.
        let plain_mode = vehicles
            .first()
            .is_some_and(|v| plain_train_driver(&v.profile));
        let lead_vacuum = vehicles
            .first()
            .and_then(|v| v.profile.native_vacuum.as_ref());
        let mut independent_last = 0;
        if let Some(lead) = lead_vacuum.filter(|p| p.engine && p.independent.is_some()) {
            while vehicles
                .get(independent_last + 1)
                .is_some_and(|v| v.profile.native_vacuum.as_ref().is_some_and(|p| p.engine))
            {
                independent_last += 1;
            }
            if lead.steam {
                if independent_last > 0
                    && vehicles[independent_last]
                        .profile
                        .native_vacuum
                        .as_ref()
                        .is_some_and(|p| p.steam)
                {
                    independent_last = 0;
                } else if independent_last == 0
                    && vehicles
                        .get(1)
                        .is_some_and(|v| v.profile.native_vacuum.as_ref().is_some_and(|p| p.tender))
                {
                    independent_last = 1;
                }
            }
        }
        let cylinders = vehicles
            .iter()
            .enumerate()
            .map(|(index, v)| {
                let mut cylinder = BrakeCylinder::new(
                    v.position_m,
                    v.max_force_n,
                    v.ep_instant,
                    v.shoe_friction.clone(),
                    v.mass_kg,
                    v.skid_adhesion_mu,
                );
                if plain_mode {
                    cylinder.native_legacy_shoe_curve = v.native_legacy_shoe_curve.clone();
                }
                cylinder.native_ep = v.profile.native_ep.clone().map(NativeEpState::new);
                cylinder.legacy_ep = v.profile.legacy_ep.clone().map(|mut p| {
                    if let Some(lead) = vehicles.first().and_then(|v| v.profile.legacy_ep.as_ref())
                    {
                        p.charged_psi = lead.charged_psi;
                    }
                    LegacyEpState::new(p, 15.)
                });
                cylinder.native_vacuum = v.profile.native_vacuum.clone().map(|mut profile| {
                    if let Some(lead) = lead_vacuum {
                        profile.charged_vacuum_psi = lead.charged_vacuum_psi;
                    }
                    profile.independent = (index <= independent_last)
                        .then(|| {
                            lead_vacuum
                                .filter(|p| p.engine)
                                .and_then(|p| p.independent.clone())
                        })
                        .flatten();
                    NativeVacuumState::new(profile, 15.)
                });
                cylinder.transfer_only = v.profile.system.as_ref().is_some_and(|s| {
                    matches!(
                        s.trim().to_ascii_lowercase().as_str(),
                        "vacuum_piped" | "air_piped"
                    )
                });
                cylinder.native_air = v
                    .profile
                    .native_air
                    .clone()
                    .filter(|p| !p.native_plain || plain_mode)
                    .map(|mut profile| {
                        let own_reservoir = profile
                            .controller
                            .notched
                            .as_ref()
                            .and_then(|n| n.main_reservoir.clone());
                        let own_compressor_rpm = profile
                            .controller
                            .notched
                            .as_ref()
                            .and_then(|n| n.mechanical_compressor_max_rpm);
                        if let Some(lead) =
                            vehicles.first().and_then(|v| v.profile.native_air.as_ref())
                        {
                            profile.controller = lead.controller.clone();
                        }
                        if let Some(n) = &mut profile.controller.notched {
                            // EQ and the brake handle follow the lead, while
                            // each locomotive keeps its own compressor and MR.
                            // Pipe 3 fitting still follows the lead controller.
                            n.main_reservoir = own_reservoir;
                            n.mechanical_compressor_max_rpm = own_compressor_rpm;
                        }
                        NativeAirState::new(profile)
                    });
                if let Some(pressure) = v.profile.max_cylinder_bar {
                    cylinder.full_pressure_bar = pressure;
                }
                if let Some(rate) = v.profile.application_bar_s {
                    cylinder.apply_ramp_rate_n_per_s =
                        v.max_force_n * rate / cylinder.full_pressure_bar;
                    cylinder.authored_response = true;
                }
                if let Some(rate) = v.profile.release_bar_s {
                    cylinder.release_ramp_rate_n_per_s =
                        v.max_force_n * rate / cylinder.full_pressure_bar;
                    cylinder.authored_response = true;
                }
                cylinder
            })
            .collect();
        let mut system = Self {
            circuits: vehicles
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    plain_mode
                        .then(|| {
                            v.profile
                                .native_air
                                .as_ref()
                                .and_then(|a| a.controller.notched.as_ref())
                                .and_then(|n| n.main_reservoir.clone())
                        })
                        .flatten()
                        .or_else(|| v.profile.air_circuit.clone())
                        .map(|p| crate::brake_circuit::AirCircuit::new(i, p))
                })
                .collect(),
            independent_command: 0.0,
            emergency: false,
            cylinders,
            pipe_speed_mps,
            prev_command: 0.0,
            train_air_lap_hold,
            train_air_full_release_s: train_air_full_release_s.max(0.5),
            ep_only_wire: vehicles
                .first()
                .is_some_and(|v| v.profile.native_ep.is_some() || v.profile.legacy_ep.is_some()),
        };
        if let Some(handle) = system.native_start_handle() {
            system.reset_native_plain_brakes(handle, false);
        }
        system
    }

    pub fn has_independent_brake(&self) -> bool {
        self.circuits.iter().any(|c| {
            self.cylinders
                .get(c.vehicle)
                .and_then(|v| v.native_air.as_ref())
                .and_then(|a| a.profile.controller.notched.as_ref())
                .is_none_or(|n| n.independent_fitted)
        }) || self.cylinders.iter().any(|c| {
            c.native_vacuum
                .as_ref()
                .is_some_and(|v| v.profile.independent.is_some())
        })
    }

    /// Actual authored Brake_Train handle used by native initialization.
    /// This is not a percentage sent through MSTS SetPercent's rescaling.
    pub fn native_start_handle(&self) -> Option<f64> {
        self.cylinders
            .first()?
            .native_air
            .as_ref()
            .filter(|a| a.profile.native_plain)?
            .profile
            .controller
            .notched
            .as_ref()
            .map(|n| n.initial_value)
    }

    /// Train.UnconditionalInitializeBrakes for supported plain equipment.
    /// Derives the initial EQ/pipe/auxiliary/cylinder state from the authored
    /// controller and volumes, never from measured oracle output.
    pub fn reset_native_plain_brakes(&mut self, handle: f64, immediate_release: bool) -> bool {
        if self.native_start_handle().is_none()
            || !handle.is_finite()
            || !(0. ..=1.).contains(&handle)
        {
            return false;
        }
        let main = self
            .circuits
            .iter()
            .find(|c| c.vehicle == 0)
            .map_or(130., |c| c.params.maximum_bar / PSI_TO_BAR);
        let head = self.cylinders[0].native_air.as_mut().unwrap();
        let charged = head.profile.controller.charged_psi;
        let full = charged - head.profile.controller.full_service_drop_psi;
        head.emergency.as_mut().unwrap().equalizing_psi = charged.min(90.);
        head.update_train_controller(handle, main, 1000.);
        let equalizing = head.emergency.as_ref().unwrap().equalizing_psi.max(full);
        for c in &mut self.cylinders {
            if let Some(a) = &mut c.native_air {
                a.initialize_native(equalizing, immediate_release);
                c.current_force_n = a.cylinder.shoe_force_n(c.max_force_n);
                c.time_pending_s = 0.;
                c.state = if a.cylinder.pressure_psi > 0. {
                    BrakeState::Applied
                } else {
                    BrakeState::Charged
                };
            }
        }
        for circuit in &mut self.circuits {
            circuit.reservoir_bar = circuit.params.maximum_bar;
            circuit.independent_bar = 0.;
            circuit.previous_service_bar = 0.;
            circuit.compressor_on = false;
        }
        self.prev_command = handle.clamp(0., 1.);
        true
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
                native_legacy_shoe_curve: None,
                profile: Default::default(),
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
        self.step_with_speed(command, dt, 0.);
    }

    /// AirSinglePipe.PropagateBrakePressure's adjacent-volume equalization and
    /// service/emergency vent equations. The driver handle supplies the EQ
    /// boundary; DP, gradual anglecock opening and two-pipe supply are outside
    /// this supported single-pipe subset. Closed cocks split the fluid graph.
    fn propagate_advanced_air(&mut self, command: f64, dt: f64, driver_source: bool) {
        if !dt.is_finite() || dt <= 0. || self.cylinders.is_empty() {
            return;
        }
        let Some(head) = self.cylinders.first().and_then(|c| c.native_air.as_ref()) else {
            return;
        };
        let Some(hardware) = head.profile.emergency.clone() else {
            return;
        };
        let controller = head.profile.controller.clone();
        let count = self.cylinders.len();
        let mut pressure = vec![0_f32; count];
        let mut volume = vec![0_f32; count];
        let mut connected = vec![false; count];
        let mut vented = vec![false; count];
        for (i, c) in self.cylinders.iter().enumerate() {
            if let Some(a) = &c.native_air
                && let Some(e) = &a.profile.emergency
            {
                pressure[i] = a.pipe_psi as f32;
                volume[i] = e.pipe_volume_m3 as f32;
                connected[i] = !c.air_isolated;
                vented[i] = c.air_vented && (i == 0 || !self.cylinders[i - 1].air_vented);
            }
        }
        let source = self.circuits.iter().position(|c| c.vehicle == 0);
        let mut main_psi = source.map_or(controller.charged_psi as f32 + 20., |i| {
            (self.circuits[i].reservoir_bar / PSI_TO_BAR) as f32
        });
        let head = self.cylinders[0].native_air.as_mut().unwrap();
        if controller.notched.is_some() {
            if driver_source {
                head.update_train_controller(command, main_psi as f64, dt);
            }
        } else {
            // Retain the approved prescribed-boundary controller convention
            // for hardware without an authored native Brake_Train.
            let target = controller.charged_psi - command * controller.full_service_drop_psi;
            let eq = head.emergency.as_mut().unwrap();
            let rate = if target < eq.equalizing_psi {
                controller.application_psi_s
            } else {
                controller.release_psi_s
            };
            eq.equalizing_psi += (target - eq.equalizing_psi).clamp(-rate * dt, rate * dt);
        }
        let equalizing = head.emergency.as_ref().unwrap().equalizing_psi as f32;
        let kind = controller.notched.as_ref().map(|n| n.setting(command).1);
        let factor = hardware.pipe_time_factor_s as f32;
        let n = (dt as f32 / factor + 1.) as usize;
        let h = dt as f32 / n.max(1) as f32;
        let source_volume = source.map_or(0.3_f32, |i| self.circuits[i].params.reservoir_m3 as f32);
        for _ in 0..n.max(1) {
            if driver_source && connected[0] {
                if self.emergency || kind == Some("emergency") {
                    pressure[0] -=
                        pressure[0] * (h / hardware.emergency_time_factor_s as f32).min(0.95);
                } else if kind != Some("neutral") && pressure[0] > equalizing {
                    let mut dp =
                        pressure[0] * (h / hardware.service_time_factor_s as f32).min(0.95);
                    if equalizing > pressure[0] - 5. {
                        dp *= ((pressure[0] - equalizing) / 5.).sqrt().min(1.);
                    }
                    pressure[0] -= dp.min(pressure[0] - equalizing);
                } else if !matches!(kind, Some("neutral" | "lap")) && pressure[0] < equalizing {
                    let charging =
                        controller
                            .notched
                            .as_ref()
                            .map_or(controller.release_psi_s, |n| {
                                if kind == Some("fullquickrelease") {
                                    n.pipe_quick_charging_psi_s
                                } else {
                                    n.pipe_charging_psi_s
                                }
                            }) as f32;
                    let mut dp = h * charging;
                    if main_psi - pressure[0] < 15. {
                        dp *= ((main_psi - pressure[0]) / 15.).clamp(0., 1.);
                    }
                    if equalizing - pressure[0] < charging / 4. {
                        dp *= ((equalizing - pressure[0]) / (charging / 4.)).powf(1. / 3.);
                    }
                    dp = dp
                        .min(equalizing - pressure[0])
                        .min(main_psi - pressure[0])
                        .max(0.);
                    pressure[0] += dp;
                    main_psi -= dp * volume[0] / source_volume;
                }
            }
            for i in 0..count {
                if i > 0 && connected[i] && connected[i - 1] && volume[i] > 0. && volume[i - 1] > 0.
                {
                    let dp = (pressure[i] - pressure[i - 1]) * (h / factor).min(1.);
                    let combined = volume[i] + volume[i - 1];
                    pressure[i] -= dp * volume[i - 1] / combined;
                    pressure[i - 1] += dp * volume[i] / combined;
                }
                if connected[i] && vented[i] {
                    pressure[i] -= (pressure[i] * h / factor).min(pressure[i]);
                }
            }
        }
        for (i, c) in self.cylinders.iter_mut().enumerate() {
            if let Some(a) = &mut c.native_air
                && a.emergency.is_some()
            {
                a.pipe_psi = pressure[i].max(0.) as f64;
            }
        }
        if let Some(i) = source {
            self.circuits[i].reservoir_bar = main_psi.max(0.) as f64 * PSI_TO_BAR;
        }
    }

    /// Prescribed head boundary for a native propagation reference. All other
    /// pipe pressures and all valve/reservoir/cylinder states evolve through
    /// the same production equalization and valve kernels. There is no driver
    /// feed valve, main reservoir or independently prescribed trailing pipe.
    pub fn advance_prescribed_pneumatic_network(
        &mut self,
        head_pipe_psi: f64,
        dt: f64,
    ) -> Result<(), String> {
        if !head_pipe_psi.is_finite()
            || !(0. ..=200.).contains(&head_pipe_psi)
            || !dt.is_finite()
            || dt <= 0.
            || self.cylinders.is_empty()
            || self
                .cylinders
                .iter()
                .any(|c| c.native_air.as_ref().is_none_or(|a| a.emergency.is_none()))
        {
            return Err("Red neumática prescrita inválida o incompleta".into());
        }
        self.cylinders[0].native_air.as_mut().unwrap().pipe_psi = head_pipe_psi;
        self.propagate_advanced_air(0., dt, false);
        for c in &mut self.cylinders {
            let pipe = c.native_air.as_ref().unwrap().pipe_psi;
            c.advance_prescribed_pneumatic_boundary(pipe, dt)?;
        }
        Ok(())
    }

    pub fn step_with_speed(&mut self, command: f64, dt: f64, speed_mps: f64) {
        let command = if self.emergency {
            1.0
        } else {
            command.clamp(0.0, 1.0)
        };
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

        self.propagate_advanced_air(command, dt, true);
        let mut lead_vacuum_pipe = None;
        for cyl in &mut self.cylinders {
            if cyl.native_air.is_none() && cyl.native_vacuum.is_none() && !cyl.air_isolated {
                let old = cyl.automatic_pipe_bar.unwrap_or(5.0);
                let target = if self.emergency || cyl.air_vented {
                    0.0
                } else if self.ep_only_wire {
                    5.0
                } else {
                    5.0 - command * 3.5
                };
                let rate = if self.emergency || cyl.air_vented {
                    5.0
                } else {
                    1.0
                };
                cyl.automatic_pipe_bar = Some(old + (target - old).clamp(-rate * dt, rate * dt));
            }
            if cyl.transfer_only {
                cyl.current_force_n = 0.;
                continue;
            }
            if let Some(vacuum) = cyl.native_vacuum.as_mut() {
                if !cyl.air_isolated {
                    vacuum.prepare_pipe();
                    cyl.time_pending_s = (cyl.time_pending_s - dt).max(0.);
                    if vacuum.profile.independent.is_some() {
                        vacuum.step_independent_pipe(
                            self.independent_command,
                            dt,
                            lead_vacuum_pipe,
                        );
                        lead_vacuum_pipe = lead_vacuum_pipe.or(vacuum.independent_pipe_psi);
                        if cyl.time_pending_s == 0. || cyl.air_vented {
                            vacuum.pipe_psi = if cyl.air_vented {
                                crate::native_vacuum_brake::ATMOSPHERE_PSI
                            } else {
                                crate::native_vacuum_brake::ATMOSPHERE_PSI
                                    - vacuum.profile.charged_vacuum_psi * (1. - command)
                            };
                        }
                        vacuum.step_pipe_independent(command, self.independent_command, dt);
                    } else if cyl.time_pending_s == 0. || cyl.air_vented {
                        vacuum.step(command, dt, cyl.air_vented);
                    } else {
                        vacuum.step_pipe(dt, false);
                    }
                    cyl.current_force_n = vacuum.shoe_force_n(cyl.max_force_n);
                    cyl.state = if command > 0. || cyl.air_vented {
                        BrakeState::Applying
                    } else {
                        BrakeState::Releasing
                    };
                }
                continue;
            }
            if cyl.air_vented
                && cyl
                    .native_air
                    .as_ref()
                    .is_none_or(|a| a.emergency.is_none())
            {
                if let Some(ep) = cyl.legacy_ep.as_mut() {
                    ep.precharge(1.);
                    cyl.current_force_n = ep.shoe_force_n(cyl.max_force_n);
                    cyl.state = BrakeState::Applied;
                    continue;
                }
                if let Some(air) = cyl.native_air.as_mut()
                    && air.emergency.is_none()
                {
                    air.precharge(1.);
                    cyl.current_force_n = air.cylinder.shoe_force_n(cyl.max_force_n);
                    cyl.state = BrakeState::Applied;
                    continue;
                }
                if let Some(ep) = cyl.native_ep.as_mut() {
                    ep.precharge(1.);
                    cyl.current_force_n = ep.shoe_force_n(cyl.max_force_n);
                    cyl.state = BrakeState::Applied;
                    continue;
                }
                cyl.current_force_n = cyl.max_force_n;
                cyl.state = BrakeState::Applied;
                continue;
            }
            if cyl.air_isolated
                && cyl
                    .native_air
                    .as_ref()
                    .is_none_or(|a| a.emergency.is_none())
            {
                continue;
            }
            if let Some(ep) = cyl.legacy_ep.as_mut() {
                ep.step(command, dt);
                cyl.current_force_n = ep.shoe_force_n(cyl.max_force_n);
                cyl.state = if command > 0. {
                    BrakeState::Applying
                } else {
                    BrakeState::Releasing
                };
                continue;
            }
            if let Some(ep) = cyl.native_ep.as_mut() {
                ep.step(command, dt, speed_mps);
                cyl.current_force_n = ep.shoe_force_n(cyl.max_force_n);
                cyl.state = if command > 0. {
                    BrakeState::Applying
                } else {
                    BrakeState::Releasing
                };
                continue;
            }
            if self.ep_only_wire && !self.emergency {
                // An ordinary air-braked car stays released while EP-only
                // service leaves the shared pneumatic pipe fully charged.
                cyl.current_force_n =
                    (cyl.current_force_n - cyl.release_ramp_rate_n_per_s * dt).max(0.);
                continue;
            }
            // Drain pending travel time.
            if cyl.time_pending_s > 0.0
                && cyl
                    .native_air
                    .as_ref()
                    .is_none_or(|a| a.emergency.is_none())
            {
                cyl.time_pending_s = (cyl.time_pending_s - dt).max(0.0);
                if cyl.time_pending_s > 0.0 {
                    // Signal hasn't arrived yet; hold current force.
                    continue;
                }
            }

            if let Some(air) = cyl.native_air.as_mut() {
                air.step_with_emergency(command, self.emergency, dt);
                cyl.current_force_n = air.cylinder.shoe_force_n(cyl.max_force_n);
                cyl.state = if command > 0. {
                    BrakeState::Applying
                } else {
                    BrakeState::Releasing
                };
                continue;
            }
            let was_latched = !cyl.ep_instant && cyl.latched_command > 0.0;
            let eff = cyl.effective_command(command, self.train_air_lap_hold);
            let target = eff * cyl.max_force_n;

            // Release: EP + latched train-air bleed at `train_air_full_release_s` when lap-hold is on
            // (OR pipe exhaust / BC bleed); otherwise use per-cylinder default release ramp.
            let delta = if cyl.current_force_n > target {
                let rate = if !cyl.authored_response
                    && command <= 0.0
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

    pub fn step_circuits(&mut self, supply: &crate::power_supply::PowerTrainState, dt: f64) {
        self.step_circuits_with_diesel(supply, &Default::default(), dt);
    }

    pub fn step_circuits_with_diesel(
        &mut self,
        supply: &crate::power_supply::PowerTrainState,
        diesel: &crate::diesel_operation::DieselTrainState,
        dt: f64,
    ) {
        for circuit in &mut self.circuits {
            let Some(c) = self.cylinders.get_mut(circuit.vehicle) else {
                continue;
            };
            let powered = supply
                .cars
                .iter()
                .find(|s| s.vehicle == circuit.vehicle)
                .is_none_or(|s| s.supply.auxiliary);
            let native_plain = c
                .native_air
                .as_ref()
                .is_some_and(|a| a.profile.native_plain);
            let independent_fitted = c
                .native_air
                .as_ref()
                .and_then(|a| a.profile.controller.notched.as_ref())
                .is_none_or(|n| n.independent_fitted);
            let compressor_factor = c
                .native_air
                .as_ref()
                .and_then(|a| a.profile.controller.notched.as_ref())
                .and_then(|n| n.mechanical_compressor_max_rpm)
                .map_or(1., |maximum| {
                    diesel
                        .cars
                        .iter()
                        .find(|s| s.vehicle == circuit.vehicle)
                        .map_or(0., |s| s.rpm / maximum)
                });
            circuit.step_with_charging_factor(
                if independent_fitted {
                    self.independent_command
                } else {
                    0.
                },
                // A plain triple valve draws service air from its auxiliary;
                // charging the train pipe already meters its main-reservoir
                // demand. Do not charge the generic extra 10 L twice.
                if native_plain {
                    0.
                } else {
                    c.service_pressure_bar()
                },
                c.native_air
                    .as_ref()
                    .and_then(|a| a.profile.emergency.as_ref())
                    .map_or(c.full_pressure_bar, |p| p.independent_max_psi * PSI_TO_BAR),
                powered,
                compressor_factor,
                dt,
            );
            c.independent_force_n = if let Some(a) = &mut c.native_air
                && a.emergency.is_some()
            {
                a.independent_psi =
                    (circuit.independent_bar / PSI_TO_BAR).min(a.cylinder.profile.max_psi);
                c.max_force_n
                    * ((a.independent_psi - a.cylinder.profile.spring_psi)
                        / (a.cylinder.profile.reference_psi - a.cylinder.profile.spring_psi))
                        .max(0.)
            } else {
                (circuit.independent_bar / c.full_pressure_bar).clamp(0.0, 1.0) * c.max_force_n
            };
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

    /// Native axle input is shoe retard, before the skid-dependent rail force.
    pub fn cylinder_retard_forces_n(&self, speed_mps: f64) -> Vec<f64> {
        self.cylinders
            .iter()
            .map(|c| c.retard_force_n(speed_mps))
            .collect()
    }

    /// Stored fields from the preceding Car.Update, not a fresh evaluation
    /// at a later velocity. These preserve the native update phase on restore.
    pub fn previous_native_retard_forces_n(&self) -> Vec<f64> {
        self.cylinders
            .iter()
            .map(|c| c.native_retard_last_n)
            .collect()
    }
    pub fn last_native_brake_force_n(&self) -> f64 {
        self.cylinders
            .iter()
            .map(|c| c.native_rail_brake_last_n)
            .sum()
    }
    pub(crate) fn record_native_brake_forces(&mut self, speed_mps: f64) {
        for cylinder in &mut self.cylinders {
            cylinder.native_retard_last_n = cylinder.retard_force_n(speed_mps);
            cylinder.native_rail_brake_last_n = cylinder.effective_force_n(speed_mps);
        }
    }

    /// Maximum total force when fully applied (N).
    pub fn max_total_force_n(&self) -> f64 {
        self.cylinders.iter().map(|c| c.max_force_n).sum()
    }

    /// Set cylinders to steady state for `command` (activity start with brakes already set).
    pub fn precharge(&mut self, command: f64) {
        if self.reset_native_plain_brakes(command, false) {
            return;
        }
        let command = command.clamp(0.0, 1.0);
        for cyl in &mut self.cylinders {
            if let Some(ep) = cyl.legacy_ep.as_mut() {
                ep.precharge(command);
                cyl.current_force_n = ep.shoe_force_n(cyl.max_force_n);
                cyl.time_pending_s = 0.;
                cyl.state = if command > 0. {
                    BrakeState::Applied
                } else {
                    BrakeState::Charged
                };
                continue;
            }
            if cyl.transfer_only {
                cyl.current_force_n = 0.;
                continue;
            }
            if let Some(vacuum) = cyl.native_vacuum.as_mut() {
                vacuum.precharge(command);
                vacuum.prepare_pipe();
                cyl.current_force_n = vacuum.shoe_force_n(cyl.max_force_n);
                cyl.time_pending_s = 0.;
                cyl.state = if command > 0. {
                    BrakeState::Applied
                } else {
                    BrakeState::Charged
                };
                continue;
            }
            if let Some(air) = cyl.native_air.as_mut() {
                air.precharge(command);
                cyl.current_force_n = air.cylinder.shoe_force_n(cyl.max_force_n);
                cyl.time_pending_s = 0.;
                cyl.state = if command > 0. {
                    BrakeState::Applied
                } else {
                    BrakeState::Charged
                };
                continue;
            }
            if let Some(ep) = cyl.native_ep.as_mut() {
                ep.precharge(command);
                cyl.current_force_n = ep.shoe_force_n(cyl.max_force_n);
                cyl.time_pending_s = 0.;
                cyl.state = if command > 0. {
                    BrakeState::Applied
                } else {
                    BrakeState::Charged
                };
                continue;
            }
            if self.ep_only_wire && !self.emergency {
                cyl.current_force_n = 0.;
                continue;
            }
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
            circuits: Vec::new(),
            independent_command: 0.0,
            emergency: false,
            cylinders: Vec::new(),
            pipe_speed_mps: 200.0,
            prev_command: 0.0,
            train_air_lap_hold: false,
            train_air_full_release_s: 3.0,
            ep_only_wire: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::BrakeShoeFrictionCurve;

    fn plain_specs() -> Vec<BrakeVehicleSpec> {
        let ast = openrailsrs_formats::parse_vehicle_text(include_str!(
            "../../../examples/physics_lab/vehicles/plain_train_control.eng"
        ))
        .unwrap();
        let lead = openrailsrs_formats::parse_vehicle_brake_profile(&ast);
        (0..3)
            .map(|i| {
                let mut profile = lead.clone();
                if i > 0 {
                    profile.native_air.as_mut().unwrap().controller.notched = None;
                }
                BrakeVehicleSpec {
                    native_legacy_shoe_curve: None,
                    profile,
                    position_m: i as f64 * 12.19,
                    max_force_n: 40_000.,
                    ep_instant: false,
                    shoe_friction: BrakeShoeFrictionCurve::identity(),
                    mass_kg: 25_000.,
                    skid_adhesion_mu: 0.,
                }
            })
            .collect()
    }

    #[test]
    fn native_plain_legacy_brakes_keep_pressure_but_reduce_rim_force_at_speed() {
        let system = BrakeSystem::from_vehicle_specs(&plain_specs(), 200., false, 3.);
        let stopped = system.total_force_n(0.);
        let running = system.total_force_n(80. / 3.6);
        assert!(stopped > 100_000.);
        assert!(running > stopped * 0.7 && running < stopped * 0.8);
        assert_eq!(running, system.total_force_n(-80. / 3.6));
        let before = system.cylinders[0].pressure_bar();
        let saved = serde_json::to_string(&system).unwrap();
        let restored: BrakeSystem = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.total_force_n(80. / 3.6), running);
        assert_eq!(restored.cylinders[0].pressure_bar(), before);
        let mut specs = plain_specs();
        for spec in &mut specs {
            spec.native_legacy_shoe_curve = Some(BrakeShoeFrictionCurve::from_kph_mu_pairs(&[
                (0., 0.4),
                (80., 0.1),
            ]));
        }
        let custom = BrakeSystem::from_vehicle_specs(&specs, 200., false, 3.);
        assert!((custom.total_force_n(80. / 3.6) / stopped - 0.25).abs() < 1e-6);
        assert!(!custom.cylinders[0].vacuum_hardware_compatible(&system.cylinders[0]));
    }

    #[test]
    fn plain_native_initialization_derives_pipe_and_cylinders_from_authored_controller() {
        let mut s = BrakeSystem::from_vehicle_specs(&plain_specs(), 200., false, 3.);
        assert_eq!(s.native_start_handle(), Some(0.7));
        assert_eq!(s.circuits.len(), 1);
        assert!(!s.has_independent_brake());
        for c in &s.cylinders {
            let status = c.emergency_pneumatic_status().unwrap();
            for (actual, expected) in [
                (status.pipe_psi, 64.),
                (status.auxiliary_psi, 64.),
                (status.control_psi, 90.),
                (status.emergency_reservoir_psi, 90.),
                (status.cylinder_psi, 65.),
            ] {
                assert!((actual - expected).abs() < 0.005, "{actual} vs {expected}");
            }
        }
        s.independent_command = 1.;
        s.step_circuits(&Default::default(), 0.05);
        assert_eq!(s.circuits[0].independent_bar, 0.);
        assert!(s.cylinders.iter().all(|c| c.independent_force_n == 0.));
        assert!(s.reset_native_plain_brakes(0., true));
        assert!(s.cylinders.iter().all(|c| c.pressure_bar() == 0.));
    }

    #[test]
    fn plain_actual_handle_uses_native_notch_fraction_and_holds_until_release() {
        let mut s = BrakeSystem::from_vehicle_specs(&plain_specs(), 200., false, 3.);
        s.step(0., 0.05);
        assert!(
            s.circuits[0].reservoir_bar < s.circuits[0].params.maximum_bar,
            "finite MR supplies the pipe before its compressor replaces air"
        );
        // Three authored auxiliaries and pipes need 11.8236 PSI*m3 of air.
        // The lead feed supplies at most 21 PSI/s * 0.01 m3, so even an
        // ideal, unchoked release takes 56.30 s. Native feed/valve chokes
        // require settling beyond that lower bound; allow 180 s, preserving
        // the pressure criterion rather than assuming release is immediate.
        for _ in 0..3600 {
            s.step(0., 0.05);
            // Rated mechanical speed is an explicit subsystem input; this
            // test drives the real finite compressor, not a fixed MR pressure.
            s.circuits[0].step_with_charging_factor(0., 0., 4.5, true, 1., 0.05);
        }
        let released: Vec<_> = s
            .cylinders
            .iter()
            .map(|c| c.emergency_pneumatic_status().unwrap())
            .collect();
        assert!(
            released.iter().all(|c| c.pipe_psi > 89.9),
            "native release did not settle: cars={released:?}, MR={}PSI, EQ={}PSI",
            s.circuits[0].reservoir_bar / PSI_TO_BAR,
            s.cylinders[0]
                .native_air
                .as_ref()
                .unwrap()
                .emergency
                .as_ref()
                .unwrap()
                .equalizing_psi
        );
        for _ in 0..800 {
            s.step(0.552000, 0.05);
        }
        let eq = s.cylinders[0]
            .native_air
            .as_ref()
            .unwrap()
            .emergency
            .as_ref()
            .unwrap()
            .equalizing_psi;
        assert!((eq - 69.92).abs() < 0.005, "actual handle 0.552: {eq}");
        for _ in 0..200 {
            s.step(0.36, 0.05);
        }
        let held = s.cylinders[0]
            .native_air
            .as_ref()
            .unwrap()
            .emergency
            .as_ref()
            .unwrap()
            .equalizing_psi;
        assert_eq!(
            eq, held,
            "native GSelfLapH cannot graduate release with default settings"
        );
        let mut restored: BrakeSystem =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        for _ in 0..200 {
            s.step(0., 0.05);
            restored.step(0., 0.05);
        }
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::to_value(&restored).unwrap()
        );
    }

    #[test]
    fn plain_shared_handle_keeps_each_locomotives_main_reservoir_and_compressor() {
        let mut specs = plain_specs();
        let mut rear = specs[0].profile.clone();
        let a = rear.native_air.as_mut().unwrap();
        a.controller.charged_psi = 110.;
        let n = a.controller.notched.as_mut().unwrap();
        n.mechanical_compressor_max_rpm = Some(1800.);
        n.independent_fitted = true;
        let mr = n.main_reservoir.as_mut().unwrap();
        mr.reservoir_m3 = 0.5;
        mr.maximum_bar = 150. * PSI_TO_BAR;
        let own_reservoir = mr.clone();
        specs[2].profile = rear;
        let s = BrakeSystem::from_vehicle_specs(&specs, 200., false, 3.);
        assert_eq!(s.circuits.len(), 2);
        assert_eq!(s.circuits[1].vehicle, 2);
        assert_eq!(s.circuits[1].params, own_reservoir);
        let rear = &s.cylinders[2]
            .native_air
            .as_ref()
            .unwrap()
            .profile
            .controller;
        assert_eq!(rear.charged_psi, 90.);
        let n = rear.notched.as_ref().unwrap();
        assert_eq!(n.mechanical_compressor_max_rpm, Some(1800.));
        assert_eq!(n.main_reservoir.as_ref(), Some(&own_reservoir));
        assert!(!n.independent_fitted && !s.has_independent_brake());
        let coach = s.cylinders[1]
            .native_air
            .as_ref()
            .unwrap()
            .profile
            .controller
            .notched
            .as_ref()
            .unwrap();
        assert!(coach.main_reservoir.is_none() && coach.mechanical_compressor_max_rpm.is_none());
    }

    #[test]
    fn plain_candidate_without_native_driver_preserves_legacy_pipe_and_ep_formation() {
        let mut specs = plain_specs();
        specs[0].profile = Default::default();
        specs[0].profile.system = Some("EP".into());
        let mut s = BrakeSystem::from_vehicle_specs(&specs, 200., false, 3.);
        assert!(s.native_start_handle().is_none() && s.circuits.is_empty());
        assert!(s.cylinders.iter().all(|c| c.native_air.is_none()));
        s.step(0., 0.05);
        assert!(
            s.cylinders
                .iter()
                .all(|c| (c.automatic_pipe_bar.unwrap() - 5.).abs() < 1e-12)
        );
    }

    #[test]
    fn vacuum_independent_never_authorizes_a_coach_controller() {
        let ast = openrailsrs_formats::parse_vehicle_text(include_str!(
            "../../../examples/vacuum_independent/vehicles/diesel_vacuum.eng"
        ))
        .unwrap();
        let mut profile = openrailsrs_formats::parse_vehicle_brake_profile(&ast);
        // A hauled coach cannot become a locomotive by declaring Brake_Engine.
        profile.native_vacuum.as_mut().unwrap().engine = false;
        let mut system = BrakeSystem::from_vehicle_specs(
            &[BrakeVehicleSpec {
                native_legacy_shoe_curve: None,
                profile,
                position_m: 0.,
                max_force_n: 30_000.,
                ep_instant: false,
                shoe_friction: BrakeShoeFrictionCurve::identity(),
                mass_kg: 37_000.,
                skid_adhesion_mu: 0.,
            }],
            200.,
            false,
            3.,
        );
        assert!(!system.has_independent_brake());
        system.independent_command = 1.;
        for _ in 0..200 {
            system.step(0., 0.05);
        }
        assert!(system.cylinder_forces_n(0.).iter().all(|f| *f < 1.));
    }

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
    #[test]
    fn native_ep_rates_apply_to_every_car_without_pipe_delay_or_global_release_override() {
        let mut system = BrakeSystem::from_vehicle_specs(
            &[BrakeVehicleSpec {
                native_legacy_shoe_curve: None,
                position_m: 250.0,
                max_force_n: 100_000.0,
                ep_instant: true,
                shoe_friction: BrakeShoeFrictionCurve::identity(),
                mass_kg: 40_000.0,
                skid_adhesion_mu: OR_DEFAULT_BRAKE_ADHESION_MU,
                profile: openrailsrs_formats::VehicleBrakeProfile {
                    system: Some("EP".into()),
                    max_cylinder_bar: Some(3.0),
                    application_bar_s: Some(2.0),
                    release_bar_s: Some(1.0),
                    ..Default::default()
                },
            }],
            200.0,
            true,
            10.0,
        );
        system.step(1.0, 0.75);
        assert!((system.cylinders[0].current_force_n - 50_000.0).abs() < 1e-6);
        assert_eq!(system.cylinders[0].time_pending_s, 0.0);
        system.step(0.0, 0.75);
        assert!((system.cylinders[0].current_force_n - 25_000.0).abs() < 1e-6);
        assert_eq!(system.cylinders[0].full_pressure_bar, 3.0);
    }
}
