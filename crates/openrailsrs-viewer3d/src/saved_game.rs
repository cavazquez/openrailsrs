//! Atomic player saves, compatible content checks and camera restoration.
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraFollowMode, DriverLookOffset, OrbitState};
use crate::floating_origin::FloatingOrigin;
use crate::live::LiveDrive;
use crate::player_launch::{ActivePlayerContent, PlayerWeather, QueuedPlayerLaunch};
use crate::player_settings::{atomic_write, player_data_dir};

fn validate_pending_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err("Identificador de servicio inválido".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedCamera {
    pub follow: String,
    pub focus: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub look_yaw: f32,
    pub look_pitch: f32,
    #[serde(default)]
    pub pose: Option<SavedCameraPose>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedCameraPose {
    pub fly: bool,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub fly_yaw: f32,
    pub fly_pitch: f32,
    pub driver_eyepoint: usize,
    pub cab2d_view: usize,
    pub passenger_car_slot: usize,
    pub passenger_view: usize,
    pub passenger_car: usize,
    pub passenger_head: [f32; 3],
    pub passenger_look: [f32; 4],
}
impl SavedCamera {
    pub fn capture(
        follow: CameraFollowMode,
        orbit: &OrbitState,
        look: &DriverLookOffset,
        origin: &FloatingOrigin,
    ) -> Self {
        Self {
            follow: format!("{follow:?}"),
            focus: (orbit.focus + origin.shift).to_array(),
            yaw: orbit.yaw,
            pitch: orbit.pitch,
            distance: orbit.distance,
            look_yaw: look.yaw,
            look_pitch: look.pitch,
            pose: None,
        }
    }
    pub fn follow_mode(&self) -> CameraFollowMode {
        match self.follow.as_str() {
            "DriverCam" => CameraFollowMode::DriverCam,
            "Cab2d" => CameraFollowMode::Cab2d,
            "ChaseCam" => CameraFollowMode::ChaseCam,
            "OrbitFollow" => CameraFollowMode::OrbitFollow,
            "PassengerCam" => CameraFollowMode::PassengerCam,
            _ => CameraFollowMode::Off,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedGame {
    #[serde(default)]
    pub control_history: Vec<String>,
    #[serde(default)]
    pub pending_control: Option<String>,
    #[serde(default)]
    pub pending_coupling: Option<String>,
    #[serde(default)]
    pub independent_couplings: Vec<openrailsrs_sim::independent_coupling::CouplingReceipt>,
    #[serde(default)]
    pub controlled_service: Option<String>,
    pub version: u32,
    #[serde(default)]
    pub created_utc: Option<i64>,
    #[serde(default)]
    pub train_name: String,
    #[serde(default)]
    pub player_options: Option<SavedPlayerOptions>,
    pub scenario_toml: String,
    pub route_root: Option<PathBuf>,
    pub description: String,
    pub session: openrailsrs_sim::SessionSnapshot,
    #[serde(default)]
    pub traffic: Vec<openrailsrs_sim::TrafficSnapshot>,
    pub start_clock_s: f64,
    pub season: String,
    pub weather: PlayerWeather,
    #[serde(default)]
    pub environment: Option<crate::environment::EnvironmentSelection>,
    #[serde(default)]
    pub weather_checkpoint: Option<crate::weather_state::WeatherCheckpoint>,
    pub camera: SavedCamera,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedPlayerOptions {
    #[serde(default)]
    pub journey_setup: crate::journey_setup::JourneySetup,
    #[serde(default)]
    pub braking_coach: bool,
    #[serde(default)]
    pub weather_track: openrailsrs_scenarios::weather_track::WeatherTrack,
    pub quick_stations: bool,
    pub strict_service: bool,
    pub traffic_brake_assistance: bool,
    pub ambient_temperature_c: Option<f32>,
    #[serde(default)]
    pub sky_date: Option<String>,
    #[serde(default)]
    pub shared_installations: Vec<PathBuf>,
}
impl SavedPlayerOptions {
    pub fn capture(s: &crate::player_settings::PlayerSettings) -> Self {
        Self {
            journey_setup: s.journey_setup.clone(),
            braking_coach: s.braking_coach,
            weather_track: s.weather_track.clone(),
            quick_stations: s.quick_station_practice,
            strict_service: s.strict_service,
            traffic_brake_assistance: s.traffic_brake_assistance,
            ambient_temperature_c: s.ambient_temperature_c,
            sky_date: Some(s.sky_date.clone()),
            shared_installations: s.shared_installations.clone(),
        }
    }
    pub fn apply(&self, s: &mut crate::player_settings::PlayerSettings) {
        s.shared_installations = self.shared_installations.clone();
        s.journey_setup = self.journey_setup.clone();
        s.braking_coach = self.braking_coach;
        s.weather_track = self.weather_track.clone();
        s.quick_station_practice = self.quick_stations;
        s.strict_service = self.strict_service;
        s.traffic_brake_assistance = self.traffic_brake_assistance;
        s.ambient_temperature_c = self.ambient_temperature_c;
        if let Some(date) = &self.sky_date {
            s.sky_date = date.clone();
        }
    }
}
impl SavedGame {
    fn encoded(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Partida demasiado grande para guardar".into());
        }
        Ok(bytes)
    }
    pub fn save(
        live: &mut LiveDrive,
        content: &ActivePlayerContent,
        camera: SavedCamera,
        path: &Path,
        weather: Option<&crate::weather_state::WeatherState>,
        settings: Option<&crate::player_settings::PlayerSettings>,
    ) -> Result<(), String> {
        let game = Self {
            control_history: live.traffic.control_history.clone(),
            pending_control: None,
            pending_coupling: None,
            independent_couplings: live.traffic.couplings.clone(),
            controlled_service: (live.session.service_id != "Jugador")
                .then(|| live.session.service_id.clone()),
            version: 1,
            created_utc: Some(chrono::Utc::now().timestamp()),
            train_name: live
                .session
                .formation
                .cars
                .iter()
                .map(|c| c.name.as_str())
                .take(3)
                .collect::<Vec<_>>()
                .join(" / "),
            player_options: settings.map(SavedPlayerOptions::capture),
            scenario_toml: canonical_scenario(live.scenario_path())?,
            route_root: content.route_root.clone(),
            description: content.description.clone(),
            session: live.session.snapshot_with_scripts()?,
            traffic: live.traffic.snapshot_with_scripts()?,
            start_clock_s: live.start_clock_s,
            season: live.season.clone(),
            weather: content.weather,
            environment: Some(content.environment),
            weather_checkpoint: weather.and_then(crate::weather_state::WeatherState::checkpoint),
            camera,
        };
        let bytes = game.encoded()?;
        atomic_write(path, &bytes)?;
        // A replaced slot remains available in history. Private application
        // archives are independent of downloaded route/vehicle content.
        if path.parent() == Some(player_data_dir().as_path()) {
            let history = player_data_dir().join("history");
            let id = format!(
                "{}.json",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            );
            atomic_write(&history.join(id), &bytes)?;
        }
        Ok(())
    }
    /// Queue an operation on a disposable reconstructed world. Failure to
    /// write/load leaves the currently displayed formation untouched.
    pub fn queue_coupling(path: &Path, id: &str) -> Result<(), String> {
        validate_pending_id(id)?;
        let mut game = Self::read(path)?;
        game.pending_coupling = Some(id.into());
        atomic_write(path, &game.encoded()?)
    }
    pub fn queue_control(path: &Path, id: &str) -> Result<(), String> {
        validate_pending_id(id)?;
        let mut game = Self::read(path)?;
        game.pending_control = Some(id.into());
        atomic_write(path, &game.encoded()?)
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("Partida demasiado grande".into());
        }
        Self::from_bytes(&std::fs::read(path).map_err(|e| e.to_string())?)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Partida demasiado grande".into());
        }
        let game: Self = serde_json::from_slice(bytes)
            .map_err(|e| format!("No se puede leer la partida: {e}"))?;
        if game.control_history.len() > 128
            || game
                .control_history
                .iter()
                .chain(game.pending_control.iter())
                .any(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
        {
            return Err("Historial de conductores inválido".into());
        }
        if !game.control_history.is_empty() {
            let last = game.control_history.last().unwrap();
            let controlled = game.controlled_service.as_deref().unwrap_or("Jugador");
            if last != controlled {
                return Err(
                    "El historial de conductores no coincide con el servicio guardado".into(),
                );
            }
        }
        if game
            .controlled_service
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
        {
            return Err("Servicio controlado inválido".into());
        }
        if game
            .pending_coupling
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256 || id.chars().any(char::is_control))
        {
            return Err("Formación a recoger inválida".into());
        }
        if game.player_options.as_ref().is_some_and(|o| {
            !o.journey_setup.valid()
                || !o.weather_track.valid()
                || o.sky_date.as_ref().is_some_and(|d| {
                    d.parse::<chrono::NaiveDate>()
                        .ok()
                        .is_none_or(|d| !(1900..=2100).contains(&chrono::Datelike::year(&d)))
                })
                || o.ambient_temperature_c
                    .is_some_and(|t| !t.is_finite() || !(-45.0..=45.0).contains(&t))
        }) || game.version != 1
            || game
                .weather_checkpoint
                .as_ref()
                .is_some_and(|weather| !weather.valid())
            || !game.start_clock_s.is_finite()
            || !game
                .camera
                .focus
                .iter()
                .chain([
                    &game.camera.yaw,
                    &game.camera.pitch,
                    &game.camera.distance,
                    &game.camera.look_yaw,
                    &game.camera.look_pitch,
                ])
                .all(|v| v.is_finite())
            || !(1.0..=10000.0).contains(&game.camera.distance)
            || game.camera.pose.as_ref().is_some_and(|p| {
                !p.position
                    .iter()
                    .chain(p.rotation.iter())
                    .chain(p.passenger_head.iter())
                    .chain(p.passenger_look.iter())
                    .chain([&p.fly_yaw, &p.fly_pitch])
                    .all(|v| v.is_finite())
                    || Quat::from_array(p.rotation).length_squared() < 0.9
            })
        {
            return Err("Versión, cámara o clima de partida inválidos".into());
        }
        Ok(game)
    }
    pub(crate) fn validate_route_available(&self) -> Result<(), String> {
        if self.route_root.as_ref().is_some_and(|root| !root.is_dir()) {
            return Err("La carpeta de la ruta guardada ya no está disponible; reinstalá el contenido original en su ubicación antes de continuar".into());
        }
        Ok(())
    }
    pub fn prepare_resume(path: &Path) -> Result<QueuedPlayerLaunch, String> {
        let game = Self::read(path)?;
        game.validate_route_available()?;
        if let Some(options) = &game.player_options {
            openrailsrs_formats::resource_links::configure(&options.shared_installations)?;
        }
        let scenario = crate::player_launch::absolute(&player_data_dir().join("resume.toml"));
        atomic_write(&scenario, game.scenario_toml.as_bytes())?;
        Ok(QueuedPlayerLaunch {
            path: scenario,
            route_root: game.route_root,
            weather: game.weather,
            environment: game
                .environment
                .unwrap_or(crate::environment::EnvironmentSelection {
                    manual_weather: game.weather,
                    ..default()
                }),
            resume: Some(path.to_path_buf()),
            weather_checkpoint: game.weather_checkpoint,
            player_options: game.player_options,
        })
    }
    pub fn restore(self, live: &mut LiveDrive) -> Result<SavedCamera, String> {
        // Replaying a role permutation or joining formations changes topology.
        // Do it on a disposable simulation so malformed history, incompatible
        // scripts or failed coupling leave the caller's current world intact.
        if !self.control_history.is_empty()
            || self.controlled_service.is_some()
            || !self.independent_couplings.is_empty()
        {
            let mut candidate = LiveDrive::from_scenario_path(live.scenario_path())?;
            let camera = self.restore_into(&mut candidate)?;
            *live = candidate;
            return Ok(camera);
        }
        self.restore_into(live)
    }
    fn restore_into(self, live: &mut LiveDrive) -> Result<SavedCamera, String> {
        self.validate_route_available()?;
        let history = if self.control_history.is_empty() {
            self.controlled_service.iter().cloned().collect::<Vec<_>>()
        } else {
            self.control_history.clone()
        };
        let mut swapped = Vec::new();
        for id in &history {
            let index = live
                .traffic
                .services
                .iter()
                .position(|s| &s.id == id)
                .ok_or("El servicio controlado ya no existe")?;
            swapped.push((
                index,
                live.traffic.services[index].departed,
                live.traffic.services[index].parked,
            ));
            live.traffic.restore_control(&mut live.session, index)?;
        }
        live.traffic
            .reconstruct_couplings(&mut live.session, &self.independent_couplings)?;
        if let Err(error) =
            live.traffic
                .restore_shared_snapshots(&mut live.session, self.session, self.traffic)
        {
            for (index, departed, parked) in swapped.into_iter().rev() {
                live.traffic.restore_control(&mut live.session, index)?;
                live.traffic.services[index].departed = departed;
                live.traffic.services[index].parked = parked;
            }
            return Err(error);
        }
        live.traffic.control_history = history;
        live.start_clock_s = self.start_clock_s;
        live.season = self.season;
        live.paused = true;
        live.reset_presentation_clock();
        Ok(self.camera)
    }
}
pub fn slot_path(slot: usize) -> PathBuf {
    player_data_dir().join(format!("save-{}.json", slot + 1))
}
#[derive(Resource)]
pub struct PendingSavedCamera(pub SavedCamera);
pub fn restore_camera(
    mut commands: Commands,
    saved: Option<Res<PendingSavedCamera>>,
    origin: Res<FloatingOrigin>,
    mut follow: ResMut<CameraFollowMode>,
    mut look: ResMut<DriverLookOffset>,
    mut mode: ResMut<crate::camera::CameraMode>,
    mut driver: ResMut<crate::camera::LiveDriverCab>,
    mut passenger: ResMut<crate::camera::PassengerCamState>,
    mut cab2d: ResMut<crate::cab_cvf_overlay::CabCvfOverlayState>,
    mut cameras: Query<
        (
            &mut OrbitState,
            &mut crate::camera::FlyState,
            &mut Transform,
        ),
        (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
    >,
) {
    let Some(saved) = saved else { return };
    let Ok((mut orbit, mut fly, mut transform)) = cameras.single_mut() else {
        return;
    };
    *follow = saved.0.follow_mode();
    look.yaw = saved.0.look_yaw;
    look.pitch = saved.0.look_pitch;
    orbit.focus = Vec3::from_array(saved.0.focus) - origin.shift;
    orbit.yaw = saved.0.yaw;
    orbit.pitch = saved.0.pitch;
    orbit.distance = saved.0.distance;
    if let Some(p) = &saved.0.pose {
        *mode = if p.fly {
            crate::camera::CameraMode::Fly
        } else {
            crate::camera::CameraMode::Orbit
        };
        transform.translation = Vec3::from_array(p.position) - origin.shift;
        transform.rotation = Quat::from_array(p.rotation).normalize();
        fly.yaw = p.fly_yaw;
        fly.pitch = p.fly_pitch;
        if let Some(eye) = driver.eyepoints.get(p.driver_eyepoint).copied() {
            driver.eyepoint_index = p.driver_eyepoint;
            let placement = driver.interior_placement;
            driver.apply_eyepoint(eye, placement);
        }
        cab2d.view_index = p.cab2d_view;
        passenger.car_slot = p.passenger_car_slot;
        passenger.view_index = p.passenger_view;
        passenger.consist_car = p.passenger_car;
        passenger.head_msts = Vec3::from_array(p.passenger_head);
        [
            passenger.look_pitch,
            passenger.look_yaw,
            passenger.pitch_limit,
            passenger.yaw_limit,
        ] = p.passenger_look;
    }
    commands.remove_resource::<PendingSavedCamera>();
}

pub(crate) fn canonical_scenario(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut value: toml::Value = toml::from_str(&source).map_err(|e| e.to_string())?;
    let dir = path.parent().ok_or("Servicio sin directorio")?;
    for (section, key) in [("route", "path"), ("train", "consist")] {
        if let Some(relative) = value
            .get(section)
            .and_then(|s| s.get(key))
            .and_then(toml::Value::as_str)
        {
            let absolute = crate::player_launch::absolute(&dir.join(relative));
            value[section][key] = toml::Value::String(absolute.to_string_lossy().into_owned());
        }
    }
    if let Some(services) = value
        .get_mut("extra_trains")
        .and_then(toml::Value::as_array_mut)
    {
        for service in services {
            if let Some(relative) = service.get("consist").and_then(toml::Value::as_str) {
                let absolute = crate::player_launch::absolute(&dir.join(relative));
                service["consist"] = toml::Value::String(absolute.to_string_lossy().into_owned());
            }
        }
    }
    toml::to_string_pretty(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_pending_operations_do_not_change_the_saved_game() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("save.json");
        std::fs::write(&path, b"existing save").unwrap();
        for id in ["", "bad\nservice", &"x".repeat(257)] {
            assert!(super::SavedGame::queue_control(&path, id).is_err());
            assert!(super::SavedGame::queue_coupling(&path, id).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"existing save");
        }
    }
    use super::*;
    #[test]
    fn source_relative_save_can_resume_from_a_different_directory() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let mut original = LiveDrive::from_scenario_path(&path).unwrap();
        original.session.driver_direction = 1.0;
        original.session.driver_throttle = 0.7;
        original.session.step_realtime(3.0, |_| {});
        let tmp = tempfile::tempdir().unwrap();
        let save = tmp.path().join("save.json");
        let cam = SavedCamera::capture(
            CameraFollowMode::ChaseCam,
            &OrbitState::default(),
            &DriverLookOffset::default(),
            &FloatingOrigin::default(),
        );
        let content = ActivePlayerContent {
            weather: PlayerWeather::Storm,
            environment: crate::environment::EnvironmentSelection {
                time: crate::environment::EnvironmentSource::LocalNow,
                weather: crate::environment::EnvironmentSource::Manual,
                manual_weather: PlayerWeather::Storm,
            },
            ..default()
        };
        let mut weather_app = App::new();
        weather_app
            .init_resource::<Time>()
            .init_resource::<crate::environment::LiveEnvironment>()
            .init_resource::<crate::weather_state::WeatherState>()
            .insert_resource(content.clone())
            .insert_resource(crate::player_settings::PlayerSettings {
                weather_profile: crate::weather_state::WeatherProfile::RandomJourney,
                weather_pace: crate::weather_journey::WeatherPace::Fast,
                weather_seed: 81,
                ..default()
            })
            .add_systems(Update, crate::weather_state::update);
        weather_app.update();
        weather_app
            .world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(145));
        weather_app.update();
        let weather = weather_app
            .world()
            .resource::<crate::weather_state::WeatherState>();
        SavedGame::save(&mut original, &content, cam, &save, Some(weather), None).unwrap();
        let game = SavedGame::read(&save).unwrap();
        assert_eq!(game.environment, Some(content.environment));
        assert_eq!(game.weather_checkpoint.as_ref().unwrap().seed, 81);
        let restored_weather = game
            .weather_checkpoint
            .as_ref()
            .unwrap()
            .restore(original.session.time_s());
        assert_eq!(
            serde_json::to_string(&weather.atmosphere).unwrap(),
            serde_json::to_string(&restored_weather.atmosphere).unwrap()
        );
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&save).unwrap()).unwrap();
        legacy.as_object_mut().unwrap().remove("environment");
        legacy.as_object_mut().unwrap().remove("weather_checkpoint");
        std::fs::write(
            tmp.path().join("old-save.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        assert!(
            SavedGame::read(&tmp.path().join("old-save.json"))
                .unwrap()
                .environment
                .is_none()
        );
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&save).unwrap()).unwrap();
        invalid["weather_checkpoint"]["atmosphere"]["rain"] = 2.into();
        std::fs::write(
            tmp.path().join("invalid-weather.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(SavedGame::read(&tmp.path().join("invalid-weather.json")).is_err());
        let scenario = tmp.path().join("resumed.toml");
        std::fs::write(&scenario, &game.scenario_toml).unwrap();
        let mut resumed = LiveDrive::from_scenario_path(&scenario).unwrap();
        game.restore(&mut resumed).unwrap();
        assert!(resumed.paused);
        assert_eq!(
            original.session.state.odometer_m,
            resumed.session.state.odometer_m
        );
    }
    #[test]
    fn different_formation_driver_transfer_and_coupling_are_atomic_and_survive_saved_game() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let temp = tempfile::tempdir().unwrap();
        let camera = SavedCamera::capture(
            CameraFollowMode::ChaseCam,
            &OrbitState::default(),
            &DriverLookOffset::default(),
            &FloatingOrigin::default(),
        );
        let content = ActivePlayerContent::default();
        let control = repo.join("examples/traffic_passing/scenario_control.toml");
        let mut original = LiveDrive::from_scenario_path(&control).unwrap();
        original.session.driver_direction = 0.5;
        let old_vehicles = original.session.formation.cars.len();
        original
            .traffic
            .take_control(&mut original.session, 0)
            .unwrap();
        assert_ne!(original.session.formation.cars.len(), old_vehicles);
        let fuel = original.session.state.diesel.cars[0].fuel_l;
        let save = temp.path().join("driver.json");
        SavedGame::save(&mut original, &content, camera.clone(), &save, None, None).unwrap();
        let game = SavedGame::read(&save).unwrap();
        let mut fresh = LiveDrive::from_scenario_path(&control).unwrap();
        game.restore(&mut fresh).unwrap();
        assert_eq!(fresh.session.service_id, "other");
        assert_eq!(fresh.session.state.diesel.cars[0].fuel_l, fuel);
        assert_eq!(fresh.traffic.services[0].id, "Jugador");
        let before = serde_json::to_value(fresh.session.snapshot()).unwrap();
        let mut bad = SavedGame::read(&save).unwrap();
        bad.control_history.push("missing-service".into());
        assert!(bad.restore(&mut fresh).is_err());
        assert_eq!(
            before,
            serde_json::to_value(fresh.session.snapshot()).unwrap()
        );
        let coupling = repo.join("examples/traffic_passing/scenario_coupling.toml");
        let mut original = LiveDrive::from_scenario_path(&coupling).unwrap();
        original
            .traffic
            .couple_service(&mut original.session, 0)
            .unwrap();
        let save = temp.path().join("coupled.json");
        SavedGame::save(&mut original, &content, camera, &save, None, None).unwrap();
        let game = SavedGame::read(&save).unwrap();
        let mut fresh = LiveDrive::from_scenario_path(&coupling).unwrap();
        game.restore(&mut fresh).unwrap();
        assert_eq!(fresh.session.formation.cars.len(), 4);
        assert!(fresh.traffic.services.is_empty());
        let before = serde_json::to_value(fresh.session.snapshot()).unwrap();
        let mut bad = SavedGame::read(&save).unwrap();
        bad.independent_couplings[0].donor = "missing-service".into();
        assert!(bad.restore(&mut fresh).is_err());
        assert_eq!(
            before,
            serde_json::to_value(fresh.session.snapshot()).unwrap()
        );
    }
}
