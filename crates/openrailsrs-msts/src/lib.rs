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

pub use error::MstsError;
pub use import_activity::{import_activity, import_activity_with_summary};
pub use import_route::{import_route, import_route_with_activity, import_route_with_summary};
