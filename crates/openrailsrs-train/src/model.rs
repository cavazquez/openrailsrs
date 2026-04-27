/// Davis rolling-resistance coefficients: F_resist = a + b·v + c·v².
#[derive(Clone, Debug, PartialEq)]
pub struct DavisCoefficients {
    pub a_n: f64,
    pub b_n_per_mps: f64,
    pub c_n_per_mps2: f64,
}

impl Default for DavisCoefficients {
    fn default() -> Self {
        Self {
            a_n: 800.0,
            b_n_per_mps: 12.0,
            c_n_per_mps2: 0.4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Locomotive {
    pub name: String,
    pub mass_kg: f64,
    pub max_power_w: f64,
    pub max_velocity_mps: f64,
    pub max_tractive_effort_n: f64,
    pub max_brake_force_n: f64,
}

#[derive(Clone, Debug)]
pub struct Wagon {
    pub name: String,
    pub mass_kg: f64,
    pub max_brake_force_n: f64,
}

#[derive(Clone, Debug)]
pub enum Vehicle {
    Loco(Locomotive),
    Wagon(Wagon),
}

#[derive(Clone, Debug)]
pub struct Consist {
    pub vehicles: Vec<Vehicle>,
    pub davis: DavisCoefficients,
}

impl Consist {
    pub fn total_mass_kg(&self) -> f64 {
        self.vehicles
            .iter()
            .map(|v| match v {
                Vehicle::Loco(l) => l.mass_kg,
                Vehicle::Wagon(w) => w.mass_kg,
            })
            .sum()
    }

    pub fn total_max_power_w(&self) -> f64 {
        self.vehicles
            .iter()
            .filter_map(|v| match v {
                Vehicle::Loco(l) => Some(l.max_power_w),
                _ => None,
            })
            .sum()
    }

    pub fn total_max_brake_n(&self) -> f64 {
        self.vehicles
            .iter()
            .map(|v| match v {
                Vehicle::Loco(l) => l.max_brake_force_n,
                Vehicle::Wagon(w) => w.max_brake_force_n,
            })
            .sum()
    }

    pub fn total_max_tractive_effort_n(&self) -> f64 {
        self.vehicles
            .iter()
            .filter_map(|v| match v {
                Vehicle::Loco(l) => Some(l.max_tractive_effort_n),
                _ => None,
            })
            .sum()
    }
}
