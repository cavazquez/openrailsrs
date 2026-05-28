use openrailsrs_core::SimTime;
use openrailsrs_train::{Consist, Vehicle};

use crate::brake::BrakeSystem;
use crate::coupler::{CouplerKind, CouplerState, VehicleState};
use crate::steam::BoilerState;

#[derive(Clone, Debug)]
pub struct TrainSimState {
    pub time: SimTime,
    pub path_edges: Vec<String>,
    pub edge_index: usize,
    pub pos_on_edge_m: f64,
    pub velocity_mps: f64,
    pub throttle: f64,
    pub brake: f64,
    /// Gross electrical energy drawn (minus regen already subtracted), in joules.
    pub cumulative_energy_j: f64,
    pub odometer_m: f64,
    /// Energy recovered by regenerative braking, in joules.
    pub regen_energy_j: f64,
    /// Diesel fuel consumed, in grams (0 for electric traction).
    pub fuel_consumption_g: f64,
    /// Number of passengers currently on board.
    pub passengers: u32,
    /// Extra mass added by passengers (kg), updated at each stop departure.
    pub extra_mass_kg: f64,
    /// Air-brake system: per-cylinder state with pipe-propagation delay.
    pub brake_system: BrakeSystem,
    /// Per-vehicle kinematic states for multi-body coupler simulation.
    /// Empty = single-mass mode (backwards-compatible).
    pub vehicles: Vec<VehicleState>,
    /// Couplers between adjacent vehicles; length = vehicles.len().saturating_sub(1).
    pub couplers: Vec<CouplerState>,
    /// Per-vehicle masses (kg) cached from the consist — parallel to `vehicles`.
    pub vehicle_masses: Vec<f64>,
    /// Steam boiler mutable state.  `None` for electric/diesel traction.
    pub boiler_state: Option<BoilerState>,
    /// Current diesel engine RPM per powered locomotive (parallel to `TrainPhysics::diesel_engines`).
    pub diesel_rpm: Vec<f64>,
    /// Legacy run-up fraction per diesel engine (`RunUpTimeToMaxForce`).
    pub diesel_run_up: Vec<f64>,
    /// Motor heat state per diesel engine for dynamic `PowerReduction`.
    pub diesel_motor_heat: Vec<f64>,
    /// Ramped tractive force per diesel engine (OR `TractionForceN`).
    pub diesel_traction_force_n: Vec<f64>,
    /// Moving-average tractive load per diesel engine (OR `AverageForceN`).
    pub diesel_average_force_n: Vec<f64>,
    /// Elapsed time (s) for ORTS lead partial-throttle run-up after brakes release (OR-P6).
    pub orts_inherit_run_up_elapsed_s: f64,
}

impl TrainSimState {
    pub fn new(path_edges: Vec<String>) -> Self {
        Self {
            time: SimTime(0.0),
            path_edges,
            edge_index: 0,
            pos_on_edge_m: 0.0,
            velocity_mps: 0.0,
            throttle: 0.0,
            brake: 0.0,
            cumulative_energy_j: 0.0,
            odometer_m: 0.0,
            regen_energy_j: 0.0,
            fuel_consumption_g: 0.0,
            passengers: 0,
            extra_mass_kg: 0.0,
            brake_system: BrakeSystem::default(),
            vehicles: Vec::new(),
            couplers: Vec::new(),
            vehicle_masses: Vec::new(),
            boiler_state: None,
            diesel_rpm: Vec::new(),
            diesel_run_up: Vec::new(),
            diesel_motor_heat: Vec::new(),
            diesel_traction_force_n: Vec::new(),
            diesel_average_force_n: Vec::new(),
            orts_inherit_run_up_elapsed_s: 0.0,
        }
    }

    /// Initialise per-vehicle kinematics and couplers for OR-P4 multi-body mode.
    ///
    /// When `enabled` is false or the consist has no vehicles, clears multi-body state.
    pub fn init_multi_body_if_enabled(
        &mut self,
        consist: &Consist,
        enabled: bool,
        coupler_kind: CouplerKind,
    ) {
        if !enabled {
            self.vehicles.clear();
            self.couplers.clear();
            self.vehicle_masses.clear();
            return;
        }
        self.vehicle_masses = consist
            .vehicles
            .iter()
            .map(|v| match v {
                Vehicle::Loco(l) => l.mass_kg,
                Vehicle::Wagon(w) => w.mass_kg,
            })
            .collect();
        let v0 = self.velocity_mps;
        self.vehicles = self
            .vehicle_masses
            .iter()
            .map(|_| VehicleState {
                velocity_mps: v0,
                position_m: 0.0,
            })
            .collect();
        let coupler = CouplerState::from_kind(coupler_kind);
        self.couplers = (0..self.vehicles.len().saturating_sub(1))
            .map(|_| coupler.clone())
            .collect();
    }

    pub fn current_edge(&self) -> Option<&str> {
        self.path_edges.get(self.edge_index).map(String::as_str)
    }

    pub fn time_s(&self) -> f64 {
        self.time.seconds()
    }
}
