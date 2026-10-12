//! Explicit player modes applied only to disposable launch scenarios.
use openrailsrs_scenarios::ScenarioFile;
use openrailsrs_sim::{LiveDriveSession, learning::Lesson};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    #[default]
    Service,
    Explore,
    Practice,
    Tutorial,
    Challenge,
}
impl PlayMode {
    pub const ALL: [Self; 5] = [
        Self::Service,
        Self::Explore,
        Self::Practice,
        Self::Tutorial,
        Self::Challenge,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Service => "Servicio del autor",
            Self::Explore => "Exploración libre",
            Self::Practice => "Práctica preparada",
            Self::Tutorial => "Tutorial guiado",
            Self::Challenge => "Desafío meteorológico",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JourneySetup {
    pub mode: PlayMode,
    pub lesson: Lesson,
    pub start_stop: usize,
    pub approach: bool,
    pub traffic: bool,
}
impl JourneySetup {
    pub fn valid(&self) -> bool {
        self.start_stop <= 4096
    }
    pub fn weather_challenge(
        &self,
        seed: u32,
    ) -> Option<openrailsrs_scenarios::weather_track::WeatherTrack> {
        use openrailsrs_scenarios::weather_track::{WeatherKind, WeatherPhase, WeatherTrack};
        if !matches!(self.mode, PlayMode::Challenge | PlayMode::Tutorial) {
            return None;
        }
        let kind = match self.lesson {
            Lesson::Rain => WeatherKind::Rain,
            Lesson::Snow => WeatherKind::Snow,
            Lesson::Fog => WeatherKind::Fog,
            _ => return None,
        };
        Some(WeatherTrack {
            schema: 1,
            seed,
            phases: vec![WeatherPhase {
                start_s: 0.,
                transition_s: 0.,
                kind,
                intensity: 0.9,
                wind_mps: [3., 1.],
            }],
        })
    }
    pub fn read(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
            .and_then(|v| v.get("player_setup").cloned())
            .and_then(|v| v.try_into().ok())
            .unwrap_or_default()
    }
    pub fn apply(&self, scenario: &mut ScenarioFile) -> Result<(), String> {
        if !self.valid() {
            return Err("Punto de práctica inválido".into());
        }
        if self.mode == PlayMode::Service {
            return Ok(());
        }
        let probe =
            LiveDriveSession::from_scenario(Path::new("."), scenario).map_err(|e| e.to_string())?;
        if self.mode == PlayMode::Tutorial
            && self.lesson == Lesson::Electric
            && (probe.physics.electric.cars.is_empty()
                || !probe.physics.electric.route.sections.iter().any(|s| {
                    s.supply.kind == openrailsrs_core::electrification::ElectricPickup::None
                }))
        {
            return Err("Este tutorial necesita un tren eléctrico y una ruta con un sector sin tensión. Elegí el ejemplo electric_supply.".into());
        }
        if self.mode == PlayMode::Tutorial
            && self.lesson == Lesson::Diesel
            && probe.physics.diesel.cars.is_empty()
        {
            return Err("Este ejercicio necesita un tren diésel; elegí traction_operation.".into());
        }
        if matches!(self.mode, PlayMode::Practice | PlayMode::Explore) && self.start_stop > 0 {
            let stop = probe
                .gameplay
                .stop_targets
                .get(self.start_stop - 1)
                .ok_or("La estación inicial ya no está disponible")?;
            let position = (stop.cum_dist_m - if self.approach { 500. } else { 0. }).max(0.);
            scenario.route.start_offset_m = Some(position);
            let skipped = self.start_stop - 1;
            scenario.route.stops.drain(..skipped);
            let clock = scenario
                .route
                .stops
                .first()
                .map_or(0., |s| s.arrive_s - 60.);
            for s in &mut scenario.route.stops {
                s.arrive_s = (s.arrive_s - clock).max(0.);
                s.depart_s = (s.depart_s - clock).max(s.arrive_s);
            }
        }
        scenario.gameplay.shunting.clear();
        scenario.route.legs.clear();
        if self.mode == PlayMode::Explore
            || (self.mode == PlayMode::Tutorial
                && !matches!(self.lesson, Lesson::Rain | Lesson::Snow | Lesson::Fog))
        {
            scenario.route.stops.clear();
        }
        if !self.traffic {
            scenario.extra_trains.clear();
        }
        scenario.scenario.name = format!("{} · {}", self.mode.label(), scenario.scenario.name);
        scenario.scenario.description = format!(
            "{}; copia de práctica, con referencias originales intactas. Señales y límites de vía siguen activos.",
            self.mode.label()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scenario() -> ScenarioFile {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_extended/scenario.toml");
        let mut s = openrailsrs_scenarios::load_scenario(&path).unwrap();
        for p in [&mut s.route.path, &mut s.train.consist] {
            *p = path
                .parent()
                .unwrap()
                .join(&*p)
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned();
        }
        s
    }
    #[test]
    fn explicit_exploration_removes_activity_goals_but_preserves_rail_rules_and_stock() {
        let original = scenario();
        let mut s = original.clone();
        JourneySetup {
            mode: PlayMode::Explore,
            ..Default::default()
        }
        .apply(&mut s)
        .unwrap();
        assert!(s.route.stops.is_empty());
        assert!(s.gameplay.shunting.is_empty());
        assert_eq!(s.route.waypoints, original.route.waypoints);
        assert_eq!(
            s.route.edge_speed_limits.len(),
            original.route.edge_speed_limits.len()
        );
        assert_eq!(s.train.consist, original.train.consist);
        assert!(!original.route.stops.is_empty());
    }
    #[test]
    fn prepared_station_changes_only_disposable_start_and_future_schedule() {
        let mut s = scenario();
        let old = s.route.start_offset_m;
        JourneySetup {
            mode: PlayMode::Practice,
            start_stop: 2,
            approach: true,
            ..Default::default()
        }
        .apply(&mut s)
        .unwrap();
        assert!(s.route.start_offset_m > old);
        assert!(s.route.stops.first().unwrap().arrive_s <= 60.);
        assert!(LiveDriveSession::from_scenario(Path::new("."), &s).is_ok());
    }
}
