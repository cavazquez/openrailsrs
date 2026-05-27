//! Driver that replays throttle/brake commands from a pre-recorded CSV keyframe file.
//!
//! CSV format (header row required):
//! ```text
//! time_s,throttle,brake
//! 0.0,1.0,0.0
//! 120.0,0.0,0.5
//! ```
//!
//! Interpolation strategy: **hold-last** — the driver applies the inputs of the most recent
//! keyframe whose `time_s` ≤ current simulation time.  If the simulation time exceeds the last
//! keyframe, the last keyframe's inputs are held indefinitely.

use std::path::Path;

use crate::SimError;
use crate::runner::{Driver, DriverInput};
use crate::state::TrainSimState;

/// A single throttle/brake command at a given simulation time.
#[derive(Debug, Clone)]
pub struct Keyframe {
    pub time_s: f64,
    pub throttle: f64,
    pub brake: f64,
}

/// Replay a pre-recorded driving script using hold-last interpolation.
pub struct ScriptedDriver {
    keyframes: Vec<Keyframe>,
    /// Index of the last keyframe that was applied (used to avoid re-scanning from index 0).
    current_idx: usize,
}

impl ScriptedDriver {
    /// Build a `ScriptedDriver` from an in-memory list of keyframes.
    /// Keyframes are sorted by `time_s` automatically.
    pub fn new(mut keyframes: Vec<Keyframe>) -> Self {
        keyframes.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
        Self {
            keyframes,
            current_idx: 0,
        }
    }

    /// Load keyframes from a CSV file (columns: `time_s`, `throttle`, `brake`).
    pub fn from_csv(path: impl AsRef<Path>) -> Result<Self, SimError> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_path(path.as_ref())
            .map_err(|e| SimError::Msg(format!("scripted driver CSV open: {e}")))?;

        let mut keyframes = Vec::new();
        for result in reader.records() {
            let record =
                result.map_err(|e| SimError::Msg(format!("scripted driver CSV record: {e}")))?;
            let time_s: f64 = record
                .get(0)
                .and_then(|s| s.trim().parse().ok())
                .ok_or_else(|| SimError::Msg("invalid time_s in driver script".into()))?;
            let throttle: f64 = record
                .get(1)
                .and_then(|s| s.trim().parse().ok())
                .ok_or_else(|| SimError::Msg("invalid throttle in driver script".into()))?;
            let brake: f64 = record
                .get(2)
                .and_then(|s| s.trim().parse().ok())
                .ok_or_else(|| SimError::Msg("invalid brake in driver script".into()))?;
            keyframes.push(Keyframe {
                time_s,
                throttle,
                brake,
            });
        }

        if keyframes.is_empty() {
            return Err(SimError::Msg(
                "driver script CSV contains no keyframes".into(),
            ));
        }

        Ok(Self::new(keyframes))
    }

    /// Number of keyframes loaded.
    pub fn len(&self) -> usize {
        self.keyframes.len()
    }

    /// True if the driver has no keyframes.
    pub fn is_empty(&self) -> bool {
        self.keyframes.is_empty()
    }
}

impl Driver for ScriptedDriver {
    fn initial_inputs(&self) -> DriverInput {
        let kf = self.keyframes.first().expect("keyframes non-empty");
        DriverInput {
            throttle: kf.throttle.clamp(0.0, 1.0),
            brake: kf.brake.clamp(0.0, 1.0),
        }
    }

    fn decide(&mut self, state: &TrainSimState, _speed_limit_mps: f64) -> DriverInput {
        let t = state.time_s();

        // Advance current_idx to the last keyframe whose time_s ≤ t (hold-last).
        while self.current_idx + 1 < self.keyframes.len()
            && self.keyframes[self.current_idx + 1].time_s <= t
        {
            self.current_idx += 1;
        }

        let kf = &self.keyframes[self.current_idx];
        DriverInput {
            throttle: kf.throttle.clamp(0.0, 1.0),
            brake: kf.brake.clamp(0.0, 1.0),
        }
    }
}
