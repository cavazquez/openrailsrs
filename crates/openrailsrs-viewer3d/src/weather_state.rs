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
    SteadySnow,
    HeavySnow,
    AfterSnow,
    StormCycle,
    RandomJourney,
}
impl WeatherProfile {
    pub const ALL: [Self; 10] = [
        Self::Automatic,
        Self::Drizzle,
        Self::SteadyRain,
        Self::Downpour,
        Self::LightSnow,
        Self::SteadySnow,
        Self::HeavySnow,
        Self::AfterSnow,
        Self::StormCycle,
        Self::RandomJourney,
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
            Self::SteadySnow => "Nevada moderada",
            Self::HeavySnow => "Nevada intensa",
            Self::AfterSnow => "Después de nevar",
            Self::StormCycle => "Tormenta: aproximación, actividad y despeje",
            Self::RandomJourney => "Aleatorio durante el recorrido",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "auto" | "automatic" => Self::Automatic,
            "drizzle" => Self::Drizzle,
            "rain" | "steady_rain" => Self::SteadyRain,
            "downpour" => Self::Downpour,
            "light_snow" => Self::LightSnow,
            "steady_snow" => Self::SteadySnow,
            "heavy_snow" => Self::HeavySnow,
            "after_snow" => Self::AfterSnow,
            "storm_cycle" => Self::StormCycle,
            "random" | "random_journey" => Self::RandomJourney,
            _ => return None,
        })
    }
    pub fn from_env() -> Option<Self> {
        std::env::var("OPENRAILSRS_WEATHER_PROFILE")
            .ok()
            .as_deref()
            .and_then(Self::parse)
    }
    pub fn weather(self) -> Option<PlayerWeather> {
        match self {
            Self::Automatic | Self::RandomJourney => None,
            Self::Drizzle | Self::SteadyRain | Self::Downpour => Some(PlayerWeather::Rain),
            Self::LightSnow | Self::SteadySnow | Self::HeavySnow => Some(PlayerWeather::Snow),
            Self::AfterSnow => Some(PlayerWeather::Overcast),
            Self::StormCycle => Some(PlayerWeather::Storm),
        }
    }
    pub fn intensity_label(self, weather: PlayerWeather) -> &'static str {
        match self {
            Self::Drizzle | Self::LightSnow => "Leve",
            Self::SteadyRain | Self::SteadySnow => "Moderada",
            Self::Downpour | Self::HeavySnow => "Intensa",
            Self::Automatic if weather == PlayerWeather::Snow => "Intensa",
            Self::Automatic if weather == PlayerWeather::Rain => "Moderada",
            _ => self.label(),
        }
    }
    pub fn cycle_intensity(self, weather: PlayerWeather, delta: i32) -> Self {
        let choices: &[Self] = match weather {
            PlayerWeather::Rain => &[Self::Drizzle, Self::SteadyRain, Self::Downpour],
            PlayerWeather::Snow => &[Self::LightSnow, Self::SteadySnow, Self::HeavySnow],
            PlayerWeather::Overcast => &[Self::Automatic, Self::AfterSnow],
            _ => &[Self::Automatic],
        };
        let index = choices
            .iter()
            .position(|profile| *profile == self)
            .unwrap_or_else(|| {
                if weather == PlayerWeather::Snow {
                    2
                } else if weather == PlayerWeather::Rain {
                    1
                } else {
                    0
                }
            });
        choices[crate::player_launch::cycle(index, choices.len(), delta)]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StormPhase {
    #[default]
    Calm,
    Approaching,
    Active,
    Clearing,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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
    #[serde(
        serialize_with = "serialize_wind",
        deserialize_with = "deserialize_wind"
    )]
    pub wind_mps: Vec3,
    pub phase: StormPhase,
}
fn serialize_wind<S: serde::Serializer>(wind: &Vec3, serializer: S) -> Result<S::Ok, S::Error> {
    wind.to_array().serialize(serializer)
}
fn deserialize_wind<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec3, D::Error> {
    <[f32; 3]>::deserialize(deserializer).map(Vec3::from_array)
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
    /// Share the same continuous atmosphere with wheel/rail contact. This is
    /// a normalized gameplay model, not measured adhesion or an ice model.
    pub fn rail_factor(self) -> f64 {
        let rain = f64::from(self.rain.clamp(0., 1.)) * 0.4;
        let snow = f64::from(self.snow.clamp(0., 1.)) * 0.5;
        let fog = f64::from(self.dense_fog_fraction()) * 0.4;
        (1. - rain.max(snow).max(fog)).clamp(0.5, 1.)
    }

    pub fn dense_fog_fraction(self) -> f32 {
        smooth((self.fog_density - 0.001) / (0.065 - 0.001))
    }

    pub(crate) fn blend(self, target: Self, amount: f32) -> Self {
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
    pub fn weather(self) -> PlayerWeather {
        if self.storm > 0.2 {
            PlayerWeather::Storm
        } else if self.snow > 0.02 && self.snow > self.rain {
            PlayerWeather::Snow
        } else if self.rain > 0.02 {
            PlayerWeather::Rain
        } else if self.visibility_m < 1_000. || self.fog_density > 0.004 {
            PlayerWeather::Fog
        } else if self.cloud_cover > 0.5 {
            PlayerWeather::Overcast
        } else {
            PlayerWeather::Clear
        }
    }
    fn valid(self) -> bool {
        [
            self.rain,
            self.snow,
            self.snow_cover,
            self.cloud_cover,
            self.overcast,
            self.storm,
        ]
        .iter()
        .all(|value| (0.0..=1.0).contains(value))
            && (0.5..=2.).contains(&self.flake_size)
            && (1.0..=100_000.).contains(&self.visibility_m)
            && (0.0..=1.).contains(&self.fog_density)
            && self.wind_mps.is_finite()
            && self.wind_mps.length() <= 150.
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
    if profile == WeatherProfile::RandomJourney {
        return crate::weather_journey::atmosphere_at(weather, seed, seconds, default());
    }
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
            let (snow, cover, size) = match profile {
                WeatherProfile::LightSnow => (0.22, 0.25, 0.7),
                WeatherProfile::SteadySnow => (0.6, 0.6, 1.),
                _ => (1., 1., 1.35),
            };
            a.snow = snow;
            a.snow_cover = cover;
            a.flake_size = size;
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
    pub journey: crate::weather_journey::JourneyOptions,
    start_s: f64,
    last_s: Option<f64>,
    selection: Option<(
        PlayerWeather,
        WeatherProfile,
        bool,
        u32,
        crate::weather_journey::JourneyOptions,
    )>,
    transition_from: Atmosphere,
}
impl Default for WeatherState {
    fn default() -> Self {
        Self {
            atmosphere: default(),
            profile: default(),
            seed: 1,
            live: false,
            journey: default(),
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
    pub fn timeline_elapsed_s(&self) -> f64 {
        self.elapsed_s(self.last_s.unwrap_or(self.start_s))
    }
    #[cfg(test)]
    fn advance(
        &mut self,
        clock: f64,
        weather: PlayerWeather,
        profile: WeatherProfile,
        seed: u32,
        sample: Option<&WeatherSample>,
    ) {
        self.advance_journey(clock, weather, profile, seed, sample, self.journey);
    }
    fn advance_journey(
        &mut self,
        clock: f64,
        weather: PlayerWeather,
        profile: WeatherProfile,
        seed: u32,
        sample: Option<&WeatherSample>,
        journey: crate::weather_journey::JourneyOptions,
    ) {
        let first = self.last_s.is_none() || self.last_s.is_some_and(|last| clock < last);
        let previous = self.last_s.replace(clock).unwrap_or(clock);
        let selection = (weather, profile, sample.is_some(), seed, journey);
        if self.selection != Some(selection) || clock < previous {
            self.start_s = clock;
            self.selection = Some(selection);
            self.transition_from = self.atmosphere;
        }
        self.profile = profile;
        self.seed = seed;
        self.journey = journey;
        self.live = sample.is_some();
        let target = sample.map_or_else(
            || {
                if profile == WeatherProfile::RandomJourney {
                    crate::weather_journey::atmosphere_at(
                        weather,
                        seed,
                        self.elapsed_s(clock),
                        journey,
                    )
                } else {
                    fixed_at(weather, profile, seed, self.elapsed_s(clock))
                }
            },
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
    pub fn checkpoint(&self) -> Option<WeatherCheckpoint> {
        let (initial, profile, live, seed, journey) = self.selection?;
        if live {
            return None;
        }
        Some(WeatherCheckpoint {
            initial,
            profile,
            seed,
            journey,
            elapsed_s: self.elapsed_s(self.last_s?),
            atmosphere: self.atmosphere,
            transition_from: self.transition_from,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WeatherCheckpoint {
    pub initial: PlayerWeather,
    pub profile: WeatherProfile,
    pub seed: u32,
    pub journey: crate::weather_journey::JourneyOptions,
    elapsed_s: f64,
    atmosphere: Atmosphere,
    transition_from: Atmosphere,
}
impl WeatherCheckpoint {
    pub fn valid(&self) -> bool {
        self.elapsed_s.is_finite()
            && (0.0..=31_536_000.).contains(&self.elapsed_s)
            && self.atmosphere.valid()
            && self.transition_from.valid()
    }
    pub fn restore(&self, clock: f64) -> WeatherState {
        WeatherState {
            atmosphere: self.atmosphere,
            profile: self.profile,
            seed: self.seed,
            live: false,
            journey: self.journey,
            start_s: clock - self.elapsed_s,
            last_s: Some(clock),
            selection: Some((self.initial, self.profile, false, self.seed, self.journey)),
            transition_from: self.transition_from,
        }
    }
    pub fn apply_settings(&self, settings: &mut crate::player_settings::PlayerSettings) {
        settings.weather_profile = self.profile;
        settings.weather_seed = self.seed;
        settings.weather_pace = self.journey.pace;
    }
}
#[derive(Resource)]
pub struct PendingWeatherRestore(pub WeatherCheckpoint);

pub fn reset(
    mut commands: Commands,
    live: Option<Res<crate::live::LiveDrive>>,
    saved: Option<Res<PendingWeatherRestore>>,
    mut state: ResMut<WeatherState>,
    mut wet: ResMut<crate::wet_surfaces::WetSurfaces>,
) {
    *state = saved.as_ref().map_or_else(WeatherState::default, |saved| {
        saved
            .0
            .restore(live.as_ref().map_or(0., |drive| drive.session.time_s()))
    });
    commands.remove_resource::<PendingWeatherRestore>();
    wet.reset_clock();
}

pub fn update(
    live: Option<Res<crate::live::LiveDrive>>,
    time: Res<Time>,
    mut content: ResMut<crate::player_launch::ActivePlayerContent>,
    settings: Res<crate::player_settings::PlayerSettings>,
    environment: Res<crate::environment::LiveEnvironment>,
    mut state: ResMut<WeatherState>,
) {
    let clock = live
        .as_ref()
        .map_or(time.elapsed_secs_f64(), |l| l.session.time_s());
    let profile = WeatherProfile::from_env().unwrap_or(settings.weather_profile);
    let seed = std::env::var("OPENRAILSRS_WEATHER_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(settings.weather_seed);
    let offset = std::env::var("OPENRAILSRS_WEATHER_PHASE_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.)
        .unwrap_or(0.);
    let journey = crate::weather_journey::JourneyOptions {
        pace: std::env::var("OPENRAILSRS_WEATHER_PACE")
            .ok()
            .as_deref()
            .and_then(crate::weather_journey::WeatherPace::parse)
            .unwrap_or(settings.weather_pace),
        winter: live.as_ref().is_some_and(|drive| drive.season == "winter"),
    };
    let initial = content.environment.manual_weather;
    state.advance_journey(
        clock + offset,
        initial,
        profile,
        seed,
        environment.current_sample(content.environment),
        journey,
    );
    // The reproducible phase is independent of the start time of the service.
    if !state.live
        && state.last_s == Some(clock + offset)
        && state.elapsed_s(clock + offset) == 0.
        && offset > 0.
    {
        state.start_s -= offset;
        state.atmosphere = if profile == WeatherProfile::RandomJourney {
            crate::weather_journey::atmosphere_at(initial, seed, offset, journey)
        } else {
            fixed_at(initial, profile, seed, offset)
        };
    }
    let effective = if profile == WeatherProfile::RandomJourney && !state.live {
        state.atmosphere.weather()
    } else if !state.live {
        profile.weather().unwrap_or(initial)
    } else {
        content.weather
    };
    if content.weather != effective {
        content.weather = effective;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn random_forecast_reaches_rail_adhesion_and_survives_environment_ticks() {
        use crate::weather_journey::WeatherPace;
        use bevy::ecs::system::RunSystemOnce;
        let mut drive =
            crate::live::LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path())
                .unwrap();
        drive.paused = false;
        let mut app = App::new();
        app.insert_resource(drive)
            .init_resource::<Time>()
            .init_resource::<Time<Real>>()
            .init_resource::<Time<Fixed>>()
            .init_resource::<crate::environment::LiveEnvironment>()
            .init_resource::<crate::player_launch::ActivePlayerContent>()
            .init_resource::<WeatherState>()
            .insert_resource(crate::player_settings::PlayerSettings {
                weather_profile: WeatherProfile::RandomJourney,
                weather_pace: WeatherPace::Fast,
                weather_seed: 81,
                ..default()
            })
            .add_systems(
                Update,
                (
                    crate::environment::update,
                    update,
                    crate::live::advance_live_sim,
                )
                    .chain(),
            );
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..=145 {
            app.world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(std::time::Duration::from_secs(1));
            app.world_mut()
                .resource_mut::<Time<Fixed>>()
                .advance_by(std::time::Duration::from_secs(1));
            app.update();
            let state = app.world().resource::<WeatherState>();
            let content = app
                .world()
                .resource::<crate::player_launch::ActivePlayerContent>();
            assert_eq!(content.weather, state.atmosphere.weather());
            let drive = app.world().resource::<crate::live::LiveDrive>();
            let rail = drive.session.state.rail_adhesion.as_ref().unwrap();
            let expected = match content.weather {
                PlayerWeather::Rain => openrailsrs_sim::adhesion::RailWeather::Rain,
                PlayerWeather::Storm => openrailsrs_sim::adhesion::RailWeather::Storm,
                PlayerWeather::Snow => openrailsrs_sim::adhesion::RailWeather::Snow,
                PlayerWeather::Fog => openrailsrs_sim::adhesion::RailWeather::Fog,
                _ => openrailsrs_sim::adhesion::RailWeather::Dry,
            };
            assert_eq!(rail.weather, expected);
            assert!(
                (rail.weather_target_factor.unwrap() - state.atmosphere.rail_factor()).abs() < 1e-9
            );
            seen.insert(content.weather.label());
        }
        assert!(seen.len() >= 2);
        assert!(app.world().resource::<WeatherState>().timeline_elapsed_s() > 140.);
        app.world_mut().run_system_once(update).unwrap();
        app.world_mut()
            .resource_mut::<crate::live::LiveDrive>()
            .paused = true;
        let before =
            serde_json::to_string(&app.world().resource::<WeatherState>().atmosphere).unwrap();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(5));
        app.update();
        let paused =
            serde_json::to_string(&app.world().resource::<WeatherState>().atmosphere).unwrap();
        assert_eq!(before, paused);
        app.update();
        assert_eq!(
            paused,
            serde_json::to_string(&app.world().resource::<WeatherState>().atmosphere).unwrap()
        );
    }

    #[test]
    fn rail_intensity_and_dense_fog_blend_without_category_jumps() {
        for (weather, profiles) in [
            (
                PlayerWeather::Rain,
                [
                    WeatherProfile::Drizzle,
                    WeatherProfile::SteadyRain,
                    WeatherProfile::Downpour,
                ],
            ),
            (
                PlayerWeather::Snow,
                [
                    WeatherProfile::LightSnow,
                    WeatherProfile::SteadySnow,
                    WeatherProfile::HeavySnow,
                ],
            ),
        ] {
            let targets = profiles.map(|p| fixed_at(weather, p, 82, 0.).rail_factor());
            assert!(targets[0] > targets[1] && targets[1] > targets[2]);
            assert!((0.5..=1.).contains(&targets[2]));
        }
        let dry = Atmosphere::default();
        let fog = fixed_at(PlayerWeather::Fog, WeatherProfile::Automatic, 82, 0.);
        let mut previous = dry;
        for i in 1..=1000 {
            let a = dry.blend(fog, i as f32 / 1000.);
            assert!((a.rail_factor() - previous.rail_factor()).abs() < 0.001);
            assert!((a.dense_fog_fraction() - previous.dense_fog_fraction()).abs() < 0.002);
            previous = a;
        }
        assert_eq!(fog.dense_fog_fraction(), 1.);
        assert!((fog.rail_factor() - 0.6).abs() < 1e-9);
        // A stronger storm must not improve contact when only the HUD category changes.
        for i in 1..=100 {
            let a = fixed_at(
                PlayerWeather::Storm,
                WeatherProfile::StormCycle,
                82,
                120. + i as f64,
            );
            assert_eq!(
                a.rail_factor(),
                (1. - f64::from(a.rain) * 0.4).clamp(0.5, 1.)
            );
        }
    }
    #[test]
    fn random_weather_is_pause_safe_frame_independent_and_restores_its_forecast() {
        use crate::weather_journey::{JourneyOptions, WeatherPace};
        let options = JourneyOptions {
            pace: WeatherPace::Fast,
            winter: true,
        };
        let mut slow = WeatherState::default();
        let mut fast = WeatherState::default();
        for i in 0..=145 {
            slow.advance_journey(
                i as f64,
                PlayerWeather::Clear,
                WeatherProfile::RandomJourney,
                81,
                None,
                options,
            );
        }
        for i in 0..=145 * 60 {
            fast.advance_journey(
                i as f64 / 60.,
                PlayerWeather::Clear,
                WeatherProfile::RandomJourney,
                81,
                None,
                options,
            );
        }
        let atmosphere = |state: &WeatherState| serde_json::to_string(&state.atmosphere).unwrap();
        assert_eq!(atmosphere(&slow), atmosphere(&fast));
        let before = atmosphere(&fast);
        fast.advance_journey(
            145.,
            PlayerWeather::Clear,
            WeatherProfile::RandomJourney,
            81,
            None,
            options,
        );
        assert_eq!(before, atmosphere(&fast));
        let saved: WeatherCheckpoint =
            serde_json::from_str(&serde_json::to_string(&fast.checkpoint().unwrap()).unwrap())
                .unwrap();
        assert!(saved.valid());
        // Resuming at a different clock origin must continue the same keyframe.
        let mut restored = saved.restore(1000.);
        assert_eq!(before, atmosphere(&restored));
        for i in 1..=300 {
            fast.advance_journey(
                145. + i as f64,
                PlayerWeather::Clear,
                WeatherProfile::RandomJourney,
                81,
                None,
                options,
            );
            restored.advance_journey(
                1000. + i as f64,
                PlayerWeather::Clear,
                WeatherProfile::RandomJourney,
                81,
                None,
                options,
            );
            assert_eq!(atmosphere(&fast), atmosphere(&restored));
        }
    }
    #[test]
    fn precipitation_intensity_is_ordered_and_checkpoints_reject_invalid_atmosphere() {
        for (weather, profiles) in [
            (
                PlayerWeather::Rain,
                [
                    WeatherProfile::Drizzle,
                    WeatherProfile::SteadyRain,
                    WeatherProfile::Downpour,
                ],
            ),
            (
                PlayerWeather::Snow,
                [
                    WeatherProfile::LightSnow,
                    WeatherProfile::SteadySnow,
                    WeatherProfile::HeavySnow,
                ],
            ),
        ] {
            let frames = profiles.map(|profile| fixed_at(weather, profile, 81, 0.));
            for pair in frames.windows(2) {
                assert!(pair[0].rain.max(pair[0].snow) < pair[1].rain.max(pair[1].snow));
                assert!(pair[0].visibility_m > pair[1].visibility_m);
                assert!(pair[0].wind_mps.length() < pair[1].wind_mps.length());
            }
        }
        let mut state = WeatherState::default();
        state.advance(
            0.,
            PlayerWeather::Clear,
            WeatherProfile::RandomJourney,
            81,
            None,
        );
        let mut checkpoint = state.checkpoint().unwrap();
        assert!(checkpoint.valid());
        checkpoint.atmosphere.rain = 2.;
        assert!(!checkpoint.valid());
        checkpoint.atmosphere.rain = 0.;
        checkpoint.elapsed_s = f64::INFINITY;
        assert!(!checkpoint.valid());
    }
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
