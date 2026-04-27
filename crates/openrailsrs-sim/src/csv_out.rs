use std::io::Write;

use csv::Writer;

use crate::state::TrainSimState;

/// Core columns always present in the output CSV.
const BASE_HEADERS: &[&str] = &[
    "time_s",
    "edge_id",
    "pos_on_edge_m",
    "velocity_mps",
    "odometer_m",
    "cumulative_energy_kwh",
    "regen_energy_kwh",
    "fuel_consumption_l",
    "passengers",
    "throttle",
    "brake",
];

/// Extra columns appended when a steam boiler state is present.
const STEAM_HEADERS: &[&str] = &["boiler_pressure_bar", "water_kg", "coal_kg"];

pub struct RunCsvWriter<W: Write> {
    inner: Writer<W>,
    /// Whether to write the three boiler telemetry columns.
    has_steam: bool,
}

impl<W: Write> RunCsvWriter<W> {
    /// Create a writer without steam columns.
    pub fn new(w: W) -> Result<Self, csv::Error> {
        Self::new_with_steam(w, false)
    }

    /// Create a writer; pass `steam = true` to include boiler telemetry columns.
    pub fn new_with_steam(w: W, steam: bool) -> Result<Self, csv::Error> {
        let mut inner = Writer::from_writer(w);
        let mut headers: Vec<&str> = BASE_HEADERS.to_vec();
        if steam {
            headers.extend_from_slice(STEAM_HEADERS);
        }
        inner.write_record(&headers)?;
        Ok(Self {
            inner,
            has_steam: steam,
        })
    }

    pub fn write_sample(&mut self, state: &TrainSimState) -> Result<(), csv::Error> {
        let edge = state
            .current_edge()
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Base fields.
        let mut record = vec![
            format!("{:.6}", state.time_s()),
            edge,
            format!("{:.3}", state.pos_on_edge_m),
            format!("{:.4}", state.velocity_mps),
            format!("{:.3}", state.odometer_m),
            format!("{:.6}", state.cumulative_energy_j / 3.6e6),
            format!("{:.6}", state.regen_energy_j / 3.6e6),
            format!("{:.4}", state.fuel_consumption_g / 840.0),
            state.passengers.to_string(),
            format!("{:.4}", state.throttle),
            format!("{:.4}", state.brake),
        ];

        // Optional steam columns.
        if self.has_steam {
            if let Some(b) = &state.boiler_state {
                record.push(format!("{:.3}", b.pressure_bar));
                record.push(format!("{:.1}", b.water_kg));
                record.push(format!("{:.1}", b.coal_kg));
            } else {
                record.push(String::new());
                record.push(String::new());
                record.push(String::new());
            }
        }

        self.inner.write_record(&record)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), csv::Error> {
        self.inner.flush().map_err(csv::Error::from)
    }
}
