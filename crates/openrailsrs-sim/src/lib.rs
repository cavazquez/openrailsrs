//! Headless simulation: integrate train dynamics on a [`TrackGraph`].

pub mod csv_out;
pub mod error;
pub mod multi_runner;
pub mod path;
pub mod path_data;
pub mod physics;
pub mod runner;
pub mod scripted_driver;
pub mod state;

pub use error::SimError;
pub use multi_runner::{
    MultiTrainResult, TrainRunResult, run_multi_train_from_scenario_file, run_scenario_multi_train,
};
pub use physics::TrainPhysics;
pub use runner::{
    AutoDriver, Driver, DriverInput, RunMetadata, SimEvent, SimRunResult, run_from_scenario_file,
    run_from_scenario_file_with_driver, run_scenario_headless, run_scenario_headless_with_driver,
};
pub use scripted_driver::{Keyframe, ScriptedDriver};
pub use state::TrainSimState;
