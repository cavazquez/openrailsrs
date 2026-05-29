//! Tokenizer and generic S-expression AST for MSTS-style files.

pub mod ast;
pub mod dispatch;
pub mod encoding;
pub mod error;
pub mod lexer;
pub mod msts_simisa;
pub mod msts_units;
pub mod parser;
pub mod shape_binary;
pub mod typed;
pub mod units;

pub use ast::{Ast, Atom};
pub use dispatch::{MstsFile, parse_msts_file};
pub use encoding::{
    decode_msts_bytes, read_msts_file_case_insensitive, read_msts_file_to_string,
    resolve_path_case_insensitive,
};
pub use error::FormatError;
pub use msts_units::{
    parse_force_n, parse_length_m, parse_mass_kg, parse_power_w, parse_pressure_bar,
    parse_velocity_mps,
};
pub use parser::{parse, parse_first, parse_from_first_paren};
pub use typed::{
    ActivityFile, ActivityObjectDef, BrakeShoeFrictionCurve, ConsistEntry, ConsistFile,
    DistanceLevel, ElevationGrid, EngineFile, FeatureGrid, LodControl, Matrix43, MstsSteamFields,
    NamedMatrix, OrtsBearingType, OrtsBrakeShoeType, OrtsFrictionFields, OrtsWagonType,
    PathDataPoint, PathFile, PrimState, Primitive, RestrictedZone, RouteFile, ShapeFile,
    SignalAspectKind, SoundRegionOverride, SubObject, TerrainFile, TerrainMeshData, TerrainPatch,
    TerrainPatchSet, TerrainSamples, TerrainShader, TerrainTexSlot, TerrainUvCalc, TrItem,
    TrItemKind, TrPinRef, TrackDbFile, TrackDbNode, TrackNodeKind, TrafficServiceDef, Vec2, Vec3,
    Vertex, WagonFile, WorldFile, WorldItem, build_patch_mesh_data, build_patch_mesh_data_ex,
    build_tile_mesh_data, parse_orts_brake_shoe, parse_orts_friction_fields, patch_affine_uv,
    read_f_raw, read_y_raw, resolve_brake_shoe_curve,
};
pub use units::{kmh_to_mps, kn_to_n, kw_to_w, lb_to_kg, mph_to_mps};
