//! Import real-world railway topology into openrailsrs.
//!
//! ## OpenStreetMap (Overpass JSON)
//!
//! Use the Overpass query template in `examples/osm/overpass_query.txt` to
//! download railway data for a bounding box, then call [`import_osm_file`]:
//!
//! ```text
//! openrailsrs import-osm overpass_result.json \
//!   --out routes/myroute/track.toml \
//!   --route-id myroute
//! ```

pub mod error;
pub mod geo;
pub mod osm;

pub use error::ImportError;
pub use osm::{OsmImportOptions, TrackToml, build_layout, import_osm_file, import_osm_str};
