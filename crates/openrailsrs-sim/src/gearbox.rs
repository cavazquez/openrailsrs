//! Authored diesel gearbox state, independent of graphics or the optional C# host.
//!
//! Legacy MSTS gears follow OR 1.6.1 GearBox.Update and its clutch/force getters.
//! The state keeps requested and engaged gears separate: a request under load
//! takes effect when the native clutch permits it. Indices use -1 for neutral.
use openrailsrs_formats::{GearboxClutch, GearboxKind, GearboxOperation, NativeGearboxProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GearboxState {
    pub current_gear_index: i32,
    pub next_gear_index: i32,
    pub clutch_pct: f64,
    pub clutch_on: bool,
    pub geared_up: bool,
    pub geared_down: bool,
    pub manual_up: bool,
    pub manual_down: bool,
    pub shaft_rpm: f64,
    pub shift_remaining_s: f64,
    pub freewheel_on: bool,
    /// Native locomotive controllers update after their engine each tick.
    #[serde(default)]
    pub previous_throttle: f64,
    #[serde(default)]
    pub previous_brake: f64,
}

impl Default for GearboxState {
    fn default() -> Self {
        Self {
            current_gear_index: -1,
            next_gear_index: -1,
            clutch_pct: 0.,
            clutch_on: false,
            geared_up: false,
            geared_down: false,
            manual_up: false,
            manual_down: false,
            shaft_rpm: 0.,
            shift_remaining_s: 0.,
            freewheel_on: false,
            previous_throttle: 0.,
            previous_brake: 0.,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GearboxInput {
    /// Signed locomotive speed, adjusted for its selected direction.
    pub speed_mps: f64,
    pub throttle: f64,
    pub rpm: f64,
    pub output_power_w: f64,
    pub running: bool,
}

/// OR's Interpolator extends the first/last segment, including above max RPM.
pub fn interpolate_extrapolated(tab: &[(f64, f64)], x: f64) -> f64 {
    if tab.len() < 2 {
        return tab.first().map_or(0., |p| p.1);
    }
    let pair = tab
        .windows(2)
        .find(|p| x <= p[1].0)
        .unwrap_or(&tab[tab.len() - 2..]);
    pair[0].1 + (x - pair[0].0) / (pair[1].0 - pair[0].0) * (pair[1].1 - pair[0].1)
}

impl GearboxState {
    pub fn valid_for(&self, p: &NativeGearboxProfile) -> bool {
        let valid_index = |i| (-1..p.max_speeds_mps.len() as i32).contains(&i);
        valid_index(self.current_gear_index)
            && valid_index(self.next_gear_index)
            && self.clutch_pct.is_finite()
            && (-100.0..=100.0).contains(&self.clutch_pct)
            && self.shaft_rpm.is_finite()
            && self.shift_remaining_s.is_finite()
            && (0.0..=2.0).contains(&self.shift_remaining_s)
            && self.previous_throttle.is_finite()
            && (0.0..=1.0).contains(&self.previous_throttle)
            && self.previous_brake.is_finite()
            && (0.0..=1.0).contains(&self.previous_brake)
    }

    pub fn indication(&self, p: &NativeGearboxProfile) -> usize {
        if p.reverse_indication {
            (p.max_speeds_mps.len() as i32 - self.current_gear_index)
                .clamp(0, p.max_speeds_mps.len() as i32) as usize
        } else {
            (self.current_gear_index + 1).max(0) as usize
        }
    }

    pub fn request(
        &mut self,
        p: &NativeGearboxProfile,
        gear: i32,
        throttle: f64,
    ) -> Result<(), String> {
        if p.operation == GearboxOperation::Automatic {
            return Err("La caja de cambios es automática".into());
        }
        if !(-1..p.max_speeds_mps.len() as i32).contains(&gear) {
            return Err("Marcha fuera del rango de esta caja".into());
        }
        if p.mechanical && matches!(p.kind, GearboxKind::C | GearboxKind::D) && throttle > 0. {
            return Err("Cerrá el regulador antes de cambiar de marcha".into());
        }
        self.manual_up = gear > self.current_gear_index;
        self.manual_down = gear < self.current_gear_index;
        if p.mechanical && p.kind == GearboxKind::B && gear != self.current_gear_index {
            self.shift_remaining_s = 2.;
        }
        self.next_gear_index = gear;
        Ok(())
    }

    pub fn ratio(p: &NativeGearboxProfile, max_rpm: f64, index: usize) -> f64 {
        if p.mechanical {
            max_rpm * std::f64::consts::TAU * p.wheel_radius_m / (p.max_speeds_mps[index] * 60.)
        } else {
            // The native legacy ratio is stored in a Single. Clutch threshold
            // feedback makes this rounding observable during a whole journey.
            ((p.max_speeds_mps[index] as f32) / (max_rpm as f32)) as f64
        }
    }

    pub fn shaft_rpm(&self, p: &NativeGearboxProfile, max_rpm: f64, rpm: f64, speed: f64) -> f64 {
        let Ok(gear) = usize::try_from(self.current_gear_index) else {
            return rpm;
        };
        let ratio = Self::ratio(p, max_rpm, gear);
        if p.mechanical && p.operation != GearboxOperation::Automatic {
            speed.abs() * 60. / (std::f64::consts::TAU * p.wheel_radius_m) * ratio
        } else {
            ((speed as f32) / (ratio as f32)) as f64
        }
    }

    /// This is a latched native getter, rather than a threshold on clutch %.
    pub fn observe_clutch(
        &mut self,
        p: &NativeGearboxProfile,
        max_rpm: f64,
        starting_rpm: f64,
        input: GearboxInput,
    ) -> bool {
        if self.current_gear_index < 0 {
            return false;
        }
        let shaft = self.shaft_rpm(p, max_rpm, input.rpm, input.speed_mps);
        self.shaft_rpm = shaft;
        if p.mechanical && p.operation != GearboxOperation::Automatic {
            if self.manual_up
                || self.manual_down
                || self.shift_remaining_s > 0.
                || self.freewheel_on
            {
                self.clutch_on = false;
            } else if shaft >= input.rpm - 10.
                && shaft < input.rpm + 10.
                && shaft < max_rpm
                && shaft > starting_rpm * 1.5
            {
                self.clutch_on = true;
            } else if input.throttle == 0. && p.clutch != GearboxClutch::Friction {
                self.clutch_on = false;
            }
        } else {
            if input.throttle > 0. && shaft >= p.down_proportion * max_rpm {
                self.clutch_on = true;
            }
            if shaft < starting_rpm {
                self.clutch_on = false;
            }
        }
        self.clutch_on
    }

    pub fn engine_demand(
        &mut self,
        p: &NativeGearboxProfile,
        max_rpm: f64,
        starting_rpm: f64,
        input: GearboxInput,
        throttle_rpm: f64,
    ) -> f64 {
        let shaft = self.shaft_rpm(p, max_rpm, input.rpm, input.speed_mps);
        self.shaft_rpm = shaft;
        self.clutch_pct = if input.rpm > 0. {
            let value = (((input.rpm as f32) - (shaft as f32)) / (input.rpm as f32) * 100.)
                .clamp(-100., 100.);
            // OR's public percentage setter stores a fraction internally.
            ((value / 100.) * 100.) as f64
        } else {
            100.
        };
        if self.observe_clutch(p, max_rpm, starting_rpm, input) {
            shaft.max(0.)
        } else {
            throttle_rpm
        }
    }

    pub fn update(&mut self, p: &NativeGearboxProfile, max_rpm: f64, input: GearboxInput, dt: f64) {
        if p.mechanical && p.operation == GearboxOperation::Manual {
            if self.manual_up && self.current_gear_index < self.next_gear_index {
                self.current_gear_index = self.next_gear_index;
                self.manual_up = false;
            }
            if self.manual_down && self.current_gear_index > self.next_gear_index {
                self.current_gear_index = self.next_gear_index;
                self.manual_down = false;
            }
        } else {
            if (self.clutch_pct <= 5. || self.clutch_pct >= 100.)
                && self.current_gear_index < self.next_gear_index
            {
                self.current_gear_index = self.next_gear_index;
            }
            if (self.clutch_pct <= 5. || self.clutch_pct >= 50.)
                && self.current_gear_index > self.next_gear_index
            {
                self.current_gear_index = self.next_gear_index;
            }
        }
        self.shift_remaining_s = (self.shift_remaining_s - dt).max(0.);
        if input.running {
            if p.operation == GearboxOperation::Manual {
                if input.throttle == 0. && input.speed_mps.abs() == 0. {
                    self.clutch_on = false;
                    self.clutch_pct = 0.;
                }
            } else {
                if self.current_gear_index >= 0 {
                    let ratio = Self::ratio(p, max_rpm, self.current_gear_index as usize);
                    if input.speed_mps > max_rpm * p.up_proportion * ratio {
                        self.auto_up(p);
                    } else if input.speed_mps < max_rpm * p.down_proportion * ratio {
                        self.auto_down();
                    } else {
                        self.geared_up = false;
                        self.geared_down = false;
                    }
                } else if input.throttle > 0. {
                    self.auto_up(p);
                }
                if input.throttle == 0. {
                    self.neutral();
                }
            }
        } else if p.operation == GearboxOperation::Manual && input.speed_mps.abs() < 0.05 {
            self.clutch_on = false;
            self.clutch_pct = 0.;
        }
        self.shaft_rpm = self.shaft_rpm(p, max_rpm, input.rpm, input.speed_mps);
    }

    fn neutral(&mut self) {
        self.next_gear_index = -1;
        self.current_gear_index = -1;
        self.clutch_on = false;
        self.geared_up = false;
        self.geared_down = false;
    }
    fn auto_up(&mut self, p: &NativeGearboxProfile) {
        if self.clutch_pct < 5. {
            if !self.geared_up {
                self.next_gear_index += 1;
                if self.next_gear_index >= p.max_speeds_mps.len() as i32 {
                    self.next_gear_index = p.max_speeds_mps.len() as i32 - 1;
                } else {
                    self.geared_up = true;
                }
            } else {
                self.geared_up = false;
            }
        }
    }
    fn auto_down(&mut self) {
        if self.clutch_pct < 5. {
            if !self.geared_down {
                self.next_gear_index -= 1;
                if self.next_gear_index <= 0 {
                    self.next_gear_index = 0;
                } else {
                    self.geared_down = true;
                }
            } else {
                self.geared_down = false;
            }
        }
    }

    /// Gearbox output before the extra legacy locomotive throttle multiplier.
    pub fn force_n(
        &self,
        p: &NativeGearboxProfile,
        idle_rpm: f64,
        max_rpm: f64,
        input: GearboxInput,
    ) -> f64 {
        let Ok(gear) = usize::try_from(self.current_gear_index) else {
            return 0.;
        };
        let max_torque = p.torque_tab.iter().map(|v| v.1).fold(0., f64::max);
        let peak = if p.forces_at_max_speed {
            p.max_forces_n[gear] * max_torque
                / interpolate_extrapolated(&p.torque_tab, max_rpm).max(f64::MIN_POSITIVE)
        } else {
            p.max_forces_n[gear]
        };
        let mut force = if p.mechanical && p.operation == GearboxOperation::Manual {
            if self.freewheel_on || self.shift_remaining_s > 0. {
                return 0.;
            }
            let rpm = input.rpm.max(idle_rpm);
            let ratio = (rpm - idle_rpm) / (max_rpm - idle_rpm);
            let torque_multiplier =
                (0.824 * input.throttle + 0.176) + (0.785 * input.throttle - 0.785) * ratio;
            torque_multiplier * interpolate_extrapolated(&p.torque_tab, rpm) / max_torque * peak
        } else if self.clutch_pct >= -20. {
            interpolate_extrapolated(&p.torque_tab, input.rpm) / max_torque * input.throttle * peak
        } else {
            return -p.coasting_force_n * (100. + self.clutch_pct) / 100.;
        };
        if input.speed_mps > 0. {
            force = force.min(input.output_power_w / input.speed_mps);
        }
        force
    }

    pub fn overspeed(&self, p: &NativeGearboxProfile, rpm: f64, max_rpm: f64) -> (bool, bool) {
        (
            self.current_gear_index >= 0 && rpm / max_rpm * 100. > 100.,
            self.current_gear_index >= 0 && rpm / max_rpm * 100. > p.overspeed_percent,
        )
    }
}
