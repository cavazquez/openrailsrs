use std::io::Write;

use csv::Writer;
use serde::Serialize;

use crate::state::TrainSimState;

#[derive(Serialize)]
struct SampleRow {
    pub time_s: f64,
    pub edge_id: String,
    pub pos_on_edge_m: f64,
    pub velocity_mps: f64,
    pub odometer_m: f64,
    pub cumulative_energy_kwh: f64,
    pub regen_energy_kwh: f64,
    pub fuel_consumption_l: f64,
    pub throttle: f64,
    pub brake: f64,
}

pub struct RunCsvWriter<W: Write> {
    inner: Writer<W>,
}

impl<W: Write> RunCsvWriter<W> {
    pub fn new(w: W) -> Result<Self, csv::Error> {
        let mut inner = Writer::from_writer(w);
        inner.write_record([
            "time_s",
            "edge_id",
            "pos_on_edge_m",
            "velocity_mps",
            "odometer_m",
            "cumulative_energy_kwh",
            "regen_energy_kwh",
            "fuel_consumption_l",
            "throttle",
            "brake",
        ])?;
        Ok(Self { inner })
    }

    pub fn write_sample(&mut self, state: &TrainSimState) -> Result<(), csv::Error> {
        let edge = state
            .current_edge()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let row = SampleRow {
            time_s: state.time_s(),
            edge_id: edge,
            pos_on_edge_m: state.pos_on_edge_m,
            velocity_mps: state.velocity_mps,
            odometer_m: state.odometer_m,
            cumulative_energy_kwh: state.cumulative_energy_j / 3.6e6,
            regen_energy_kwh: state.regen_energy_j / 3.6e6,
            fuel_consumption_l: state.fuel_consumption_g / 840.0,
            throttle: state.throttle,
            brake: state.brake,
        };
        self.inner.serialize(row)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), csv::Error> {
        self.inner.flush().map_err(csv::Error::from)
    }
}
