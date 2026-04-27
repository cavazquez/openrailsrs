use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SignalAspect {
    Clear,
    Caution,
    Stop,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackSignal {
    /// Unique identifier for this signal (used by the runner's state map).
    pub id: String,
    pub edge_id: String,
    /// Distance from the start of the edge where the signal is placed.
    pub position_m: f64,
    /// Initial aspect loaded from the route definition.
    pub aspect: SignalAspect,
    /// If `Some(t)`, a `Stop` signal automatically clears at simulation time `t` (seconds).
    /// Allows single-train scenarios to model temporary blocks without an external controller.
    #[serde(default)]
    pub clear_after_s: Option<f64>,
}
