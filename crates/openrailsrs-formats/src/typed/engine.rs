use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::msts_units::{
    parse_force_n, parse_length_m, parse_mass_kg, parse_power_w, parse_pressure_bar,
    parse_velocity_mps,
};
use crate::units::kmh_to_mps;

use super::brake_shoe::{BrakeShoeFrictionCurve, OrtsBrakeShoeType, parse_orts_brake_shoe};
use super::friction::parse_orts_friction_fields;
use super::{
    OrtsFrictionFields, atom_to_number, atom_to_string, find_list_value,
    find_optional_string_field, walk_lists_find,
};

/// Optional MSTS steam parameters parsed from `.eng` (mapped to `SteamParams` in train crate).
#[derive(Clone, Debug, PartialEq)]
pub struct MstsSteamFields {
    pub cylinder_count: u32,
    pub cylinder_bore_m: f64,
    pub piston_stroke_m: f64,
    pub driving_wheel_radius_m: f64,
    pub working_pressure_bar: f64,
    pub evaporation_rate_kg_per_s: f64,
    pub coal_consumption_kg_per_s: f64,
    pub initial_water_kg: f64,
    pub initial_coal_kg: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EngineFile {
    pub name: String,
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_velocity_mps: f64,
    pub max_tractive_effort_n: f64,
    /// MSTS `MaxContinuousForce`; legacy diesel P/v uses this when set.
    pub max_continuous_force_n: f64,
    pub max_brake_force_n: f64,
    pub regen_factor: f64,
    pub diesel_sfc_g_per_kwh: Option<f64>,
    pub traction_curve: Vec<(f64, f64)>,
    /// ORTS per-notch tractive curves `(throttle, points)`; forces converted to N.
    pub diesel_notch_curves: Vec<(f64, Vec<(f64, f64)>)>,
    /// `DieselPowerTab`: (RPM, Watts) pairs — engine shaft power vs RPM.
    pub diesel_power_tab: Vec<(f64, f64)>,
    /// `ThrottleRPMTab`: (throttle 0-1, target RPM) pairs.
    pub diesel_throttle_rpm_tab: Vec<(f64, f64)>,
    /// Engine idle RPM (from `IdleRPM`).
    pub diesel_idle_rpm: f64,
    /// Engine max RPM (from `MaxRPM`).
    pub diesel_max_rpm: f64,
    /// OR rolling resistance (N); summed across consist with B and C.
    pub davis_a_n: f64,
    pub davis_b_n_per_mps: f64,
    pub davis_c_n_per_mps2: f64,
    /// Legacy MSTS ramp to full tractive effort (seconds).
    pub run_up_time_s: f64,
    /// OR `RateOfChangeUpRPMpSS` from first diesel engine block.
    pub diesel_rate_of_change_up_rpm_pss: f64,
    pub diesel_rate_of_change_down_rpm_pss: f64,
    pub diesel_change_up_rpm_ps: f64,
    pub diesel_change_down_rpm_ps: f64,
    /// `LocomotiveMaxRailOutputPowerW` / ORTS equivalent (W); 0 → derive from `MaxPower`.
    pub max_rail_output_power_w: f64,
    /// `ORTSUnloadingSpeed` — generator unload speed (m/s); 0 = disabled.
    pub unloading_speed_mps: f64,
    /// When true, OR skips apparent-throttle curve limiting.
    pub tractive_force_power_limited: bool,
    /// `SpeedOfMaxContinuousForce` for default rail-power derivation (m/s).
    pub speed_of_max_continuous_force_mps: f64,
    /// Optional explicit `ReverseThrottleRPMTab` (RPM → throttle 0–1).
    pub diesel_reverse_throttle_rpm_tab: Vec<(f64, f64)>,
    /// `ORTSDriveWheelWeight` for Curtius-Kniffler adhesion.
    pub drive_wheel_mass_kg: f64,
    /// Curtius-Kniffler A/B/C; zero → OR defaults at load time.
    pub curtius_a: f64,
    pub curtius_b: f64,
    pub curtius_c: f64,
    /// OR `TractiveForceAvgUsageTimeFactor` for motor heating (seconds).
    pub motor_heating_time_s: f64,
    /// OR `ORTSTractiveForceRampUpRate` (N/s).
    pub traction_force_ramp_up_nps: f64,
    pub traction_force_ramp_down_nps: f64,
    /// Negative in OR means “use ramp down”; stored as-is, resolved at load time.
    pub traction_force_ramp_down_to_zero_nps: f64,
    /// OR `ORTSTractivePowerRampUpRate` (W/s).
    pub traction_power_ramp_up_wps: f64,
    pub traction_power_ramp_down_wps: f64,
    pub traction_power_ramp_down_to_zero_wps: f64,
    /// OR `ORTSContinuousForceTimeFactor` (seconds); 0 → OR default 1800 when continuous limit active.
    pub continuous_force_time_factor_s: f64,
    pub wagon_shape: Option<String>,
    pub length_m: f64,
    pub steam: Option<MstsSteamFields>,
    pub friction: OrtsFrictionFields,
    pub brake_shoe_type: OrtsBrakeShoeType,
    pub brake_shoe_friction: Option<BrakeShoeFrictionCurve>,
}

impl EngineFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let context = "Engine";
        let mass_kg = find_mass_field(ast, &["Mass", "MassKG"], context)?;
        let mut max_power_w = find_optional_quantity_field(
            ast,
            QuantityKind::Power,
            &["MaxPower", "MaxForce"],
            context,
        )?
        .unwrap_or(0.0);
        if max_power_w <= 0.0 {
            max_power_w = parse_diesel_power_tab_max(ast).unwrap_or(0.0);
        }
        let max_velocity_mps = find_optional_quantity_field(
            ast,
            QuantityKind::Velocity,
            &["MaxVelocity", "MaxSpeed"],
            context,
        )?
        .unwrap_or(kmh_to_mps(120.0));
        let max_tractive_effort_n = find_optional_quantity_field(
            ast,
            QuantityKind::Force,
            &["MaxForce", "MaxTractiveEffort"],
            context,
        )?
        .unwrap_or(350_000.0);
        let max_continuous_force_n = find_optional_quantity_field(
            ast,
            QuantityKind::Force,
            &["MaxContinuousForce", "MaxContinuousTractiveForce"],
            context,
        )?
        .unwrap_or(0.0);
        let max_brake_force_n = find_optional_quantity_field(
            ast,
            QuantityKind::Force,
            &[
                "MaxBrakeForce",
                "Brake",
                "ORTSMaxBrakeShoeForce",
                "MaxBrakeShoeForce",
            ],
            context,
        )?
        .unwrap_or(200_000.0);
        let name = find_optional_string_field(ast, &["Name"], context)?
            .unwrap_or_else(|| "engine".to_string());
        let regen_factor =
            find_optional_scalar_field(ast, &["RegenFactor", "RegenBraking"], context)?
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
        let diesel_sfc_g_per_kwh =
            find_optional_scalar_field(ast, &["SpecificFuelConsumption", "DieselSfc"], context)?;
        let wagon_shape = find_optional_string_field(ast, &["WagonShape", "Shape"], context)?;
        let length_m = parse_length_from_ast(ast).unwrap_or(18.0);
        let mut traction_curve = parse_traction_curve(ast);
        let diesel_notch_curves = parse_orts_notch_curves(ast);
        if traction_curve.is_empty() {
            if let Some((_, curve)) = diesel_notch_curves
                .iter()
                .max_by(|a, b| a.0.total_cmp(&b.0))
            {
                traction_curve = curve.clone();
            }
        }
        let steam = parse_steam_fields(ast);
        let mut diesel_power_tab = parse_rpm_power_tab(ast);
        let mut diesel_throttle_rpm_tab = parse_throttle_rpm_tab(ast);
        let mut diesel_idle_rpm =
            find_optional_scalar_field(ast, &["IdleRPM", "ORTSIdleRPM"], context)?.unwrap_or(0.0);
        let mut diesel_max_rpm =
            find_optional_scalar_field(ast, &["MaxRPM", "ORTSMaxRPM"], context)?.unwrap_or(0.0);
        let mut diesel_block = None;
        if let Some(block) = parse_ortsdieselengines_diesel(ast) {
            diesel_block = Some(block.clone());
            if diesel_power_tab.is_empty() {
                diesel_power_tab = block.power_tab;
            }
            if diesel_throttle_rpm_tab.is_empty() {
                diesel_throttle_rpm_tab = block.throttle_rpm_tab;
            }
            if diesel_idle_rpm <= 0.0 {
                diesel_idle_rpm = block.idle_rpm;
            }
            if diesel_max_rpm <= 0.0 {
                diesel_max_rpm = block.max_rpm;
            }
        }
        let diesel_rate_of_change_up_rpm_pss = diesel_block
            .as_ref()
            .map(|b| b.rate_of_change_up_rpm_pss)
            .unwrap_or(0.0);
        let diesel_rate_of_change_down_rpm_pss = diesel_block
            .as_ref()
            .map(|b| b.rate_of_change_down_rpm_pss)
            .unwrap_or(0.0);
        let diesel_change_up_rpm_ps = diesel_block
            .as_ref()
            .map(|b| b.change_up_rpm_ps)
            .unwrap_or(0.0);
        let diesel_change_down_rpm_ps = diesel_block
            .as_ref()
            .map(|b| b.change_down_rpm_ps)
            .unwrap_or(0.0);
        let (davis_a_n, davis_b_n_per_mps, davis_c_n_per_mps2) = parse_orts_davis(ast);
        let run_up_time_s =
            find_optional_scalar_field(ast, &["RunUpTimeToMaxForce"], context)?.unwrap_or(0.0);
        let drive_wheel_mass_kg = parse_drive_wheel_mass_kg(ast);
        let (curtius_a, curtius_b, curtius_c) = parse_curtius_kniffler(ast);
        let motor_heating_time_s = find_optional_scalar_field(
            ast,
            &[
                "TractiveForceAvgUsageTimeFactor",
                "ORTSTractiveForceAvgUsageTimeFactor",
            ],
            context,
        )?
        .unwrap_or(0.0);
        let traction_force_ramp_up_nps = parse_force_rate_field(
            ast,
            &[
                "ORTSTractiveForceRampUpRate",
                "TractiveForceRampUpRate",
                "TractionForceRampUpNpS",
            ],
        );
        let traction_force_ramp_down_nps = parse_force_rate_field(
            ast,
            &[
                "ORTSTractiveForceRampDownRate",
                "TractiveForceRampDownRate",
                "TractionForceRampDownNpS",
            ],
        );
        let traction_force_ramp_down_to_zero_nps = parse_force_rate_field(
            ast,
            &[
                "ORTSTractiveForceRampDownToZeroRate",
                "TractiveForceRampDownToZeroRate",
                "TractionForceRampDownToZeroNpS",
            ],
        );
        let traction_power_ramp_up_wps = parse_power_rate_field(
            ast,
            &[
                "ORTSTractivePowerRampUpRate",
                "TractivePowerRampUpRate",
                "TractionPowerRampUpWpS",
            ],
        );
        let traction_power_ramp_down_wps = parse_power_rate_field(
            ast,
            &[
                "ORTSTractivePowerRampDownRate",
                "TractivePowerRampDownRate",
                "TractionPowerRampDownWpS",
            ],
        );
        let traction_power_ramp_down_to_zero_wps = parse_power_rate_field(
            ast,
            &[
                "ORTSTractivePowerRampDownToZeroRate",
                "TractivePowerRampDownToZeroRate",
                "TractionPowerRampDownToZeroWpS",
            ],
        );
        let continuous_force_time_factor_s = find_optional_scalar_field(
            ast,
            &["ORTSContinuousForceTimeFactor", "ContinuousForceTimeFactor"],
            context,
        )?
        .unwrap_or(0.0);

        let max_rail_output_power_w = find_optional_quantity_field(
            ast,
            QuantityKind::Power,
            &[
                "LocomotiveMaxRailOutputPowerW",
                "ORTSMaxRailOutputPowerW",
                "MaxRailOutputPowerW",
            ],
            context,
        )?
        .unwrap_or(0.0);
        let unloading_speed_mps = find_optional_quantity_field(
            ast,
            QuantityKind::Velocity,
            &["ORTSUnloadingSpeed", "UnloadingSpeedMpS", "UnloadingSpeed"],
            context,
        )?
        .unwrap_or(0.0);
        let tractive_force_power_limited = parse_orts_bool_field(
            ast,
            &[
                "ORTSTractiveForceIsPowerLimited",
                "TractiveForceIsPowerLimited",
            ],
        );
        let speed_of_max_continuous_force_mps = find_optional_quantity_field(
            ast,
            QuantityKind::Velocity,
            &["SpeedOfMaxContinuousForce", "ORTSSpeedOfMaxContinuousForce"],
            context,
        )?
        .unwrap_or(0.0);
        let diesel_reverse_throttle_rpm_tab = parse_reverse_throttle_rpm_tab(ast);
        let friction = parse_orts_friction_fields(ast, true, &name);
        let (brake_shoe_type, brake_shoe_friction) = parse_orts_brake_shoe(ast);

        Ok(Self {
            name,
            mass_kg,
            max_power_w,
            max_velocity_mps,
            max_tractive_effort_n,
            max_continuous_force_n,
            max_brake_force_n,
            regen_factor,
            diesel_sfc_g_per_kwh,
            traction_curve,
            diesel_notch_curves,
            diesel_power_tab,
            diesel_throttle_rpm_tab,
            diesel_idle_rpm,
            diesel_max_rpm,
            davis_a_n,
            davis_b_n_per_mps,
            davis_c_n_per_mps2,
            run_up_time_s,
            diesel_rate_of_change_up_rpm_pss,
            diesel_rate_of_change_down_rpm_pss,
            diesel_change_up_rpm_ps,
            diesel_change_down_rpm_ps,
            max_rail_output_power_w,
            unloading_speed_mps,
            tractive_force_power_limited,
            speed_of_max_continuous_force_mps,
            diesel_reverse_throttle_rpm_tab,
            drive_wheel_mass_kg,
            curtius_a,
            curtius_b,
            curtius_c,
            motor_heating_time_s,
            traction_force_ramp_up_nps,
            traction_force_ramp_down_nps,
            traction_force_ramp_down_to_zero_nps,
            traction_power_ramp_up_wps,
            traction_power_ramp_down_wps,
            traction_power_ramp_down_to_zero_wps,
            continuous_force_time_factor_s,
            wagon_shape,
            length_m,
            steam,
            friction,
            brake_shoe_type,
            brake_shoe_friction,
        })
    }
}

#[derive(Clone, Copy)]
enum QuantityKind {
    Force,
    Velocity,
    Power,
    Length,
    Pressure,
}

fn find_mass_field(root: &Ast, keys: &[&str], context: &str) -> Result<f64, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return parse_mass_ast(value).ok_or_else(|| FormatError::UnexpectedAtom {
                key: (*key).to_string(),
                context: context.to_string(),
                expected: "mass quantity".to_string(),
            });
        }
    }
    Err(FormatError::MissingField {
        key: keys.join("|"),
        context: context.to_string(),
    })
}

fn find_optional_quantity_field(
    root: &Ast,
    kind: QuantityKind,
    keys: &[&str],
    context: &str,
) -> Result<Option<f64>, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            let parsed = match kind {
                QuantityKind::Force => parse_force_ast(value),
                QuantityKind::Velocity => parse_velocity_ast(value),
                QuantityKind::Power => parse_power_ast(value),
                QuantityKind::Length => parse_length_ast(value),
                QuantityKind::Pressure => parse_pressure_ast(value),
            };
            return parsed.map(Some).ok_or_else(|| FormatError::UnexpectedAtom {
                key: (*key).to_string(),
                context: context.to_string(),
                expected: "MSTS quantity".to_string(),
            });
        }
    }
    Ok(None)
}

fn find_optional_scalar_field(
    root: &Ast,
    keys: &[&str],
    context: &str,
) -> Result<Option<f64>, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return parse_scalar_ast(value)
                .map(Some)
                .ok_or_else(|| FormatError::UnexpectedAtom {
                    key: (*key).to_string(),
                    context: context.to_string(),
                    expected: "numeric quantity".to_string(),
                });
        }
    }
    Ok(None)
}

fn parse_scalar_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| s.parse::<f64>().ok())),
        Ast::List(items) => items.iter().find_map(parse_scalar_ast),
    }
}

fn parse_mass_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom).or_else(|| atom_to_string(atom).and_then(|s| parse_mass_kg(&s)))
        }
        Ast::List(items) => {
            if items.len() >= 2
                && let (Some(num), Some(unit)) = (
                    items.first().and_then(|a| match a {
                        Ast::Atom(atom) => atom_to_number(atom),
                        _ => None,
                    }),
                    items.get(1).and_then(|a| match a {
                        Ast::Atom(atom) => atom_to_string(atom),
                        _ => None,
                    }),
                )
                && let Some(kg) = parse_mass_kg(&format!("{num}{unit}"))
            {
                return Some(kg);
            }
            items.first().and_then(parse_mass_ast)
        }
    }
}

fn parse_force_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom).or_else(|| atom_to_string(atom).and_then(|s| parse_force_n(&s)))
        }
        Ast::List(items) => items.first().and_then(parse_force_ast),
    }
}

fn parse_velocity_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => atom_to_number(atom)
            .map(kmh_to_mps)
            .or_else(|| atom_to_string(atom).and_then(|s| parse_velocity_mps(&s))),
        Ast::List(items) => items.first().and_then(parse_velocity_ast),
    }
}

fn parse_power_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom).or_else(|| atom_to_string(atom).and_then(|s| parse_power_w(&s)))
        }
        Ast::List(items) => items.first().and_then(parse_power_ast),
    }
}

fn parse_length_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => {
            atom_to_number(atom).or_else(|| atom_to_string(atom).and_then(|s| parse_length_m(&s)))
        }
        Ast::List(items) => items.first().and_then(parse_length_ast),
    }
}

fn parse_pressure_ast(value: &Ast) -> Option<f64> {
    match value {
        Ast::Atom(atom) => atom_to_number(atom)
            .or_else(|| atom_to_string(atom).and_then(|s| parse_pressure_bar(&s))),
        Ast::List(items) => items.first().and_then(parse_pressure_ast),
    }
}

fn quantity_from_atom(atom: &Atom) -> Option<String> {
    atom_to_string(atom)
}

fn parse_length_from_ast(ast: &Ast) -> Option<f64> {
    for key in [
        "ORTSLengthCouplerFace",
        "ORTSLengthCarBody",
        "Length",
        "WagonLength",
    ] {
        if let Some(v) = find_list_value(ast, key) {
            if let Some(len) = parse_length_ast(v) {
                return Some(len);
            }
        }
    }
    let mut found = None;
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("Size") {
                if items.len() >= 4 {
                    found = parse_length_ast(&items[3]);
                } else if let Some(Ast::List(dims)) = items.get(1) {
                    if dims.len() >= 3 {
                        found = parse_length_ast(&dims[2]);
                    }
                }
            }
        }
        None
    });
    found
}

/// Extract flat numeric pairs from the children of a named list.
///
/// Handles three formats that appear in MSTS/OR files:
/// 1. Flat-sibling: `(Name a b c d …)` → pairs from direct Atom children.
/// 2. Nested-row: `(Name (a b) (c d) …)` → one pair per inner list.
/// 3. Single-wrapper: `(Name (a b c d …))` → all flat atoms from the single
///    inner list (produced by stub files that wrap values in an extra paren).
fn extract_pair_tab(items: &[Ast]) -> Vec<(f64, f64)> {
    let mut result = Vec::new();
    let mut flat = Vec::new();

    // Collect children after the head token.
    let children: Vec<&Ast> = items.iter().skip(1).collect();

    // If there is exactly ONE child and it is a list of pure atoms (no symbol
    // head), treat it as the single-wrapper format: unwrap and use its atoms.
    let effective: Vec<&Ast> = if children.len() == 1 {
        if let Ast::List(inner) = children[0] {
            let all_atoms = inner
                .iter()
                .all(|n| matches!(n, Ast::Atom(Atom::Number(_) | Atom::Integer(_))));
            if all_atoms {
                inner.iter().collect()
            } else {
                children
            }
        } else {
            children
        }
    } else {
        children
    };

    for item in &effective {
        match item {
            Ast::List(row) if row.len() >= 2 => {
                // Nested-row: `(a b)` or `(a b extra…)` — take first pair only.
                let a = row.first().and_then(parse_scalar_ast);
                let b = row.get(1).and_then(parse_scalar_ast);
                if let (Some(a), Some(b)) = (a, b) {
                    result.push((a, b));
                }
            }
            Ast::Atom(atom) => {
                if let Some(v) = quantity_from_atom(atom)
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| atom_to_number(atom))
                {
                    flat.push(v);
                }
            }
            _ => {}
        }
    }
    for chunk in flat.chunks(2) {
        if chunk.len() == 2 {
            result.push((chunk[0], chunk[1]));
        }
    }
    result
}

/// Fields extracted from `ORTSDieselEngines` (first engine block).
#[derive(Clone, Debug, Default, PartialEq)]
struct OrtsDieselEngineBlock {
    power_tab: Vec<(f64, f64)>,
    throttle_rpm_tab: Vec<(f64, f64)>,
    idle_rpm: f64,
    max_rpm: f64,
    rate_of_change_up_rpm_pss: f64,
    rate_of_change_down_rpm_pss: f64,
    change_up_rpm_ps: f64,
    change_down_rpm_ps: f64,
}

/// Parse `ORTSDieselEngines` — nested `Diesel ( ... )`, flat `Diesel(` siblings, or
/// fields directly under `ORTSDieselEngines`.
fn parse_ortsdieselengines_diesel(ast: &Ast) -> Option<OrtsDieselEngineBlock> {
    walk_lists_find::<OrtsDieselEngineBlock, _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("ORTSDieselEngines") {
                let block = parse_diesel_block_fields(items);
                if !block.power_tab.is_empty()
                    || !block.throttle_rpm_tab.is_empty()
                    || block.idle_rpm > 0.0
                    || block.rate_of_change_up_rpm_pss > 0.0
                {
                    return Some(block);
                }
            }
        }
        None
    })
}

fn parse_diesel_scalar_in_subtree(ast: &Ast, key: &str) -> f64 {
    let mut value = 0.0_f64;
    walk_lists_find::<(), _>(ast, &mut |items| {
        if items.len() >= 2
            && matches!(&items[0], Ast::Atom(Atom::Symbol(head)) if head.eq_ignore_ascii_case(key))
            && let Some(v) = parse_scalar_ast(&items[1])
        {
            value = value.max(v);
        }
        None
    });
    value
}

fn parse_diesel_block_fields(diesel_items: &[Ast]) -> OrtsDieselEngineBlock {
    let subtree = Ast::List(diesel_items.to_vec());
    OrtsDieselEngineBlock {
        power_tab: parse_rpm_power_tab(&subtree),
        throttle_rpm_tab: parse_throttle_rpm_tab(&subtree),
        idle_rpm: parse_diesel_scalar_in_subtree(&subtree, "IdleRPM")
            .max(parse_diesel_scalar_in_subtree(&subtree, "ORTSIdleRPM")),
        max_rpm: parse_diesel_scalar_in_subtree(&subtree, "MaxRPM")
            .max(parse_diesel_scalar_in_subtree(&subtree, "ORTSMaxRPM")),
        rate_of_change_up_rpm_pss: parse_diesel_scalar_in_subtree(&subtree, "RateOfChangeUpRPMpSS"),
        rate_of_change_down_rpm_pss: parse_diesel_scalar_in_subtree(
            &subtree,
            "RateOfChangeDownRPMpSS",
        ),
        change_up_rpm_ps: parse_diesel_scalar_in_subtree(&subtree, "ChangeUpRPMpS"),
        change_down_rpm_ps: parse_diesel_scalar_in_subtree(&subtree, "ChangeDownRPMpS"),
    }
}

fn parse_drive_wheel_mass_kg(ast: &Ast) -> f64 {
    find_list_value(ast, "ORTSDriveWheelWeight")
        .and_then(parse_mass_ast)
        .unwrap_or(0.0)
}

fn parse_curtius_kniffler(ast: &Ast) -> (f64, f64, f64) {
    walk_lists_find::<(f64, f64, f64), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("ORTSCurtius_Kniffler") {
                let coeffs: Vec<f64> = if items.len() >= 4 {
                    (1..4).filter_map(|i| parse_scalar_ast(&items[i])).collect()
                } else if let Some(Ast::List(nested)) = items.get(1) {
                    nested.iter().filter_map(parse_scalar_ast).collect()
                } else {
                    Vec::new()
                };
                if coeffs.len() >= 3 {
                    return Some((coeffs[0], coeffs[1], coeffs[2]));
                }
            }
        }
        None
    })
    .unwrap_or((0.0, 0.0, 0.0))
}

/// OR `ORTSDavis_*` in default SI units (N, N/(m/s), N/(m/s)²).
/// Parse force rate fields (N/s); accepts plain numbers or `kN/s`-style quantities.
fn parse_force_rate_field(ast: &Ast, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(v) = find_list_value(ast, key).and_then(parse_scalar_ast) {
            return v;
        }
        if let Some(v) = find_list_value(ast, key).and_then(parse_force_ast) {
            return v;
        }
    }
    0.0
}

/// Parse power rate fields (W/s).
fn parse_power_rate_field(ast: &Ast, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(v) = find_list_value(ast, key).and_then(parse_scalar_ast) {
            return v;
        }
        if let Some(v) = find_list_value(ast, key).and_then(parse_power_ast) {
            return v;
        }
    }
    0.0
}

fn parse_orts_davis(ast: &Ast) -> (f64, f64, f64) {
    let context = "ORTSDavis";
    (
        find_optional_scalar_field(ast, &["ORTSDavis_A"], context)
            .ok()
            .flatten()
            .unwrap_or(0.0),
        find_optional_scalar_field(ast, &["ORTSDavis_B"], context)
            .ok()
            .flatten()
            .unwrap_or(0.0),
        find_optional_scalar_field(ast, &["ORTSDavis_C"], context)
            .ok()
            .flatten()
            .unwrap_or(0.0),
    )
}

/// Parse `DieselPowerTab` (RPM → shaft power in Watts).
fn parse_rpm_power_tab(ast: &Ast) -> Vec<(f64, f64)> {
    let mut found = Vec::new();
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("DieselPowerTab") {
                found = extract_pair_tab(items);
            }
        }
        None
    });
    found
}

fn parse_orts_bool_field(ast: &Ast, keys: &[&str]) -> bool {
    for key in keys {
        if let Some(v) = find_list_value(ast, key).and_then(parse_scalar_ast) {
            return v != 0.0;
        }
    }
    false
}

/// Parse `ReverseThrottleRPMTab` (RPM → throttle %); converts % to 0–1.
fn parse_reverse_throttle_rpm_tab(ast: &Ast) -> Vec<(f64, f64)> {
    let mut found = Vec::new();
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("ReverseThrottleRPMTab") {
                let raw = extract_pair_tab(items);
                found = raw.into_iter().map(|(r, t)| (r, t / 100.0)).collect();
            }
        }
        None
    });
    found
}

/// Parse `ThrottleRPMTab` (throttle % → target RPM); converts % to 0-1.
fn parse_throttle_rpm_tab(ast: &Ast) -> Vec<(f64, f64)> {
    let mut found = Vec::new();
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("ThrottleRPMTab") {
                let raw = extract_pair_tab(items);
                found = raw.into_iter().map(|(t, r)| (t / 100.0, r)).collect();
            }
        }
        None
    });
    found
}

fn parse_diesel_power_tab_max(ast: &Ast) -> Option<f64> {
    let mut best = 0.0_f64;
    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("DieselPowerTab") {
                let mut nums = Vec::new();
                for item in items.iter().skip(1) {
                    match item {
                        Ast::List(row) if row.len() >= 2 => {
                            if let Some(p) = row.get(1).and_then(parse_scalar_ast) {
                                best = best.max(p);
                            }
                        }
                        Ast::Atom(atom) => {
                            if let Some(v) = quantity_from_atom(atom)
                                .and_then(|s| s.parse::<f64>().ok())
                                .or_else(|| atom_to_number(atom))
                            {
                                nums.push(v);
                            }
                        }
                        _ => {}
                    }
                }
                for chunk in nums.chunks(2) {
                    if chunk.len() == 2 {
                        best = best.max(chunk[1]);
                    }
                }
            }
        }
        None
    });
    if best > 0.0 { Some(best) } else { None }
}

fn parse_steam_fields(ast: &Ast) -> Option<MstsSteamFields> {
    let cylinder_count = find_optional_scalar_field(
        ast,
        &["NumCylinders", "ORTSNumCylinder", "NumCylinder"],
        "Steam",
    )
    .ok()
    .flatten()
    .map(|v| v.round().max(1.0) as u32);
    let bore = find_optional_quantity_field(
        ast,
        QuantityKind::Length,
        &["CylinderDiameter", "ORTSCylinderDiameter"],
        "Steam",
    )
    .ok()
    .flatten();
    let stroke = find_optional_quantity_field(
        ast,
        QuantityKind::Length,
        &["CylinderStroke", "ORTSCylinderStroke"],
        "Steam",
    )
    .ok()
    .flatten();
    let wheel = find_optional_quantity_field(
        ast,
        QuantityKind::Length,
        &[
            "WheelRadius",
            "DrivingWheelDiameter",
            "ORTSDrivingWheelDiameter",
        ],
        "Steam",
    )
    .ok()
    .flatten()
    .map(|d| {
        // DrivingWheelDiameter is full diameter; WheelRadius is radius.
        if d > 2.0 { d / 2.0 } else { d }
    });
    let pressure = find_optional_quantity_field(
        ast,
        QuantityKind::Pressure,
        &[
            "MaxBoilerPressure",
            "BoilerPressure",
            "ORTSMaxBoilerPressure",
        ],
        "Steam",
    )
    .ok()
    .flatten();

    let cylinder_count = cylinder_count?;
    let cylinder_bore_m = bore?;
    let piston_stroke_m = stroke?;
    let driving_wheel_radius_m = wheel?;
    let working_pressure_bar = pressure.unwrap_or(16.0);

    Some(MstsSteamFields {
        cylinder_count,
        cylinder_bore_m,
        piston_stroke_m,
        driving_wheel_radius_m,
        working_pressure_bar,
        evaporation_rate_kg_per_s: 8.0,
        coal_consumption_kg_per_s: 0.5,
        initial_water_kg: 12_000.0,
        initial_coal_kg: 6_000.0,
    })
}

fn parse_traction_curve(ast: &Ast) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::new();

    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("MaxTractiveEffortCurves") {
                for item in items.iter().skip(1) {
                    if let Ast::List(entry_items) = item {
                        if let Some(Ast::Atom(Atom::Symbol(tag))) = entry_items.first() {
                            if tag.eq_ignore_ascii_case("CurveEntry") && entry_items.len() >= 3 {
                                let v = entry_items.get(1).and_then(parse_scalar_ast);
                                let f = entry_items.get(2).and_then(parse_scalar_ast);
                                if let (Some(v_val), Some(f_val)) = (v, f) {
                                    points.push((kmh_to_mps(v_val), f_val));
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    });

    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    points
}

/// `ORTSMaxTractiveForceCurves` values have no unit suffix in MSTS/OR files,
/// which means OR's STFReader treats them as Newtons (the SI default for Force).
/// No conversion is needed; return the value unchanged.
fn orts_curve_force_n(value: f64) -> f64 {
    value
}

/// Parse (speed_mps, force_n) pairs from a notch sub-list.
///
/// Speed values in `ORTSMaxTractiveForceCurves` are in m/s (OR's default for
/// Speed when no unit suffix is present).  Force values are in N (OR's default
/// for Force).  No unit conversion is applied here.
fn parse_orts_curve_points(items: &[Ast]) -> Vec<(f64, f64)> {
    let mut curve = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if let Ast::List(pair) = &items[i] {
            if pair.len() >= 2 {
                if let (Some(v), Some(f)) = (parse_scalar_ast(&pair[0]), parse_scalar_ast(&pair[1]))
                {
                    curve.push((v, orts_curve_force_n(f)));
                }
            }
            i += 1;
            continue;
        }
        if let (Some(v), Some(f)) = (
            parse_scalar_ast(&items[i]),
            items.get(i + 1).and_then(parse_scalar_ast),
        ) {
            curve.push((v, orts_curve_force_n(f)));
            i += 2;
        } else {
            i += 1;
        }
    }
    curve.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    curve
}

fn parse_orts_notch_curves(ast: &Ast) -> Vec<(f64, Vec<(f64, f64)>)> {
    let mut out: Vec<(f64, Vec<(f64, f64)>)> = Vec::new();

    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("ORTSMaxTractiveForceCurves") {
                let mut i = 1;
                while i < items.len() {
                    match &items[i] {
                        Ast::List(group) if group.len() >= 2 => {
                            let mut j = 0;
                            while j + 1 < group.len() {
                                if let Some(throttle) = parse_scalar_ast(&group[j]) {
                                    let curve = match &group[j + 1] {
                                        Ast::List(curve_items) => {
                                            parse_orts_curve_points(curve_items)
                                        }
                                        _ => parse_orts_curve_points(&group[j + 1..]),
                                    };
                                    if !curve.is_empty() {
                                        out.push((throttle, curve));
                                        j += 2;
                                        continue;
                                    }
                                }
                                j += 1;
                            }
                            i += 1;
                        }
                        throttle_ast => {
                            if let (Some(throttle), Some(Ast::List(curve_items))) =
                                (parse_scalar_ast(throttle_ast), items.get(i + 1))
                            {
                                let curve = parse_orts_curve_points(curve_items);
                                if !curve.is_empty() {
                                    out.push((throttle, curve));
                                }
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                    }
                }
            }
        }
        None
    });

    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_from_first_paren;

    #[test]
    fn parse_msts_diesel_engine_with_units() {
        let text = r#"
( Engine
    ( Name "Blue Pullman" )
    ( Mass ( 68t-uk ) )
    ( MaxForce 12000lbf )
    ( MaxVelocity 90mph )
    ( MaxBrakeForce 70kN )
    ( Size 2.871m 3.876m 20.602m )
    ( DieselPowerTab (
        ( 0 0 )
        ( 1500 745513 )
    ))
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert!(eng.max_tractive_effort_n > 10_000.0);
        assert!(eng.max_power_w > 0.0);
    }

    #[test]
    fn parse_orts_notch_curves_reads_n_values_directly() {
        // OR's STFReader treats bare numbers in ORTSMaxTractiveForceCurves as
        // Newtons (default SI unit for Force) and m/s (default for Speed).
        // Speeds like 10.0 and 20.0 are m/s; forces like 86073 are N.
        let text = r#"
( Engine
    ( Mass 68000 )
    ( MaxForce 12000lbf )
    ( ORTSMaxTractiveForceCurves (
        0.10 (
            0.0 5945
            10.0 2432
        )
        1.00 (
            0.0 86073
            20.0 993
        )
    ))
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert_eq!(eng.diesel_notch_curves.len(), 2);
        let (_, full) = eng
            .diesel_notch_curves
            .iter()
            .find(|(n, _)| (*n - 1.0).abs() < 1e-6)
            .expect("full notch");
        // Stall force at v=0: 86073 N (already Newtons, no conversion).
        let stall = full.iter().find(|(v, _)| v.abs() < 1e-6).unwrap().1;
        assert!(
            (stall - 86073.0).abs() < 1.0,
            "expected 86073 N, got {stall}"
        );
        // Speed axis: 20.0 stored as 20.0 m/s (no km/h→m/s conversion).
        let high_speed_entry = full.iter().find(|(v, _)| (v - 20.0).abs() < 0.1).unwrap();
        assert!((high_speed_entry.0 - 20.0).abs() < 0.1, "speed axis is m/s");
    }

    #[test]
    fn parse_ortsdieselengines_block() {
        let text = r#"
( Engine
    ( Mass 68000 )
    ( MaxForce 53379 )
    ( ORTSDieselEngines (
        ( IdleRPM 650 )
        ( MaxRPM 1500 )
        ( DieselPowerTab (
            0 0
            650 22188
            1500 745513
        ))
        ( ThrottleRPMTab (
            0 650
            100 1500
        ))
    ))
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert_eq!(eng.diesel_idle_rpm, 650.0);
        assert_eq!(eng.diesel_max_rpm, 1500.0);
        assert!(!eng.diesel_power_tab.is_empty());
        assert!((eng.diesel_power_tab.last().unwrap().1 - 745_513.0).abs() < 1.0);
        assert_eq!(eng.diesel_throttle_rpm_tab.len(), 2);
        assert!((eng.diesel_throttle_rpm_tab[1].0 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn parse_orts_davis_fields() {
        let text = r#"
( Engine
    ( Mass 68000 )
    ( ORTSDavis_A ( 433 ) )
    ( ORTSDavis_B ( 27.2 ) )
    ( ORTSDavis_C ( 3.50 ) )
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert_eq!(eng.davis_a_n, 433.0);
        assert_eq!(eng.davis_b_n_per_mps, 27.2);
        assert_eq!(eng.davis_c_n_per_mps2, 3.50);
    }

    #[test]
    fn parse_ortsdieselengines_flat_msts() {
        // MSTS count prefix + fields as siblings (OR content layout).
        let text = r#"
( Engine
    ( Mass 68000 )
    ( ORTSDieselEngines ( 1
        ( IdleRPM 650 )
        ( MaxRPM 1500 )
        ( ChangeUpRPMpS 50 )
        ( ChangeDownRPMpS 40 )
        ( RateOfChangeUpRPMpSS 10 )
        ( RateOfChangeDownRPMpSS 10 )
        ( DieselPowerTab (
            0 0
            650 22188
            1500 745513
        ))
        ( ThrottleRPMTab (
            0 650
            100 1500
        ))
    ))
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert_eq!(eng.diesel_idle_rpm, 650.0);
        assert_eq!(eng.diesel_change_up_rpm_ps, 50.0);
        assert_eq!(eng.diesel_rate_of_change_up_rpm_pss, 10.0);
        assert!(!eng.diesel_power_tab.is_empty());
    }

    #[test]
    fn parse_drive_wheel_and_curtius() {
        let text = r#"
( Engine
    ( Mass 68000 )
    ( ORTSDriveWheelWeight ( 64t-uk ) )
    ( ORTSCurtius_Kniffler ( 6.18 44 0.161 0.7 ) )
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert!(eng.drive_wheel_mass_kg > 60_000.0);
        assert_eq!(eng.curtius_a, 6.18);
        assert_eq!(eng.curtius_b, 44.0);
        assert_eq!(eng.curtius_c, 0.161);
    }

    #[test]
    fn parse_chiltern_dmbsa_stub_diesel_engine_params() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng");
        if !path.exists() {
            return;
        }
        let text = std::fs::read_to_string(&path).expect("read stub");
        let ast = parse_from_first_paren(&text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        assert_eq!(eng.davis_a_n, 433.0);
        assert!(!eng.diesel_power_tab.is_empty());
        assert!(!eng.diesel_throttle_rpm_tab.is_empty());
        assert_eq!(eng.diesel_idle_rpm, 650.0);
        assert_eq!(eng.diesel_max_rpm, 1500.0);
        assert_eq!(eng.diesel_change_up_rpm_ps, 50.0);
        assert!(eng.drive_wheel_mass_kg > 60_000.0);
    }

    #[test]
    fn parse_msts_steam_fields_when_present() {
        let text = r#"
( Engine
    ( Name "Consolidation" )
    ( Mass 82000 )
    ( NumCylinders 2 )
    ( CylinderDiameter ( 0.470m ) )
    ( CylinderStroke ( 0.660m ) )
    ( WheelRadius ( 0.970m ) )
    ( MaxBoilerPressure ( 16bar ) )
)"#;
        let ast = parse_from_first_paren(text).expect("parse");
        let eng = EngineFile::from_ast(&ast).expect("engine");
        let steam = eng.steam.expect("steam");
        assert_eq!(steam.cylinder_count, 2);
        assert!((steam.cylinder_bore_m - 0.47).abs() < 0.01);
    }
}
