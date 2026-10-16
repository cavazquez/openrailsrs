//! Logical track graph (headless).

pub mod error;
pub mod graph;
pub mod signal;
pub mod sigscript;
pub mod track_sound;
pub mod tunnel;

pub use error::TrackError;
pub use graph::{
    Edge, EdgePhysicsProfile, Node, NodeKind, PositionCurve, PositionGrade, PositionPhysicalCant,
    PositionPlatform, PositionSpeedLimit, SwitchPosition, TrackGraph,
};
pub use signal::{SignalAspect, SignalScript, TrackSignal};
