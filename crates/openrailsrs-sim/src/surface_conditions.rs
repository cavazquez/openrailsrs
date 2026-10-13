//! Optional physical water film and phase change on exposed rail surfaces.
//! This is a Rust extension; OR's weather multipliers remain independently fixed.
use serde::{Deserialize, Serialize};
const LATENT_J_KG: f64 = 334_000.;
const HEAT_CAPACITY_J_M2_K: f64 = 5_000.;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceWeather {
    pub temperature_c: f64,
    pub rain: f64,
    pub snow: f64,
    pub wind_mps: [f64; 2],
}
impl Default for SurfaceWeather {
    fn default() -> Self {
        Self {
            temperature_c: 20.,
            rain: 0.,
            snow: 0.,
            wind_mps: [0.; 2],
        }
    }
}
impl SurfaceWeather {
    pub fn valid(self) -> bool {
        self.temperature_c.is_finite()
            && (-100. ..=65.).contains(&self.temperature_c)
            && [self.rain, self.snow]
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && self.wind_mps.iter().all(|v| v.is_finite())
            && self.wind_mps[0].hypot(self.wind_mps[1]) <= 100.
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceConditions {
    pub weather: SurfaceWeather,
    pub temperature_c: f64,
    /// Millimetres water equivalent (one kg/m²); reservoirs are finite.
    pub liquid_mm: f64,
    pub ice_mm: f64,
    pub snow_water_mm: f64,
    pub deposited_mm: f64,
    pub drained_mm: f64,
    pub elapsed_s: f64,
}
impl SurfaceConditions {
    pub fn new(weather: SurfaceWeather) -> Self {
        Self {
            temperature_c: weather.temperature_c,
            weather,
            liquid_mm: 0.,
            ice_mm: 0.,
            snow_water_mm: 0.,
            deposited_mm: 0.,
            drained_mm: 0.,
            elapsed_s: 0.,
        }
    }
    pub fn valid(&self) -> bool {
        self.weather.valid()
            && self.temperature_c.is_finite()
            && (-100. ..=100.).contains(&self.temperature_c)
            && [
                self.liquid_mm,
                self.ice_mm,
                self.snow_water_mm,
                self.deposited_mm,
                self.drained_mm,
                self.elapsed_s,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
            && self.liquid_mm + self.ice_mm + self.snow_water_mm <= 2.000001
            && (self.deposited_mm
                - self.drained_mm
                - self.liquid_mm
                - self.ice_mm
                - self.snow_water_mm)
                .abs()
                <= 1e-5
    }
    pub fn grip_multiplier(&self) -> f64 {
        // Continuous icing contribution; sanding is applied separately by the
        // original wheel/rail equipment, never by generating extra sand.
        let cover = (self.ice_mm / 0.25).clamp(0., 1.);
        1. - 0.80 * cover
    }
    pub fn advance(&mut self, dt: f64) {
        if !self.weather.valid() || !dt.is_finite() || dt <= 0. || dt > 60. {
            return;
        }
        let count = (dt / 0.05).ceil() as usize;
        let h = dt / count as f64;
        for _ in 0..count {
            // Normalized game intensity maps to up to 6 mm/h water equivalent;
            // provider quantities must be normalized by their own adapter.
            let rain = self.weather.rain * 6. / 3600. * h;
            let snow = self.weather.snow * 6. / 3600. * h;
            self.liquid_mm += rain;
            self.snow_water_mm += snow;
            self.deposited_mm += rain + snow;
            let total = self.liquid_mm + self.ice_mm + self.snow_water_mm;
            if total > 2. {
                let fraction = 2. / total;
                self.liquid_mm *= fraction;
                self.ice_mm *= fraction;
                self.snow_water_mm *= fraction;
                self.drained_mm += total - 2.;
            }
            self.temperature_c = self.weather.temperature_c
                + (self.temperature_c - self.weather.temperature_c) * (-h / 30.).exp();
            if self.temperature_c < 0. {
                let frozen = self
                    .liquid_mm
                    .min(-self.temperature_c * HEAT_CAPACITY_J_M2_K / LATENT_J_KG);
                self.liquid_mm -= frozen;
                self.ice_mm += frozen;
                self.temperature_c += frozen * LATENT_J_KG / HEAT_CAPACITY_J_M2_K;
            } else {
                let melted = (self.ice_mm + self.snow_water_mm)
                    .min(self.temperature_c * HEAT_CAPACITY_J_M2_K / LATENT_J_KG);
                let ice = melted.min(self.ice_mm);
                self.ice_mm -= ice;
                self.snow_water_mm = (self.snow_water_mm - (melted - ice)).max(0.);
                self.liquid_mm += melted;
                self.temperature_c -= melted * LATENT_J_KG / HEAT_CAPACITY_J_M2_K;
            }
            let drained = self.liquid_mm * (1. - (-h / 180.).exp());
            self.liquid_mm -= drained;
            self.drained_mm += drained;
            self.elapsed_s += h;
        }
    }
}
/// The same route program feeds running trains and trains awaiting departure.
pub fn update_program_input(state: &mut crate::TrainSimState) {
    let Some(program) = &state.weather_track else {
        return;
    };
    let frame = program.at_position(
        state.time_s(),
        state
            .path_edges
            .get(state.edge_index)
            .map_or("", String::as_str),
        state.pos_on_edge_m,
    );
    let input = SurfaceWeather {
        temperature_c: f64::from(frame.temperature_c),
        rain: f64::from(frame.rain),
        snow: f64::from(frame.snow),
        wind_mps: frame.wind_mps.map(f64::from),
    };
    state.ambient_temperature_c = input.temperature_c;
    state
        .surface_conditions
        .get_or_insert_with(|| SurfaceConditions::new(input))
        .weather = input;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freezing_rain_and_thaw_conserve_water_and_restore_the_rail_grip() {
        let mut s = SurfaceConditions::new(SurfaceWeather {
            temperature_c: -8.,
            rain: 1.,
            ..Default::default()
        });
        for _ in 0..300 {
            s.advance(1.);
        }
        assert!(s.valid());
        assert!(s.ice_mm > 0.3);
        assert!(s.grip_multiplier() < 0.3);
        let mut restored: SurfaceConditions =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        s.weather = SurfaceWeather {
            temperature_c: 15.,
            ..Default::default()
        };
        restored.weather = s.weather;
        for _ in 0..1200 {
            s.advance(0.25);
            restored.advance(0.25);
        }
        assert_eq!(s, restored);
        assert!(s.valid());
        assert_eq!(s.ice_mm, 0.);
        assert_eq!(s.grip_multiplier(), 1.);
    }
    #[test]
    fn shared_quanta_are_independent_of_frame_grouping_and_pause() {
        let weather = SurfaceWeather {
            temperature_c: -5.,
            rain: 0.6,
            snow: 0.4,
            ..Default::default()
        };
        let mut a = SurfaceConditions::new(weather);
        let mut b = a.clone();
        for _ in 0..60 {
            a.advance(1.);
        }
        for _ in 0..1200 {
            b.advance(0.05);
        }
        assert!((a.ice_mm - b.ice_mm).abs() < 1e-8);
        assert!((a.snow_water_mm - b.snow_water_mm).abs() < 1e-8);
        let old = b.clone();
        b.advance(0.);
        assert_eq!(b, old);
        assert!(a.valid() && b.valid());
        b.weather.temperature_c = f64::NAN;
        b.advance(1.);
        assert_eq!(b.ice_mm, old.ice_mm);
        assert!(!b.valid());
    }
}
