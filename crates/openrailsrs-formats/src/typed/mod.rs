mod activity;
mod brake_shoe;
mod consist;
mod engine;
mod friction;
mod path;
mod route;
mod shape;
mod terrain;
mod track_db;
mod wagon;
mod world;

pub use activity::{
    ActivityFile, ActivityObjectDef, RestrictedZone, SoundRegionOverride, TrafficServiceDef,
};
pub use brake_shoe::{
    BrakeShoeFrictionCurve, OrtsBrakeShoeType, parse_orts_brake_shoe, resolve_brake_shoe_curve,
};
pub use consist::{ConsistEntry, ConsistFile};
pub use engine::{EngineFile, MstsSteamFields};
pub use friction::{
    OrtsBearingType, OrtsFrictionFields, OrtsWagonType, parse_orts_friction_fields,
};
pub use path::{PathDataPoint, PathFile};
pub use route::RouteFile;
pub use shape::{
    DistanceLevel, LodControl, Matrix43, NamedMatrix, PrimState, Primitive, ShapeFile, SubObject,
    Vec2, Vec3, Vertex,
};
pub use terrain::{
    ElevationGrid, FeatureGrid, TerrainFile, TerrainMeshData, TerrainPatch, TerrainPatchSet,
    TerrainSamples, TerrainShader, TerrainTexSlot, TerrainUvCalc, build_patch_mesh_data,
    build_patch_mesh_data_ex, build_tile_mesh_data, parse_tile_xz_from_filename, patch_affine_uv,
    read_f_raw, read_y_raw,
};
pub use track_db::{
    SignalAspectKind, TrItem, TrItemKind, TrPinRef, TrackDbFile, TrackDbNode, TrackNodeKind,
    TrackVectorGeometry, TrackVectorPoint,
};
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
