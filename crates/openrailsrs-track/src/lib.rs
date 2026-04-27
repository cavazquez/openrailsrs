//! Logical track graph (headless).

pub mod error;
pub mod graph;
pub mod signal;

pub use error::TrackError;
pub use graph::{Edge, Node, NodeKind, SwitchPosition, TrackGraph};
pub use signal::{SignalAspect, SignalScript, TrackSignal};
