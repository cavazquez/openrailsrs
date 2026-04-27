use std::path::Path;

use openrailsrs_formats::parse_from_first_paren;
use openrailsrs_formats::{Ast, Atom};

use crate::error::TrainError;
use crate::model::{Consist, Locomotive, Vehicle, Wagon};

pub fn load_engine_from_path(path: impl AsRef<Path>) -> Result<Locomotive, TrainError> {
    let text = std::fs::read_to_string(path.as_ref())?;
    let ast = parse_from_first_paren(&text)?;
    engine_from_ast(&ast, path.as_ref().display().to_string())
}

pub fn load_wagon_from_path(path: impl AsRef<Path>) -> Result<Wagon, TrainError> {
    let text = std::fs::read_to_string(path.as_ref())?;
    let ast = parse_from_first_paren(&text)?;
    wagon_from_ast(&ast, path.as_ref().display().to_string())
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

fn atom_string(a: &Atom) -> Option<String> {
    match a {
        Atom::String(s) => Some(s.clone()),
        Atom::Symbol(s) => Some(s.clone()),
        _ => None,
    }
}

fn atom_number(a: &Atom) -> Option<f64> {
    match a {
        Atom::Number(n) => Some(*n),
        Atom::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

fn find_numeric_field(root: &Ast, names: &[&str]) -> Option<f64> {
    walk_lists_find(root, &mut |items| {
        if items.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &items[0] {
                if names.iter().any(|n| n.eq_ignore_ascii_case(head)) {
                    if let Ast::Atom(a) = &items[1] {
                        return atom_number(a);
                    }
                }
            }
        }
        None
    })
}

fn find_string_field(root: &Ast, names: &[&str]) -> Option<String> {
    walk_lists_find(root, &mut |items| {
        if items.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(head)) = &items[0] {
                if names.iter().any(|n| n.eq_ignore_ascii_case(head)) {
                    if let Ast::Atom(a) = &items[1] {
                        return atom_string(a);
                    }
                }
            }
        }
        None
    })
}

fn walk_lists_find<T, F>(ast: &Ast, f: &mut F) -> Option<T>
where
    F: FnMut(&[Ast]) -> Option<T>,
{
    match ast {
        Ast::List(items) => {
            if let Some(v) = f(items) {
                return Some(v);
            }
            for sub in items {
                if let Some(v) = walk_lists_find(sub, f) {
                    return Some(v);
                }
            }
            None
        }
        _ => None,
    }
}

fn engine_from_ast(ast: &Ast, ctx: String) -> Result<Locomotive, TrainError> {
    let mass =
        find_numeric_field(ast, &["Mass", "MassKG"]).ok_or_else(|| TrainError::MissingField {
            field: "Mass".into(),
            context: ctx.clone(),
        })?;
    let max_power = find_numeric_field(ast, &["MaxPower", "MaxForce"]).unwrap_or(0.0);
    let max_vel = find_numeric_field(ast, &["MaxVelocity", "MaxSpeed"]).unwrap_or(55.0);
    let name = walk_lists_find(ast, &mut |items| {
        if items.len() >= 2 {
            if let Ast::Atom(Atom::Symbol(h)) = &items[0] {
                if h.eq_ignore_ascii_case("Name") {
                    if let Ast::Atom(a) = &items[1] {
                        return atom_string(a);
                    }
                }
            }
        }
        None
    })
    .unwrap_or_else(|| "engine".into());
    Ok(Locomotive {
        name,
        mass_kg: mass,
        max_power_w: max_power,
        max_velocity_mps: max_vel / 3.6,
        max_tractive_effort_n: find_numeric_field(ast, &["MaxTractiveEffort"]).unwrap_or(350_000.0),
        max_brake_force_n: find_numeric_field(ast, &["MaxBrakeForce", "Brake"])
            .unwrap_or(200_000.0),
    })
}

fn wagon_from_ast(ast: &Ast, ctx: String) -> Result<Wagon, TrainError> {
    let mass =
        find_numeric_field(ast, &["Mass", "MassKG"]).ok_or_else(|| TrainError::MissingField {
            field: "Mass".into(),
            context: ctx.clone(),
        })?;
    let name = find_string_field(ast, &["Type", "Name"]).unwrap_or_else(|| "wagon".into());
    Ok(Wagon {
        name,
        mass_kg: mass,
        max_brake_force_n: find_numeric_field(ast, &["MaxBrakeForce", "Brake"]).unwrap_or(80_000.0),
    })
}

fn consist_from_ast(ast: &Ast, base: &Path) -> Result<Consist, TrainError> {
    let mut vehicles = Vec::new();
    collect_vehicles(ast, base, &mut vehicles)?;
    if vehicles.is_empty() {
        return Err(TrainError::Parse(
            "consist contains no Engine/Wagon entries".into(),
        ));
    }
    Ok(Consist { vehicles })
}

fn collect_vehicles(ast: &Ast, base: &Path, out: &mut Vec<Vehicle>) -> Result<(), TrainError> {
    if let Ast::List(items) = ast {
        if let Some(Ast::Atom(Atom::Symbol(head))) = items.first() {
            if head.eq_ignore_ascii_case("Engine") {
                if let Some(Ast::Atom(path_a)) = items.get(1) {
                    let rel = atom_string(path_a).ok_or_else(|| {
                        TrainError::Parse("Engine path must be string or symbol".into())
                    })?;
                    let p = resolve_path(base, &rel);
                    out.push(Vehicle::Loco(load_engine_from_path(&p)?));
                    return Ok(());
                }
            }
            if head.eq_ignore_ascii_case("Wagon") {
                if let Some(Ast::Atom(path_a)) = items.get(1) {
                    let rel = atom_string(path_a).ok_or_else(|| {
                        TrainError::Parse("Wagon path must be string or symbol".into())
                    })?;
                    let p = resolve_path(base, &rel);
                    out.push(Vehicle::Wagon(load_wagon_from_path(&p)?));
                    return Ok(());
                }
            }
        }
        for sub in items {
            collect_vehicles(sub, base, out)?;
        }
    }
    Ok(())
}

fn resolve_path(base: &Path, rel: &str) -> std::path::PathBuf {
    let trimmed = rel.trim().replace('\\', "/");
    base.join(trimmed)
}
