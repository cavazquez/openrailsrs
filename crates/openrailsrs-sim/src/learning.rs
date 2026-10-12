//! Observable driving lessons. These never operate the player's controls or
//! suppress safety interventions; progress follows actual simulator states.
use crate::{LiveDriveSession, exterior::DoorState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lesson {
    #[default]
    Basic,
    Electric,
    Emergency,
    Battery,
    Diesel,
    Steam,
    Rain,
    Snow,
    Fog,
}
impl Lesson {
    pub const ALL: [Self; 9] = [
        Self::Basic,
        Self::Electric,
        Self::Emergency,
        Self::Battery,
        Self::Diesel,
        Self::Steam,
        Self::Rain,
        Self::Snow,
        Self::Fog,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Conducción desde cabina",
            Self::Electric => "Alimentación y sector sin tensión",
            Self::Emergency => "Freno de emergencia",
            Self::Battery => "Diagnóstico de batería",
            Self::Diesel => "Diagnóstico del motor diésel",
            Self::Steam => "Conductor y fogonero de vapor",
            Self::Rain => "Lluvia y adherencia",
            Self::Snow => "Nieve y frenado",
            Self::Fog => "Visibilidad con niebla",
        }
    }
    pub fn steps(self) -> &'static [&'static str] {
        match self {
            Self::Basic => &[
                "Cerrá el regulador y colocá el inversor en neutro.",
                "Abrí las puertas con el tren detenido.",
                "Cerrá las puertas.",
                "Seleccioná adelante.",
                "Soltá el freno y acelerá suavemente.",
                "Cerrá el regulador y aplicá el freno.",
                "Detenete con el freno aplicado.",
                "Volvé a neutro con el regulador cerrado.",
            ],
            Self::Electric => &[
                "Cerrá regulador y colocá neutro.",
                "Bajá el pantógrafo: comprobá falta de contacto.",
                "Subí el pantógrafo y esperá a que termine su recorrido.",
                "Abrí el disyuntor.",
                "Cerrá el disyuntor y esperá alimentación real.",
                "Conducí hacia el sector sin tensión.",
                "Al pasar por el corte, observá la pérdida de alimentación.",
                "Avanzá hasta recuperar tensión y esperá el retardo del disyuntor.",
                "Cerrá regulador y detenete con el freno aplicado.",
            ],
            Self::Emergency => &[
                "Observá la emergencia y cerrá el regulador.",
                "Detenete sin ocultar la intervención del freno.",
                "Rearmá la emergencia estando detenido.",
                "Esperá la recuperación de tubería y soltá el freno.",
                "Volvé a conducir con tracción disponible.",
            ],
            Self::Battery => &[
                "Identificá que falta alimentación de baja tensión.",
                "Con el regulador cerrado, conectá batería en operaciones F9.",
                "Esperá alimentación de cabina y auxiliares.",
                "Comprobá tracción al iniciar marcha.",
            ],
            Self::Diesel => &[
                "Identificá el motor detenido y cerrá el regulador.",
                "Ordená arranque con batería y combustible disponibles.",
                "Esperá el arranque real del motor.",
                "Iniciá marcha y comprobá entrega de potencia.",
            ],
            Self::Steam => &[
                "Cerrá regulador y poné neutro. Revisá presión, agua y carbón reales en B.",
                "Desactivá el fogonero automático en B; regulá pala y tiro sin perder agua.",
                "Aumentá fuego y tiro: comprobá evaporación y consumo real de carbón.",
                "Activá un inyector: comprobá transferencia de agua desde el ténder, con presión disponible.",
                "Cerrá inyectores y prepará corte de 40–65%. El ténder debe conservar agua y carbón.",
                "Conducí suavemente: el regulador usa presión real de la caldera.",
                "Reducí corte a menos de 35% al avanzar: observá demanda de vapor y presión.",
                "Cerrá regulador y frená hasta detenerte; conservá agua sobre el nivel seguro.",
                "Activá asistencia del fogonero para recuperar presión con las reservas restantes.",
            ],
            Self::Rain | Self::Snow | Self::Fog => &[
                "Revisá el clima, agarre y visibilidad antes de partir.",
                "Conducí por debajo del límite de velocidad.",
                "Anticipá la frenada y completá la próxima parada.",
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LearningState {
    pub lesson: Lesson,
    pub stage: usize,
    pub attempts: u32,
    pub entered_s: f64,
    pub entered_speed_mps: f64,
    pub last_observed_s: f64,
    pub completed_s: Option<f64>,
    pub initial_completed_stops: usize,
}
impl LearningState {
    pub fn new(lesson: Lesson, s: &LiveDriveSession) -> Self {
        Self {
            lesson,
            stage: 0,
            attempts: 1,
            entered_s: s.time_s(),
            entered_speed_mps: s.velocity_mps(),
            last_observed_s: -1.,
            completed_s: None,
            initial_completed_stops: s.gameplay.stop_results.len(),
        }
    }
    pub fn instruction(&self) -> &'static str {
        self.lesson
            .steps()
            .get(self.stage)
            .copied()
            .unwrap_or("Ejercicio completado. Podés repetirlo o continuar conduciendo.")
    }
    pub fn hint(&self, s: &LiveDriveSession) -> Option<&'static str> {
        if self.lesson != Lesson::Steam || s.time_s() - self.entered_s < 15. {
            return None;
        }
        let b = s.state.boiler_state.as_ref()?;
        if b.low_water_failure {
            return Some(
                "La caldera tiene un fallo por bajo nivel: la asistencia no la repara. En práctica podés repetir el punto anterior; en servicio necesitás detener y revisar el equipo.",
            );
        }
        if b.coal_kg <= 0. || b.tender_water_kg <= 0. {
            return Some(
                "El ténder no tiene reservas suficientes. Un inyector o el fogonero no crean agua/carbón; hace falta abastecimiento compatible.",
            );
        }
        match self.stage {
            2 => Some(
                "Revisá fuego y tiro en B: pala/tiro superiores a 30%, combustión y evaporación mayores que cero.",
            ),
            3 if b.water_kg >= b.initial_water_kg * 0.99 => Some(
                "La caldera está llena. El inyector necesita espacio para transferir agua; observá el consumo real antes de volver a inyectar.",
            ),
            3 => Some(
                "Elegí un inyector en B y comprobá caudal mayor que cero. Necesita presión y agua real en el ténder.",
            ),
            5 => Some(
                "Cerrá puertas, seleccioná adelante, soltá frenos y abrí regulador suavemente. Revisá presión, corte y frenos por coche.",
            ),
            6 => Some(
                "Con velocidad mayor que 2 m/s, reducí corte por debajo de 35%; no basta cerrar el regulador.",
            ),
            8 => Some(
                "Activá el fogonero automático estando frenado. La recuperación exige reservas y evaporación superior al uso de vapor.",
            ),
            _ => None,
        }
    }
    pub fn valid(&self) -> bool {
        self.stage <= self.lesson.steps().len()
            && self.attempts > 0
            && [self.entered_s, self.entered_speed_mps, self.last_observed_s]
                .iter()
                .all(|v| v.is_finite())
            && self.entered_s >= 0.
            && self.entered_speed_mps >= 0.
            && self.last_observed_s >= -1.
            && self
                .completed_s
                .is_none_or(|v| v.is_finite() && v >= self.entered_s)
    }
    pub fn observe(&mut self, s: &LiveDriveSession) -> bool {
        if self.completed_s.is_some() || s.time_s() <= self.last_observed_s {
            return false;
        }
        self.last_observed_s = s.time_s();
        let t = s.cab_telemetry();
        let stopped = s.velocity_mps() < 0.1;
        let closed = s.driver_throttle < 0.01;
        let neutral = (s.driver_direction - 0.5).abs() < 0.01;
        let running = t.main_power && s.velocity_mps() > 0.3 && s.driver_throttle > 0.05;
        if self.lesson == Lesson::Basic && self.stage == 5 {
            self.entered_speed_mps = self.entered_speed_mps.max(s.velocity_mps());
        }
        let done = match (self.lesson, self.stage) {
            (Lesson::Basic, 0) | (Lesson::Electric, 0) => closed && neutral,
            (Lesson::Basic, 1) => stopped && s.exterior.door == DoorState::Open,
            (Lesson::Basic, 2) => s.exterior.door == DoorState::Closed,
            (Lesson::Basic, 3) => s.driver_direction > 0.5 && stopped,
            (Lesson::Basic, 4) => running,
            (Lesson::Basic, 5) => {
                closed && s.driver_brake > 0.2 && s.velocity_mps() < self.entered_speed_mps
            }
            (Lesson::Basic, 6) => stopped && s.driver_brake > 0.2,
            (Lesson::Basic, 7) => stopped && closed && neutral,
            (Lesson::Electric, 1) => t.pantograph_fraction < 0.01 && !t.main_power,
            (Lesson::Electric, 2) => t.pantograph_fraction > 0.99,
            (Lesson::Electric, 3) => !s.state.electric.breaker_command_closed,
            (Lesson::Electric, 4) => t.main_power && t.circuit_breaker_state == 2,
            (Lesson::Electric, 5) => running,
            (Lesson::Electric, 6) => s
                .state
                .electric
                .cars
                .iter()
                .any(|c| c.loss == crate::electric::PowerLoss::NoSupply),
            (Lesson::Electric, 7) => {
                t.main_power && t.line_voltage_v > 100. && s.time_s() > self.entered_s
            }
            (Lesson::Electric, 8) => stopped && closed && s.driver_brake > 0.2,
            (Lesson::Emergency, 0) => t.emergency && closed,
            (Lesson::Emergency, 1) => stopped && t.emergency,
            (Lesson::Emergency, 2) => stopped && !t.emergency,
            (Lesson::Emergency, 3) => {
                (if t.vacuum_brake {
                    t.brake_pipe_bar < 0.65
                } else {
                    t.brake_pipe_bar > 4.5
                }) && s.driver_brake < 0.05
            }
            (Lesson::Emergency, 4) => running,
            (Lesson::Battery, 0) => !t.low_voltage_power,
            (Lesson::Battery, 1) => {
                closed
                    && s.formation
                        .cars
                        .get(s.cab_vehicle())
                        .is_some_and(|c| c.battery_on)
            }
            (Lesson::Battery, 2) => t.cab_power && t.auxiliary_power,
            (Lesson::Battery, 3) => running,
            (Lesson::Diesel, 0) => {
                closed
                    && t.diesel
                        .as_ref()
                        .is_some_and(|d| d.phase == crate::diesel_operation::EnginePhase::Stopped)
            }
            (Lesson::Diesel, 1) => t.diesel.as_ref().is_some_and(|d| d.command_running),
            (Lesson::Diesel, 2) => t
                .diesel
                .as_ref()
                .is_some_and(|d| d.phase == crate::diesel_operation::EnginePhase::Running),
            (Lesson::Diesel, 3) => running,
            (Lesson::Steam, 0) => closed && neutral && s.state.boiler_state.is_some(),
            (Lesson::Steam, 1) => s.state.boiler_state.as_ref().is_some_and(|b| {
                !b.controls.automatic_fireman && b.coal_kg > 0. && b.tender_water_kg > 0.
            }),
            (Lesson::Steam, 2) => s.state.boiler_state.as_ref().is_some_and(|b| {
                b.controls.firing > 0.3
                    && b.controls.damper > 0.3
                    && b.coal_burn_kg_s > 0.
                    && b.evaporation_kg_s > 0.
            }),
            (Lesson::Steam, 3) => s.state.boiler_state.as_ref().is_some_and(|b| {
                b.injection_kg_s > 0. && (b.controls.injector1 || b.controls.injector2)
            }),
            (Lesson::Steam, 4) => s.state.boiler_state.as_ref().is_some_and(|b| {
                !b.controls.injector1
                    && !b.controls.injector2
                    && (0.4..=0.65).contains(&b.controls.cutoff)
                    && b.coal_kg > 0.
                    && b.tender_water_kg > 0.
            }),
            (Lesson::Steam, 5) => {
                running
                    && s.state
                        .boiler_state
                        .as_ref()
                        .is_some_and(|b| b.tractive_force_n > 0. && !b.low_water_failure)
            }
            (Lesson::Steam, 6) => {
                s.velocity_mps() > 2.
                    && s.state
                        .boiler_state
                        .as_ref()
                        .is_some_and(|b| b.controls.cutoff < 0.35 && b.steam_usage_kg_s > 0.)
            }
            (Lesson::Steam, 7) => {
                closed
                    && stopped
                    && s.driver_brake > 0.2
                    && s.state
                        .boiler_state
                        .as_ref()
                        .is_some_and(|b| !b.low_water_failure)
            }
            (Lesson::Steam, 8) => s.state.boiler_state.as_ref().is_some_and(|b| {
                b.controls.automatic_fireman
                    && b.coal_kg > 0.
                    && b.tender_water_kg > 0.
                    && b.evaporation_kg_s > b.steam_usage_kg_s
                    && !b.low_water_failure
            }),
            (Lesson::Rain | Lesson::Snow | Lesson::Fog, 0) => s.time_s() > self.entered_s + 1.,
            (Lesson::Rain | Lesson::Snow | Lesson::Fog, 1) => {
                s.velocity_mps() > 1. && s.velocity_mps() <= s.effective_speed_limit_mps()
            }
            (Lesson::Rain | Lesson::Snow | Lesson::Fog, 2) => {
                s.gameplay.stop_results.len() > self.initial_completed_stops
            }
            _ => false,
        };
        if done {
            if matches!(self.lesson, Lesson::Rain | Lesson::Snow | Lesson::Fog) && self.stage == 1 {
                self.initial_completed_stops = s.gameplay.stop_results.len();
            }
            self.stage += 1;
            self.entered_s = s.time_s();
            self.entered_speed_mps = s.velocity_mps();
            if self.stage == self.lesson.steps().len() {
                self.completed_s = Some(s.time_s());
            }
        }
        done
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrakingAdvice {
    pub distance_m: f64,
    pub target_distance_m: f64,
    pub deceleration_mps2: f64,
    pub reaction_and_fill_s: f64,
    pub grade_percent: f64,
    pub grip: f64,
    pub message: String,
}
pub fn braking_advice(s: &LiveDriveSession) -> Option<BrakingAdvice> {
    let stop = s.gameplay.stop_targets.get(s.gameplay.next_stop_idx)?;
    let target = stop.cum_dist_m - s.head_chainage_m();
    if target < 0. || s.velocity_mps() < 0.2 {
        return None;
    }
    let grade = s
        .path_data
        .edges
        .get(s.state.edge_index)?
        .grade_at(s.pos_on_edge_m());
    let grip = s
        .state
        .rail_adhesion
        .as_ref()
        .map_or(1., |a| a.weather_factor);
    let available = (s.physics.max_brake_n / s.effective_mass_kg()).min(9.81 * 0.25 * grip);
    let decel = available + 9.81 * grade / 100.;
    let delay = 3. + s.formation.length_m() / 200.;
    let distance = if decel > 0.05 {
        s.velocity_mps() * delay + s.velocity_mps().powi(2) / (2. * decel)
    } else {
        f64::INFINITY
    };
    let message = if !distance.is_finite() {
        "Frenado insuficiente para esta pendiente: reducí velocidad y prepará emergencia."
    } else if target <= distance * 1.15 {
        "Empezá a frenar: la parada ya está dentro de la distancia estimada."
    } else {
        "Todavía hay margen de frenado; conservá una velocidad apropiada."
    };
    Some(BrakingAdvice {
        distance_m: distance,
        target_distance_m: target,
        deceleration_mps2: decel,
        reaction_and_fill_s: delay,
        grade_percent: grade,
        grip,
        message: message.into(),
    })
}

#[cfg(test)]
mod steam_tutorial_acceptance {
    use super::*;
    fn reach(s: &mut LiveDriveSession, stage: usize) {
        for _ in 0..600 {
            if s.state.learning.as_ref().unwrap().stage >= stage {
                return;
            }
            s.step_realtime(0.05, |_| {});
        }
        panic!(
            "Tutorial quedó en paso {} antes de {stage}: {}",
            s.state.learning.as_ref().unwrap().stage,
            s.state.boiler_state.as_ref().unwrap().status()
        );
    }
    #[test]
    fn steam_lesson_uses_actual_fire_injection_cutoff_and_braking() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/steam/scenario.toml");
        let scenario = openrailsrs_scenarios::load_scenario(&p).unwrap();
        let mut s = LiveDriveSession::from_scenario(p.parent().unwrap(), &scenario).unwrap();
        s.state.learning = Some(LearningState::new(Lesson::Steam, &s));
        reach(&mut s, 1);
        s.steam_command(crate::steam::SteamCommand::AutomaticFireman)
            .unwrap();
        reach(&mut s, 2);
        s.steam_command(crate::steam::SteamCommand::Firing(0.6))
            .unwrap();
        s.steam_command(crate::steam::SteamCommand::Damper(0.6))
            .unwrap();
        reach(&mut s, 3);
        let water = s.state.boiler_state.as_ref().unwrap().tender_water_kg;
        s.steam_command(crate::steam::SteamCommand::Injector1)
            .unwrap();
        reach(&mut s, 4);
        assert!(s.state.boiler_state.as_ref().unwrap().tender_water_kg < water);
        s.steam_command(crate::steam::SteamCommand::Injector1)
            .unwrap();
        s.steam_command(crate::steam::SteamCommand::Cutoff(-0.25))
            .unwrap();
        reach(&mut s, 5);
        s.set_direction(1.).unwrap();
        s.driver_throttle = 0.4;
        reach(&mut s, 6);
        s.steam_command(crate::steam::SteamCommand::Cutoff(-0.2))
            .unwrap();
        reach(&mut s, 7);
        s.driver_throttle = 0.;
        s.driver_brake = 0.8;
        reach(&mut s, 8);
        let saved = s.snapshot();
        let mut restored = LiveDriveSession::from_scenario(p.parent().unwrap(), &scenario).unwrap();
        restored.restore_snapshot(saved).unwrap();
        assert_eq!(restored.state.learning.as_ref().unwrap().stage, 8);
        assert_eq!(
            restored.state.boiler_state.as_ref().unwrap().coal_kg,
            s.state.boiler_state.as_ref().unwrap().coal_kg
        );
        s.steam_command(crate::steam::SteamCommand::AutomaticFireman)
            .unwrap();
        reach(&mut s, 9);
        assert!(s.state.learning.as_ref().unwrap().completed_s.is_some());
    }
}
