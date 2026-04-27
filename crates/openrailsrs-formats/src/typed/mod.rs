mod activity;
mod consist;
mod engine;
mod path;
mod route;
mod shape;
mod track_db;
mod wagon;
mod world;

pub use activity::{
    ActivityFile, ActivityObjectDef, RestrictedZone, SoundRegionOverride, TrafficServiceDef,
};
pub use consist::{ConsistEntry, ConsistFile};
pub use engine::EngineFile;
pub use path::{PathDataPoint, PathFile};
pub use route::RouteFile;
pub use shape::{
    DistanceLevel, LodControl, Matrix43, NamedMatrix, PrimState, Primitive, ShapeFile, SubObject,
    Vec2, Vec3,
};
pub use track_db::{SignalAspectKind, TrItem, TrItemKind, TrackDbFile, TrackDbNode, TrackNodeKind};
pub use wagon::WagonFile;
pub use world::{WorldFile, WorldItem};

use crate::ast::{Ast, Atom};
use crate::error::FormatError;

fn atom_to_string(atom: &Atom) -> Option<String> {
    match atom {
        Atom::String(value) => Some(value.clone()),
        Atom::Symbol(value) => Some(value.clone()),
        _ => None,
    }
}

fn atom_to_number(atom: &Atom) -> Option<f64> {
    match atom {
        Atom::Number(value) => Some(*value),
        Atom::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

fn find_list_value<'a>(root: &'a Ast, key: &str) -> Option<&'a Ast> {
    walk_lists_find(root, &mut |items| {
        if items.len() >= 2
            && matches!(&items[0], Ast::Atom(Atom::Symbol(head)) if head.eq_ignore_ascii_case(key))
        {
            return Some(&items[1]);
        }
        None
    })
}

fn find_numeric_field(root: &Ast, keys: &[&str], context: &str) -> Result<f64, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return match value {
                Ast::Atom(atom) => {
                    atom_to_number(atom).ok_or_else(|| FormatError::UnexpectedAtom {
                        key: (*key).to_string(),
                        context: context.to_string(),
                        expected: "numeric atom".to_string(),
                    })
                }
                _ => Err(FormatError::UnexpectedAtom {
                    key: (*key).to_string(),
                    context: context.to_string(),
                    expected: "numeric atom".to_string(),
                }),
            };
        }
    }
    Err(FormatError::MissingField {
        key: keys.join("|"),
        context: context.to_string(),
    })
}

fn find_optional_numeric_field(
    root: &Ast,
    keys: &[&str],
    context: &str,
) -> Result<Option<f64>, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return match value {
                Ast::Atom(atom) => {
                    atom_to_number(atom)
                        .map(Some)
                        .ok_or_else(|| FormatError::UnexpectedAtom {
                            key: (*key).to_string(),
                            context: context.to_string(),
                            expected: "numeric atom".to_string(),
                        })
                }
                _ => Err(FormatError::UnexpectedAtom {
                    key: (*key).to_string(),
                    context: context.to_string(),
                    expected: "numeric atom".to_string(),
                }),
            };
        }
    }
    Ok(None)
}

fn find_optional_string_field(
    root: &Ast,
    keys: &[&str],
    context: &str,
) -> Result<Option<String>, FormatError> {
    for key in keys {
        if let Some(value) = find_list_value(root, key) {
            return match value {
                Ast::Atom(atom) => {
                    atom_to_string(atom)
                        .map(Some)
                        .ok_or_else(|| FormatError::UnexpectedAtom {
                            key: (*key).to_string(),
                            context: context.to_string(),
                            expected: "string or symbol atom".to_string(),
                        })
                }
                _ => Err(FormatError::UnexpectedAtom {
                    key: (*key).to_string(),
                    context: context.to_string(),
                    expected: "string or symbol atom".to_string(),
                }),
            };
        }
    }
    Ok(None)
}

fn walk_lists_find<'a, T, F>(ast: &'a Ast, f: &mut F) -> Option<T>
where
    F: FnMut(&'a [Ast]) -> Option<T>,
{
    match ast {
        Ast::List(items) => {
            if let Some(value) = f(items) {
                return Some(value);
            }
            for item in items {
                if let Some(value) = walk_lists_find(item, f) {
                    return Some(value);
                }
            }
            None
        }
        Ast::Atom(_) => None,
    }
}
