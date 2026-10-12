//! Bounded railway-time driving telemetry. Loading frames and renderer timing
//! never enter the report; assistance is separate from driver input.
use crate::LiveDriveSession;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MAX_REPORT_SAMPLES: usize = 3600;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrivingSample {
    pub time_s: f64,
    pub distance_m: f64,
    pub speed_mps: f64,
    pub brake: f64,
    pub requested_brake: f64,
    pub acceleration_mps2: f64,
    pub energy_j: f64,
    pub fuel_g: f64,
    pub stop_index: usize,
    pub intervention: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DrivingReport {
    pub samples: VecDeque<DrivingSample>,
    pub braking_events: u64,
    pub emergency_events: u64,
    pub protection_events: u64,
    pub overspeed_s: f64,
    pub assisted_s: f64,
    pub braking_s: f64,
    pub comfort_accel_squared: f64,
    pub comfort_duration_s: f64,
    pub maximum_jerk_mps3: f64,
    pub last_sample_s: Option<f64>,
    pub last_emergency: bool,
    #[serde(default)]
    pub last_tick: Option<DrivingSample>,
}
impl DrivingSample {
    fn valid(&self) -> bool {
        [
            self.time_s,
            self.distance_m,
            self.speed_mps,
            self.brake,
            self.requested_brake,
            self.acceleration_mps2,
            self.energy_j,
            self.fuel_g,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.time_s >= 0.
            && self.distance_m >= 0.
            && self.speed_mps >= 0.
            && (0.0..=1.).contains(&self.brake)
            && (0.0..=1.).contains(&self.requested_brake)
            && self.stop_index <= 4096
            && self.intervention.as_ref().is_none_or(|s| s.len() <= 4096)
    }
}
impl DrivingReport {
    pub fn comfort_rms_mps2(&self) -> f64 {
        (self.comfort_accel_squared / self.comfort_duration_s.max(1.)).sqrt()
    }
    pub fn valid(&self) -> bool {
        self.samples.len() <= MAX_REPORT_SAMPLES
            && [
                self.overspeed_s,
                self.assisted_s,
                self.braking_s,
                self.comfort_accel_squared,
                self.comfort_duration_s,
                self.maximum_jerk_mps3,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            && self.last_sample_s.is_none_or(|v| v.is_finite() && v >= 0.)
            && self.last_tick.as_ref().is_none_or(DrivingSample::valid)
            && self.samples.iter().all(DrivingSample::valid)
            && self
                .samples
                .iter()
                .zip(self.samples.iter().skip(1))
                .all(|(a, b)| a.time_s < b.time_s)
    }
    pub fn observe(&mut self, s: &LiveDriveSession) {
        let now = s.time_s();
        let previous = self.last_tick.as_ref();
        let dt = previous.map_or(0., |p| now - p.time_s);
        if dt < 0. || (dt == 0. && previous.is_some()) {
            return;
        }
        let acceleration = previous
            .filter(|_| dt > 0.)
            .map_or(0., |p| (s.velocity_mps() - p.speed_mps) / dt);
        if dt > 0. {
            self.comfort_accel_squared += acceleration.powi(2) * dt;
            self.comfort_duration_s += dt;
            if let Some(p) = previous {
                self.maximum_jerk_mps3 = self
                    .maximum_jerk_mps3
                    .max((acceleration - p.acceleration_mps2).abs() / dt);
            }
            if s.gameplay.overspeed_active {
                self.overspeed_s += dt;
            }
            if s.state.brake > s.driver_brake + 0.01 {
                self.assisted_s += dt;
            }
            if s.driver_brake > 0.05 {
                self.braking_s += dt;
            }
        }
        if s.driver_brake > 0.05 && previous.is_none_or(|p| p.requested_brake <= 0.05) {
            self.braking_events += 1;
        }
        let emergency = s.state.brake_system.emergency;
        if emergency && !self.last_emergency {
            self.emergency_events += 1;
        }
        let intervention = s.braking_intervention().map(str::to_owned);
        if intervention.is_some() && previous.is_none_or(|p| p.intervention.is_none()) {
            self.protection_events += 1;
        }
        self.last_tick = Some(DrivingSample {
            time_s: now,
            distance_m: s.state.odometer_m,
            speed_mps: s.velocity_mps(),
            brake: s.state.brake,
            requested_brake: s.driver_brake,
            acceleration_mps2: acceleration,
            energy_j: s.state.cumulative_energy_j,
            fuel_g: s.state.fuel_consumption_g,
            stop_index: s.gameplay.next_stop_idx,
            intervention: intervention.clone(),
        });
        self.last_emergency = emergency;
        if self.last_sample_s.is_some_and(|last| now - last < 1.) {
            return;
        }
        self.samples.push_back(DrivingSample {
            time_s: now,
            distance_m: s.state.odometer_m,
            speed_mps: s.velocity_mps(),
            brake: s.state.brake,
            requested_brake: s.driver_brake,
            acceleration_mps2: acceleration,
            energy_j: s.state.cumulative_energy_j,
            fuel_g: s.state.fuel_consumption_g,
            stop_index: s.gameplay.next_stop_idx,
            intervention,
        });
        while self.samples.len() > MAX_REPORT_SAMPLES {
            self.samples.pop_front();
        }
        self.last_sample_s = Some(now);
        self.last_emergency = emergency;
    }
}
