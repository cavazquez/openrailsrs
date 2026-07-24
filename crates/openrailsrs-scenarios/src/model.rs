use serde::{Deserialize, Serialize};

use crate::ScenarioError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioFile {
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
    pub consist: String,
    #[serde(default)]
    pub davis: Option<DavisSection>,
    /// Maximum passenger capacity; `None` = unlimited.
    #[serde(default)]
    pub max_capacity: Option<u32>,
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

/// Definition for an extra train in a multi-train scenario.
///
/// The primary train is described by `[train]` + `[route]`; additional trains use
/// `[[extra_trains]]` with their own route, consist, and departure time.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrainEntryDef {
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
pub struct SimulationSection {
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
        if self.scenario.name.trim().is_empty() {
            return Err(ScenarioError::Validation(
                "scenario.name must not be empty".into(),
            ));
        }
        if self.simulation.duration <= 0.0 {
            return Err(ScenarioError::Validation(
                "simulation.duration must be positive".into(),
            ));
        }
        if self.simulation.time_step <= 0.0 {
            return Err(ScenarioError::Validation(
                "simulation.time_step must be positive".into(),
            ));
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
