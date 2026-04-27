//! Headless simulation: integrate train dynamics on a [`TrackGraph`].

pub mod csv_out;
pub mod error;
pub mod path;
pub mod physics;
pub mod runner;
pub mod state;

pub use error::SimError;
pub use physics::TrainPhysics;
pub use runner::{
    AutoDriver, Driver, DriverInput, RunMetadata, SimEvent, SimRunResult, run_from_scenario_file,
    run_scenario_headless, run_scenario_headless_with_driver,
};
pub use state::TrainSimState;
