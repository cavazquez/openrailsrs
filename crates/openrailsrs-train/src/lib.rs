//! Train/consist model built from MSTS-style `.eng`, `.wag`, `.con` AST.

pub mod diesel;
pub mod error;
pub mod from_ast;
pub mod model;
pub mod steam_loader;

pub use diesel::DieselTractionModel;
pub use error::TrainError;
pub use from_ast::{
    consist_asset_root, load_consist_from_path, load_consist_with_asset_root,
    load_engine_from_path, load_wagon_from_path,
};
pub use model::{
    Consist, DavisCoefficients, Locomotive, SteamParams, TractiveCurve, Vehicle, Wagon,
};
pub use steam_loader::{load_steam_engine_from_toml, parse_steam_engine_toml};
