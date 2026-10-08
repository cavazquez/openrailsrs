//! Bounded observations of real travel, weather contact and rendering time.
use bevy::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

use crate::{live::LiveDrive, weather_state::WeatherState};

const SAMPLE_INTERVAL_S: f64 = 10.;
const SAMPLE_CAPACITY: usize = 720;

#[derive(Default, Serialize)]
struct WeatherFrames {
    frames: u64,
    elapsed_wall_s: f64,
    longest_frame_ms: f64,
    hitches_over_100_ms: u64,
}

#[derive(Serialize)]
struct Sample {
    simulation_s: f64,
    odometer_m: f64,
    speed_mps: f64,
    weather: &'static str,
    rain: f32,
    snow: f32,
    visibility_m: f32,
    rail_factor: Option<f64>,
    rail_target: Option<f64>,
    throttle: f64,
    driver_brake: f64,
    applied_brake: f64,
    wheel_slip: bool,
    maximum_wheel_slip_mps: f64,
    requested_traction_n: f64,
    transmitted_traction_n: f64,
    wheel_rail_brake_n: f64,
    completed_stops: usize,
}

#[derive(Resource)]
pub struct WeatherJourneyTelemetry {
    last_s: Option<f64>,
    next_sample_s: f64,
    previous_grip: Option<f64>,
    min_grip: f64,
    max_grip_change_per_sim_s: f64,
    maximum_wheel_slip_mps: f64,
    slip_simulation_s: f64,
    wet_braking_simulation_s: f64,
    min_visibility_m: f32,
    samples: VecDeque<Sample>,
    frames: BTreeMap<&'static str, WeatherFrames>,
}
impl Default for WeatherJourneyTelemetry {
    fn default() -> Self {
        Self {
            last_s: None,
            next_sample_s: 0.,
            previous_grip: None,
            min_grip: 1.,
            max_grip_change_per_sim_s: 0.,
            maximum_wheel_slip_mps: 0.,
            slip_simulation_s: 0.,
            wet_braking_simulation_s: 0.,
            min_visibility_m: 100_000.,
            samples: VecDeque::new(),
            frames: BTreeMap::new(),
        }
    }
}
impl WeatherJourneyTelemetry {
    fn observe(&mut self, live: &LiveDrive, state: &WeatherState, frame_s: f64) {
        let session = &live.session;
        let clock = session.time_s();
        if self.last_s.is_some_and(|previous| clock < previous) {
            *self = Self::default();
        }
        if live.paused {
            return;
        }
        let advanced = self.last_s != Some(clock);
        // Rendering can produce multiple frames between physics ticks. Count
        // each active frame once, but keep contact integrals on simulation time.
        // Arrival can advance the last tick; subsequent terminal hold is excluded.
        if (advanced || !session.arrived) && frame_s.is_finite() && frame_s > 0. {
            let frames = self
                .frames
                .entry(state.atmosphere.weather().label())
                .or_default();
            frames.frames += 1;
            frames.elapsed_wall_s += frame_s;
            frames.longest_frame_ms = frames.longest_frame_ms.max(frame_s * 1000.);
            frames.hitches_over_100_ms += u64::from(frame_s > 0.1);
        }
        if !advanced {
            return;
        }
        let dt = self.last_s.map_or(0., |last| (clock - last).max(0.));
        self.last_s = Some(clock);
        let rail = session.state.rail_adhesion.as_ref();
        let grip = rail.map(|rail| rail.weather_factor);
        if let Some(grip) = grip {
            self.min_grip = self.min_grip.min(grip);
            if let Some(previous) = self.previous_grip
                && dt > 0.
            {
                self.max_grip_change_per_sim_s = self
                    .max_grip_change_per_sim_s
                    .max((grip - previous).abs() / dt);
            }
            self.previous_grip = Some(grip);
        }
        let slipping = rail.is_some_and(|rail| rail.slipping());
        let slip = rail.map_or(0., |rail| {
            rail.cars
                .iter()
                .enumerate()
                .map(|(i, car)| {
                    let speed = session
                        .state
                        .vehicles
                        .get(i)
                        .map_or(session.velocity_mps(), |v| v.velocity_mps);
                    (car.wheel_speed_mps.abs() - speed.abs()).abs()
                })
                .fold(0., f64::max)
        });
        self.maximum_wheel_slip_mps = self.maximum_wheel_slip_mps.max(slip);
        self.slip_simulation_s += if slipping { dt } else { 0. };
        self.wet_braking_simulation_s +=
            if session.state.brake > 0.01 && grip.is_some_and(|grip| grip < 0.99) {
                dt
            } else {
                0.
            };
        self.min_visibility_m = self.min_visibility_m.min(state.atmosphere.visibility_m);
        let station_changed = self
            .samples
            .back()
            .is_some_and(|sample| sample.completed_stops != session.gameplay.stop_results.len());
        if clock < self.next_sample_s && !station_changed {
            return;
        }
        self.next_sample_s = clock + SAMPLE_INTERVAL_S;
        if self.samples.len() == SAMPLE_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            simulation_s: clock,
            odometer_m: session.state.odometer_m,
            speed_mps: session.velocity_mps(),
            weather: state.atmosphere.weather().label(),
            rain: state.atmosphere.rain,
            snow: state.atmosphere.snow,
            visibility_m: state.atmosphere.visibility_m,
            rail_factor: grip,
            rail_target: rail.and_then(|rail| rail.weather_target_factor),
            throttle: session.driver_throttle,
            driver_brake: session.driver_brake,
            applied_brake: session.state.brake,
            wheel_slip: slipping,
            maximum_wheel_slip_mps: slip,
            requested_traction_n: rail.map_or(0., |rail| {
                rail.cars.iter().map(|car| car.requested_force_n).sum()
            }),
            transmitted_traction_n: rail.map_or(0., |rail| {
                rail.cars.iter().map(|car| car.rail_force_n).sum()
            }),
            wheel_rail_brake_n: session.wheel_rail_brake_forces_n().iter().sum(),
            completed_stops: session.gameplay.stop_results.len(),
        });
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "scope": "current journey segment; real simulation, sampled every 10 s and at station completion; bounded to 720 points; frame buckets exclude initial loading, pause and terminal hold, retaining streamed-sector work",
            "sample_capacity": SAMPLE_CAPACITY,
            "sample_interval_s": SAMPLE_INTERVAL_S,
            "minimum_rail_factor": self.min_grip,
            "maximum_grip_change_per_simulation_s": self.max_grip_change_per_sim_s,
            "maximum_wheel_slip_mps": self.maximum_wheel_slip_mps,
            "slip_simulation_s": self.slip_simulation_s,
            "wet_braking_simulation_s": self.wet_braking_simulation_s,
            "minimum_visibility_m": self.min_visibility_m,
            "weather_frames": self.frames,
            "samples": self.samples,
        })
    }
}

pub fn reset(mut telemetry: ResMut<WeatherJourneyTelemetry>) {
    *telemetry = default();
}

pub fn update(
    live: Option<Res<LiveDrive>>,
    loading: Option<Res<crate::route_bootstrap::ViewerLoadingScreen>>,
    weather: Res<WeatherState>,
    time: Res<Time<Real>>,
    mut telemetry: ResMut<WeatherJourneyTelemetry>,
) {
    if let Some(live) = live
        && loading.is_none()
    {
        telemetry.observe(&live, &weather, time.delta_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_telemetry_does_not_accumulate_and_long_journeys_remain_bounded() {
        let mut drive =
            LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path()).unwrap();
        let weather = WeatherState::default();
        let mut telemetry = WeatherJourneyTelemetry::default();
        drive.paused = true;
        telemetry.observe(&drive, &weather, 0.02);
        assert!(telemetry.samples.is_empty());
        drive.paused = false;
        for i in 0..800 {
            drive.session.state.time = openrailsrs_core::SimTime(i as f64 * 11.);
            telemetry.observe(&drive, &weather, 0.02);
        }
        assert_eq!(telemetry.samples.len(), SAMPLE_CAPACITY);
        assert_eq!(telemetry.frames["Despejado"].frames, 800);
        let frames = telemetry.frames["Despejado"].frames;
        let samples = telemetry.samples.len();
        let braking = telemetry.wet_braking_simulation_s;
        // A rendered frame without a physics tick must remain visible in the
        // performance counters, without duplicating physical observations.
        telemetry.observe(&drive, &weather, 0.02);
        assert_eq!(telemetry.frames["Despejado"].frames, frames + 1);
        assert_eq!(telemetry.samples.len(), samples);
        assert_eq!(telemetry.wet_braking_simulation_s, braking);
        drive.session.arrived = true;
        telemetry.observe(&drive, &weather, 1.);
        assert_eq!(telemetry.frames["Despejado"].frames, frames + 1);
        drive.session.arrived = false;
        drive.session.state.time = openrailsrs_core::SimTime(0.);
        telemetry.observe(&drive, &weather, 0.02);
        assert_eq!(telemetry.samples.len(), 1);
    }
}
