use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SignalAspect {
    Clear,
    Caution,
    Stop,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackSignal {
    pub edge_id: String,
    pub position_m: f64,
    pub aspect: SignalAspect,
}
