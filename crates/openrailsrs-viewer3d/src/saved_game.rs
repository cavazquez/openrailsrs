//! Atomic player saves, compatible content checks and camera restoration.
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraFollowMode, DriverLookOffset, OrbitState};
use crate::floating_origin::FloatingOrigin;
use crate::live::LiveDrive;
use crate::player_launch::{ActivePlayerContent, PlayerWeather, QueuedPlayerLaunch};
use crate::player_settings::{atomic_write, player_data_dir};

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
    pub version: u32,
    pub scenario_toml: String,
    pub route_root: Option<PathBuf>,
    pub description: String,
    pub session: openrailsrs_sim::SessionSnapshot,
    #[serde(default)]
    pub traffic: Vec<openrailsrs_sim::TrafficSnapshot>,
    pub start_clock_s: f64,
    pub season: String,
    pub weather: PlayerWeather,
    pub camera: SavedCamera,
}
impl SavedGame {
    pub fn save(
        live: &LiveDrive,
        content: &ActivePlayerContent,
        camera: SavedCamera,
        path: &Path,
    ) -> Result<(), String> {
        let game = Self {
            version: 1,
            scenario_toml: canonical_scenario(live.scenario_path())?,
            route_root: content.route_root.clone(),
            description: content.description.clone(),
            session: live.session.snapshot(),
            traffic: live.traffic.snapshot(),
            start_clock_s: live.start_clock_s,
            season: live.season.clone(),
            weather: content.weather,
            camera,
        };
        atomic_write(
            path,
            &serde_json::to_vec_pretty(&game).map_err(|e| e.to_string())?,
        )
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("Partida demasiado grande".into());
        }
        let game: Self = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("No se puede leer la partida: {e}"))?;
        if game.version != 1
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
            return Err("Versión o cámara de partida inválida".into());
        }
        Ok(game)
    }
    pub fn prepare_resume(path: &Path) -> Result<QueuedPlayerLaunch, String> {
        let game = Self::read(path)?;
        let scenario = crate::player_launch::absolute(&player_data_dir().join("resume.toml"));
        atomic_write(&scenario, game.scenario_toml.as_bytes())?;
        Ok(QueuedPlayerLaunch {
            path: scenario,
            route_root: game.route_root,
            weather: game.weather,
            resume: Some(path.to_path_buf()),
        })
    }
    pub fn restore(self, live: &mut LiveDrive) -> Result<SavedCamera, String> {
        live.session.validate_snapshot(&self.session)?;
        live.traffic.validate_snapshot(&self.traffic)?;
        live.session.restore_snapshot(self.session)?;
        live.traffic.restore_snapshot(self.traffic)?;
        live.traffic.synchronize_occupancy(&mut live.session);
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
        With<Camera3d>,
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

fn canonical_scenario(path: &Path) -> Result<String, String> {
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
        SavedGame::save(&original, &ActivePlayerContent::default(), cam, &save).unwrap();
        let game = SavedGame::read(&save).unwrap();
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
}
