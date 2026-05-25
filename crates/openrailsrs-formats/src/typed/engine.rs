use crate::ast::{Ast, Atom};
use crate::error::FormatError;
use crate::units::kmh_to_mps;

use super::{
    atom_to_number, find_numeric_field, find_optional_numeric_field, find_optional_string_field,
    walk_lists_find,
};

#[derive(Clone, Debug, PartialEq)]
pub struct EngineFile {
    pub name: String,
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_velocity_mps: f64,
    pub max_tractive_effort_n: f64,
    pub max_brake_force_n: f64,
    /// Fraction of braking energy recovered (0.0 = none, 0.7 = modern EMU).
    pub regen_factor: f64,
    /// Specific fuel consumption in g/kWh; `None` for electric traction.
    pub diesel_sfc_g_per_kwh: Option<f64>,
    /// Piecewise-linear traction curve as (velocity_mps, force_n) pairs.
    /// Empty when not present in the file (caller falls back to P/v law).
    pub traction_curve: Vec<(f64, f64)>,
    /// Visual shape filename (`WagonShape` in MSTS `.eng`).
    pub wagon_shape: Option<String>,
    /// Body length in metres (coupling to coupling approximation).
    pub length_m: f64,
}

impl EngineFile {
    pub fn from_ast(ast: &Ast) -> Result<Self, FormatError> {
        let context = "Engine";
        let mass_kg = find_numeric_field(ast, &["Mass", "MassKG"], context)?;
        let max_power_w =
            find_optional_numeric_field(ast, &["MaxPower", "MaxForce"], context)?.unwrap_or(0.0);
        let max_velocity_mps = kmh_to_mps(
            find_optional_numeric_field(ast, &["MaxVelocity", "MaxSpeed"], context)?
                .unwrap_or(120.0),
        );
        let max_tractive_effort_n =
            find_optional_numeric_field(ast, &["MaxTractiveEffort"], context)?.unwrap_or(350_000.0);
        let max_brake_force_n =
            find_optional_numeric_field(ast, &["MaxBrakeForce", "Brake"], context)?
                .unwrap_or(200_000.0);
        let name = find_optional_string_field(ast, &["Name"], context)?
            .unwrap_or_else(|| "engine".to_string());
        let regen_factor =
            find_optional_numeric_field(ast, &["RegenFactor", "RegenBraking"], context)?
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
        let diesel_sfc_g_per_kwh =
            find_optional_numeric_field(ast, &["SpecificFuelConsumption", "DieselSfc"], context)?;
        let wagon_shape = find_optional_string_field(ast, &["WagonShape", "Shape"], context)?;
        let length_m =
            find_optional_numeric_field(ast, &["Length", "WagonLength"], context)?.unwrap_or(18.0);
        let traction_curve = parse_traction_curve(ast);

        Ok(Self {
            name,
            mass_kg,
            max_power_w,
            max_velocity_mps,
            max_tractive_effort_n,
            max_brake_force_n,
            regen_factor,
            diesel_sfc_g_per_kwh,
            traction_curve,
            wagon_shape,
            length_m,
        })
    }
}

/// Parse `(MaxTractiveEffortCurves (CurveEntry v f) ...)` from any MSTS engine file.
///
/// Returns an empty vec if the section is absent (caller uses P/v fallback).
/// Velocity is assumed to be in km/h and converted to m/s; force in Newtons.
fn parse_traction_curve(ast: &Ast) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::new();

    walk_lists_find::<(), _>(ast, &mut |items| {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            // Locate the MaxTractiveEffortCurves container.
            if head.eq_ignore_ascii_case("MaxTractiveEffortCurves") {
                for item in items.iter().skip(1) {
                    if let Ast::List(entry_items) = item {
                        if let Some(Ast::Atom(Atom::Symbol(tag))) = entry_items.first() {
                            if tag.eq_ignore_ascii_case("CurveEntry") && entry_items.len() >= 3 {
                                let v = entry_items.get(1).and_then(|a| match a {
                                    Ast::Atom(at) => atom_to_number(at),
                                    _ => None,
                                });
                                let f = entry_items.get(2).and_then(|a| match a {
                                    Ast::Atom(at) => atom_to_number(at),
                                    _ => None,
                                });
                                if let (Some(v_val), Some(f_val)) = (v, f) {
                                    // Velocity in the curve is km/h; convert to m/s.
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

    // Sort by velocity so the curve is monotonically ordered.
    points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    points
}
