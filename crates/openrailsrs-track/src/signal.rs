use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalAspect {
    Clear,
    Caution,
    Stop,
}

/// Declarative script that drives a signal's aspect based on block occupancy.
///
/// Rules are evaluated in priority order:
/// 1. If the block immediately ahead (`edge_id`'s destination edge) is occupied →
///    use `on_block_ahead` (if set).
/// 2. If the block two steps ahead is occupied → use `on_second_block_ahead` (if set).
/// 3. Otherwise → use `default` (if set); signal stays at its current aspect if unset.
///
/// This is intentionally simple — a superset can be added later (e.g. speed-based
/// aspects, timed rules, etc.) without breaking the existing format.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SignalScript {
    /// Aspect to show when the immediately-ahead block is occupied by any train.
    #[serde(default)]
    pub on_block_ahead: Option<SignalAspect>,
    /// Aspect to show when the second block ahead is occupied (but the first is clear).
    #[serde(default)]
    pub on_second_block_ahead: Option<SignalAspect>,
    /// Aspect to show when all lookahead blocks are clear.
    #[serde(default)]
    pub default: Option<SignalAspect>,
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
    /// Optional reactive script; when present the aspect is updated each evaluation cycle.
    #[serde(default)]
    pub script: Option<SignalScript>,
}
