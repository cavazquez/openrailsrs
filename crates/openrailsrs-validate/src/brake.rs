//! OR brake pressure → scripted-driver command → sim cylinder fraction.

use serde::{Deserialize, Serialize};

/// Open Rails / MSTS driver UI treats ~121 PSI as full brake command.
pub const OR_DEFAULT_BRAKE_FULL_SCALE_PSI: f64 = 121.0;

/// Maps normalized driver `[0, 1]` brake commands to Westinghouse cylinder fractions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrakeCommandMapping {
    /// PSI divisor used when exporting OR evaluation logs to `driver_or.csv`.
    pub driver_full_scale_psi: f64,
    /// PSI reference for full cylinder force in the physics model.
    pub cylinder_full_scale_psi: f64,
}

impl Default for BrakeCommandMapping {
    fn default() -> Self {
        Self::identity()
    }
}

impl BrakeCommandMapping {
    /// Driver commands map 1:1 to cylinder fraction (both scaled by 121 PSI).
    pub fn identity() -> Self {
        Self {
            driver_full_scale_psi: OR_DEFAULT_BRAKE_FULL_SCALE_PSI,
            cylinder_full_scale_psi: OR_DEFAULT_BRAKE_FULL_SCALE_PSI,
        }
    }

    /// Build from optional scenario fields; cylinder defaults to driver scale when omitted.
    pub fn from_scenario_fields(
        driver_full_scale_psi: Option<f64>,
        cylinder_full_scale_psi: Option<f64>,
    ) -> Self {
        let driver = driver_full_scale_psi
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(OR_DEFAULT_BRAKE_FULL_SCALE_PSI);
        let cylinder = cylinder_full_scale_psi
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(driver);
        Self {
            driver_full_scale_psi: driver,
            cylinder_full_scale_psi: cylinder,
        }
    }

    /// Convert scripted-driver brake command to cylinder force fraction.
    pub fn command_to_cylinder_fraction(&self, command: f64) -> f64 {
        let cmd = command.clamp(0.0, 1.0);
        if self.cylinder_full_scale_psi <= 0.0 || self.driver_full_scale_psi <= 0.0 {
            return cmd;
        }
        (cmd * self.driver_full_scale_psi / self.cylinder_full_scale_psi).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_mapping_is_one_to_one() {
        let m = BrakeCommandMapping::identity();
        assert!((m.command_to_cylinder_fraction(0.5) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn chiltern_calibration_maps_nine_psi_service() {
        let m = BrakeCommandMapping::from_scenario_fields(None, Some(35.0));
        let cmd = 9.0 / OR_DEFAULT_BRAKE_FULL_SCALE_PSI;
        let frac = m.command_to_cylinder_fraction(cmd);
        assert!((frac - 9.0 / 35.0).abs() < 1e-6);
        assert!(
            frac > cmd,
            "cylinder fraction should exceed raw driver command"
        );
    }
}
