//! Seeded weather keyframes, evaluated in railway time rather than per frame.
use serde::{Deserialize, Serialize};

use crate::{
    environment::{EnvironmentSelection, EnvironmentSource},
    player_launch::PlayerWeather,
    weather_state::{Atmosphere, StormPhase, WeatherProfile, fixed_at},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherMode {
    Fixed,
    Random,
    LocalNow,
}
impl WeatherMode {
    pub const ALL: [Self; 3] = [Self::Fixed, Self::Random, Self::LocalNow];
    pub fn selected(environment: EnvironmentSelection, profile: WeatherProfile) -> Self {
        if environment.weather == EnvironmentSource::LocalNow {
            Self::LocalNow
        } else if profile == WeatherProfile::RandomJourney {
            Self::Random
        } else {
            Self::Fixed
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "Fijo · elegido por vos",
            Self::Random => "Aleatorio · cambia durante el viaje",
            Self::LocalNow => "Actual del lugar · con conexión",
        }
    }
    pub fn cycle(self, delta: i32) -> Self {
        let index = Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0);
        Self::ALL[crate::player_launch::cycle(index, Self::ALL.len(), delta)]
    }
    pub fn apply(self, environment: &mut EnvironmentSelection, profile: &mut WeatherProfile) {
        environment.weather = if self == Self::LocalNow {
            EnvironmentSource::LocalNow
        } else {
            EnvironmentSource::Manual
        };
        *profile = if self == Self::Random {
            WeatherProfile::RandomJourney
        } else {
            WeatherProfile::Automatic
        };
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherPace {
    Slow,
    #[default]
    Normal,
    Fast,
}
impl WeatherPace {
    pub const ALL: [Self; 3] = [Self::Slow, Self::Normal, Self::Fast];
    pub fn label(self) -> &'static str {
        match self {
            Self::Slow => "Lento · cada 10 minutos",
            Self::Normal => "Normal · cada 4 minutos",
            Self::Fast => "Rápido · cada minuto",
        }
    }
    pub fn interval_s(self) -> f64 {
        match self {
            Self::Slow => 600.,
            Self::Normal => 240.,
            Self::Fast => 60.,
        }
    }
    pub fn cycle(self, delta: i32) -> Self {
        let index = Self::ALL.iter().position(|pace| *pace == self).unwrap_or(1);
        Self::ALL[crate::player_launch::cycle(index, Self::ALL.len(), delta)]
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "slow" => Some(Self::Slow),
            "normal" => Some(Self::Normal),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyOptions {
    pub pace: WeatherPace,
    /// Snow is possible in winter, or when the player starts in snow.
    pub winter: bool,
}

fn target(initial: PlayerWeather, seed: u32, index: u32, winter: bool) -> Atmosphere {
    if index == 0 {
        return fixed_at(initial, WeatherProfile::Automatic, seed, 180.);
    }
    // Alternating dry/wet groups guarantee a change in each interval, while
    // independent seeded choices avoid a repeating storm loop. No global RNG.
    let dry = [
        PlayerWeather::Clear,
        PlayerWeather::Overcast,
        PlayerWeather::Fog,
    ];
    let wet = [
        WeatherProfile::Drizzle,
        WeatherProfile::SteadyRain,
        WeatherProfile::Downpour,
        WeatherProfile::StormCycle,
        WeatherProfile::LightSnow,
        WeatherProfile::SteadySnow,
        WeatherProfile::HeavySnow,
    ];
    let initial_wet = matches!(
        initial,
        PlayerWeather::Rain | PlayerWeather::Snow | PlayerWeather::Storm
    );
    let wet_frame = index.is_multiple_of(2) == initial_wet;
    let choice = crate::precipitation::rain_rng01(seed, index.wrapping_add(701));
    if wet_frame {
        let count = if winter || initial == PlayerWeather::Snow {
            wet.len()
        } else {
            4
        };
        let profile = wet[((choice * count as f32) as usize).min(count - 1)];
        fixed_at(initial, profile, seed.wrapping_add(index), 180.)
    } else {
        let weather = dry[((choice * dry.len() as f32) as usize).min(dry.len() - 1)];
        fixed_at(weather, WeatherProfile::Automatic, seed, 0.)
    }
}

pub fn atmosphere_at(
    initial: PlayerWeather,
    seed: u32,
    seconds: f64,
    options: JourneyOptions,
) -> Atmosphere {
    let seconds = seconds.max(0.);
    let interval = options.pace.interval_s();
    let index = (seconds / interval) as u32;
    let transition = (interval * 0.5).min(90.);
    let fraction =
        ((seconds.rem_euclid(interval) - interval + transition) / transition).clamp(0., 1.) as f32;
    let amount = fraction * fraction * (3. - 2. * fraction);
    let mut atmosphere = target(initial, seed, index, options.winter).blend(
        target(initial, seed, index.wrapping_add(1), options.winter),
        amount,
    );
    atmosphere.phase = if atmosphere.storm > 0.5 {
        StormPhase::Active
    } else if atmosphere.storm > 0.001 {
        if index % 2
            == u32::from(matches!(
                initial,
                PlayerWeather::Rain | PlayerWeather::Snow | PlayerWeather::Storm
            ))
        {
            StormPhase::Approaching
        } else {
            StormPhase::Clearing
        }
    } else {
        StormPhase::Calm
    };
    atmosphere
}

#[cfg(test)]
mod tests {
    use super::*;
    fn serialized(atmosphere: Atmosphere) -> String {
        serde_json::to_string(&atmosphere).unwrap()
    }
    #[test]
    fn starts_with_chosen_weather_and_changes_without_a_repeating_cycle() {
        let options = JourneyOptions {
            pace: WeatherPace::Fast,
            winter: false,
        };
        for initial in PlayerWeather::ALL {
            assert_eq!(
                serialized(atmosphere_at(initial, 81, 0., options)),
                serialized(fixed_at(initial, WeatherProfile::Automatic, 81, 180.))
            );
            let frames = (0..=12)
                .map(|i| atmosphere_at(initial, 81, i as f64 * 60., options))
                .collect::<Vec<_>>();
            for pair in frames.windows(2) {
                assert_ne!(serialized(pair[0]), serialized(pair[1]));
            }
            assert!((1..=12).any(|i| serialized(frames[i])
                != serialized(atmosphere_at(initial, 82, i as f64 * 60., options))));
        }
    }
    #[test]
    fn transitions_are_continuous_and_do_not_depend_on_sampling_or_camera() {
        for pace in WeatherPace::ALL {
            let options = JourneyOptions { pace, winter: true };
            for seed in 1..=16 {
                for key in 1..=12 {
                    let t = key as f64 * pace.interval_s();
                    let before = atmosphere_at(PlayerWeather::Clear, seed, t - 0.001, options);
                    let after = atmosphere_at(PlayerWeather::Clear, seed, t + 0.001, options);
                    assert!((before.rain - after.rain).abs() < 0.001);
                    assert!((before.snow - after.snow).abs() < 0.001);
                    assert!((before.visibility_m - after.visibility_m).abs() < 1.);
                    assert!((before.fog_density - after.fog_density).abs() < 0.001);
                    assert!(before.wind_mps.distance(after.wind_mps) < 0.001);
                    assert_eq!(
                        serialized(after),
                        serialized(atmosphere_at(
                            PlayerWeather::Clear,
                            seed,
                            t + 0.001,
                            options
                        ))
                    );
                }
            }
        }
    }
    #[test]
    fn snow_uses_the_selected_season_and_can_also_start_in_snow() {
        let mut found_snow = false;
        let options = JourneyOptions {
            pace: WeatherPace::Fast,
            winter: false,
        };
        for seed in 1..=12 {
            for index in 1..=24 {
                let t = index as f64 * 60.;
                assert_eq!(
                    atmosphere_at(PlayerWeather::Clear, seed, t, options).snow,
                    0.
                );
                found_snow |= atmosphere_at(
                    PlayerWeather::Clear,
                    seed,
                    t,
                    JourneyOptions {
                        winter: true,
                        ..options
                    },
                )
                .snow
                    > 0.;
            }
        }
        assert!(found_snow);
        assert!(atmosphere_at(PlayerWeather::Snow, 81, 0., options).snow > 0.);
    }
}
