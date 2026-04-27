use serde::{Deserialize, Serialize};

/// Simulation clock in seconds from scenario start.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SimTime(pub f64);

impl SimTime {
    pub fn seconds(self) -> f64 {
        self.0
    }
}

impl std::ops::Add<f64> for SimTime {
    type Output = SimTime;

    fn add(self, rhs: f64) -> Self::Output {
        SimTime(self.0 + rhs)
    }
}
