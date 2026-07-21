//! Tokenizer and generic S-expression AST for MSTS-style files.

pub mod ast;
pub mod cab_link;
pub mod dispatch;
pub mod encoding;
pub mod error;
pub mod lexer;
pub mod msts_file_text;
pub mod msts_simisa;
pub mod msts_tile_name;
pub mod msts_units;
pub mod parser;
pub mod shape_binary;
pub mod shape_binary_direct;
pub mod shape_binary_reader;
pub mod tile_paths;
pub mod typed;
pub mod units;
pub mod vehicle_audit;
pub mod vehicle_field_catalog;

pub use ast::{Ast, Atom};
pub use cab_link::{
    ResolvedCabAssets, find_cabview_dir, pick_cab_shape_in_dir, resolve_cab_assets,
    resolve_cab_assets_scan,
};
pub use dispatch::{MstsFile, parse_msts_file};
pub use encoding::{
    decode_msts_bytes, msts_filename_is_relative_path, msts_latin_bytes, normalize_msts_filename,
    read_msts_file_case_insensitive, read_msts_file_to_string, resolve_path_case_insensitive,
    resolve_route_relative_file, utf16le_msts_to_latin_bytes,
};
pub use error::FormatError;
pub use msts_tile_name::{
    MSTS_TILE_ZOOM_SMALL, msts_tile_name_from_xz, msts_tile_world_origin,
    msts_tile_x_index_for_coord, msts_tile_z_index_for_coord, parse_world_w_tile_xz,
    world_w_filename_from_tile_xz,
};
pub use msts_units::{
    parse_force_n, parse_length_m, parse_mass_kg, parse_power_w, parse_pressure_bar,
    parse_velocity_mps,
};
pub use parser::{
    parse, parse_all_top_level, parse_all_top_level_lenient, parse_first,
    parse_first_from_first_paren, parse_from_first_paren,
};
pub use shape_binary_direct::{
    binary_shape_to_ast, is_binary_shape_payload, shape_from_binary_payload,
};
pub use shape_binary_reader::{BinaryBlockReader, apply_token_offset};
pub use tile_paths::{
    build_terrain_tile_catalog, build_world_tile_catalog, find_named_subdir,
    resolve_hash_terrain_tile_file, resolve_world_tile_file, scan_hash_terrain_tiles_from_world,
    scan_world_tile_files, terrain_subdirs, tiles_subdirs, world_subdirs,
};
pub use typed::{
    ActivityFile, ActivityObjectDef, AnimController, AnimNode, Animation, BrakeShoeFrictionCurve,
    CabControl, CabView, CabViewFile, CarSpawnerCatalog, CarSpawnerItem, CarSpawnerList,
    ConsistEntry, ConsistFile, ControlState, ControlType, DistanceLevel, ElevationGrid,
    EngineCabView, EngineFile, FeatureGrid, HazardFile, IndexedTrVectorSection, LodControl,
    Matrix43, MstsSteamFields, NamedMatrix, OrtsBearingType, OrtsBrakeShoeType, OrtsFrictionFields,
    OrtsWagonType, OverheadWireParams, PathDataPoint, PathFile, PrimState, Primitive,
    RestrictedZone, RouteFile, RouteStart, SKEW_AS_CURVE_RADIUS_M, ScreenRect, ShapeFile,
    SigCfgFile, SignalAspectKind,
    SignalDrawStateDef, SignalLightDef, SignalShapeDef, SignalShapeSubObjDef, SignalTypeDef,
    SignalUnitRef, SoundRegionOverride, SubObject, TSectionCatalog, TerrainFile, TerrainMeshData,
    TerrainPatch, TerrainPatchSet, TerrainSamples, TerrainShader, TerrainTexSlot, TerrainUvCalc,
    TrItem, TrItemHost, TrItemKind, TrItemWorldPose, TrPinRef, TrVectorSectionRecord, TrackDbFile,
    TrackDbNode, TrackNodeKind, TrackProceduralDims, TrackProceduralLink, TrackVectorGeometry,
    TrackVectorPoint, TrafficServiceDef, Vec2, Vec3, Vertex, VtxState, WagonFile, WorldFile,
    WorldItem, build_patch_mesh_data, build_patch_mesh_data_ex, build_patch_mesh_data_sampled,
    build_tile_mesh_data, build_tile_mesh_data_sampled, find_trk_path, lit_light_indices_for_aspect,
    parse_orts_brake_shoe, parse_orts_friction_fields, parse_tile_xz_from_filename, patch_affine_uv,
    read_f_raw, read_y_raw, resolve_brake_shoe_curve, resolve_hazard_shape_name,
    terrain_patches_per_side,
};
pub use units::{kmh_to_mps, kn_to_n, kw_to_w, lb_to_kg, mph_to_mps};
pub use vehicle_audit::{
    FieldAuditEntry, VehicleAuditReport, audit_vehicle_file, collect_msts_list_head_symbols,
};
pub use vehicle_field_catalog::{
    ParserSupport, VEHICLE_FIELD_CATALOG, VehicleFieldSpec, VehicleKind, catalog_for_kind,
    lookup_field,
};
