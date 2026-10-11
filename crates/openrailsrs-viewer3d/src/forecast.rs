//! Forecast describes the selected source; querying it never mutates weather.
use crate::{
    environment::{EnvironmentSelection, LiveEnvironment},
    player_launch::PlayerWeather,
    weather_journey::{JourneyOptions, WeatherMode},
    weather_state::{Atmosphere, WeatherProfile},
};
fn description(a: Atmosphere) -> String {
    let name = if a.snow > 0.01 {
        "Nieve"
    } else if a.storm > 0.5 {
        "Tormenta"
    } else if a.rain > 0.01 {
        "Lluvia"
    } else if a.visibility_m < 2000. {
        "Niebla"
    } else if a.overcast > 0.35 {
        "Nublado"
    } else {
        "Despejado"
    };
    format!(
        "{name} · precipitación {:.0}% · viento {:.1} m/s",
        a.rain.max(a.snow) * 100.,
        a.wind_mps.length()
    )
}
pub fn text(
    selection: EnvironmentSelection,
    profile: WeatherProfile,
    initial: PlayerWeather,
    seed: u32,
    journey: JourneyOptions,
    elapsed: f64,
    environment: Option<&LiveEnvironment>,
) -> String {
    match WeatherMode::selected(selection, profile) {
        WeatherMode::Fixed => format!(
            "Pronóstico elegido: {}. Se mantiene durante el viaje.\nSin datos externos; no es un pronóstico meteorológico.",
            if matches!(initial, PlayerWeather::Rain | PlayerWeather::Snow) {
                format!("{} · {}", initial.label(), profile.intensity_label(initial))
            } else {
                initial.label().into()
            }
        ),
        WeatherMode::Random => {
            let mut out = format!(
                "Pronóstico del viaje · semilla {seed}. Programa reproducible, sin datos externos."
            );
            for offset in [
                0.,
                journey.pace.interval_s(),
                journey.pace.interval_s() * 2.,
                journey.pace.interval_s() * 3.,
            ] {
                out += &format!(
                    "\n+{:.0} min: {}",
                    offset / 60.,
                    description(crate::weather_journey::atmosphere_at(
                        initial,
                        seed,
                        elapsed + offset,
                        journey
                    ))
                );
            }
            out
        }
        WeatherMode::LocalNow => {
            let Some(environment) = environment else {
                return "Pronóstico del lugar: disponible al iniciar con conexión. Datos estimados de Open-Meteo.".into();
            };
            let Some(sample) = environment.current_sample(selection) else {
                return format!(
                    "Pronóstico del lugar pendiente o vencido. Respaldo elegido: {}.\n{}",
                    initial.label(),
                    environment.network_status
                );
            };
            let now = environment.utc.map_or(sample.time, |t| t.timestamp());
            let mut out = format!(
                "Open-Meteo · estimación del modelo · dato de hace {} min.\nLa probabilidad expresa incertidumbre; las condiciones reales pueden diferir.",
                (now - sample.time).max(0) / 60
            );
            for point in sample.forecast.iter().filter(|p| p.time >= now).take(5) {
                let date = chrono::DateTime::from_timestamp(point.time, 0).unwrap();
                let time = environment.timezone.map_or_else(
                    || date.format("%H:%M UTC").to_string(),
                    |z| date.with_timezone(&z).format("%H:%M").to_string(),
                );
                out += &format!(
                    "\n{time}: {} · {:.1} °C · precipitación {:.0}% · viento {:.1} m/s",
                    crate::environment::weather_for_code(point.weather_code)
                        .unwrap()
                        .label(),
                    point.temperature_c,
                    point.precipitation_probability,
                    point.wind_mps
                );
            }
            if sample.forecast.is_empty() {
                out += "\nSólo se recibió el dato actual; no hay pronóstico horario.";
            }
            out
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forecast_preserves_manual_selection_and_replays_seed() {
        let selection = EnvironmentSelection {
            manual_weather: PlayerWeather::Rain,
            ..Default::default()
        };
        let j = JourneyOptions::default();
        let fixed = text(
            selection,
            WeatherProfile::Downpour,
            PlayerWeather::Rain,
            1,
            j,
            0.,
            None,
        );
        assert!(fixed.contains("Se mantiene"));
        let random = text(
            selection,
            WeatherProfile::RandomJourney,
            PlayerWeather::Rain,
            81,
            j,
            0.,
            None,
        );
        assert_eq!(
            random,
            text(
                selection,
                WeatherProfile::RandomJourney,
                PlayerWeather::Rain,
                81,
                j,
                0.,
                None
            )
        );
        assert_ne!(
            random,
            text(
                selection,
                WeatherProfile::RandomJourney,
                PlayerWeather::Rain,
                82,
                j,
                0.,
                None
            )
        );
        assert_eq!(selection.manual_weather, PlayerWeather::Rain);
    }
}
