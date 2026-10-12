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

#[derive(Resource, Default)]
pub struct PlayerStudy {
    pub active: bool,
    pub setup: JourneySetup,
    pub checkpoints: std::collections::BTreeMap<usize, SessionSnapshot>,
    pub last_stage: Option<usize>,
    pub notice: String,
}
impl PlayerStudy {
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
pub fn checkpoint(mut study: ResMut<PlayerStudy>, mut live: Option<ResMut<LiveDrive>>) {
    if let Some(l) = live.as_mut() {
        study.checkpoint(l);
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
