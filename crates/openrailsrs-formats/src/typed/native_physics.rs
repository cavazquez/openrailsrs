//! Optional authored vehicle dynamics used by the native physics path.
use super::vehicle_runtime::scalar;
use crate::Ast;

#[derive(Clone, Debug, Default)]
pub struct NativeVehiclePhysics {
    pub roller_bearing: bool,
    pub legacy_friction: bool,
    pub wheel_radius_m: f64,
    pub axle_load_kg: f64,
    pub drive_axles: f64,
    pub pitch_span_m: Option<f64>,
    pub throttle_notches: Vec<(f64, bool)>,
    pub traction_cutoff_bar: Option<f64>,
    pub authored_force_curves: bool,
    pub rail_power_limit_w: Option<f64>,
    pub rigid_connection: bool,
    pub driven_mass_kg: f64,
    pub sander: NativeSanderParams,
}

/// OR 1.6.1 sandbox defaults; quantities are resolved once, before simulation.
#[derive(Clone, Debug)]
pub struct NativeSanderParams {
    pub capacity_m3: f64,
    pub forward_m3_s: f64,
    pub reverse_m3_s: f64,
    pub max_speed_mps: f64,
    pub effect_up_to_mps: f64,
}
impl Default for NativeSanderParams {
    fn default() -> Self {
        Self {
            capacity_m3: 40. * 0.028316846592,
            forward_m3_s: 3.4 * 0.028316846592 / 3600.,
            reverse_m3_s: 0.,
            max_speed_mps: 30.,
            effect_up_to_mps: 0.,
        }
    }
}
fn sander_params(ast: &Ast) -> NativeSanderParams {
    // ENG uses Sanding both as a physical speed limit and inside the cab's
    // EngineControllers. Only direct vehicle parameters are physical values.
    let mut fields = Vec::new();
    let mut collect = |block: &Ast| {
        let Ast::List(items) = block else { return };
        for item in items {
            let Ast::List(values) = item else { continue };
            let Some(Ast::Atom(crate::Atom::Symbol(key))) = values.first() else {
                continue;
            };
            if [
                "Sanding",
                "ORTSSanderSpeedEffectUpTo",
                "ORTSMaxTrackSanderBoxCapacity",
                "ORTSMaxTrackSanderSandConsumptionForward",
                "ORTSMaxTrackSanderSandConsumptionReverse",
            ]
            .iter()
            .any(|k| key.eq_ignore_ascii_case(k))
            {
                fields.push(item.clone());
            } else if key.eq_ignore_ascii_case("ORTS") {
                fields.extend(values.iter().filter(|v| {
                    matches!(v, Ast::List(items) if matches!(items.first(), Some(Ast::Atom(crate::Atom::Symbol(k))) if k.eq_ignore_ascii_case("ORTSSanderSpeedEffectUpTo")))
                }).cloned());
            }
        }
    };
    if let Ast::List(items) = ast {
        if matches!(items.first(), Some(Ast::Atom(_))) {
            collect(ast);
        } else {
            for block in items {
                collect(block);
            }
        }
    }
    let ast = &Ast::List(fields);
    let defaults = NativeSanderParams::default();
    let quantity = |key, unit, fallback| match super::traction_operation::quantity(ast, key, unit) {
        Ok(Some(value)) => value / 1000., // strict volume quantities are litres
        Ok(None) => fallback,
        Err(_) => 0., // unsupported authored units disable this supply
    };
    let speed = |key: &str, fallback| match super::traction_operation::quantity(ast, key, "m/s") {
        Ok(Some(value)) => value,
        Ok(None) => fallback,
        Err(_) => 0.,
    };
    NativeSanderParams {
        capacity_m3: quantity("ORTSMaxTrackSanderBoxCapacity", "ft3", defaults.capacity_m3),
        forward_m3_s: quantity(
            "ORTSMaxTrackSanderSandConsumptionForward",
            "ft3/s",
            defaults.forward_m3_s,
        ),
        reverse_m3_s: quantity(
            "ORTSMaxTrackSanderSandConsumptionReverse",
            "ft3/s",
            defaults.reverse_m3_s,
        ),
        max_speed_mps: speed("Sanding", defaults.max_speed_mps),
        effect_up_to_mps: speed("ORTSSanderSpeedEffectUpTo", defaults.effect_up_to_mps),
    }
}

/// Resolve numerical vehicle parameters without loading the shape or textures.
pub fn parse_native_vehicle_physics(ast: &Ast, engine: bool, mass_kg: f64) -> NativeVehiclePhysics {
    let number = |key: &str| {
        scalar(ast, key)
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };
    let length = |key: &str| {
        let block = super::named_blocks(ast, key).into_iter().next()?;
        let Ast::List(items) = block else { return None };
        let values: Vec<_> = items
            .iter()
            .filter_map(|a| match a {
                Ast::Atom(crate::Atom::Symbol(s) | crate::Atom::String(s))
                    if !s.eq_ignore_ascii_case(key) =>
                {
                    Some(s.clone())
                }
                Ast::Atom(crate::Atom::Number(n)) => Some(n.to_string()),
                Ast::Atom(crate::Atom::Integer(n)) => Some(n.to_string()),
                _ => None,
            })
            .collect();
        crate::msts_units::parse_length_m(&values.join(" ")).filter(|v| v.is_finite() && *v > 0.)
    };
    let native_engine_wheels = super::named_blocks(ast, "NumWheels")
        .into_iter()
        .rev()
        .find_map(|block| {
            let Ast::List(items) = block else { return None };
            items
                .iter()
                .find_map(|v| match v {
                    Ast::Atom(a) => super::atom_to_number(a),
                    _ => None,
                })
                .filter(|v| *v > 0. && *v < 7.)
        });
    let drive_axles = number("ORTSNumberDriveAxles")
        .filter(|v| *v > 0.)
        .unwrap_or_else(|| {
            if engine {
                native_engine_wheels.unwrap_or(4.)
            } else {
                0.
            }
        });
    let drive_mass = scalar(ast, "ORTSDriveWheelWeight")
        .and_then(|v| crate::msts_units::parse_mass_kg(&v))
        .unwrap_or(mass_kg);
    let mut notches = Vec::new();
    for block in super::named_blocks(ast, "Throttle") {
        for notch in super::named_blocks(block, "Notch") {
            let Ast::List(items) = notch else { continue };
            let numbers: Vec<_> = items
                .iter()
                .filter_map(|v| match v {
                    Ast::Atom(a) => super::atom_to_number(a),
                    _ => None,
                })
                .collect();
            if numbers.len() >= 2 && (0.0..=1.).contains(&numbers[0]) {
                notches.push((numbers[0], numbers[1] != 0.));
            }
        }
    }
    notches.sort_by(|a, b| a.0.total_cmp(&b.0));
    notches.dedup();
    let cutoff = if number("DoesBrakeCutPower").is_some_and(|v| v != 0.) {
        Some(
            scalar(ast, "BrakeCutsPowerAtBrakeCylinderPressure")
                .and_then(super::vehicle_runtime::pressure)
                .unwrap_or(4. * 0.0689475729),
        )
    } else {
        None
    };
    NativeVehiclePhysics {
        roller_bearing: scalar(ast, "ORTSBearingType")
            .is_some_and(|v| v.eq_ignore_ascii_case("Roller")),
        legacy_friction: super::parse_orts_friction_fields(ast, engine, "")
            .legacy_friction_speed_mps
            .is_some_and(|v| !(0.0..=4.4407).contains(&v)),
        wheel_radius_m: length("WheelRadius").unwrap_or(if engine { 0.5334 } else { 0.4572 }),
        axle_load_kg: if engine {
            drive_mass / drive_axles.max(1.)
        } else {
            mass_kg / number("ORTSNumberAxles").filter(|v| *v > 0.).unwrap_or(4.)
        },
        drive_axles,
        pitch_span_m: length("ORTSLengthBogieCentre"),
        throttle_notches: notches,
        traction_cutoff_bar: cutoff,
        authored_force_curves: !super::named_blocks(ast, "ORTSMaxTractiveForceCurves").is_empty(),
        rail_power_limit_w: scalar(ast, "ORTSMaxRailOutputPower")
            .or_else(|| scalar(ast, "MaxPower"))
            .and_then(|v| crate::msts_units::parse_power_w(&v))
            .filter(|v| v.is_finite() && *v > 0.),
        rigid_connection: super::named_blocks(ast, "CouplingHasRigidConnection")
            .into_iter()
            .any(|block| {
                scalar(block, "CouplingHasRigidConnection")
                    .is_none_or(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false"))
            }),
        driven_mass_kg: drive_mass.clamp(0., mass_kg),
        sander: sander_params(ast),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanding_speed_ignores_cab_controller_ranges_and_reads_final_physical_value() {
        for text in [
            "Engine ( loco EngineControllers ( Sanding ( 0 1 0 ) ) )",
            "(Engine loco (EngineControllers (Sanding 0 1 0)))",
        ] {
            let ast = crate::parse_vehicle_text(text).unwrap();
            let p = parse_native_vehicle_physics(&ast, true, 80000.);
            assert_eq!(p.sander.max_speed_mps, 30.);
        }
        let ast = crate::parse_vehicle_text("Engine ( loco EngineControllers ( Sanding ( 0 1 0 ) ) Sanding ( 20mph ) Sanding ( 6 mph ) ORTS ( ORTSSanderSpeedEffectUpTo ( 36 km/h ) ) )").unwrap();
        let p = parse_native_vehicle_physics(&ast, true, 80000.);
        assert_eq!(p.sander.max_speed_mps, 2.68224);
        assert_eq!(p.sander.effect_up_to_mps, 10.);
        let ast = crate::parse_vehicle_text("Engine ( loco Sanding ( 6banana ) )").unwrap();
        assert_eq!(
            parse_native_vehicle_physics(&ast, true, 80000.)
                .sander
                .max_speed_mps,
            0.
        );
    }
    #[test]
    fn sander_keeps_authored_volume_flow_speed_units_and_rejects_bad_supplies() {
        let ast = crate::parse_vehicle_text("Wagon ( loco ORTSDriveWheelWeight ( 40t ) ) Engine ( loco Sanding ( 20mph ) ORTSSanderSpeedEffectUpTo ( 72km/h ) ORTSMaxTrackSanderBoxCapacity ( 2m3 ) ORTSMaxTrackSanderSandConsumptionForward ( 3.6m3/h ) ORTSMaxTrackSanderSandConsumptionReverse ( 1l/s ) )").unwrap();
        let p = parse_native_vehicle_physics(&ast, true, 80000.);
        assert_eq!(p.driven_mass_kg, 40000.);
        assert_eq!(p.sander.capacity_m3, 2.);
        assert!((p.sander.forward_m3_s - 0.001).abs() < 1e-12);
        assert_eq!(p.sander.reverse_m3_s, 0.001);
        assert!((p.sander.max_speed_mps - 8.9408).abs() < 1e-8);
        assert_eq!(p.sander.effect_up_to_mps, 20.);
        let native = crate::parse_vehicle_text("Engine ( loco ORTSMaxTrackSanderBoxCapacity ( 4 ) ORTSMaxTrackSanderSandConsumptionForward ( 0.01*(ft^3) ) ORTSMaxTrackSanderSandConsumptionReverse ( 0.02 ) )").unwrap();
        let p = parse_native_vehicle_physics(&native, true, 80000.);
        assert!((p.sander.capacity_m3 - 4. * 0.028316846592).abs() < 1e-12);
        assert!((p.sander.forward_m3_s - 0.01 * 0.028316846592).abs() < 1e-12);
        assert!((p.sander.reverse_m3_s - 0.02 * 0.028316846592).abs() < 1e-12);
        let invalid = crate::parse_vehicle_text("Engine ( loco ORTSMaxTrackSanderBoxCapacity ( 12banana ) ORTSMaxTrackSanderSandConsumptionForward ( -2m3/s ) )").unwrap();
        let p = parse_native_vehicle_physics(&invalid, true, 80000.);
        assert_eq!(p.sander.capacity_m3, 0.);
        assert_eq!(p.sander.forward_m3_s, 0.);
    }
    #[test]
    fn native_engine_axles_override_visible_wheels_and_keep_compound_bogie_span() {
        let ast = crate::parse_vehicle_text("Wagon ( tail Mass ( 67t ) NumWheels ( 4 ) WheelRadius ( 42in/2 ) ORTSLengthBogieCentre ( 46ft 6in ) ) Engine ( tail NumWheels ( 1 ) DoesBrakeCutPower ( 1 ) )").unwrap();
        let params = parse_native_vehicle_physics(&ast, true, 67000.);
        assert_eq!(params.drive_axles, 1.);
        assert!((params.wheel_radius_m - 0.5334).abs() < 1e-8);
        assert!((params.pitch_span_m.unwrap() - 14.1732).abs() < 1e-8);
        assert!((params.traction_cutoff_bar.unwrap() / 0.0689475729 - 4.).abs() < 1e-8);
    }
    #[test]
    fn explicit_elastic_couplings_are_not_treated_as_rigid() {
        for value in ["0", "false"] {
            let ast = crate::parse_vehicle_text(&format!(
                "Wagon ( car Coupling ( CouplingHasRigidConnection ( {value} ) ) )"
            ))
            .unwrap();
            assert!(!parse_native_vehicle_physics(&ast, false, 30000.).rigid_connection);
        }
        for value in ["", "1", "true"] {
            let ast = crate::parse_vehicle_text(&format!(
                "Wagon ( car Coupling ( CouplingHasRigidConnection ( {value} ) ) )"
            ))
            .unwrap();
            assert!(parse_native_vehicle_physics(&ast, false, 30000.).rigid_connection);
        }
    }
}
