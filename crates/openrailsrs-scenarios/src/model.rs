use serde::{Deserialize, Serialize};

use crate::ScenarioError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_activity: Option<openrailsrs_core::activity::ActivityDefinition>,
    pub scenario: ScenarioMeta,
    pub route: RouteSection,
    pub train: TrainSection,
    pub gameplay: GameplaySection,
    pub simulation: SimulationSection,
    #[serde(default)]
    pub output: OutputSection,
    /// Additional trains for multi-train simulation (optional).
    #[serde(default)]
    pub extra_trains: Vec<TrainEntryDef>,
    /// Ambient sound regions activated when the player train enters their
    /// `position_m ± radius_m` window on `edge_id`. Optional — emitted by the
    /// MSTS importer when the source `.tdb` declares `SoundSourceItem`s.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sound_regions: Vec<SoundRegionDef>,
    /// Optional Open Rails baseline path and comparison tolerances (metadata).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate: Option<ValidateSection>,
}

/// Optional validation thresholds and Open Rails baseline reference.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ValidateSection {
    #[serde(flatten)]
    pub thresholds: openrailsrs_validate::ValidationConfig,
    /// Path to an Open Rails `dump.csv` baseline (relative to scenario directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_or: Option<String>,
    /// Optional phase boundaries for `compare-or` diagnostics (e.g. `[0.0, 61.0, 136.0]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_bounds: Option<Vec<f64>>,
    /// Optional velocity RMS limit for phase checks (defaults to `max_velocity_rms`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_max_velocity_rms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Optional wall-clock start time (seconds since midnight). Set by MSTS
    /// imports from `(StartTime h m s)`; ignored by the simulator unless a
    /// scheduling layer reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time_s: Option<f64>,
    /// Optional season string (`"spring"`, `"summer"`, `"autumn"`, `"winter"`).
    /// Imported from MSTS `(Season ...)`; consumed by visual/weather layers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<String>,
}

/// Intermediate stop along the route with target arrival and departure times.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StopDef {
    pub node: String,
    /// Display name when the stop lies on a plain MSTS junction node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Signed metres along the route from this node to the stopping point.
    /// Negative values place the stop inside the incoming edge (MSTS platforms).
    #[serde(default)]
    pub offset_m: f64,
    pub arrive_s: f64,
    pub depart_s: f64,
    /// How long the train must dwell at this stop before departing (seconds, default 0).
    #[serde(default)]
    pub dwell_s: f64,
    /// Passengers boarding at this stop.
    #[serde(default)]
    pub passengers_on: u32,
    /// Passengers alighting at this stop.
    #[serde(default)]
    pub passengers_off: u32,
}

/// Override the runtime position of a named switch node for this scenario.
///
/// ```toml
/// [[switches]]
/// node = "junction_a"
/// position = "diverging"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SwitchDef {
    pub node: String,
    /// `"straight"` (default) or `"diverging"`.
    #[serde(default)]
    pub position: SwitchPositionDef,
}

/// String representation of a switch position used in TOML.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SwitchPositionDef {
    #[default]
    Straight,
    Diverging,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouteSection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legs: Vec<RouteLeg>,
    /// Physical supply; overrides route metadata without changing wire meshes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electric_supply: Option<openrailsrs_core::electrification::RouteElectricSupply>,
    pub path: String,
    pub start: String,
    pub destination: String,
    /// Metres from [`Self::start`] along the resolved path to the **consist head**
    /// (lead vehicle reference used by the sim / viewer) (#132).
    ///
    /// Not Open Rails `DistanceDownPath` and not the rear traveller. TrackPDP import
    /// snaps PDP[0] (OR rear); optional rear→head conversion uses consist length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset_m: Option<f64>,
    #[serde(default)]
    pub stops: Vec<StopDef>,
    /// Runtime switch overrides; applied after `track.toml` defaults.
    #[serde(default)]
    pub switches: Vec<SwitchDef>,
    /// Ordered graph node ids from the player `.pat` (import-msts). When present
    /// (≥2 entries), the sim follows this route instead of BFS start→destination.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waypoints: Vec<String>,
    /// When true, all signals start as Clear at t=0 (OR `AUTO_SIGNAL` / evaluation baselines).
    #[serde(default)]
    pub assume_signals_clear: bool,
    /// Per-edge speed cap overrides (km/h); applied after `track.toml` defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_speed_limits: Vec<EdgeSpeedLimitDef>,
}

/// Native timetable operations on a named static portion of this formation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimetableOperation {
    pub chainage_m: f64,
    pub section: String,
    pub manual: bool,
    pub action: TimetableAction,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimetableAction {
    Detach {
        keep: usize,
    },
    PickUp {
        count: usize,
    },
    JoinService {
        service: String,
        #[serde(default)]
        front: Option<bool>,
    },
    SplitService {
        service: String,
    },
}

/// An authored PAT subpath; reversals keep the same vehicle reference and
/// physical formation. Offsets refer to the complete native edge geometry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RouteLeg {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<TimetableOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    /// Relative service clock for a reused formation's next departure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_s: Option<f64>,
    pub start: String,
    pub destination: String,
    pub edges: Vec<String>,
    pub start_offset_m: f64,
    pub end_chainage_m: f64,
    pub reversed: bool,
    pub wait_s: f64,
    #[serde(default)]
    pub stops: Vec<StopDef>,
    #[serde(default)]
    pub switches: Vec<SwitchDef>,
}

/// Override `speed_limit_kmh` on one track edge for this scenario.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EdgeSpeedLimitDef {
    pub edge: String,
    pub speed_limit_kmh: f64,
}

/// Optional Davis resistance override (falls back to consist defaults if absent).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DavisSection {
    pub a_n: f64,
    pub b_n_per_mps: f64,
    pub c_n_per_mps2: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrainSection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bogie_dynamics: Vec<crate::bogie_dynamics::BogieDynamicsDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freight: Option<crate::freight::FreightService>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<crate::vehicle_systems::VehicleSystemsDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platform_sides: Vec<crate::vehicle_systems::PlatformSideDef>,
    #[serde(default)]
    pub announcements: crate::announcements::AnnouncementPackage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentation: Vec<crate::presentation::VehiclePresentation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vehicle_profiles: Vec<openrailsrs_core::vehicle_profile::VehicleProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cab_equipment: Vec<crate::cab_equipment::CabEquipmentDef>,
    /// Optional declared electrical demand; no guessed HVAC consumption.
    #[serde(default)]
    pub power_load: PowerLoadSection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub electric_pickups: Vec<openrailsrs_core::electrification::ElectricPickupOverride>,
    pub consist: String,
    #[serde(default)]
    pub davis: Option<DavisSection>,
    /// Maximum passenger capacity; `None` = unlimited.
    #[serde(default)]
    pub max_capacity: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PowerLoadSection {
    pub auxiliary_w: f64,
    pub continuous_w: f64,
    pub heating_w: f64,
    pub cooling_w: f64,
}
impl PowerLoadSection {
    pub fn total_w(&self) -> f64 {
        self.auxiliary_w + self.continuous_w + self.heating_w + self.cooling_w
    }
    pub fn valid(&self) -> bool {
        [
            self.auxiliary_w,
            self.continuous_w,
            self.heating_w,
            self.cooling_w,
        ]
        .iter()
        .all(|v| v.is_finite() && (0.0..=10_000_000.0).contains(v))
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveKind {
    #[default]
    ArriveOnTime,
    Arrive,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    #[default]
    Normal,
    Easy,
    Hard,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GameplaySection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shunting: Vec<ShuntingStep>,
    #[serde(default)]
    pub objective: ObjectiveKind,
    pub time_limit_seconds: Option<u64>,
    #[serde(default)]
    pub difficulty: Difficulty,
    /// Points deducted per second of delay beyond `STOP_GRACE_S` at each stop.
    /// Default 1.0 (linear; set to 0.0 to disable graduated penalties).
    #[serde(default = "default_penalty_rate")]
    pub penalty_per_second_late: f64,
}

fn default_penalty_rate() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShuntingKind {
    Cut,
    MoveClear,
    Switch,
    PickUp,
    Depart,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ShuntingStep {
    pub kind: ShuntingKind,
    pub instruction: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub position_m: f64,
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub switch_position: SwitchPositionDef,
}

/// Definition for an extra train in a multi-train scenario.
///
/// The primary train is described by `[train]` + `[route]`; additional trains use
/// `[[extra_trains]]` with their own route, consist, and departure time.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrainEntryDef {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bogie_dynamics: Vec<crate::bogie_dynamics::BogieDynamicsDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_uids: Vec<u32>,
    /// Ordered original PAT subpaths, retaining reversal and wait points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legs: Vec<RouteLeg>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentation: Vec<crate::presentation::VehiclePresentation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cab_equipment: Vec<crate::cab_equipment::CabEquipmentDef>,
    /// Original AI running before the activity starts; bounded pre-simulation.
    #[serde(default)]
    pub pre_roll_s: f64,
    /// Native TRF times are relative to the service's own initial time.
    #[serde(default)]
    pub relative_stop_times: bool,
    #[serde(default)]
    pub parked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freight: Option<crate::freight::FreightService>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<crate::vehicle_systems::VehicleSystemsDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub electric_pickups: Vec<openrailsrs_core::electrification::ElectricPickupOverride>,
    /// Unique identifier used in `BlockWait`/`BlockClear` events.
    pub id: String,
    /// Path to the consist file (relative to scenario directory).
    pub consist: String,
    /// Start node id in the route graph.
    pub start: String,
    /// Destination node id in the route graph.
    pub destination: String,
    /// Simulated time (seconds from t=0) at which this train departs.
    #[serde(default)]
    pub start_time_s: f64,
    /// Explicit route through junctions; an empty list uses the normal path resolver.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waypoints: Vec<String>,
    /// Initial head chainage along this service's own path, in metres.
    #[serde(default)]
    pub start_offset_m: f64,
    /// Intermediate stops for this train.
    #[serde(default)]
    pub stops: Vec<StopDef>,
    /// Optional Davis resistance override.
    #[serde(default)]
    pub davis: Option<DavisSection>,
    /// Switch overrides specific to this train's path.
    #[serde(default)]
    pub switches: Vec<SwitchDef>,
    /// Output CSV filename (relative to scenario directory).
    pub output_csv: String,
}

/// Ambient sound region anchored to a track edge.
///
/// The cab runtime treats a region as active when the player train is on
/// `edge_id` and `(position_m - region.position_m).abs() <= radius_m`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SoundRegionDef {
    /// Stable identifier (e.g. `"sr12"` derived from the source `TrItemId`).
    pub id: String,
    /// Track edge id from `track.toml`.
    pub edge_id: String,
    /// Position along the edge in metres.
    pub position_m: f64,
    /// Activation radius (metres). Default `50.0` when the importer has no
    /// override.
    pub radius_m: f64,
    /// Region kind (`"ambient"`, `"tunnel"`, `"depot"`, ...).
    pub kind: String,
    /// Base playback volume in `[0.0, 1.0]`.
    pub base_volume: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NativePhysicsEnvironment {
    pub ambient_c: f64,
    pub adhesion_factor: f64,
    /// Match OR's simple physics capture without disabling native resistance,
    /// controls or brakes. Normal gameplay keeps the Pacha axle model.
    #[serde(default)]
    pub simple_control_physics: bool,
    #[serde(default)]
    pub first_slippery_spot: Option<[f64; 2]>,
    #[serde(default)]
    pub paused_weather: bool,
}
impl Default for NativePhysicsEnvironment {
    fn default() -> Self {
        Self {
            ambient_c: 20.,
            adhesion_factor: 1.,
            simple_control_physics: false,
            first_slippery_spot: None,
            paused_weather: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimulationSection {
    #[serde(default)]
    pub rail_operations: crate::rail_operations::RailOperationsDef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather_track: Option<crate::weather_track::WeatherTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_physics: Option<NativePhysicsEnvironment>,
    pub duration: f64,
    pub time_step: f64,
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// PSI divisor for OR `BRAKEPRESSURE` when building `driver_or.csv` (default 121).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_brake_full_scale_psi: Option<f64>,
    /// PSI reference for full cylinder force; defaults to `driver_brake_full_scale_psi`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brake_cylinder_full_scale_psi: Option<f64>,
    /// Use pre-OR-P1 diesel P/v cap (default true for baseline compatibility).
    #[serde(default = "default_legacy_power_cap")]
    pub legacy_power_cap: bool,
    /// Hold train-air cylinders during driver lap release (Chiltern activity brake bleed).
    #[serde(default)]
    pub train_air_lap_hold: bool,
    /// Train-air dump time (s) after driver full release when lap-hold is enabled.
    #[serde(default = "default_train_air_full_release_s")]
    pub train_air_full_release_s: f64,
    /// Scale wheel-rim brake force by μ(v)/μ(0) from OR shoe curves (OR-P6b).
    #[serde(default)]
    pub brake_shoe_speed_factor: bool,
    /// Cap per-vehicle brake at mass × g × μ_adhesion (OR-P6c).
    #[serde(default)]
    pub brake_skid_limit: bool,
    /// Per-vehicle coupler dynamics (OR-P4). Default off until validated on long runs.
    #[serde(default)]
    pub multi_body: bool,
    /// Coupler preset when `multi_body = true`: `freight` (default) or `passenger`.
    #[serde(default)]
    pub coupler_kind: String,
    /// Below this speed (m/s), with throttle off, multi-body uses scalar Davis+brake
    /// decay (matches OR coast after service brake). Omit to keep full coupler dynamics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_body_scalar_coast_below_v_mps: Option<f64>,
    /// Deprecated experiment (OR-P6): partial MSTS run-up on ORTS lead. OR uses RPM dynamics.
    #[serde(default)]
    pub orts_inherit_partial_run_up: bool,
}

fn default_train_air_full_release_s() -> f64 {
    3.0
}

fn default_legacy_power_cap() -> bool {
    true
}

fn default_seed() -> u64 {
    42
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputSection {
    pub csv: String,
    pub metadata: String,
}

impl Default for OutputSection {
    fn default() -> Self {
        Self {
            csv: "run.csv".to_string(),
            metadata: "run.json".to_string(),
        }
    }
}

impl ScenarioFile {
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if self.native_activity.as_ref().is_some_and(|a| !a.valid()) {
            return Err(ScenarioError::Validation(
                "Definición original ACT inválida".into(),
            ));
        }
        if !self.train.announcements.valid() {
            return Err(ScenarioError::Validation(
                "Paquete de anuncios sin procedencia/licencia o inválido".into(),
            ));
        }
        if self.train.freight.as_ref().is_some_and(|p| !p.valid())
            || self.extra_trains.iter().any(|e| {
                !e.pre_roll_s.is_finite()
                    || !(0. ..=21600.).contains(&e.pre_roll_s)
                    || e.freight.as_ref().is_some_and(|p| !p.valid())
                    || !crate::vehicle_systems::valid_definitions(&e.systems, &[])
            })
        {
            return Err(crate::ScenarioError::Validation(
                "Servicio de mercancías o equipos de tráfico inválidos".into(),
            ));
        }
        if !crate::vehicle_systems::valid_definitions(
            &self.train.systems,
            &self.train.platform_sides,
        ) {
            return Err(ScenarioError::Validation(
                "Equipos de vehículo o lados de andén inválidos".into(),
            ));
        }
        if self.train.bogie_dynamics.len() > 256
            || self.train.bogie_dynamics.iter().any(|v| !v.valid())
            || self
                .train
                .bogie_dynamics
                .iter()
                .map(|v| v.vehicle)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.train.bogie_dynamics.len()
        {
            return Err(ScenarioError::Validation(
                "Dinámica de bogies inválida".into(),
            ));
        }
        if self.train.presentation.len() > 256
            || self.train.presentation.iter().any(|v| !v.valid())
            || self
                .train
                .presentation
                .iter()
                .map(|v| v.vehicle)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.train.presentation.len()
        {
            return Err(ScenarioError::Validation(
                "Presentación de vehículo inválida".into(),
            ));
        }
        if !self.simulation.rail_operations.valid() {
            return Err(ScenarioError::Validation(
                "Operaciones ferroviarias inválidas".into(),
            ));
        }
        if self.train.vehicle_profiles.len() > 256
            || self.train.vehicle_profiles.iter().any(|p| !p.valid())
        {
            return Err(ScenarioError::Validation(
                "Perfil de vehículo inválido".into(),
            ));
        }
        if self.train.cab_equipment.len() > 256
            || self.train.cab_equipment.iter().any(|c| !c.valid())
            || self
                .train
                .cab_equipment
                .iter()
                .map(|c| c.vehicle)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.train.cab_equipment.len()
        {
            return Err(ScenarioError::Validation(
                "Equipo de cabina inválido o repetido".into(),
            ));
        }
        if self
            .simulation
            .weather_track
            .as_ref()
            .is_some_and(|p| !p.valid())
        {
            return Err(ScenarioError::Validation(
                "Evolución meteorológica inválida".into(),
            ));
        }
        if self.route.legs.len() > 256
            || self.extra_trains.iter().any(|e| {
                e.legs.len() > 256
                    || e.bogie_dynamics.len() > 256
                    || e.bogie_dynamics.iter().any(|v| !v.valid())
                    || e.bogie_dynamics
                        .iter()
                        .map(|v| v.vehicle)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != e.bogie_dynamics.len()
                    || e.presentation.len() > 256
                    || e.presentation.iter().any(|v| !v.valid())
                    || e.cab_equipment.len() > 256
                    || e.cab_equipment.iter().any(|c| !c.valid())
            })
            || self
                .route
                .legs
                .iter()
                .chain(self.extra_trains.iter().flat_map(|e| &e.legs))
                .any(|leg| {
                    leg.edges.is_empty()
                        || leg.edges.len() > 50_000
                        || ![leg.start_offset_m, leg.end_chainage_m, leg.wait_s]
                            .iter()
                            .all(|n| n.is_finite() && *n >= 0.0)
                        || leg.end_chainage_m < leg.start_offset_m
                        || leg.operations.len() > 256
                        || leg.operations.iter().any(|op| {
                            !op.chainage_m.is_finite()
                                || op.chainage_m < leg.start_offset_m - 1.0
                                || op.chainage_m > leg.end_chainage_m + 1.0
                                || op.section.trim().is_empty()
                                || op.section.len() > 256
                                || match op.action {
                                    TimetableAction::Detach { keep } => !(1..=256).contains(&keep),
                                    TimetableAction::PickUp { count } => {
                                        !(1..=256).contains(&count)
                                    }
                                    TimetableAction::JoinService { ref service, .. }
                                    | TimetableAction::SplitService { ref service } => {
                                        service.trim().is_empty()
                                            || service.len() > 256
                                            || service.chars().any(char::is_control)
                                    }
                                }
                        })
                        || leg
                            .operations
                            .windows(2)
                            .any(|ops| ops[0].chainage_m > ops[1].chainage_m)
                        || leg
                            .not_before_s
                            .is_some_and(|n| !n.is_finite() || !(0.0..=172800.0).contains(&n))
                        || leg
                            .service_id
                            .as_ref()
                            .is_some_and(|id| id.trim().is_empty() || id.len() > 256)
                })
        {
            return Err(ScenarioError::Validation(
                "Invalid operational itinerary".into(),
            ));
        }
        if self.gameplay.shunting.len() > 100
            || self.gameplay.shunting.iter().any(|step| {
                step.instruction.trim().is_empty()
                    || step.instruction.len() > 500
                    || !step.position_m.is_finite()
                    || step.position_m < 0.0
                    || matches!(step.kind, ShuntingKind::Cut | ShuntingKind::PickUp)
                        && (step.count == 0 || step.count > 1000)
                    || step.kind == ShuntingKind::Switch && step.node.is_empty()
            })
        {
            return Err(ScenarioError::Validation(
                "Invalid shunting objectives".into(),
            ));
        }
        if !self.train.power_load.valid() {
            return Err(ScenarioError::Validation(
                "train.power_load must contain finite nonnegative watts".into(),
            ));
        }
        if self.scenario.name.trim().is_empty() {
            return Err(ScenarioError::Validation(
                "scenario.name must not be empty".into(),
            ));
        }
        if !self.simulation.duration.is_finite() || self.simulation.duration <= 0.0 {
            return Err(ScenarioError::Validation(
                "simulation.duration must be positive".into(),
            ));
        }
        if !self.simulation.time_step.is_finite() || self.simulation.time_step <= 0.0 {
            return Err(ScenarioError::Validation(
                "simulation.time_step must be positive".into(),
            ));
        }
        if let Some(native) = &self.simulation.native_physics
            && (!native.ambient_c.is_finite()
                || !(-100.0..=100.0).contains(&native.ambient_c)
                || !native.adhesion_factor.is_finite()
                || !(0.05..=2.5).contains(&native.adhesion_factor)
                || native
                    .first_slippery_spot
                    .is_some_and(|[distance, length]| {
                        !distance.is_finite()
                            || !length.is_finite()
                            || distance < 0.
                            || length <= 0.
                    }))
        {
            return Err(ScenarioError::Validation("native physics environment: finite temperature, adhesion and positive slippery spot length required".into()));
        }
        if self.route.path.trim().is_empty() {
            return Err(ScenarioError::Validation("route.path is required".into()));
        }
        if self.output.csv.trim().is_empty() || self.output.metadata.trim().is_empty() {
            return Err(ScenarioError::Validation(
                "output.csv and output.metadata are required".into(),
            ));
        }
        for stop in &self.route.stops {
            if !stop.arrive_s.is_finite()
                || !stop.depart_s.is_finite()
                || !stop.dwell_s.is_finite()
                || !stop.offset_m.is_finite()
                || stop.arrive_s < 0.0
                || stop.dwell_s < 0.0
            {
                return Err(ScenarioError::Validation(format!(
                    "stop '{}': times and offset must be finite; arrival and dwell must be non-negative",
                    stop.node
                )));
            }
            if stop.arrive_s > stop.depart_s {
                return Err(ScenarioError::Validation(format!(
                    "stop '{}': arrive_s ({}) must be <= depart_s ({})",
                    stop.node, stop.arrive_s, stop.depart_s
                )));
            }
        }
        Ok(())
    }

    /// Brake pressure mapping for OR driver export and physics cylinder force.
    pub fn brake_mapping(&self) -> openrailsrs_validate::BrakeCommandMapping {
        openrailsrs_validate::BrakeCommandMapping::from_scenario_fields(
            self.simulation.driver_brake_full_scale_psi,
            self.simulation.brake_cylinder_full_scale_psi,
        )
    }
}
