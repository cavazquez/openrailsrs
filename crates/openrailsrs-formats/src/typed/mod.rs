mod activity;
mod brake_shoe;
mod carspawn;
mod consist;
mod cvf;
mod engine;
mod friction;
mod hazard;
mod path;
mod route;
mod shape;
mod sigcfg;
mod terrain;
mod track_db;
mod tsection;
mod wagon;
mod world;

pub use activity::{
    ActivityFile, ActivityObjectDef, RestrictedZone, SoundRegionOverride, TrafficServiceDef,
};
pub use brake_shoe::{
    BrakeShoeFrictionCurve, OrtsBrakeShoeType, parse_orts_brake_shoe, resolve_brake_shoe_curve,
};
pub use carspawn::{CarSpawnerCatalog, CarSpawnerItem, CarSpawnerList};
pub use consist::{ConsistEntry, ConsistFile};
pub use cvf::{
    CabControl, CabDialParams, CabDigitalParams, CabGaugeParams, CabLeverFrames, CabView,
    CabViewFile, ControlState, ControlType, ScreenRect,
};
pub use engine::{EngineCabView, EngineFile, MstsSteamFields, Orts3dCabViewpoint};
pub use friction::{
    OrtsBearingType, OrtsFrictionFields, OrtsWagonType, parse_orts_friction_fields,
};
pub use hazard::{HazardFile, resolve_hazard_shape_name};
pub use path::{PathDataPoint, PathFile};
pub use route::{OverheadWireParams, RouteFile, RouteStart, find_trk_path};
pub use shape::{
    AnimController, AnimNode, Animation, DistanceLevel, LightModelCfg, LodControl, Matrix43,
    MstsTexAddrMode, NamedMatrix, PrimState, Primitive, ShapeFile, ShapeTextureSlot, SubObject,
    UvOp, UvOpKind, Vec2, Vec3, Vertex, VtxState, msts_tex_addr_mode,
};
pub use sigcfg::{
    LightColour, LightTextureDef, SigCfgFile, SignalDrawStateDef, SignalLightDef, SignalShapeDef,
    SignalShapeSubObjDef, SignalTypeDef, lit_light_indices_for_aspect,
};
pub use terrain::{
    ElevationGrid, FeatureGrid, TerrainFile, TerrainMeshData, TerrainPatch, TerrainPatchSet,
    TerrainSamples, TerrainShader, TerrainTexSlot, TerrainUvCalc, build_patch_mesh_data,
    build_patch_mesh_data_ex, build_patch_mesh_data_sampled, build_tile_mesh_data,
    build_tile_mesh_data_sampled, parse_tile_xz_from_filename, patch_affine_uv, read_f_raw,
    read_f_raw_bytes, read_y_raw, read_y_raw_bytes, terrain_patches_per_side,
};
pub use track_db::{
    IndexedTrVectorSection, SignalAspectKind, TrItem, TrItemHost, TrItemKind, TrItemWorldPose,
    TrPinRef, TrVectorSectionRecord, TrackDbFile, TrackDbNode, TrackNodeKind, TrackVectorGeometry,
    TrackVectorPoint,
};
pub use tsection::{
    SKEW_AS_CURVE_RADIUS_M, TSectionCatalog, TrackProceduralDims, TrackProceduralLink,
    TrackSectionDef, TrackShapeDef, TrackShapePath,
};
pub use wagon::{PassengerViewpoint, WagonFile, parse_passenger_viewpoints};
pub use world::{DyntrackSection, SignalUnitRef, WorldFile, WorldItem, WorldTrItemRef};

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

/// Visit every list node (does not short-circuit). Used when collecting repeated blocks
/// such as multiple `ORTS3DCab` / `HeadOut` entries.
fn walk_lists_visit<'a, F>(ast: &'a Ast, f: &mut F)
where
    F: FnMut(&'a [Ast]),
{
    match ast {
        Ast::List(items) => {
            f(items);
            for item in items {
                walk_lists_visit(item, f);
            }
        }
        Ast::Atom(_) => {}
    }
}
