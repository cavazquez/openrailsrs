//! Headless simulation: integrate train dynamics on a [`TrackGraph`].

pub mod brake;
pub mod coupler;
pub mod csv_out;
pub mod error;
pub mod etcs;
pub mod exterior;
pub mod live_drive;
pub mod multi_runner;
pub mod path;
pub mod path_data;
pub mod physics;
pub mod runner;
pub mod scripted_driver;
pub mod state;
pub mod steam;

pub use brake::{
    BrakeCylinder, BrakeState, BrakeSystem, BrakeVehicleSpec, OR_DEFAULT_BRAKE_ADHESION_MU,
    vehicle_specs_from_consist,
};
pub use coupler::{CouplerKind, CouplerState, VehicleState};
pub use error::SimError;
pub use etcs::{BasicEtcsTcs, EtcsTcs, EtcsTcsStatus};
pub use exterior::{DoorState, RollingStockExteriorState};
pub use live_drive::{CabTelemetry, LiveDriveSession, LiveGameplay, LiveStopTarget};
pub use multi_runner::{
    LiveMultiSim, LiveTrainSnapshot, MultiTrainResult, TrainRunResult, TrainStatus,
    run_multi_train_from_scenario_file, run_scenario_multi_train,
};
pub use physics::TrainPhysics;
pub use runner::{
    AutoDriver, Driver, DriverInput, RunMetadata, SimEvent, SimRunResult, run_from_scenario_file,
    run_from_scenario_file_with_driver, run_scenario_headless, run_scenario_headless_with_driver,
};
pub use scripted_driver::{Keyframe, ScriptedDriver};
pub use state::TrainSimState;
pub use steam::BoilerState;
