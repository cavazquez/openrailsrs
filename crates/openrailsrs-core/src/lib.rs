//! Core types shared across openrailsrs crates.

pub mod electrification;
pub mod ids;
pub mod power_supply;
pub mod time;
pub mod vehicle_profile;

pub use ids::{EdgeId, NodeId};
pub use time::SimTime;
pub mod activity;
