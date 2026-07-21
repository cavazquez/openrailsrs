//! Route metadata and loading a [`TrackGraph`] from route layout TOML.

pub mod error;
pub mod load;
pub mod path;

pub use error::RouteError;
pub use load::{
    LoadedRoute, MstsAlias, RouteLayoutFile, load_route_from_dir, load_track_graph_from_route_dir,
};
pub use path::{
    allowed_outgoing_edges, direct_edge, edge_path, edge_path_ignoring_switches,
    edge_path_via_waypoints,
};
