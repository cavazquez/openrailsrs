//! Steam traction model: boiler thermodynamics + cylinder tractive-effort formula.
//!
//! # Physics
//!
//! ## Tractive effort
//!
//! The instantaneous tractive force produced by the cylinders is:
//!
//! ```text
//! cutoff   = regulator × MAX_CUTOFF                      (MAX_CUTOFF = 0.75)
//! P_mep    = cutoff × P_boiler × ETA_INDICATOR            (ETA = 0.85)
//! F_te     = n_cyl × (π/4) × bore² × stroke × P_mep / r_wheel
//! ```
//!
//! At v = 0 the formula gives the maximum stall force.  At higher speeds it
//! stays constant until the piston speed limit is reached (approximated by
//! capping at the `max_velocity_mps` inherited from `Locomotive`).
//!
//! ## Boiler dynamics
//!
//! Pressure is modelled as a first-order ODE driven by the balance between
//! steam supply (evaporation at constant fire) and steam demand (cylinder
//! consumption proportional to mechanical work):
//!
//! ```text
//! steam_demand  = (F_te × v) / STEAM_ENTHALPY      (STEAM_ENTHALPY = 2.5 MJ/kg)
//! steam_supply  = evaporation_rate_kg_per_s         (fire at full blast)
//! dP/dt         = K_P × (steam_supply − steam_demand) / water_kg
//! P_new         = clamp(P + dP×dt,  P_MIN, safety_valve_bar)
//! ```
//!
//! Water decreases with steam demand; coal decreases at a fixed rate.
//!
//! ## Automatic injector (headless)
//!
//! In headless mode there is no human fireman.  An automatic rule tops up
//! water whenever the level falls below 30 % of the initial tender capacity.
//! This prevents the simulation from stalling mid-trip due to empty tender.

use openrailsrs_train::SteamParams;

// ── Physical constants ────────────────────────────────────────────────────────

/// Maximum valve-gear cutoff ratio (full forward motion).
pub const MAX_CUTOFF: f64 = 0.75;
/// Indicator diagram efficiency (accounts for incomplete expansion, valve timing).
pub const ETA_INDICATOR: f64 = 0.85;
/// Approximate specific enthalpy of saturated steam at working pressure (J/kg).
pub const STEAM_ENTHALPY_J_KG: f64 = 2_500_000.0;
/// Boiler pressure gain constant (bar·s/kg per kg/s imbalance).
pub const K_PRESSURE: f64 = 0.5;
/// Minimum boiler pressure (bar) — fire never goes out completely.
pub const P_MIN_BAR: f64 = 2.0;
/// Safety-valve factor: opens at this multiple of working pressure.
pub const SAFETY_VALVE_FACTOR: f64 = 1.05;
/// Water level fraction that triggers the automatic injector.
pub const INJECTOR_THRESHOLD: f64 = 0.30;
/// Water top-up rate when the injector is open (kg/s).
pub const INJECTOR_RATE_KG_PER_S: f64 = 5.0;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Mutable boiler state carried in [`TrainSimState`].
///
/// This is initialised from [`SteamParams`] at the start of a simulation and
/// updated every physics step.
#[derive(Clone, Debug, PartialEq)]
pub struct BoilerState {
    /// Current boiler pressure (bar).
    pub pressure_bar: f64,
    /// Water remaining in tender + boiler (kg).
    pub water_kg: f64,
    /// Coal remaining in tender (kg).
    pub coal_kg: f64,
    /// Cached initial water capacity for the automatic injector threshold.
    pub initial_water_kg: f64,
}

impl BoilerState {
    /// Initialise from the static parameters of a steam locomotive.
    pub fn from_params(params: &SteamParams) -> Self {
        Self {
            pressure_bar: params.working_pressure_bar,
            water_kg: params.initial_water_kg,
            coal_kg: params.initial_coal_kg,
            initial_water_kg: params.initial_water_kg,
        }
    }
}

// ── Core step function ────────────────────────────────────────────────────────

/// Advance the boiler model by `dt` seconds and return the tractive force (N).
///
/// `regulator` maps directly to `state.throttle` (0 = closed, 1 = full open).
///
/// # Side effects
/// Updates `boiler.pressure_bar`, `boiler.water_kg`, and `boiler.coal_kg` in
/// place.  The caller must persist these changes back into `TrainSimState`.
pub fn steam_step(
    boiler: &mut BoilerState,
    params: &SteamParams,
    regulator: f64,
    velocity_mps: f64,
    dt: f64,
) -> f64 {
    use std::f64::consts::PI;

    let regulator = regulator.clamp(0.0, 1.0);

    // ── Tractive effort ───────────────────────────────────────────────────────
    let cutoff = regulator * MAX_CUTOFF;
    let p_mep_pa = cutoff * boiler.pressure_bar * 1e5 * ETA_INDICATOR;
    let f_te = params.cylinder_count as f64
        * (PI / 4.0)
        * params.cylinder_bore_m.powi(2)
        * params.piston_stroke_m
        * p_mep_pa
        / params.driving_wheel_radius_m;

    // ── Steam demand (proportional to mechanical work done) ───────────────────
    // At v = 0 the piston still moves; use a minimum "steam consumption for
    // starting effort" equal to 10 % of rated evaporation.
    let work_w = f_te * velocity_mps.max(0.0);
    let steam_demand_kg_s = if work_w > 0.0 {
        work_w / STEAM_ENTHALPY_J_KG
    } else {
        // Stationary but regulator open: boiler still loses steam through
        // cylinder cocks and standing losses.
        params.evaporation_rate_kg_per_s * 0.10 * regulator
    };

    // ── Boiler pressure dynamics ──────────────────────────────────────────────
    let steam_supply_kg_s = params.evaporation_rate_kg_per_s; // fire at constant rate
    let dp_dt = if boiler.water_kg > 10.0 {
        K_PRESSURE * (steam_supply_kg_s - steam_demand_kg_s) / boiler.water_kg.max(1.0)
    } else {
        // Almost dry boiler: pressure drops rapidly.
        -1.0
    };
    let safety_valve_bar = params.working_pressure_bar * SAFETY_VALVE_FACTOR;
    boiler.pressure_bar = (boiler.pressure_bar + dp_dt * dt).clamp(P_MIN_BAR, safety_valve_bar);

    // ── Water and coal consumption ────────────────────────────────────────────
    boiler.water_kg = (boiler.water_kg - steam_demand_kg_s * dt).max(0.0);
    boiler.coal_kg = (boiler.coal_kg - params.coal_consumption_kg_per_s * dt).max(0.0);

    // ── Automatic injector (headless fireman) ─────────────────────────────────
    // Replenish water from an implicit infinite water supply (the timetable
    // assumes servicing stops are handled externally).
    if boiler.water_kg < boiler.initial_water_kg * INJECTOR_THRESHOLD {
        boiler.water_kg =
            (boiler.water_kg + INJECTOR_RATE_KG_PER_S * dt).min(boiler.initial_water_kg);
    }

    f_te
}

// ── Convenience ───────────────────────────────────────────────────────────────

/// Return the theoretical maximum tractive force at stall (v = 0, full boiler
/// pressure, full cutoff).  Matches [`SteamParams::max_tractive_effort_n`].
pub fn stall_force_n(params: &SteamParams) -> f64 {
    use std::f64::consts::PI;
    let p_mep_pa = MAX_CUTOFF * params.working_pressure_bar * 1e5 * ETA_INDICATOR;
    params.cylinder_count as f64
        * (PI / 4.0)
        * params.cylinder_bore_m.powi(2)
        * params.piston_stroke_m
        * p_mep_pa
        / params.driving_wheel_radius_m
}
