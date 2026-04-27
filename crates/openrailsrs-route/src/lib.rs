//! Route metadata and loading a [`TrackGraph`] from route layout TOML.

pub mod error;
pub mod load;

pub use error::RouteError;
pub use load::{RouteLayoutFile, load_track_graph_from_route_dir};
