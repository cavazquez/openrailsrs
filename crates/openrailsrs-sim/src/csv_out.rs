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

/// Per-vehicle brake telemetry (head = vehicle 0, tail = last cylinder).
const BRAKE_CYLINDER_HEADERS: &[&str] = &["brake_f_head_n", "brake_f_tail_n"];

pub struct RunCsvWriter<W: Write> {
    inner: Writer<W>,
    has_steam: bool,
    brake_cylinder_telemetry: bool,
}

impl<W: Write> RunCsvWriter<W> {
    /// Create a writer without optional telemetry columns.
    pub fn new(w: W) -> Result<Self, csv::Error> {
        Self::new_with_options(w, false, false)
    }

    /// Create a writer; pass `steam = true` to include boiler telemetry columns.
    pub fn new_with_steam(w: W, steam: bool) -> Result<Self, csv::Error> {
        Self::new_with_options(w, steam, false)
    }

    /// Create a writer with optional steam and per-cylinder brake force columns.
    pub fn new_with_options(
        w: W,
        steam: bool,
        brake_cylinder_telemetry: bool,
    ) -> Result<Self, csv::Error> {
        let mut inner = Writer::from_writer(w);
        let mut headers: Vec<&str> = BASE_HEADERS.to_vec();
        if steam {
            headers.extend_from_slice(STEAM_HEADERS);
        }
        if brake_cylinder_telemetry {
            headers.extend_from_slice(BRAKE_CYLINDER_HEADERS);
        }
        inner.write_record(&headers)?;
        Ok(Self {
            inner,
            has_steam: steam,
            brake_cylinder_telemetry,
        })
    }

    pub fn write_sample(&mut self, state: &TrainSimState) -> Result<(), csv::Error> {
        let edge = state
            .current_edge()
            .map(|s| s.to_string())
            .unwrap_or_default();

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

        if self.brake_cylinder_telemetry {
            let forces = state
                .brake_system
                .cylinder_forces_n(state.velocity_mps.max(0.0));
            let head = forces.first().copied().unwrap_or(0.0);
            let tail = forces.last().copied().unwrap_or(0.0);
            record.push(format!("{:.1}", head));
            record.push(format!("{:.1}", tail));
        }

        self.inner.write_record(&record)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), csv::Error> {
        self.inner.flush().map_err(csv::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use openrailsrs_formats::BrakeShoeFrictionCurve;

    use crate::brake::{BrakeSystem, BrakeVehicleSpec};
    use crate::state::TrainSimState;

    use super::*;

    #[test]
    fn writes_brake_head_tail_columns() {
        let shoe = BrakeShoeFrictionCurve::identity();
        let vehicles = vec![
            BrakeVehicleSpec {
                position_m: 0.0,
                max_force_n: 100_000.0,
                ep_instant: true,
                shoe_friction: shoe.clone(),
                mass_kg: 50_000.0,
                skid_adhesion_mu: 0.0,
            },
            BrakeVehicleSpec {
                position_m: 20.0,
                max_force_n: 80_000.0,
                ep_instant: false,
                shoe_friction: shoe,
                mass_kg: 40_000.0,
                skid_adhesion_mu: 0.0,
            },
        ];
        let mut state = TrainSimState::new(vec!["e1".into()]);
        state.brake_system = BrakeSystem::from_vehicle_specs(&vehicles, 200.0, false, 3.0);
        state.brake_system.precharge(1.0);

        let buf = Cursor::new(Vec::new());
        let mut w = RunCsvWriter::new_with_options(buf, false, true).unwrap();
        w.write_sample(&state).unwrap();
        w.flush().unwrap();
        let data = w.inner.into_inner().unwrap().into_inner();
        let text = String::from_utf8(data).unwrap();
        assert!(text.contains("brake_f_head_n"));
        assert!(text.contains("brake_f_tail_n"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let fields: Vec<&str> = lines[1].split(',').collect();
        let head: f64 = fields[fields.len() - 2].parse().unwrap();
        let tail: f64 = fields[fields.len() - 1].parse().unwrap();
        assert!(head > 90_000.0);
        assert!(tail < head, "head EP should lead train-air tail at t=0");
    }
}
