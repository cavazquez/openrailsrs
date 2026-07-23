use std::path::Path;

use openrailsrs_formats::parse_from_first_paren;
use openrailsrs_formats::read_msts_file_to_string;
use openrailsrs_formats::{Ast, ConsistEntry, ConsistFile, EngineFile, MstsSteamFields, WagonFile};

use crate::diesel::{DieselTractionModel, OR_DEFAULT_CURTIUS, TractionDynamicsParams};
use crate::error::TrainError;
use crate::model::{Consist, DavisCoefficients, Locomotive, SteamParams, Vehicle, Wagon};

pub fn load_engine_from_path(path: impl AsRef<Path>) -> Result<Locomotive, TrainError> {
    if crate::steam_loader::is_toml_eng(path.as_ref()).unwrap_or(false) {
        return crate::steam_loader::load_steam_engine_from_toml(path);
    }
    let text = read_msts_file_to_string(path.as_ref())
        .map_err(|e| TrainError::Parse(format!("read engine: {e}")))?;
    let ast = parse_from_first_paren(&text)?;
    let engine = EngineFile::from_ast(&ast)?;
    Ok(engine.into())
}

pub fn load_wagon_from_path(path: impl AsRef<Path>) -> Result<Wagon, TrainError> {
    let text = read_msts_file_to_string(path.as_ref())
        .map_err(|e| TrainError::Parse(format!("read wagon: {e}")))?;
    let ast = parse_from_first_paren(&text)?;
    let wagon = WagonFile::from_ast(&ast)?;
    Ok(wagon.into())
}

/// Load a `.con` file; engine/wagon relative paths resolve against the **consist file's parent** directory.
pub fn load_consist_from_path(path: impl AsRef<Path>) -> Result<Consist, TrainError> {
    let p = path.as_ref();
    let base = p.parent().unwrap_or_else(|| Path::new("."));
    load_consist_with_asset_root(p, base)
}

/// Load a `.con` file; `asset_root` is the directory used to resolve `Engine` / `Wagon` paths (typically the scenario folder).
pub fn load_consist_with_asset_root(
    consist: impl AsRef<Path>,
    asset_root: impl AsRef<Path>,
) -> Result<Consist, TrainError> {
    let text = read_msts_file_to_string(consist.as_ref())
        .map_err(|e| TrainError::Parse(format!("read consist: {e}")))?;
    let ast = parse_from_first_paren(&text)?;
    consist_from_ast(&ast, asset_root.as_ref())
}

fn consist_from_ast(ast: &Ast, base: &Path) -> Result<Consist, TrainError> {
    let consist_file = ConsistFile::from_ast(ast)?;
    let mut vehicles = Vec::with_capacity(consist_file.entries.len());
    for entry in consist_file.entries {
        match entry {
            ConsistEntry::Engine { path, flipped, .. } => {
                let p = resolve_path(base, &path);
                let mut loco = load_engine_from_path(&p)?;
                loco.flipped = flipped;
                vehicles.push(Vehicle::Loco(loco));
            }
            ConsistEntry::Wagon { path, flipped, .. } => {
                let p = resolve_path(base, &path);
                let mut wagon = load_wagon_from_path(&p)?;
                wagon.flipped = flipped;
                vehicles.push(Vehicle::Wagon(wagon));
            }
        }
    }
    if vehicles.is_empty() {
        return Err(TrainError::Parse(
            "consist contains no Engine/Wagon entries".into(),
        ));
    }
    let mut consist = Consist {
        vehicles,
        davis: DavisCoefficients::default(),
    };
    upgrade_trail_diesel_from_lead_orts(&mut consist);
    consist.davis = consist.aggregate_davis();
    Ok(consist)
}

/// OR-P13: trail locos with legacy MSTS diesel inherit scaled ORTS curves from the lead.
fn upgrade_trail_diesel_from_lead_orts(consist: &mut Consist) {
    let lead = consist.vehicles.iter().find_map(|v| match v {
        Vehicle::Loco(l) => l.diesel_traction.as_ref().and_then(|m| {
            if m.engine.is_some() && !m.is_empty() {
                Some((**m).clone())
            } else {
                None
            }
        }),
        _ => None,
    });
    let Some(lead) = lead else {
        return;
    };

    for v in &mut consist.vehicles {
        let Vehicle::Loco(l) = v else { continue };
        let Some(trail) = l.diesel_traction.as_mut() else {
            continue;
        };
        if trail.engine.is_some() {
            continue;
        }
        let continuous = if trail.max_continuous_force_n > 0.0 {
            trail.max_continuous_force_n
        } else {
            l.max_tractive_effort_n
        };
        if let Some(upgraded) = DieselTractionModel::from_lead_orts_scaled(
            &lead,
            l.max_power_w,
            l.max_tractive_effort_n,
            continuous,
        ) {
            **trail = upgraded;
        }
    }
}

fn resolve_path(base: &Path, rel: &str) -> std::path::PathBuf {
    let trimmed = rel.trim().replace('\\', "/");
    base.join(trimmed)
}

/// Directory used to resolve `Engine` / `Wagon` paths in a scenario layout
/// (`examples/smoke/consists/foo.con` → `examples/smoke/`).
pub fn consist_asset_root(consist_path: &Path) -> &Path {
    consist_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| consist_path.parent().unwrap_or(consist_path))
}

fn msts_steam_to_params(s: MstsSteamFields) -> SteamParams {
    SteamParams {
        cylinder_count: s.cylinder_count,
        cylinder_bore_m: s.cylinder_bore_m,
        piston_stroke_m: s.piston_stroke_m,
        driving_wheel_radius_m: s.driving_wheel_radius_m,
        working_pressure_bar: s.working_pressure_bar,
        evaporation_rate_kg_per_s: s.evaporation_rate_kg_per_s,
        coal_consumption_kg_per_s: s.coal_consumption_kg_per_s,
        initial_water_kg: s.initial_water_kg,
        initial_coal_kg: s.initial_coal_kg,
    }
}

impl From<EngineFile> for Locomotive {
    fn from(value: EngineFile) -> Self {
        use crate::model::TractiveCurve;
        let traction_dynamics = TractionDynamicsParams {
            max_force_n: value.max_tractive_effort_n,
            max_continuous_force_n: value.max_continuous_force_n,
            force_ramp_up_nps: value.traction_force_ramp_up_nps,
            force_ramp_down_nps: value.traction_force_ramp_down_nps,
            force_ramp_down_to_zero_nps: value.traction_force_ramp_down_to_zero_nps,
            power_ramp_up_wps: value.traction_power_ramp_up_wps,
            power_ramp_down_wps: value.traction_power_ramp_down_wps,
            power_ramp_down_to_zero_wps: value.traction_power_ramp_down_to_zero_wps,
            continuous_force_time_factor_s: value.continuous_force_time_factor_s,
        };
        let tractive_curve = if value.traction_curve.is_empty() {
            None
        } else {
            Some(TractiveCurve {
                points: value.traction_curve,
            })
        };
        let legacy_diesel = value.diesel_notch_curves.is_empty();
        let diesel_traction = if legacy_diesel {
            if value.max_power_w > 0.0 && value.max_tractive_effort_n > 0.0 {
                let continuous = if value.max_continuous_force_n > 0.0 {
                    value.max_continuous_force_n
                } else {
                    value.max_tractive_effort_n
                };
                let mut model = DieselTractionModel::from_power_and_effort(
                    value.max_power_w,
                    continuous,
                    value.run_up_time_s,
                );
                let curtius = if value.curtius_a > 0.0 {
                    (value.curtius_a, value.curtius_b, value.curtius_c)
                } else {
                    OR_DEFAULT_CURTIUS
                };
                model.configure_traction_limits(
                    value.mass_kg,
                    value.drive_wheel_mass_kg,
                    curtius,
                    value.motor_heating_time_s,
                );
                model.configure_traction_dynamics(traction_dynamics);
                Some(Box::new(model))
            } else {
                None
            }
        } else {
            let mut model = DieselTractionModel::from_notch_curves(value.diesel_notch_curves);
            let scale_force = if value.max_continuous_force_n > 0.0 {
                value.max_continuous_force_n
            } else {
                value.max_tractive_effort_n
            };
            model.calibrate_effort_scale(scale_force);
            // Attach engine thermodynamic model if DieselPowerTab / ThrottleRPMTab are present.
            if !value.diesel_power_tab.is_empty() && !value.diesel_throttle_rpm_tab.is_empty() {
                let idle_rpm = if value.diesel_idle_rpm > 0.0 {
                    value.diesel_idle_rpm
                } else {
                    value
                        .diesel_throttle_rpm_tab
                        .first()
                        .map(|(_, r)| *r)
                        .unwrap_or(325.0)
                };
                let max_rpm = if value.diesel_max_rpm > 0.0 {
                    value.diesel_max_rpm
                } else {
                    value
                        .diesel_throttle_rpm_tab
                        .last()
                        .map(|(_, r)| *r)
                        .unwrap_or(750.0)
                };
                let reverse_tab = if !value.diesel_reverse_throttle_rpm_tab.is_empty() {
                    value.diesel_reverse_throttle_rpm_tab.clone()
                } else {
                    crate::diesel::build_reverse_throttle_rpm_tab(&value.diesel_throttle_rpm_tab)
                };
                model.engine = Some(Box::new(crate::diesel::DieselEngineParams {
                    power_tab: value.diesel_power_tab,
                    throttle_rpm_tab: value.diesel_throttle_rpm_tab,
                    idle_rpm,
                    max_rpm,
                    rpm_time_constant_s: 2.0,
                    rate_of_change_up_rpm_pss: value.diesel_rate_of_change_up_rpm_pss,
                    rate_of_change_down_rpm_pss: value.diesel_rate_of_change_down_rpm_pss,
                    change_up_rpm_ps: value.diesel_change_up_rpm_ps,
                    change_down_rpm_ps: value.diesel_change_down_rpm_ps,
                    reverse_throttle_rpm_tab: reverse_tab,
                }));
            }
            model.max_rail_output_power_w = if value.max_rail_output_power_w > 0.0 {
                value.max_rail_output_power_w
            } else {
                value.max_power_w
            };
            model.unloading_speed_mps = value.unloading_speed_mps;
            model.tractive_force_power_limited = value.tractive_force_power_limited;
            model.max_continuous_force_n = scale_force;
            let curtius = if value.curtius_a > 0.0 {
                (value.curtius_a, value.curtius_b, value.curtius_c)
            } else {
                OR_DEFAULT_CURTIUS
            };
            model.configure_traction_limits(
                value.mass_kg,
                value.drive_wheel_mass_kg,
                curtius,
                value.motor_heating_time_s,
            );
            model.configure_traction_dynamics(traction_dynamics);
            Some(Box::new(model))
        };
        let steam = value.steam.map(msts_steam_to_params);
        let parsed_davis = DavisCoefficients {
            a_n: value.davis_a_n,
            b_n_per_mps: value.davis_b_n_per_mps,
            c_n_per_mps2: value.davis_c_n_per_mps2,
        };
        let davis = crate::davis_est::resolve_davis_coefficients(
            parsed_davis,
            value.mass_kg,
            true,
            &value.friction,
        );
        Self {
            name: value.name,
            mass_kg: value.mass_kg,
            max_power_w: value.max_power_w,
            max_velocity_mps: value.max_velocity_mps,
            max_tractive_effort_n: value.max_tractive_effort_n,
            max_brake_force_n: value.max_brake_force_n,
            tractive_curve,
            diesel_traction,
            regen_factor: value.regen_factor,
            diesel_sfc_g_per_kwh: value.diesel_sfc_g_per_kwh,
            steam,
            wagon_shape: value.wagon_shape,
            length_m: value.length_m,
            davis,
            brake_shoe_type: value.brake_shoe_type,
            brake_shoe_friction: value.brake_shoe_friction,
            flipped: false,
        }
    }
}

impl From<WagonFile> for Wagon {
    fn from(value: WagonFile) -> Self {
        Self {
            name: value.name,
            mass_kg: value.mass_kg,
            max_brake_force_n: value.max_brake_force_n,
            length_m: value.length_m,
            davis: crate::davis_est::resolve_davis_coefficients(
                DavisCoefficients {
                    a_n: value.davis_a_n,
                    b_n_per_mps: value.davis_b_n_per_mps,
                    c_n_per_mps2: value.davis_c_n_per_mps2,
                },
                value.mass_kg,
                false,
                &value.friction,
            ),
            wagon_shape: value.wagon_shape,
            brake_shoe_type: value.brake_shoe_type,
            brake_shoe_friction: value.brake_shoe_friction,
            flipped: false,
        }
    }
}
