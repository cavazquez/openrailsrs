//! Logical track graph (headless).

pub mod error;
pub mod graph;
pub mod signal;
pub mod sigscript;

pub use error::TrackError;
pub use graph::{
    Edge, EdgePhysicsProfile, Node, NodeKind, PositionGrade, PositionSpeedLimit, SwitchPosition,
    TrackGraph,
};
pub use signal::{SignalAspect, SignalScript, TrackSignal};
