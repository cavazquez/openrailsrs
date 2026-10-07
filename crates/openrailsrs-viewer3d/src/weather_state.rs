//! Shared visual atmosphere. No railway physics or wall-clock-dependent RNG.
//! Fixed profiles are analytic in railway time; live samples approach their
//! target exponentially. All consumers see one bounded, pause-safe snapshot.
use crate::{environment::WeatherSample, player_launch::PlayerWeather};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherProfile {
    #[default]
    Automatic,
    Drizzle,
    SteadyRain,
    Downpour,
    LightSnow,
    HeavySnow,
    AfterSnow,
    StormCycle,
}
impl WeatherProfile {
    pub const ALL: [Self; 8] = [
        Self::Automatic,
        Self::Drizzle,
        Self::SteadyRain,
        Self::Downpour,
        Self::LightSnow,
        Self::HeavySnow,
        Self::AfterSnow,
        Self::StormCycle,
    ];
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|p| *p == self).unwrap_or(0) + 1) % Self::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Según el clima elegido",
            Self::Drizzle => "Llovizna",
            Self::SteadyRain => "Lluvia sostenida",
            Self::Downpour => "Lluvia intensa",
            Self::LightSnow => "Nevada leve",
            Self::HeavySnow => "Nevada intensa",
            Self::AfterSnow => "Después de nevar",
            Self::StormCycle => "Tormenta: aproximación, actividad y despeje",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "auto" | "automatic" => Self::Automatic,
            "drizzle" => Self::Drizzle,
            "rain" | "steady_rain" => Self::SteadyRain,
            "downpour" => Self::Downpour,
            "light_snow" => Self::LightSnow,
            "heavy_snow" => Self::HeavySnow,
            "after_snow" => Self::AfterSnow,
            "storm_cycle" => Self::StormCycle,
            _ => return None,
        })
    }
    pub fn weather(self) -> Option<PlayerWeather> {
        match self {
            Self::Automatic => None,
            Self::Drizzle | Self::SteadyRain | Self::Downpour => Some(PlayerWeather::Rain),
            Self::LightSnow | Self::HeavySnow => Some(PlayerWeather::Snow),
            Self::AfterSnow => Some(PlayerWeather::Overcast),
            Self::StormCycle => Some(PlayerWeather::Storm),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StormPhase {
    #[default]
    Calm,
    Approaching,
    Active,
    Clearing,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Atmosphere {
    pub rain: f32,
    pub snow: f32,
    pub snow_cover: f32,
    pub flake_size: f32,
    pub cloud_cover: f32,
    pub overcast: f32,
    pub storm: f32,
    pub visibility_m: f32,
    pub fog_density: f32,
    #[serde(serialize_with = "serialize_wind")]
    pub wind_mps: Vec3,
    pub phase: StormPhase,
}
fn serialize_wind<S: serde::Serializer>(wind: &Vec3, serializer: S) -> Result<S::Ok, S::Error> {
    wind.to_array().serialize(serializer)
}
impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            rain: 0.,
            snow: 0.,
            snow_cover: 0.,
            flake_size: 1.,
            cloud_cover: 0.22,
            overcast: 0.,
            storm: 0.,
            visibility_m: 20_000.,
            fog_density: 0.00004,
            wind_mps: Vec3::new(0.8, 0., 0.3),
            phase: StormPhase::Calm,
        }
    }
}
impl Atmosphere {
    fn blend(self, target: Self, amount: f32) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * amount;
        Self {
            rain: lerp(self.rain, target.rain),
            snow: lerp(self.snow, target.snow),
            snow_cover: lerp(self.snow_cover, target.snow_cover),
            flake_size: lerp(self.flake_size, target.flake_size),
            cloud_cover: lerp(self.cloud_cover, target.cloud_cover),
            overcast: lerp(self.overcast, target.overcast),
            storm: lerp(self.storm, target.storm),
            visibility_m: lerp(self.visibility_m, target.visibility_m),
            fog_density: lerp(self.fog_density, target.fog_density),
            wind_mps: self.wind_mps.lerp(target.wind_mps, amount),
            phase: target.phase,
        }
    }
}
fn smooth(value: f32) -> f32 {
    let t = value.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// One 660-second cycle. Continuous values and zero derivatives at boundaries
/// make the same seed/time independent of frame rate and camera/LOD changes.
pub fn fixed_at(
    weather: PlayerWeather,
    profile: WeatherProfile,
    seed: u32,
    seconds: f64,
) -> Atmosphere {
    let mut a = Atmosphere::default();
    let weather = profile.weather().unwrap_or(weather);
    match weather {
        PlayerWeather::Clear => {}
        PlayerWeather::Overcast => {
            a.cloud_cover = 0.78;
            a.overcast = 0.5;
            a.fog_density = 0.00008;
        }
        PlayerWeather::Fog => {
            a.cloud_cover = 0.96;
            a.overcast = 0.85;
            a.visibility_m = 120.;
            a.fog_density = 0.065;
        }
        PlayerWeather::Rain => {
            a.rain = match profile {
                WeatherProfile::Drizzle => 0.18,
                WeatherProfile::Downpour => 1.,
                _ => 0.62,
            };
            a.cloud_cover = 0.7 + a.rain * 0.28;
            a.overcast = 0.45 + a.rain * 0.4;
            a.visibility_m = 12_000. - a.rain * 9_000.;
            a.fog_density = 0.00008 + a.rain * 0.0005;
            a.wind_mps = Vec3::new(0.8 + a.rain * 3., 0., 0.3 + a.rain);
        }
        PlayerWeather::Snow => {
            a.snow = if profile == WeatherProfile::LightSnow {
                0.22
            } else {
                1.
            };
            a.snow_cover = if profile == WeatherProfile::LightSnow {
                0.25
            } else {
                1.
            };
            a.flake_size = if profile == WeatherProfile::LightSnow {
                0.7
            } else {
                1.35
            };
            a.cloud_cover = 0.6 + a.snow * 0.28;
            a.overcast = 0.45 + a.snow * 0.25;
            a.visibility_m = 2_500. - a.snow * 2_000.;
            a.fog_density = 0.0001 + a.snow * 0.0007;
            a.wind_mps = Vec3::new(0.8 + a.snow * 2.8, 0., 0.3 + a.snow * 1.2);
        }
        PlayerWeather::Storm => {
            let t = seconds.max(0.).rem_euclid(660.) as f32;
            let (phase, strength) = if t < 120. {
                (StormPhase::Approaching, smooth(t / 120.))
            } else if t < 360. {
                (StormPhase::Active, 1.)
            } else if t < 480. {
                (StormPhase::Clearing, 1. - smooth((t - 360.) / 120.))
            } else {
                (StormPhase::Calm, 0.)
            };
            a.phase = phase;
            a.storm = strength;
            a.rain = strength * strength;
            a.cloud_cover = 0.22 + 0.77 * strength;
            a.overcast = 0.92 * strength;
            a.visibility_m = 20_000. - strength * 17_000.;
            a.fog_density = 0.00004 + strength * 0.001;
            // Seed affects the phase of smooth gusts, never the frame schedule.
            let offset = crate::precipitation::rain_rng01(seed, 17) * std::f32::consts::TAU;
            let gust = ((seconds * 0.13) as f32 + offset).sin() * 0.5 + 0.5;
            a.wind_mps = Vec3::new(
                0.8 + strength * (5. + gust * 5.),
                0.,
                0.3 + strength * (1. + gust * 2.),
            );
        }
    }
    if profile == WeatherProfile::AfterSnow {
        a.snow_cover = 1.;
        a.visibility_m = 12_000.;
    }
    a
}

fn live_at(sample: &WeatherSample) -> Atmosphere {
    let mut a = fixed_at(
        sample.weather().unwrap_or_default(),
        WeatherProfile::Automatic,
        0,
        180.,
    );
    a.cloud_cover = sample.cloud_cover / 100.;
    a.overcast = (a.cloud_cover * 0.9).clamp(0., 0.95);
    a.wind_mps = sample.wind();
    if a.rain > 0. {
        a.rain = (sample.precipitation / 5.).clamp(0.08, 1.);
    }
    if a.snow > 0. {
        a.snow = (sample.snowfall / 0.3).clamp(0.08, 1.);
    }
    a
}

#[derive(Resource, Debug)]
pub struct WeatherState {
    pub atmosphere: Atmosphere,
    pub profile: WeatherProfile,
    pub seed: u32,
    pub live: bool,
    start_s: f64,
    last_s: Option<f64>,
    selection: Option<(PlayerWeather, WeatherProfile, bool, u32)>,
    transition_from: Atmosphere,
}
impl Default for WeatherState {
    fn default() -> Self {
        Self {
            atmosphere: default(),
            profile: default(),
            seed: 1,
            live: false,
            start_s: 0.,
            last_s: None,
            selection: None,
            transition_from: default(),
        }
    }
}
impl WeatherState {
    pub fn elapsed_s(&self, clock: f64) -> f64 {
        (clock - self.start_s).max(0.)
    }
    fn advance(
        &mut self,
        clock: f64,
        weather: PlayerWeather,
        profile: WeatherProfile,
        seed: u32,
        sample: Option<&WeatherSample>,
    ) {
        let first = self.last_s.is_none() || self.last_s.is_some_and(|last| clock < last);
        let previous = self.last_s.replace(clock).unwrap_or(clock);
        let selection = (weather, profile, sample.is_some(), seed);
        if self.selection != Some(selection) || clock < previous {
            self.start_s = clock;
            self.selection = Some(selection);
            self.transition_from = self.atmosphere;
        }
        self.profile = profile;
        self.seed = seed;
        self.live = sample.is_some();
        let target = sample.map_or_else(
            || fixed_at(weather, profile, seed, self.elapsed_s(clock)),
            live_at,
        );
        let dt = (clock - previous).clamp(0., 5.) as f32;
        // Fixed profiles are analytic after startup. A user changing weather or
        // live data gets a bounded transition instead of a flash between modes.
        if first {
            self.atmosphere = target;
            self.transition_from = target;
        } else if sample.is_some() {
            self.atmosphere = self.atmosphere.blend(target, 1. - (-dt / 6.).exp());
        } else {
            self.atmosphere = self
                .transition_from
                .blend(target, 1. - (-self.elapsed_s(clock) as f32 / 6.).exp());
        }
    }
}

pub fn reset(mut state: ResMut<WeatherState>, mut wet: ResMut<crate::wet_surfaces::WetSurfaces>) {
    *state = default();
    wet.reset_clock();
}

pub fn update(
    live: Option<Res<crate::live::LiveDrive>>,
    time: Res<Time>,
    content: Res<crate::player_launch::ActivePlayerContent>,
    settings: Res<crate::player_settings::PlayerSettings>,
    environment: Res<crate::environment::LiveEnvironment>,
    mut state: ResMut<WeatherState>,
) {
    let clock = live
        .as_ref()
        .map_or(time.elapsed_secs_f64(), |l| l.session.time_s());
    let profile = std::env::var("OPENRAILSRS_WEATHER_PROFILE")
        .ok()
        .as_deref()
        .and_then(WeatherProfile::parse)
        .unwrap_or(settings.weather_profile);
    let seed = std::env::var("OPENRAILSRS_WEATHER_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(settings.weather_seed);
    let offset = std::env::var("OPENRAILSRS_WEATHER_PHASE_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.)
        .unwrap_or(0.);
    state.advance(
        clock + offset,
        content.weather,
        profile,
        seed,
        environment.current_sample(content.environment),
    );
    // The reproducible phase is independent of the start time of the service.
    if !state.live
        && state.last_s == Some(clock + offset)
        && state.elapsed_s(clock + offset) == 0.
        && offset > 0.
    {
        state.start_s -= offset;
        state.atmosphere = fixed_at(content.weather, profile, seed, offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storm_boundaries_are_continuous_and_seeded_gusts_repeat() {
        for t in [120., 360., 480., 660.] {
            let before = fixed_at(
                PlayerWeather::Storm,
                WeatherProfile::StormCycle,
                81,
                t - 0.001,
            );
            let after = fixed_at(
                PlayerWeather::Storm,
                WeatherProfile::StormCycle,
                81,
                t + 0.001,
            );
            assert!((before.rain - after.rain).abs() < 0.001);
            assert!((before.visibility_m - after.visibility_m).abs() < 1.);
        }
        let a = fixed_at(PlayerWeather::Storm, WeatherProfile::StormCycle, 81, 180.);
        assert_eq!(
            a.wind_mps,
            fixed_at(PlayerWeather::Storm, WeatherProfile::StormCycle, 81, 180.).wind_mps
        );
        assert_ne!(
            a.wind_mps,
            fixed_at(PlayerWeather::Storm, WeatherProfile::StormCycle, 82, 180.).wind_mps
        );
    }
    #[test]
    fn storm_snapshot_is_independent_of_sampling_rate_and_rewinds() {
        let mut slow = WeatherState::default();
        let mut fast = WeatherState::default();
        for i in 0..=540 {
            slow.advance(
                i as f64,
                PlayerWeather::Storm,
                WeatherProfile::StormCycle,
                81,
                None,
            );
        }
        for i in 0..=540 * 60 {
            fast.advance(
                i as f64 / 60.,
                PlayerWeather::Storm,
                WeatherProfile::StormCycle,
                81,
                None,
            );
        }
        assert_eq!(
            serde_json::to_string(&slow.atmosphere).unwrap(),
            serde_json::to_string(&fast.atmosphere).unwrap()
        );
        fast.advance(
            0.,
            PlayerWeather::Storm,
            WeatherProfile::StormCycle,
            81,
            None,
        );
        assert_eq!(fast.atmosphere.rain, 0.);
        assert_eq!(fast.elapsed_s(0.), 0.);
    }
    #[test]
    fn accumulated_snow_does_not_require_active_snowfall() {
        let a = fixed_at(PlayerWeather::Clear, WeatherProfile::AfterSnow, 1, 0.);
        assert_eq!(a.snow, 0.);
        assert_eq!(a.snow_cover, 1.);
        let light = fixed_at(PlayerWeather::Snow, WeatherProfile::LightSnow, 1, 0.);
        let heavy = fixed_at(PlayerWeather::Snow, WeatherProfile::HeavySnow, 1, 0.);
        assert!(light.snow < heavy.snow && light.flake_size < heavy.flake_size);
        assert!(light.visibility_m > heavy.visibility_m);
    }
    #[test]
    fn changing_weather_is_gradual_and_pausing_freezes_all_channels() {
        let mut state = WeatherState::default();
        state.advance(0., PlayerWeather::Clear, WeatherProfile::Automatic, 1, None);
        state.advance(0.02, PlayerWeather::Rain, WeatherProfile::Downpour, 1, None);
        assert_eq!(state.atmosphere.rain, 0.);
        state.advance(0.04, PlayerWeather::Rain, WeatherProfile::Downpour, 1, None);
        assert!(state.atmosphere.rain > 0. && state.atmosphere.rain < 0.01);
        let before = serde_json::to_string(&state.atmosphere).unwrap();
        state.advance(0.04, PlayerWeather::Rain, WeatherProfile::Downpour, 1, None);
        assert_eq!(before, serde_json::to_string(&state.atmosphere).unwrap());
        for i in 1..=900 {
            state.advance(
                i as f64 / 30.,
                PlayerWeather::Rain,
                WeatherProfile::Downpour,
                1,
                None,
            );
        }
        assert!(state.atmosphere.rain > 0.99);
    }
}
