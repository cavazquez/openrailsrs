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
    Rain,
    Snow,
    Fog,
}
impl Lesson {
    pub const ALL: [Self; 8] = [
        Self::Basic,
        Self::Electric,
        Self::Emergency,
        Self::Battery,
        Self::Diesel,
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
