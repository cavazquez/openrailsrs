//! Headless game layer: objectives, scoring, penalties, timeline.

pub mod error;
pub mod evaluate;

pub use error::GameError;
pub use evaluate::{PlayOutcome, StopResult, TimelineEvent, play_headless_from_scenario_file};
