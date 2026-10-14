//! Headless simulation: integrate train dynamics on a [`TrackGraph`].

pub mod adhesion;
pub mod brake;
pub mod brake_circuit;
pub mod cab_equipment;
pub mod cab_meter;
pub mod consumable_mass;
pub mod coupler;
pub mod csv_out;
pub mod diesel_operation;
pub mod driving_report;
pub mod electric;
pub mod error;
pub mod etcs;
pub mod exterior;
pub mod learning;
mod legacy_ep_brake;
pub mod live_drive;
pub mod live_traffic;
mod native_ep_brake;
mod native_vacuum_brake;
pub use live_traffic::{LiveTraffic, TrafficService, TrafficSnapshot};
pub mod multi_runner;
mod operational_path;
pub mod operations;
mod timetable_operations;
pub use operations::{CarOperation, CarOperationState, FormationState, SessionSnapshot};
pub mod native_dynamics;
pub mod path;
pub mod path_data;
pub mod physics;
pub mod power_supply;
pub mod refill;
pub mod runner;
pub mod scripted_driver;
pub mod state;
pub mod steam;
pub mod steam_injectors;

pub use brake::{
    BrakeCylinder, BrakeState, BrakeSystem, BrakeVehicleSpec, OR_DEFAULT_BRAKE_ADHESION_MU,
    vehicle_specs_from_consist,
};
pub use coupler::{CouplerKind, CouplerState, VehicleState};
pub use error::SimError;
pub use etcs::{BasicEtcsTcs, EtcsTcs, EtcsTcsStatus};
pub use exterior::{DoorState, RollingStockExteriorState};
pub use live_drive::{CabTelemetry, LiveDriveSession};
pub mod service;
pub mod shunting;
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
pub use service::{LiveGameplay, LiveStopTarget, ServicePhase, ServiceStopResult};
pub use state::TrainSimState;
mod articulation;
pub mod depot;
pub mod level_crossing;
pub mod shunt_authority;
pub use steam::BoilerState;

pub mod native_signals;
mod track_reservations;
pub use track_reservations::{DispatcherStatus, SwitchLock};

pub mod wiper;

pub mod rail_operations;

pub mod announcements;

pub mod crosswind;
pub mod replay;
pub mod surface_conditions;
pub mod traction_energy;
pub mod vehicle_systems;
pub mod voltage_filter;

pub mod brake_equipment;
pub mod engine_equipment;

pub mod tilting;

pub mod multiple_unit;

pub mod auxiliary_work;

pub mod steam_equipment;

pub mod freight;
pub mod independent_coupling;
mod passing_plan;
pub mod yard_delivery;
