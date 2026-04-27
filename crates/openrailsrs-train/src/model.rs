/// Tractive-effort curve: ordered list of (velocity_mps, force_n) breakpoints.
/// Interpolation is piecewise-linear inside the range; outside the range the nearest
/// endpoint value is returned (clamped, never extrapolated).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TractiveCurve {
    pub points: Vec<(f64, f64)>,
}

impl TractiveCurve {
    /// Interpolate traction force at the given velocity.
    /// Returns `None` if the curve has no points (caller falls back to P/v).
    pub fn interpolate(&self, v_mps: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        if self.points.len() == 1 {
            return Some(self.points[0].1);
        }
        if v_mps <= self.points.first().unwrap().0 {
            return Some(self.points.first().unwrap().1);
        }
        if v_mps >= self.points.last().unwrap().0 {
            return Some(self.points.last().unwrap().1);
        }
        // Binary search for the enclosing segment.
        let idx = self
            .points
            .partition_point(|(v, _)| *v <= v_mps)
            .saturating_sub(1);
        let (v0, f0) = self.points[idx];
        let (v1, f1) = self.points[idx + 1];
        if (v1 - v0).abs() < f64::EPSILON {
            return Some(f0);
        }
        let t = (v_mps - v0) / (v1 - v0);
        Some(f0 + t * (f1 - f0))
    }

    /// Build a synthetic two-point curve from max power and max tractive effort.
    /// This reproduces the P/v shape as a baseline: F_stall = max_tractive_effort_n at v=0,
    /// transitioning to the power-limited regime at v = P / F_stall.
    pub fn from_power_and_effort(max_power_w: f64, max_tractive_effort_n: f64) -> Self {
        if max_power_w <= 0.0 || max_tractive_effort_n <= 0.0 {
            return Self::default();
        }
        let v_corner = max_power_w / max_tractive_effort_n;
        let v_max = max_power_w / (max_tractive_effort_n * 0.05);
        Self {
            points: vec![
                (0.0, max_tractive_effort_n),
                (v_corner, max_tractive_effort_n),
                (v_max.min(100.0), max_power_w / v_max.min(100.0)),
            ],
        }
    }
}

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
    /// Optional explicit traction curve; if absent, P/v law is used.
    pub tractive_curve: Option<TractiveCurve>,
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

    /// Build an aggregate tractive curve for the whole consist.
    /// If any locomotive has an explicit curve, those are summed point-by-point on a merged
    /// velocity grid.  If none have a curve, returns an empty `TractiveCurve` and the caller
    /// should fall back to the P/v law.
    pub fn aggregate_tractive_curve(&self) -> TractiveCurve {
        let locos: Vec<&Locomotive> = self
            .vehicles
            .iter()
            .filter_map(|v| match v {
                Vehicle::Loco(l) => Some(l),
                _ => None,
            })
            .collect();

        let any_explicit = locos.iter().any(|l| l.tractive_curve.is_some());
        if !any_explicit {
            return TractiveCurve::default();
        }

        // Collect all velocity breakpoints from all explicit curves.
        let mut v_set: Vec<f64> = locos
            .iter()
            .filter_map(|l| l.tractive_curve.as_ref())
            .flat_map(|c| c.points.iter().map(|(v, _)| *v))
            .collect();
        v_set.sort_by(f64::total_cmp);
        v_set.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

        let points = v_set
            .into_iter()
            .map(|v| {
                let total_f: f64 = locos
                    .iter()
                    .map(|l| {
                        if let Some(curve) = &l.tractive_curve {
                            curve.interpolate(v).unwrap_or(0.0)
                        } else {
                            // Fallback P/v for locos without an explicit curve.
                            TractiveCurve::from_power_and_effort(
                                l.max_power_w,
                                l.max_tractive_effort_n,
                            )
                            .interpolate(v)
                            .unwrap_or(0.0)
                        }
                    })
                    .sum();
                (v, total_f)
            })
            .collect();

        TractiveCurve { points }
    }
}
