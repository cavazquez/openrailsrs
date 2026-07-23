//! MSTS / Open Rails compatibility layer for openrailsrs.
//!
//! Provides importers that convert MSTS file formats to the openrailsrs
//! native TOML representations:
//!
//! - [`import_route`] — `.tdb` → `track.toml`
//! - [`import_activity`] — `.act` + `.pat` → `scenario.toml`

pub mod error;
pub mod import_activity;
pub mod import_route;
pub mod path_placement;

pub use error::MstsError;
pub use import_activity::{
    import_activity, import_activity_with_summary, import_activity_with_track,
};
pub use import_route::{
    import_route, import_route_with_activity, import_route_with_summary, patch_track_coordinates,
};
pub use path_placement::{
    consist_length_from_vehicle_lengths, head_offset_from_rear_snap, pat_edge_path,
    pat_edge_path_with_offset, pat_outbound_waypoints, pat_waypoints, pat_waypoints_from_world,
    pat_waypoints_with_offset, placement_for_pat, placement_for_pat_with_consist,
    placement_from_imported_route, placement_from_imported_route_with_consist,
    read_distance_down_path,
};
