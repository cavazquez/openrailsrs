use std::path::Path;

use openrailsrs_formats::parse_from_first_paren;
use openrailsrs_formats::read_msts_file_to_string;
use openrailsrs_formats::{Ast, ConsistEntry, ConsistFile, EngineFile, MstsSteamFields, WagonFile};

use crate::diesel::DieselTractionModel;
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
            ConsistEntry::Engine { path } => {
                let p = resolve_path(base, &path);
                vehicles.push(Vehicle::Loco(load_engine_from_path(&p)?));
            }
            ConsistEntry::Wagon { path } => {
                let p = resolve_path(base, &path);
                vehicles.push(Vehicle::Wagon(load_wagon_from_path(&p)?));
            }
        }
    }
    if vehicles.is_empty() {
        return Err(TrainError::Parse(
            "consist contains no Engine/Wagon entries".into(),
        ));
    }
    Ok(Consist {
        vehicles,
        davis: DavisCoefficients::default(),
    })
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
        let tractive_curve = if value.traction_curve.is_empty() {
            None
        } else {
            Some(TractiveCurve {
                points: value.traction_curve,
            })
        };
        let diesel_traction = if value.diesel_notch_curves.is_empty() {
            if value.max_power_w > 0.0 && value.max_tractive_effort_n > 0.0 {
                Some(Box::new(DieselTractionModel::from_power_and_effort(
                    value.max_power_w,
                    value.max_tractive_effort_n,
                )))
            } else {
                None
            }
        } else {
            let mut model = DieselTractionModel::from_notch_curves(value.diesel_notch_curves);
            model.calibrate_effort_scale(value.max_tractive_effort_n);
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
                model.engine = Some(Box::new(crate::diesel::DieselEngineParams {
                    power_tab: value.diesel_power_tab,
                    throttle_rpm_tab: value.diesel_throttle_rpm_tab,
                    idle_rpm,
                    max_rpm,
                    rpm_time_constant_s: 2.0,
                }));
            }
            Some(Box::new(model))
        };
        let steam = value.steam.map(msts_steam_to_params);
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
            wagon_shape: value.wagon_shape,
        }
    }
}
