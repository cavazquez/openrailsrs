//! Optional local wall clock and modelled current weather. Neither changes the
//! timetable/physics clock. HTTP, decoding and cache writes stay off the ECS thread.
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::Duration,
};

use bevy::prelude::*;
use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use openrailsrs_formats::geography::GeographicPosition;
use serde::{Deserialize, Serialize};

use crate::{
    player_launch::{ActivePlayerContent, PlayerWeather},
    player_settings::{atomic_write, player_data_dir},
    route_lighting::RouteSunState,
    shapes::RouteAssets,
};

const REFRESH_S: f64 = 600.0;
const MAX_WEATHER_AGE_S: i64 = 7200;
const MAX_BODY_BYTES: u64 = 65536;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentSource {
    #[default]
    Manual,
    LocalNow,
}
impl EnvironmentSource {
    pub fn next(self) -> Self {
        match self {
            Self::Manual => Self::LocalNow,
            Self::LocalNow => Self::Manual,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Elegido por el jugador",
            Self::LocalNow => "Actual del lugar",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvironmentSelection {
    pub time: EnvironmentSource,
    pub weather: EnvironmentSource,
    pub manual_weather: PlayerWeather,
}

/// Explicit metadata also lets synthetic/imported tracks opt in without guessing
/// their location from a small coordinate origin. Angles here are WGS84 degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteLocation {
    pub latitude: f64,
    pub longitude: f64,
}
#[derive(Deserialize)]
struct LocationFile {
    #[serde(flatten)]
    location: RouteLocation,
    #[serde(default)]
    timezone: Option<String>,
}
impl RouteLocation {
    fn valid(self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
    }
    pub fn geographic(self) -> GeographicPosition {
        GeographicPosition {
            latitude: self.latitude.to_radians(),
            longitude: self.longitude.to_radians(),
        }
    }
    fn cache_path(self) -> PathBuf {
        player_data_dir()
            .join("environment")
            .join(format!("{:.4}_{:.4}.json", self.latitude, self.longitude))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WeatherSample {
    pub time: i64,
    pub weather_code: u16,
    pub temperature_2m: f32,
    pub cloud_cover: f32,
    pub precipitation: f32,
    pub snowfall: f32,
    pub wind_speed_10m: f32,
    pub wind_direction_10m: f32,
    pub timezone: String,
}
impl WeatherSample {
    pub fn weather(&self) -> Option<PlayerWeather> {
        weather_for_code(self.weather_code)
    }
    fn fresh(&self, now: i64) -> bool {
        (-900..=MAX_WEATHER_AGE_S).contains(&(now.saturating_sub(self.time)))
    }
    fn valid(&self) -> bool {
        DateTime::from_timestamp(self.time, 0).is_some()
            && self.weather().is_some()
            && self.timezone.parse::<Tz>().is_ok()
            && self.temperature_2m.is_finite()
            && (-100.0..=65.0).contains(&self.temperature_2m)
            && (0.0..=100.0).contains(&self.cloud_cover)
            && (0.0..=1000.0).contains(&self.precipitation)
            && (0.0..=1000.0).contains(&self.snowfall)
            && (0.0..=150.0).contains(&self.wind_speed_10m)
            && (0.0..=360.0).contains(&self.wind_direction_10m)
    }
    pub fn wind(&self) -> Vec3 {
        // Meteorological bearing is where wind comes FROM; Bevy +Z is south.
        let bearing = self.wind_direction_10m.to_radians();
        Vec3::new(-bearing.sin(), 0.0, bearing.cos()) * self.wind_speed_10m.min(18.0)
    }
}

pub fn weather_for_code(code: u16) -> Option<PlayerWeather> {
    Some(match code {
        0..=1 => PlayerWeather::Clear,
        2..=3 => PlayerWeather::Overcast,
        45 | 48 => PlayerWeather::Fog,
        51 | 53 | 55 | 56 | 57 | 61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => PlayerWeather::Rain,
        71 | 73 | 75 | 77 | 85 | 86 => PlayerWeather::Snow,
        95 | 96 | 97 | 99 => PlayerWeather::Storm,
        _ => return None,
    })
}

#[derive(Deserialize)]
struct ProviderResponse {
    latitude: f64,
    longitude: f64,
    timezone: String,
    current: ProviderCurrent,
}
#[derive(Deserialize)]
struct ProviderCurrent {
    time: i64,
    weather_code: u16,
    temperature_2m: f32,
    cloud_cover: f32,
    precipitation: f32,
    snowfall: f32,
    wind_speed_10m: f32,
    wind_direction_10m: f32,
}
pub fn parse_weather(
    bytes: &[u8],
    location: RouteLocation,
    now: i64,
) -> Result<WeatherSample, String> {
    if bytes.len() > MAX_BODY_BYTES as usize || !location.valid() {
        return Err("Respuesta meteorológica demasiado grande o ubicación inválida".into());
    }
    let response: ProviderResponse = serde_json::from_slice(bytes)
        .map_err(|_| "Respuesta meteorológica incompleta".to_string())?;
    let grid = RouteLocation {
        latitude: response.latitude,
        longitude: response.longitude,
    };
    // Provider selects a nearby model grid cell, rather than the exact point.
    let longitude_distance = (grid.longitude - location.longitude)
        .abs()
        .min(360.0 - (grid.longitude - location.longitude).abs())
        * location.latitude.to_radians().cos().abs();
    if !grid.valid() || (grid.latitude - location.latitude).abs() > 1.0 || longitude_distance > 1.0
    {
        return Err("Datos meteorológicos de otra ubicación".into());
    }
    let c = response.current;
    let sample = WeatherSample {
        time: c.time,
        weather_code: c.weather_code,
        temperature_2m: c.temperature_2m,
        cloud_cover: c.cloud_cover,
        precipitation: c.precipitation,
        snowfall: c.snowfall,
        wind_speed_10m: c.wind_speed_10m,
        wind_direction_10m: c.wind_direction_10m,
        timezone: response.timezone,
    };
    if !sample.valid() || !sample.fresh(now) {
        return Err("Datos meteorológicos inválidos o vencidos".into());
    }
    Ok(sample)
}

#[derive(Serialize, Deserialize)]
struct CachedWeather {
    location: RouteLocation,
    sample: WeatherSample,
}
fn read_cache(location: RouteLocation, path: &Path) -> Option<WeatherSample> {
    if std::fs::metadata(path).ok()?.len() > MAX_BODY_BYTES {
        return None;
    }
    let cache: CachedWeather = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (cache.location == location && cache.sample.valid()).then_some(cache.sample)
}

fn fetch_weather(location: RouteLocation) -> Result<WeatherSample, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(10)))
        .timeout_resolve(Some(Duration::from_secs(3)))
        .timeout_connect(Some(Duration::from_secs(4)))
        .user_agent("openrailsrs/0.1 (optional local weather)")
        .build()
        .into();
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.5}&longitude={:.5}&current=temperature_2m,weather_code,cloud_cover,precipitation,snowfall,wind_speed_10m,wind_direction_10m&wind_speed_unit=ms&timeformat=unixtime&timezone=auto&forecast_days=1",
        location.latitude, location.longitude
    );
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("Open-Meteo: {e}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_vec()
        .map_err(|_| "No se pudo leer Open-Meteo (límite 64 KiB / 10 s)".to_string())?;
    let sample = parse_weather(&bytes, location, Utc::now().timestamp())?;
    if let Ok(bytes) = serde_json::to_vec(&CachedWeather {
        location,
        sample: sample.clone(),
    }) {
        let _ = atomic_write(&location.cache_path(), &bytes);
    }
    Ok(sample)
}

struct WeatherRequest {
    location: RouteLocation,
    rx: Mutex<mpsc::Receiver<Result<WeatherSample, String>>>,
}
#[derive(Resource, Default)]
pub struct LiveEnvironment {
    pub location: Option<RouteLocation>,
    pub timezone: Option<Tz>,
    pub sample: Option<WeatherSample>,
    pub network_status: String,
    pub utc: Option<DateTime<Utc>>,
    request: Option<WeatherRequest>,
    next_request_s: f64,
    next_tick_s: f64,
    failures: u32,
    last_selection: Option<EnvironmentSelection>,
}
impl LiveEnvironment {
    pub fn solar_utc(&self, selection: EnvironmentSelection) -> Option<DateTime<Utc>> {
        if selection.time != EnvironmentSource::LocalNow || self.location.is_none() {
            return None;
        }
        self.utc
    }
    pub fn real_clock(&self, selection: EnvironmentSelection) -> Option<DateTime<Tz>> {
        if selection.time != EnvironmentSource::LocalNow {
            return None;
        }
        Some(self.utc?.with_timezone(&self.timezone?))
    }
    pub fn current_sample(&self, selection: EnvironmentSelection) -> Option<&WeatherSample> {
        let now = self.utc?.timestamp();
        self.sample
            .as_ref()
            .filter(|s| selection.weather == EnvironmentSource::LocalNow && s.fresh(now))
    }
    pub fn hud_text(&self, content: &ActivePlayerContent, manual_clock: f64) -> String {
        let time = self.real_clock(content.environment).map_or_else(
            || {
                format!(
                    "Hora elegida: {}{}",
                    crate::player_ui::clock(manual_clock),
                    if content.environment.time == EnvironmentSource::LocalNow {
                        " · hora real pendiente"
                    } else {
                        ""
                    }
                )
            },
            |local| {
                format!(
                    "Hora real: {} · {}",
                    local.format("%d/%m/%Y %H:%M:%S"),
                    local.timezone()
                )
            },
        );
        let mut out = format!(
            "{time}\nClima: {} · {}",
            content.weather.label(),
            content.environment.weather.label()
        );
        if let Some(sample) = self.current_sample(content.environment) {
            out += &format!(
                "\nOpen-Meteo · CC BY 4.0 · open-meteo.com\n{:.1} °C · nubes {:.0}% · viento {:.1} m/s\nDato: {} UTC · antigüedad {} min",
                sample.temperature_2m,
                sample.cloud_cover,
                sample.wind_speed_10m,
                DateTime::from_timestamp(sample.time, 0)
                    .unwrap()
                    .format("%H:%M"),
                (self.utc.unwrap().timestamp() - sample.time).max(0) / 60
            );
        }
        if !self.network_status.is_empty() {
            out += &format!("\n{}", self.network_status);
        }
        if let Some(p) = self.location {
            out += &format!("\nRuta: {:.4}°, {:.4}°", p.latitude, p.longitude);
        }
        out
    }
}

/// Only native WORLD coordinates or explicit metadata establish a route location.
pub(crate) fn location_for_route(
    assets: &RouteAssets,
    position: Option<GeographicPosition>,
) -> (Option<RouteLocation>, Option<Tz>) {
    let explicit = assets.route_dir.join("route-location.json");
    let metadata = std::fs::metadata(&explicit)
        .ok()
        .filter(|m| m.len() <= 4096)
        .and_then(|_| std::fs::read(&explicit).ok())
        .and_then(|bytes| serde_json::from_slice::<LocationFile>(&bytes).ok())
        .filter(|m| m.location.valid());
    let explicit_timezone = metadata
        .as_ref()
        .and_then(|m| m.timezone.as_ref())
        .and_then(|name| name.parse::<Tz>().ok());
    let location = metadata
        .map(|m| m.location)
        .filter(|p| p.valid())
        .or_else(|| {
            assets
                .route_dir
                .join("WORLD")
                .is_dir()
                .then(|| {
                    position.map(|s| RouteLocation {
                        latitude: s.latitude.to_degrees(),
                        longitude: s.longitude.to_degrees(),
                    })
                })
                .flatten()
        });
    (location, explicit_timezone)
}

pub fn init(
    assets: Res<RouteAssets>,
    sun: Option<Res<RouteSunState>>,
    mut environment: ResMut<LiveEnvironment>,
) {
    let (location, explicit_timezone) =
        location_for_route(&assets, sun.as_ref().map(|s| s.position));
    let location = location.map(|p| RouteLocation {
        latitude: (p.latitude * 10000.0).round() / 10000.0,
        longitude: (p.longitude * 10000.0).round() / 10000.0,
    });
    environment.location = location;
    environment.sample = None;
    environment.timezone = explicit_timezone;
    environment.next_request_s = 0.0;
    environment.next_tick_s = 0.0;
    environment.failures = 0;
    environment.last_selection = None;
    if let Some(location) = location
        && let Some(sample) = read_cache(location, &location.cache_path())
    {
        environment.timezone = sample.timezone.parse().ok();
        environment.sample = Some(sample);
        environment.network_status = "Último dato guardado; comprobando conexión".into();
    }
}

#[derive(Component)]
pub struct EnvironmentBadge;
pub fn update_badge(
    environment: Res<LiveEnvironment>,
    content: Res<ActivePlayerContent>,
    mut badges: Query<(&mut Text, &mut Visibility), With<EnvironmentBadge>>,
) {
    for (mut text, mut visibility) in &mut badges {
        let selection = content.environment;
        *visibility = if selection.time == EnvironmentSource::Manual
            && selection.weather == EnvironmentSource::Manual
        {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        let clock = environment.real_clock(selection).map_or_else(
            || {
                if selection.time == EnvironmentSource::LocalNow {
                    "Hora real pendiente".to_string()
                } else {
                    "Hora manual".to_string()
                }
            },
            |t| format!("{} · {}", t.format("%H:%M:%S"), t.timezone()),
        );
        let source = if selection.weather == EnvironmentSource::LocalNow {
            if environment.current_sample(selection).is_some() {
                "Open-Meteo"
            } else {
                "respaldo manual"
            }
        } else {
            "manual"
        };
        let status = if environment.network_status.starts_with("Sin ")
            || environment.network_status.starts_with("Dato ")
            || environment.network_status.starts_with("Ruta sin")
        {
            " · sin actualización (F8)"
        } else if environment.request.is_some() {
            " · consultando…"
        } else {
            ""
        };
        **text = format!("{clock} · {} ({source}){status}", content.weather.label());
    }
}

pub fn update(
    time: Res<Time<Real>>,
    mut environment: ResMut<LiveEnvironment>,
    mut content: ResMut<ActivePlayerContent>,
) {
    let elapsed = time.elapsed_secs_f64();
    let selection = content.environment;
    let changed = environment.last_selection != Some(selection);
    if !changed && elapsed < environment.next_tick_s {
        return;
    }
    environment.next_tick_s = elapsed + 0.25;
    environment.utc = Some(Utc::now());
    if changed {
        environment.last_selection = Some(selection);
    }
    let needed = selection.weather == EnvironmentSource::LocalNow
        || (selection.time == EnvironmentSource::LocalNow && environment.timezone.is_none());
    let completed = environment.request.as_ref().and_then(|request| {
        match request.rx.lock().unwrap().try_recv() {
            Ok(result) => Some((request.location, result)),
            Err(mpsc::TryRecvError::Disconnected) => Some((
                request.location,
                Err("Consulta meteorológica interrumpida".into()),
            )),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    });
    if let Some((location, result)) = completed {
        environment.request = None;
        if environment.location == Some(location) && needed {
            match result {
                Ok(sample) => {
                    environment.timezone = sample.timezone.parse().ok();
                    environment.sample = Some(sample);
                    environment.failures = 0;
                    environment.network_status =
                        "Open-Meteo · condiciones estimadas · actualización cada 10 min".into();
                    environment.next_request_s = elapsed + REFRESH_S;
                }
                Err(error) => {
                    environment.failures = (environment.failures + 1).min(4);
                    environment.next_request_s =
                        elapsed + (30.0 * 2_f64.powi(environment.failures as i32)).min(REFRESH_S);
                    environment.network_status =
                        format!("Sin actualización: {error}. Reintentando en segundo plano");
                }
            }
        }
    }
    if needed && environment.location.is_none() {
        environment.network_status =
            "Ruta sin ubicación geográfica: se mantiene la selección manual".into();
    } else if needed && environment.request.is_none() && elapsed >= environment.next_request_s {
        let location = environment.location.unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("local-weather".into())
            .spawn(move || {
                let _ = tx.send(fetch_weather(location));
            }) {
            Ok(_) => {
                environment.request = Some(WeatherRequest {
                    location,
                    rx: Mutex::new(rx),
                });
                environment.network_status = "Consultando Open-Meteo…".into();
            }
            Err(_) => {
                environment.network_status =
                    "No se pudo iniciar la consulta; se mantiene el clima manual".into();
                environment.next_request_s = elapsed + REFRESH_S;
            }
        }
    } else if selection.time == EnvironmentSource::Manual
        && selection.weather == EnvironmentSource::Manual
    {
        environment.network_status.clear();
    }
    let weather = environment
        .current_sample(selection)
        .and_then(WeatherSample::weather)
        .unwrap_or(selection.manual_weather);
    if selection.weather == EnvironmentSource::LocalNow
        && environment
            .sample
            .as_ref()
            .is_some_and(|s| !s.fresh(environment.utc.unwrap().timestamp()))
    {
        environment.network_status =
            "Dato meteorológico vencido (>2 h): clima manual; reintentando conexión".into();
    }
    if selection.time == EnvironmentSource::LocalNow
        && selection.weather == EnvironmentSource::Manual
        && let Some(zone) = environment.timezone
    {
        environment.network_status = format!("Zona horaria: {zone} · clima elegido por el jugador");
    }
    if content.weather != weather {
        content.weather = weather;
    }
}

/// NOAA solar equation with UTC and the actual date. Local civil time (including
/// DST) is for display; route .env's representative seasonal sunrise is manual only.
pub fn real_solar_direction(position: GeographicPosition, utc: DateTime<Utc>) -> Vec3 {
    let seconds = f64::from(utc.num_seconds_from_midnight());
    let year_days = if utc.date_naive().leap_year() {
        366.0
    } else {
        365.0
    };
    let gamma = std::f64::consts::TAU / year_days
        * (f64::from(utc.ordinal()) - 1.0 + seconds / 86400.0 - 0.5);
    let decl = 0.006918 - 0.399912 * gamma.cos() + 0.070257 * gamma.sin()
        - 0.006758 * (2.0 * gamma).cos()
        + 0.000907 * (2.0 * gamma).sin()
        - 0.002697 * (3.0 * gamma).cos()
        + 0.001480 * (3.0 * gamma).sin();
    let equation = 229.18
        * (0.000075 + 0.001868 * gamma.cos()
            - 0.032077 * gamma.sin()
            - 0.014615 * (2.0 * gamma).cos()
            - 0.040849 * (2.0 * gamma).sin());
    let hour = ((seconds / 60.0 + equation + 4.0 * position.longitude.to_degrees()) / 4.0 - 180.0)
        .to_radians();
    let (lat_s, lat_c) = position.latitude.sin_cos();
    let (decl_s, decl_c) = decl.sin_cos();
    Vec3::new(
        (-hour.sin() * decl_c) as f32,
        (lat_s * decl_s + lat_c * decl_c * hour.cos()) as f32,
        (lat_s * decl_c * hour.cos() - lat_c * decl_s) as f32,
    )
    .normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    fn location() -> RouteLocation {
        RouteLocation {
            latitude: 51.55,
            longitude: -0.37,
        }
    }
    fn response() -> serde_json::Value {
        serde_json::json!({"latitude":51.55,"longitude":-0.375,"timezone":"Europe/London",
            "current":{"time":1791100800i64,"weather_code":95,"temperature_2m":12.3,"cloud_cover":98,
            "precipitation":2.5,"snowfall":0,"wind_speed_10m":4,"wind_direction_10m":270}})
    }
    #[test]
    fn native_codes_include_snow_fog_and_thunder_without_guessing_unknown_codes() {
        for code in [71, 73, 75, 77, 85, 86] {
            assert_eq!(weather_for_code(code), Some(PlayerWeather::Snow));
        }
        for code in [95, 96, 97, 99] {
            assert_eq!(weather_for_code(code), Some(PlayerWeather::Storm));
        }
        assert_eq!(weather_for_code(48), Some(PlayerWeather::Fog));
        assert_eq!(weather_for_code(3), Some(PlayerWeather::Overcast));
        assert_eq!(weather_for_code(42), None);
    }
    #[test]
    fn rejects_expired_foreign_incomplete_and_oversized_weather() {
        let now = 1791100900;
        let parse =
            |r: &serde_json::Value| parse_weather(&serde_json::to_vec(r).unwrap(), location(), now);
        assert_eq!(
            parse(&response()).unwrap().weather(),
            Some(PlayerWeather::Storm)
        );
        for (key, value) in [
            ("weather_code", serde_json::json!(42)),
            ("cloud_cover", serde_json::json!(101)),
            ("time", serde_json::json!(now - MAX_WEATHER_AGE_S - 1)),
            ("time", serde_json::json!(now + 901)),
        ] {
            let mut r = response();
            r["current"][key] = value;
            assert!(parse(&r).is_err());
        }
        let mut r = response();
        r["timezone"] = "Unknown/Zone".into();
        assert!(parse(&r).is_err());
        r = response();
        r["latitude"] = (-34.6).into();
        assert!(parse(&r).is_err());
        assert!(parse_weather(&vec![b' '; 65537], location(), now).is_err());
        assert!(parse(&serde_json::json!({"error":true})).is_err());
    }
    #[test]
    fn local_clock_follows_london_dst_and_argentina_date_rollover() {
        let summer = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let winter = Utc.with_ymd_and_hms(2026, 12, 1, 12, 0, 0).unwrap();
        assert_eq!(summer.with_timezone(&chrono_tz::Europe::London).hour(), 13);
        assert_eq!(winter.with_timezone(&chrono_tz::Europe::London).hour(), 12);
        let utc = Utc.with_ymd_and_hms(2026, 10, 5, 1, 0, 0).unwrap();
        let arg = utc.with_timezone(&chrono_tz::America::Argentina::Buenos_Aires);
        assert_eq!((arg.day(), arg.hour()), (4, 22));
        let before = Utc.with_ymd_and_hms(2026, 10, 25, 0, 59, 59).unwrap();
        let after = before + chrono::Duration::seconds(2);
        assert!(
            real_solar_direction(location().geographic(), before)
                .distance(real_solar_direction(location().geographic(), after))
                < 0.001
        );
    }
    #[test]
    fn real_sun_uses_utc_longitude_in_both_hemispheres() {
        let argentina = RouteLocation {
            latitude: -34.6,
            longitude: -58.4,
        }
        .geographic();
        let noon = Utc.with_ymd_and_hms(2026, 12, 21, 16, 0, 0).unwrap();
        assert!(real_solar_direction(argentina, noon).y > 0.9);
        assert!(real_solar_direction(argentina, noon + chrono::Duration::hours(12)).y < 0.0);
        assert!(real_solar_direction(location().geographic(), noon).is_finite());
    }
    #[test]
    fn independent_manual_selection_keeps_cached_data_from_overwriting_player() {
        let sample = parse_weather(
            &serde_json::to_vec(&response()).unwrap(),
            location(),
            1791100900,
        )
        .unwrap();
        let mut environment = LiveEnvironment {
            sample: Some(sample),
            utc: DateTime::from_timestamp(1791100900, 0),
            timezone: Some(chrono_tz::Europe::London),
            ..default()
        };
        let mut selection = EnvironmentSelection {
            time: EnvironmentSource::LocalNow,
            ..default()
        };
        assert!(environment.real_clock(selection).is_some());
        assert!(environment.current_sample(selection).is_none());
        selection.time = EnvironmentSource::Manual;
        selection.weather = EnvironmentSource::LocalNow;
        assert!(environment.real_clock(selection).is_none());
        assert!(environment.current_sample(selection).is_some());
        environment.utc = DateTime::from_timestamp(1791100800 + MAX_WEATHER_AGE_S + 1, 0);
        assert!(environment.current_sample(selection).is_none());
    }
    #[test]
    fn cache_is_bound_to_route_and_old_cache_still_resolves_timezone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let sample = parse_weather(
            &serde_json::to_vec(&response()).unwrap(),
            location(),
            1791100900,
        )
        .unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&CachedWeather {
                location: location(),
                sample,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(
            read_cache(
                RouteLocation {
                    latitude: 35.,
                    longitude: 139.
                },
                &path
            )
            .is_none()
        );
        let cached = read_cache(location(), &path).unwrap();
        assert!(cached.timezone.parse::<Tz>().is_ok());
        assert!(!cached.fresh(1791200900));
    }
    #[test]
    fn ecs_manual_mode_never_requests_weather_and_ignores_late_results() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut value = response();
        let now = Utc::now().timestamp();
        value["current"]["time"] = now.into();
        let sample = parse_weather(&serde_json::to_vec(&value).unwrap(), location(), now).unwrap();
        tx.send(Ok(sample)).unwrap();
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .insert_resource(LiveEnvironment {
                location: Some(location()),
                request: Some(WeatherRequest {
                    location: location(),
                    rx: Mutex::new(rx),
                }),
                ..default()
            })
            .insert_resource(ActivePlayerContent {
                environment: EnvironmentSelection {
                    manual_weather: PlayerWeather::Snow,
                    ..default()
                },
                ..default()
            })
            .add_systems(Update, update);
        app.update();
        let env = app.world().resource::<LiveEnvironment>();
        assert!(env.request.is_none());
        assert!(env.sample.is_none());
        assert!(env.network_status.is_empty());
        assert_eq!(
            app.world().resource::<ActivePlayerContent>().weather,
            PlayerWeather::Snow
        );
    }
    #[test]
    fn offline_update_keeps_valid_weather_and_cached_timezone_then_expires_weather() {
        let now = Utc::now().timestamp();
        let mut value = response();
        value["current"]["time"] = (now - 600).into();
        let sample = parse_weather(&serde_json::to_vec(&value).unwrap(), location(), now).unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Err("conexión de prueba caída".into())).unwrap();
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .insert_resource(LiveEnvironment {
                location: Some(location()),
                timezone: Some(chrono_tz::Europe::London),
                sample: Some(sample),
                request: Some(WeatherRequest {
                    location: location(),
                    rx: Mutex::new(rx),
                }),
                ..default()
            })
            .insert_resource(ActivePlayerContent {
                environment: EnvironmentSelection {
                    time: EnvironmentSource::LocalNow,
                    weather: EnvironmentSource::LocalNow,
                    manual_weather: PlayerWeather::Clear,
                },
                ..default()
            })
            .add_systems(Update, update);
        app.update();
        assert_eq!(
            app.world().resource::<ActivePlayerContent>().weather,
            PlayerWeather::Storm
        );
        let env = app.world().resource::<LiveEnvironment>();
        assert!(env.network_status.starts_with("Sin actualización"));
        assert!(
            env.real_clock(app.world().resource::<ActivePlayerContent>().environment)
                .is_some()
        );
        assert_eq!(env.next_request_s, 60.0);
        {
            let mut env = app.world_mut().resource_mut::<LiveEnvironment>();
            env.sample.as_mut().unwrap().time = now - MAX_WEATHER_AGE_S - 60;
            env.next_tick_s = 0.;
        }
        app.update();
        assert_eq!(
            app.world().resource::<ActivePlayerContent>().weather,
            PlayerWeather::Clear
        );
        assert!(
            app.world()
                .resource::<LiveEnvironment>()
                .network_status
                .contains("vencido")
        );
    }
    #[test]
    #[ignore = "optional public provider integration; normal tests never need the network"]
    fn public_provider_returns_timestamp_and_iana_timezone() {
        let sample = fetch_weather(location()).unwrap();
        assert_eq!(sample.timezone, "Europe/London");
        assert!(sample.valid());
    }
}
