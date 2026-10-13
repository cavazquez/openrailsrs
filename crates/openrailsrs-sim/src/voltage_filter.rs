//! First-order 0.7 Hz Butterworth filter used by OR 1.6.1 DefaultElectricPowerSupply.
//! Arithmetic deliberately follows Filter.cs single-precision coefficients.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VoltageFilter {
    pub previous_input: f32,
    pub previous_output: f32,
}
impl VoltageFilter {
    pub fn valid(&self) -> bool {
        [self.previous_input, self.previous_output]
            .iter()
            .all(|v| v.is_finite() && (0. ..=1_000_000.).contains(v))
    }
    pub fn filter(&mut self, input: f64, dt: f64) -> f64 {
        let (input, dt) = (input as f32, dt as f32);
        if !input.is_finite() || input < 0. || !dt.is_finite() || dt <= 0. {
            return 0.;
        }
        let cutoff = 2f32 * std::f32::consts::PI * 0.7f32;
        if 1f32 / dt < cutoff / (2f32 * std::f32::consts::PI) {
            return f64::from(input);
        }
        let warp = 4f32 * f64::from(cutoff * dt / 2f32).tan() as f32;
        let a = (warp * 0.5f32) / (2f32 + warp * 0.5f32);
        let b = (warp * 0.5f32 - 2f32) / (2f32 + warp * 0.5f32);
        let mut output = a * input;
        output += a * self.previous_input - b * self.previous_output;
        self.previous_input = input;
        self.previous_output = output;
        f64::from(output)
    }
}
