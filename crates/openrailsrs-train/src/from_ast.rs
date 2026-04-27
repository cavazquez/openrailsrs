use std::path::Path;

use openrailsrs_formats::parse_from_first_paren;
use openrailsrs_formats::{Ast, ConsistEntry, ConsistFile, EngineFile, WagonFile};

use crate::error::TrainError;
use crate::model::{Consist, DavisCoefficients, Locomotive, Vehicle, Wagon};

pub fn load_engine_from_path(path: impl AsRef<Path>) -> Result<Locomotive, TrainError> {
    let text = std::fs::read_to_string(path.as_ref())?;
    let ast = parse_from_first_paren(&text)?;
    let engine = EngineFile::from_ast(&ast)?;
    Ok(engine.into())
}

pub fn load_wagon_from_path(path: impl AsRef<Path>) -> Result<Wagon, TrainError> {
    let text = std::fs::read_to_string(path.as_ref())?;
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
    let text = std::fs::read_to_string(consist.as_ref())?;
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
        Self {
            name: value.name,
            mass_kg: value.mass_kg,
            max_power_w: value.max_power_w,
            max_velocity_mps: value.max_velocity_mps,
            max_tractive_effort_n: value.max_tractive_effort_n,
            max_brake_force_n: value.max_brake_force_n,
            tractive_curve,
            regen_factor: value.regen_factor,
            diesel_sfc_g_per_kwh: value.diesel_sfc_g_per_kwh,
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
        }
    }
}
