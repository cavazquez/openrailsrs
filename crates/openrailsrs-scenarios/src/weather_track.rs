//! Authored weather evaluated on the railway clock, shared by viewer and CPU simulation.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherKind {
    #[default]
    Clear,
    Overcast,
    Rain,
    Snow,
    Fog,
    Storm,
}
impl WeatherKind {
    pub const ALL: [Self; 6] = [
        Self::Clear,
        Self::Overcast,
        Self::Rain,
        Self::Snow,
        Self::Fog,
        Self::Storm,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Despejado",
            Self::Overcast => "Cubierto",
            Self::Rain => "Lluvia",
            Self::Snow => "Nieve",
            Self::Fog => "Niebla",
            Self::Storm => "Tormenta",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherPhase {
    pub start_s: f64,
    pub transition_s: f64,
    pub kind: WeatherKind,
    pub intensity: f32,
    pub wind_mps: [f32; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherTrack {
    pub schema: u32,
    pub seed: u32,
    pub phases: Vec<WeatherPhase>,
}
impl Default for WeatherTrack {
    fn default() -> Self {
        Self {
            schema: 1,
            seed: 1,
            phases: vec![
                WeatherPhase {
                    start_s: 0.,
                    transition_s: 0.,
                    kind: WeatherKind::Clear,
                    intensity: 0.5,
                    wind_mps: [1., 0.3],
                },
                WeatherPhase {
                    start_s: 240.,
                    transition_s: 60.,
                    kind: WeatherKind::Rain,
                    intensity: 0.7,
                    wind_mps: [3., 1.],
                },
                WeatherPhase {
                    start_s: 600.,
                    transition_s: 120.,
                    kind: WeatherKind::Clear,
                    intensity: 0.5,
                    wind_mps: [1., 0.3],
                },
            ],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct WeatherFrame {
    pub rain: f32,
    pub snow: f32,
    pub cloud: f32,
    pub storm: f32,
    pub visibility_m: f32,
    pub fog_density: f32,
    pub wind_mps: [f32; 2],
}
impl WeatherFrame {
    pub fn rail_factor(self) -> f64 {
        let t = ((self.fog_density - 0.001) / (0.065 - 0.001)).clamp(0., 1.);
        let fog = f64::from(t * t * (3. - 2. * t)) * 0.4;
        (1. - (f64::from(self.rain) * 0.4)
            .max(f64::from(self.snow) * 0.5)
            .max(fog))
        .clamp(0.5, 1.)
    }
    fn target(phase: &WeatherPhase) -> Self {
        let i = phase.intensity;
        let mut frame = Self {
            rain: 0.,
            snow: 0.,
            cloud: 0.22,
            storm: 0.,
            visibility_m: 20_000.,
            fog_density: 0.00004,
            wind_mps: phase.wind_mps,
        };
        match phase.kind {
            WeatherKind::Clear => {}
            WeatherKind::Overcast => {
                frame.cloud = 0.5 + i * 0.5;
            }
            WeatherKind::Rain | WeatherKind::Storm => {
                frame.rain = i;
                frame.cloud = 0.7 + i * 0.28;
                frame.visibility_m = 12_000. - i * 9_000.;
                frame.fog_density = 0.00008 + i * 0.0005;
                if phase.kind == WeatherKind::Storm {
                    frame.storm = i;
                }
            }
            WeatherKind::Snow => {
                frame.snow = i;
                frame.cloud = 0.6 + i * 0.28;
                frame.visibility_m = 2500. - i * 2000.;
                frame.fog_density = 0.0001 + i * 0.0007;
            }
            WeatherKind::Fog => {
                frame.cloud = 0.8;
                frame.visibility_m = 5000. - i * 4880.;
                frame.fog_density = 0.00004 + i * 0.06496;
            }
        }
        frame
    }
    fn blend(self, to: Self, t: f32) -> Self {
        let l = |a, b| a + (b - a) * t;
        Self {
            rain: l(self.rain, to.rain),
            snow: l(self.snow, to.snow),
            cloud: l(self.cloud, to.cloud),
            storm: l(self.storm, to.storm),
            visibility_m: l(self.visibility_m, to.visibility_m),
            fog_density: l(self.fog_density, to.fog_density),
            wind_mps: [
                l(self.wind_mps[0], to.wind_mps[0]),
                l(self.wind_mps[1], to.wind_mps[1]),
            ],
        }
    }
}
impl WeatherTrack {
    pub fn valid(&self) -> bool {
        self.schema == 1
            && !self.phases.is_empty()
            && self.phases.len() <= 128
            && self.phases[0].start_s == 0.
            && self.phases.iter().all(|p| {
                p.start_s.is_finite()
                    && (0.0..=604800.).contains(&p.start_s)
                    && p.transition_s.is_finite()
                    && (0.0..=3600.).contains(&p.transition_s)
                    && (0.0..=1.).contains(&p.intensity)
                    && p.wind_mps.iter().all(|v| v.is_finite() && v.abs() <= 100.)
                    && p.wind_mps[0].hypot(p.wind_mps[1]) <= 100.
            })
            && self.phases.windows(2).all(|p| {
                p[1].start_s > p[0].start_s && p[1].transition_s <= p[1].start_s - p[0].start_s
            })
    }
    pub fn at(&self, seconds: f64) -> WeatherFrame {
        if !self.valid() || !seconds.is_finite() {
            return WeatherFrame::target(&Self::default().phases[0]);
        }
        let now = seconds.max(0.);
        let index = self
            .phases
            .partition_point(|p| p.start_s <= now)
            .saturating_sub(1);
        let mut frame = WeatherFrame::target(&self.phases[index]);
        if let Some(next) = self.phases.get(index + 1)
            && next.transition_s > 0.
        {
            let t =
                ((now - next.start_s + next.transition_s) / next.transition_s).clamp(0., 1.) as f32;
            frame = frame.blend(WeatherFrame::target(next), t * t * (3. - 2. * t));
        }
        let gust = 1. + 0.08 * (now / 7. + f64::from(self.seed) * 0.013).sin() as f32;
        frame.wind_mps = frame.wind_mps.map(|v| v * gust);
        frame
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_is_continuous_reproducible_and_keeps_grip_bounded() {
        let p = WeatherTrack::default();
        assert!(p.valid());
        let mut previous = p.at(0.).rain;
        for i in 1..=7000 {
            let a = p.at(i as f64 / 10.);
            assert!((a.rain - previous).abs() < 0.002);
            assert!((0.5..=1.).contains(&a.rail_factor()));
            previous = a.rain;
        }
        assert_eq!(p.at(200.).rain, p.at(200.).rain);
        assert!(p.at(240.).rain > 0.6);
        assert_eq!(p.at(600.).rain, 0.);
    }
    #[test]
    fn rejects_corrupt_and_overlapping_timelines() {
        let mut p = WeatherTrack::default();
        p.phases[1].transition_s = 300.;
        assert!(!p.valid());
        p.phases[1].transition_s = 60.;
        p.phases[1].intensity = f32::NAN;
        assert!(!p.valid());
    }
}
