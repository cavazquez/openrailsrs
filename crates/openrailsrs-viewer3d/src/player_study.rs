//! Practice checkpoints, lesson setup and advisory HUD; no automatic driving.
use crate::{
    journey_setup::{JourneySetup, PlayMode},
    live::LiveDrive,
};
use bevy::prelude::*;
use openrailsrs_sim::{
    CarOperation, SessionSnapshot,
    learning::{LearningState, Lesson},
};

#[derive(Clone)]
struct PracticePoint {
    session: SessionSnapshot,
    traffic: Vec<openrailsrs_sim::TrafficSnapshot>,
    weather: Option<crate::weather_state::WeatherCheckpoint>,
    bytes: usize,
}
#[derive(Resource, Default)]
pub struct PlayerStudy {
    pub active: bool,
    pub setup: JourneySetup,
    pub checkpoints: std::collections::BTreeMap<usize, SessionSnapshot>,
    pub last_stage: Option<usize>,
    pub notice: String,
    history: std::collections::VecDeque<PracticePoint>,
    history_bytes: usize,
    last_capture: Option<f64>,
}
impl PlayerStudy {
    pub fn record_practice(
        &mut self,
        live: &mut LiveDrive,
        weather: Option<&crate::weather_state::WeatherState>,
    ) -> Result<(), String> {
        if self.setup.mode != PlayMode::Practice || live.paused {
            return Ok(());
        }
        let now = live.session.time_s();
        if self
            .last_capture
            .is_some_and(|t| now >= t && now - t < 5. - 1e-9)
        {
            return Ok(());
        }
        let session = live.session.snapshot_with_scripts()?;
        let traffic = live.traffic.snapshot_with_scripts()?;
        let weather = weather.and_then(|w| w.checkpoint());
        let bytes = serde_json::to_vec(&(&session, &traffic, &weather))
            .map_err(|e| e.to_string())?
            .len();
        if bytes > 8 * 1024 * 1024 {
            return Err("Esta sesión excede el presupuesto de retroceso (8 MiB por punto)".into());
        }
        while self.history.len() >= 24 || self.history_bytes + bytes > 64 * 1024 * 1024 {
            if let Some(p) = self.history.pop_front() {
                self.history_bytes = self.history_bytes.saturating_sub(p.bytes);
            } else {
                break;
            }
        }
        self.history.push_back(PracticePoint {
            session,
            traffic,
            weather,
            bytes,
        });
        self.history_bytes += bytes;
        self.last_capture = Some(now);
        Ok(())
    }
    pub fn rewind(
        &mut self,
        live: &mut LiveDrive,
        seconds: f64,
    ) -> Result<Option<crate::weather_state::WeatherCheckpoint>, String> {
        if self.setup.mode != PlayMode::Practice {
            return Err("El retroceso sólo está disponible en Práctica preparada".into());
        }
        if !seconds.is_finite() || !(1. ..=120.).contains(&seconds) {
            return Err("Intervalo de retroceso inválido".into());
        }
        let target = live.session.time_s() - seconds;
        let point = self
            .history
            .iter()
            .rev()
            .find(|p| p.session.state.time.0 <= target + 1e-6)
            .cloned()
            .ok_or("Todavía no hay un punto tan antiguo")?;
        live.session.validate_snapshot(&point.session)?;
        live.traffic.validate_snapshot(&point.traffic)?;
        openrailsrs_sim::SessionSnapshot::validate_shared_dispatcher(
            std::iter::once(&point.session).chain(point.traffic.iter().map(|t| &t.session)),
        )?;
        let player = live.session.prepare_script_restore(&point.session)?;
        let traffic = live.traffic.prepare_script_restore(&point.traffic)?;
        live.session
            .restore_prepared_snapshot(point.session, player)?;
        live.traffic
            .restore_prepared_snapshot(point.traffic, traffic)?;
        live.traffic.synchronize_occupancy(&mut live.session);
        let now = live.session.time_s();
        while self
            .history
            .back()
            .is_some_and(|p| p.session.state.time.0 > now)
        {
            if let Some(p) = self.history.pop_back() {
                self.history_bytes = self.history_bytes.saturating_sub(p.bytes);
            }
        }
        self.last_capture = Some(now);
        live.reset_presentation_clock();
        live.paused = true;
        Ok(point.weather)
    }
    pub fn activate(&mut self, live: &mut LiveDrive) -> Result<(), String> {
        if self.active {
            return Ok(());
        }
        self.active = true;
        self.setup = JourneySetup::read(live.scenario_path());
        self.checkpoints.clear();
        self.last_stage = None;
        let s = &mut live.session;
        if let Some(lesson) = s.state.learning.as_ref() {
            let stage = lesson.stage;
            self.checkpoints.insert(stage, s.snapshot_with_scripts()?);
            self.last_stage = Some(stage);
            return Ok(());
        }
        if self.setup.mode == PlayMode::Service {
            return Ok(());
        }
        s.gameplay.quick_station_practice = true;
        s.gameplay.missed_stop_policy = openrailsrs_sim::service::MissedStopPolicy::Continue;
        if matches!(self.setup.mode, PlayMode::Tutorial | PlayMode::Challenge) {
            let lesson = self.setup.lesson;
            s.driver_throttle = 0.;
            s.set_direction(0.5)?;
            match lesson {
                Lesson::Emergency => {
                    s.state.brake_system.emergency = true;
                    s.driver_brake = 1.;
                    s.state.velocity_mps = 12.;
                    for v in &mut s.state.vehicles {
                        v.velocity_mps = 12.;
                    }
                }
                Lesson::Battery => {
                    let index = s.cab_vehicle();
                    if s.formation.cars[index].battery_on {
                        s.operate_car(index, CarOperation::Battery)?;
                    }
                }
                Lesson::Diesel => {
                    let index = s.cab_vehicle();
                    if let Some(d) = s.state.diesel.cars.iter_mut().find(|d| d.vehicle == index) {
                        d.command_running = false;
                        d.phase = openrailsrs_sim::diesel_operation::EnginePhase::Stopped;
                        d.rpm = 0.;
                    }
                }
                _ => {}
            }
            s.state.learning = Some(LearningState::new(lesson, s));
            self.last_stage = Some(0);
            self.checkpoints.insert(0, s.snapshot_with_scripts()?);
        }
        Ok(())
    }
    pub fn checkpoint(&mut self, live: &mut LiveDrive) {
        let Some(stage) = live.session.state.learning.as_ref().map(|s| s.stage) else {
            return;
        };
        if self.last_stage != Some(stage) {
            match live.session.snapshot_with_scripts() {
                Ok(saved) => {
                    self.checkpoints.insert(stage, saved);
                }
                Err(e) => self.notice = e,
            }
            self.last_stage = Some(stage);
        }
    }
    pub fn repeat(&mut self, live: &mut LiveDrive, whole: bool) -> Result<(), String> {
        let stage = if whole {
            0
        } else {
            live.session
                .state
                .learning
                .as_ref()
                .map_or(0, |s| s.stage.min(s.lesson.steps().len().saturating_sub(1)))
        };
        let saved = self
            .checkpoints
            .get(&stage)
            .cloned()
            .ok_or("No hay un punto de repetición compatible para este paso")?;
        let attempts = live
            .session
            .state
            .learning
            .as_ref()
            .map_or(1, |s| s.attempts)
            .saturating_add(1);
        live.session.restore_snapshot(saved)?;
        if let Some(lesson) = &mut live.session.state.learning {
            lesson.attempts = attempts;
        } else {
            live.session.state.learning =
                Some(LearningState::new(self.setup.lesson, &live.session));
        }
        self.last_stage = Some(stage);
        live.paused = false;
        Ok(())
    }
}

pub fn activate(mut study: ResMut<PlayerStudy>, mut live: Option<ResMut<LiveDrive>>) {
    if let Some(l) = live.as_mut()
        && let Err(e) = study.activate(l)
    {
        study.notice = e;
    }
}
pub fn checkpoint(
    mut study: ResMut<PlayerStudy>,
    mut live: Option<ResMut<LiveDrive>>,
    weather: Option<Res<crate::weather_state::WeatherState>>,
) {
    if let Some(l) = live.as_mut() {
        study.checkpoint(l);
        if let Err(e) = study.record_practice(l, weather.as_deref()) {
            study.notice = e;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loaded_lesson_checkpoints_restore_controls_reserves_and_progress() {
        let temp = tempfile::tempdir().unwrap();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/scenario.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        let mut value: toml::Value =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (section, field) in [("route", "path"), ("train", "consist")] {
            let file = path
                .parent()
                .unwrap()
                .join(value[section][field].as_str().unwrap())
                .canonicalize()
                .unwrap();
            value[section][field] = toml::Value::String(file.to_string_lossy().into());
        }
        value.as_table_mut().unwrap().insert(
            "player_setup".into(),
            toml::Value::try_from(JourneySetup {
                mode: PlayMode::Tutorial,
                lesson: Lesson::Basic,
                ..default()
            })
            .unwrap(),
        );
        let prepared = temp.path().join("scenario.toml");
        std::fs::write(&prepared, toml::to_string(&value).unwrap()).unwrap();
        let mut live = LiveDrive::from_scenario_path(&prepared).unwrap();
        let mut study = PlayerStudy::default();
        study.activate(&mut live).unwrap();
        assert_eq!(study.setup.mode, PlayMode::Tutorial);
        assert!(live.session.state.learning.is_some());
        assert_eq!(live.session.state.learning.as_ref().unwrap().stage, 0);
        live.session.step_realtime(0.1, |_| {});
        study.checkpoint(&mut live);
        assert_eq!(live.session.state.learning.as_ref().unwrap().stage, 1);
        live.session.exterior.toggle_door();
        live.session.step_realtime(1.1, |_| {});
        study.checkpoint(&mut live);
        assert_eq!(live.session.state.learning.as_ref().unwrap().stage, 2);
        let fuel = study.checkpoints[&1].state.diesel.cars[0].fuel_l;
        study.repeat(&mut live, true).unwrap();
        assert_eq!(live.session.state.learning.as_ref().unwrap().stage, 0);
        assert_eq!(live.session.state.learning.as_ref().unwrap().attempts, 2);
        assert_eq!(live.session.driver_throttle, 0.);
        assert!(live.session.state.diesel.cars[0].fuel_l >= fuel);
        let saved = live.session.snapshot();
        let mut resumed = LiveDrive::from_scenario_path(&prepared).unwrap();
        resumed.session.restore_snapshot(saved).unwrap();
        let mut next = PlayerStudy::default();
        next.activate(&mut resumed).unwrap();
        assert_eq!(next.last_stage, Some(0));
        next.repeat(&mut resumed, false).unwrap();
        assert_eq!(resumed.session.state.learning.as_ref().unwrap().attempts, 3);
        assert!(!scenario.train.consist.is_empty());
    }
}
#[cfg(test)]
mod practice_acceptance {
    use super::*;
    fn difference(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
        if a == b {
            return None;
        }
        match (a, b) {
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                for (k, av) in a {
                    if let Some(bv) = b.get(k) {
                        if let Some(d) = difference(av, bv, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    } else {
                        return Some(format!("{path}.{k}: absent"));
                    }
                }
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                for (i, (av, bv)) in a.iter().zip(b).enumerate() {
                    if let Some(d) = difference(av, bv, &format!("{path}[{i}]")) {
                        return Some(d);
                    }
                }
            }
            _ => return Some(format!("{path}: actual {a}, expected {b}")),
        }
        Some(format!("{path}: length differs"))
    }
    #[test]
    fn rewind_restores_traffic_commands_consumables_and_discards_future() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_traffic/scenario.toml");
        let mut live = LiveDrive::from_scenario_path(&path).unwrap();
        live.paused = false;
        let mut study = PlayerStudy::default();
        study.setup.mode = PlayMode::Practice;
        let mut first = None;
        for i in 0..8 {
            live.traffic.advance(&mut live.session, 5., None, |_| {});
            study.record_practice(&mut live, None).unwrap();
            if i == 3 {
                first = Some((live.session.snapshot(), live.traffic.snapshot()));
            }
        }
        let (player, traffic) = first.unwrap();
        study.rewind(&mut live, 20.).unwrap();
        assert!(live.paused);
        assert_eq!(live.session.time_s(), player.state.time.0);
        assert_eq!(live.session.state.diesel, player.state.diesel);
        let restored = live.traffic.snapshot();
        assert_eq!(restored.len(), traffic.len());
        for (actual, expected) in restored.iter().zip(&traffic) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.departed, expected.departed);
            assert!(
                serde_json::to_value(&actual.session.state).unwrap()
                    == serde_json::to_value(&expected.session.state).unwrap(),
                "El retroceso debe conservar toda la física del tráfico: {}",
                difference(
                    &serde_json::to_value(&actual.session.state).unwrap(),
                    &serde_json::to_value(&expected.session.state).unwrap(),
                    "state"
                )
                .unwrap_or_default()
            );
            assert!(
                serde_json::to_value(&actual.session.gameplay).unwrap()
                    == serde_json::to_value(&expected.session.gameplay).unwrap(),
                "El servicio del tráfico debe conservarse"
            );
            assert_eq!(
                actual.session.driver_throttle,
                expected.session.driver_throttle
            );
            assert_eq!(actual.session.driver_brake, expected.session.driver_brake);
            assert!(
                serde_json::to_value(&actual.session.dispatcher.own_locks).unwrap()
                    == serde_json::to_value(&expected.session.dispatcher.own_locks).unwrap()
            );
        }
        assert!(
            study
                .history
                .iter()
                .all(|p| p.session.state.time.0 <= live.session.time_s())
        );
        study.setup.mode = PlayMode::Service;
        assert!(study.rewind(&mut live, 10.).is_err());
    }
}
