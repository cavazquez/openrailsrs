//! Compressor hysteresis and a finite main reservoir. Independent and train
//! demands share the wheel brake; their forces are selected, never added.
use openrailsrs_formats::typed::vehicle_runtime::AirCircuitParams;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AirCircuit {
    #[serde(default)]
    pub charged_bar_m3: f64,
    pub vehicle: usize,
    pub params: AirCircuitParams,
    pub reservoir_bar: f64,
    pub compressor_on: bool,
    pub independent_bar: f64,
    pub previous_service_bar: f64,
}
impl AirCircuit {
    pub fn new(vehicle: usize, params: AirCircuitParams) -> Self {
        Self {
            charged_bar_m3: 0.,
            vehicle,
            reservoir_bar: params.maximum_bar,
            params,
            compressor_on: false,
            independent_bar: 0.0,
            previous_service_bar: 0.0,
        }
    }
    pub fn step(&mut self, command: f64, service_bar: f64, full_bar: f64, powered: bool, dt: f64) {
        self.charged_bar_m3 = 0.;
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let target = (command.clamp(0.0, 1.0) * full_bar).min(self.reservoir_bar);
        let old = self.independent_bar;
        self.independent_bar = if target > old {
            (old + self.params.application_bar_s * dt).min(target)
        } else {
            (old - self.params.release_bar_s * dt).max(target)
        };
        // Effective cylinder volume is a conservative 10 L per locomotive.
        // It consumes reservoir air only on rising demand, not every lap tick.
        let drawn =
            (self.independent_bar.max(service_bar) - old.max(self.previous_service_bar)).max(0.0);
        self.reservoir_bar =
            (self.reservoir_bar - drawn * 0.01 / self.params.reservoir_m3).max(0.0);
        self.previous_service_bar = service_bar;
        if !powered || self.reservoir_bar >= self.params.maximum_bar {
            self.compressor_on = false;
        } else if self.reservoir_bar < self.params.restart_bar {
            self.compressor_on = true;
        }
        if self.compressor_on {
            let before = self.reservoir_bar;
            self.reservoir_bar =
                (self.reservoir_bar + self.params.charging_bar_s * dt).min(self.params.maximum_bar);
            self.charged_bar_m3 = (self.reservoir_bar - before) * self.params.reservoir_m3;
        }
    }
    pub fn valid(&self) -> bool {
        self.params.valid()
            && [
                self.reservoir_bar,
                self.independent_bar,
                self.previous_service_bar,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
            && self.reservoir_bar <= self.params.maximum_bar + 1e-6
            && self.independent_bar <= self.params.maximum_bar
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_air_compressor_hysteresis_power_loss_and_resume() {
        let mut c = AirCircuit::new(0, AirCircuitParams::default());
        c.reservoir_bar = 7.0;
        c.step(0.0, 0.0, 4.5, true, 1.0);
        assert!(c.compressor_on);
        c.step(0.0, 0.0, 4.5, false, 1.0);
        assert!(!c.compressor_on);
        for _ in 0..20 {
            c.step(1.0, 0.0, 4.5, false, 0.5);
            c.step(0.0, 0.0, 4.5, false, 0.5);
        }
        assert!(c.reservoir_bar < 7.0);
        let mut restored: AirCircuit =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        for _ in 0..200 {
            c.step(0.0, 0.0, 4.5, true, 1.0);
            restored.step(0.0, 0.0, 4.5, true, 1.0);
        }
        assert!(c.valid());
        assert_eq!(c.reservoir_bar, restored.reservoir_bar);
        assert!((c.reservoir_bar - c.params.maximum_bar).abs() < 1e-6);
    }
}
