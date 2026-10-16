//! Bounded LAN sessions. Only the host advances railway physics.
pub mod authority;
pub mod protocol;
pub mod transport;
pub mod world;

pub use authority::{Authority, AuthorityCheckpoint, Member, RosterEntry};
pub use protocol::*;
pub use transport::{ClientTransport, HostTransport, TransportEvent};
pub use world::{WorldCheckpoint, apply_command, capture_world, restore_world, train_catalog};
