//! Train/consist model built from MSTS-style `.eng`, `.wag`, `.con` AST.

pub mod error;
pub mod from_ast;
pub mod model;

pub use error::TrainError;
pub use from_ast::{
    load_consist_from_path, load_consist_with_asset_root, load_engine_from_path,
    load_wagon_from_path,
};
pub use model::{Consist, Locomotive, Vehicle, Wagon};
