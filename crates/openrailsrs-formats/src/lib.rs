//! Tokenizer and generic S-expression AST for MSTS-style files.

pub mod ast;
pub mod dispatch;
pub mod encoding;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod typed;
pub mod units;

pub use ast::{Ast, Atom};
pub use dispatch::{MstsFile, parse_msts_file};
pub use encoding::{decode_msts_bytes, read_msts_file_to_string};
pub use error::FormatError;
pub use parser::{parse, parse_first, parse_from_first_paren};
pub use typed::{
    ActivityFile, ActivityObjectDef, ConsistEntry, ConsistFile, DistanceLevel, EngineFile,
    LodControl, Matrix43, NamedMatrix, PathDataPoint, PathFile, PrimState, Primitive,
    RestrictedZone, RouteFile, ShapeFile, SignalAspectKind, SoundRegionOverride, SubObject,
    TerrainFile, TerrainMeshData, TerrainSamples, TrItem, TrItemKind, TrackDbFile, TrackDbNode,
    TrackNodeKind, TrafficServiceDef, Vec2, Vec3, WagonFile, WorldFile, WorldItem,
    build_patch_mesh_data, build_tile_mesh_data, read_y_raw,
};
pub use units::{kmh_to_mps, kn_to_n, kw_to_w, lb_to_kg, mph_to_mps};
