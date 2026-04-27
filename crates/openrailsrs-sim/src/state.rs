use openrailsrs_core::SimTime;

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
        }
    }

    pub fn current_edge(&self) -> Option<&str> {
        self.path_edges.get(self.edge_index).map(String::as_str)
    }

    pub fn time_s(&self) -> f64 {
        self.time.seconds()
    }
}
