//! Optional authored vehicle dynamics used by the native physics path.
use super::vehicle_runtime::scalar;
use crate::Ast;

#[derive(Clone, Debug, Default)]
pub struct NativeVehiclePhysics {
    pub max_current_a: Option<f64>,
    pub roller_bearing: bool,
    pub legacy_friction: bool,
    /// Original Friction0N for legacy/FCalc stock, resolved before movement.
    pub legacy_start_force_n: Option<f64>,
    /// Native locomotive inventory mass changes only with continuous freight
    /// animations. Absence keeps the ENG mass fixed even as fuel is consumed.
    pub inventory_mass: Option<NativeInventoryMass>,
    /// TrainCar.UpdateTunnelForce uses Size width * height, not the optional
    /// aerodynamic ORTSWagonFrontalArea parameter. Native dimensions are Single.
    pub body_cross_section_m2: f64,
    pub wheel_radius_m: f64,
    /// Local vehicle axes relative to the current train ordering; supplied by
    /// the consist loader, never inferred from the mesh or force measurements.
    pub flipped: bool,
    pub curtius_kniffler: [f64; 3],
    pub adhesion_k: f64,
    pub slip_warning_percent: f64,
    /// The legacy virtual axle keeps its initialization damping/friction even
    /// while consumable mass changes. Authored Wheelset models are separate.
    pub axle_damping_ns: f64,
    pub axle_friction_n: f64,
    pub axle_load_kg: f64,
    pub drive_axles: f64,
    pub pitch_span_m: Option<f64>,
    pub coupler_face_length_m: Option<f64>,
    pub curve_gauge_m: f64,
    pub curve_rigid_wheelbase_m: f64,
    pub throttle_notches: Vec<(f64, bool)>,
    pub traction_cutoff_bar: Option<f64>,
    pub authored_force_curves: bool,
    pub rail_power_limit_w: Option<f64>,
    pub rigid_connection: bool,
    pub driven_mass_kg: f64,
    pub sander: NativeSanderParams,
}

/// Authored mass bounds used by MSTSWagon.UpdateLocomotiveLoadPhysics.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeInventoryMass {
    pub empty_kg: f64,
    pub full_kg: f64,
}

fn inventory_mass(ast: &Ast, mass_kg: f64) -> Option<NativeInventoryMass> {
    let animations = super::named_blocks(ast, "ORTSFreightAnims")
        .into_iter()
        .last()?;
    // Unlike a commercial payload, locomotive inventory does not require an
    // IntakePoint in the continuous animation (tenders commonly omit it).
    let continuous = super::named_blocks(animations, "FreightAnimContinuous")
        .into_iter()
        .last()?;
    let authored_empty = scalar(animations, "WagonEmptyWeight")
        .and_then(|value| crate::msts_units::parse_mass_kg(&value))
        .map(|value| f64::from(value as f32))
        .filter(|value| value.is_finite());
    let empty = authored_empty
        .filter(|value| *value > 0.)
        .unwrap_or(mass_kg);
    let payload = scalar(continuous, "FreightWeightWhenFull")
        .and_then(|value| crate::msts_units::parse_mass_kg(&value))
        .map(|value| f64::from(value as f32))
        .filter(|value| value.is_finite() && *value > 0.);
    // Native LoadFullMassKg uses the authored WagonEmptyWeight (default -1),
    // rather than the fallback LoadEmptyMassKg, when a full weight is present.
    let full = payload.map_or(mass_kg, |weight| {
        f64::from(authored_empty.unwrap_or(-1.) as f32 + weight as f32)
    });
    let empty = f64::from(empty as f32);
    let full = f64::from(full as f32);
    (empty.is_finite() && full.is_finite() && empty > 0. && full >= empty).then_some(
        NativeInventoryMass {
            empty_kg: empty,
            full_kg: full,
        },
    )
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
    let length_at = |scope: &Ast, key: &str| {
        let block = super::named_blocks(scope, key).into_iter().next()?;
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
    let length = |key: &str| length_at(ast, key);
    // MSTSWagon.Parse has separate wagon(wheelradius and engine(wheelradius
    // cases. The first radius in an ENG is commonly the wagon wheel, not the
    // driving wheel used by adhesion/inertia. A missing engine radius retains
    // TrainCar's native 30-inch driver default, not the wagon value.
    let wheel_radius_m = if engine {
        super::named_blocks(ast, "Engine")
            .into_iter()
            .rev()
            .find_map(|scope| length_at(scope, "WheelRadius"))
            .unwrap_or(30. * 0.0254)
    } else {
        super::named_blocks(ast, "Wagon")
            .into_iter()
            .find_map(|scope| length_at(scope, "WheelRadius"))
            .or_else(|| length("WheelRadius"))
            .unwrap_or(18. * 0.0254)
    };
    let mut adhesion = [7.5, 44., 0.161, 0.7];
    if let Some(Ast::List(values)) = super::named_blocks(ast, "ORTSCurtius_Kniffler").first() {
        for (slot, value) in adhesion.iter_mut().zip(values.iter().filter_map(|v| {
            let Ast::Atom(atom) = v else { return None };
            super::atom_to_number(atom)
        })) {
            if value.is_finite() && value > 0. {
                *slot = value;
            }
        }
    }
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
    let steam_engine = engine
        && super::named_blocks(ast, "Engine").iter().any(|scope| {
            scalar(scope, "Type").is_some_and(|value| value.eq_ignore_ascii_case("Steam"))
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
    let friction = super::parse_orts_friction_fields(ast, engine, "");
    let authored_davis = ["ORTSDavis_A", "ORTSDavis_B", "ORTSDavis_C"]
        .iter()
        .all(|key| number(key).is_some_and(|value| value > 0.));
    let legacy_friction = friction.legacy_friction_speed_mps.is_some() && !authored_davis;
    NativeVehiclePhysics {
        max_current_a: scalar(ast, "MaxCurrent")
            .and_then(|s| s.trim_end_matches(['A', 'a']).trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.),
        roller_bearing: scalar(ast, "ORTSBearingType")
            .is_some_and(|v| v.eq_ignore_ascii_case("Roller")),
        legacy_friction,
        legacy_start_force_n: if legacy_friction {
            friction.legacy_start_force_n(mass_kg)
        } else {
            None
        },
        inventory_mass: inventory_mass(ast, mass_kg),
        body_cross_section_m2: f64::from(
            friction.car_width_m.unwrap_or(2.5) as f32 * friction.car_height_m.unwrap_or(4.) as f32,
        ),
        wheel_radius_m,
        flipped: false,
        curtius_kniffler: [adhesion[0], adhesion[1], adhesion[2]],
        adhesion_k: adhesion[3],
        slip_warning_percent: number("ORTSSlipWarningThreshold")
            .filter(|v| *v > 0.)
            .unwrap_or(70.),
        axle_damping_ns: mass_kg / 1000.,
        axle_friction_n: mass_kg / 1000.,
        axle_load_kg: if engine {
            drive_mass / drive_axles.max(1.)
        } else {
            mass_kg / number("ORTSNumberAxles").filter(|v| *v > 0.).unwrap_or(4.)
        },
        drive_axles,
        pitch_span_m: length("ORTSLengthBogieCentre"),
        coupler_face_length_m: length("ORTSLengthCouplerFace"),
        curve_gauge_m: super::parse_vehicle_curve_parameters(ast, engine)
            .track_gauge_m
            .unwrap_or(1.435),
        curve_rigid_wheelbase_m: length("ORTSRigidWheelBase").unwrap_or(if steam_engine {
            // TrainCar.Update's steam approximation. The legacy engine
            // NumWheels is a wheel count in the supported six-wheel tank;
            // OR resolves it to three driving axles before this equation.
            let axles = drive_axles as f32 / 2.;
            f64::from(1.25_f32 * (axles - 1.).max(0.) * (wheel_radius_m as f32 * 2.))
        } else if engine && drive_axles >= 6. {
            3.5052
        } else {
            1.6764
        }),
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
    fn native_inventory_requires_continuous_animation_but_not_a_commercial_intake() {
        for text in [
            "Wagon (w Mass(25t)) Engine(e Type(Diesel))",
            "Wagon (w Mass(25t) ORTSFreightAnims(WagonEmptyWeight(20t))) Engine(e Type(Diesel))",
        ] {
            let ast = crate::parse_named_stf(text).unwrap();
            assert!(
                parse_native_vehicle_physics(&ast, true, 25000.)
                    .inventory_mass
                    .is_none()
            );
        }
        let ast = crate::parse_named_stf(
            "Wagon(w Mass(25t) ORTSFreightAnims(WagonEmptyWeight(20t) FreightAnimContinuous(FreightWeightWhenFull(6t)))) Engine(e Type(Diesel))",
        ).unwrap();
        let p = parse_native_vehicle_physics(&ast, true, 25000.);
        assert_eq!(
            p.inventory_mass,
            Some(NativeInventoryMass {
                empty_kg: 20000.,
                full_kg: 26000.
            })
        );
    }

    #[test]
    fn native_tunnel_area_uses_single_size_dimensions_and_native_defaults() {
        let ast = crate::parse_named_stf("Wagon(w ORTSWagonFrontalArea(99m^2))").unwrap();
        assert_eq!(
            parse_native_vehicle_physics(&ast, false, 20000.).body_cross_section_m2,
            10.
        );
        let ast = crate::parse_named_stf(
            "Wagon(w Size(2.688m 3.915m 9.519m) ORTSWagonFrontalArea(99m^2))",
        )
        .unwrap();
        assert_eq!(
            parse_native_vehicle_physics(&ast, false, 20000.).body_cross_section_m2,
            f64::from(2.688_f32 * 3.915_f32)
        );
    }

    #[test]
    fn engine_and_wagon_wheel_radii_have_independent_native_scopes() {
        for text in [
            "Wagon (w WheelRadius(54.6in)) Engine (e WheelRadius(27.3in) NumWheels(1))",
            "((Wagon w (WheelRadius 54.6in)) (Engine e (WheelRadius 27.3in) (NumWheels 1)))",
        ] {
            let ast = crate::parse_named_stf(text).unwrap();
            let engine = parse_native_vehicle_physics(&ast, true, 25000.);
            let wagon = parse_native_vehicle_physics(&ast, false, 25000.);
            assert!((engine.wheel_radius_m - 0.69342).abs() < 1e-10);
            assert!((wagon.wheel_radius_m - 1.38684).abs() < 1e-10);
        }
        let ast = crate::parse_named_stf("Wagon(w WheelRadius(54.6in)) Engine(e MaxPower(1000kW))")
            .unwrap();
        assert!(
            (parse_native_vehicle_physics(&ast, true, 25000.).wheel_radius_m - 0.762).abs() < 1e-10
        );
    }
    #[test]
    fn native_adhesion_preserves_declared_coefficients_and_warning() {
        let ast = crate::parse_vehicle_text("Wagon(w ORTSAdhesion(ORTSCurtius_Kniffler(6.18 44 0.161 0.8) ORTSSlipWarningThreshold(65))) Engine(e)").unwrap();
        let p = parse_native_vehicle_physics(&ast, true, 25000.);
        assert_eq!(p.curtius_kniffler, [6.18, 44., 0.161]);
        assert_eq!(p.adhesion_k, 0.8);
        assert_eq!(p.slip_warning_percent, 65.);
        assert_eq!(p.axle_damping_ns, 25.);
        assert_eq!(p.axle_friction_n, 25.);
    }
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
        // No Engine.WheelRadius was authored: TrainCar retains its 30-inch
        // driver default independently of the Wagon's 42in/2 radius.
        assert!((params.wheel_radius_m - 0.762).abs() < 1e-10);
        let scoped = crate::parse_named_stf(
            "Wagon ( tail WheelRadius ( 42in/2 ) ) Engine ( tail NumWheels ( 1 ) )",
        )
        .unwrap();
        assert!(
            (parse_native_vehicle_physics(&scoped, false, 67000.).wheel_radius_m - 0.5334).abs()
                < 1e-10
        );
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

#[cfg(test)]
mod legacy_steam_metadata_tests {
    #[test]
    fn simple_tank_retains_its_fcalc_start_force_and_native_rigid_base() {
        let ast = crate::parse_named_stf(include_str!(
            "../../../../examples/physics_lab/vehicles/native_steam.eng"
        ))
        .unwrap();
        let p = super::parse_native_vehicle_physics(&ast, true, 50_000.);
        assert!(p.legacy_friction);
        assert_eq!(p.legacy_start_force_n, Some(707.));
        assert_eq!(p.curve_rigid_wheelbase_m, f64::from(3.495_f32));
    }
}
