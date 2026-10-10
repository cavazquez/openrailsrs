//! Core types shared across openrailsrs crates.

pub mod electrification;
pub mod ids;
pub mod power_supply;
pub mod time;

pub use ids::{EdgeId, NodeId};
pub use time::SimTime;
