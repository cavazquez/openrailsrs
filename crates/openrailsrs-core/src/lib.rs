//! Core types shared across openrailsrs crates.

pub mod ids;
pub mod time;

pub use ids::{EdgeId, NodeId};
pub use time::SimTime;
