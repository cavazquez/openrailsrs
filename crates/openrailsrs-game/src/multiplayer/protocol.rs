use serde::{Deserialize, Serialize};
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_CLIENTS: usize = 8;
pub const MAX_TRAINS: usize = 16;
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_PENDING_COMMANDS: usize = 64;
pub const FRAME_INTERVAL_S: f64 = 0.2;
pub type PeerId = u64;

/// Never formatted in logs, UI notices, frames, or test fixtures.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
impl Secret {
    pub fn valid(&self) -> bool {
        (12..=128).contains(&self.0.len()) && !self.0.chars().any(char::is_control)
    }
    pub fn matches(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .bytes()
                .zip(other.0.bytes())
                .fold(0u8, |x, (a, b)| x | (a ^ b))
                == 0
    }
}
pub fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Driver,
    Fireman,
    Dispatcher,
    #[default]
    Spectator,
}
impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Driver => "Conductor",
            Self::Fireman => "Fogonero",
            Self::Dispatcher => "Despachador",
            Self::Spectator => "Espectador",
        }
    }
    pub fn needs_train(self) -> bool {
        matches!(self, Self::Driver | Self::Fireman)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub version: u32,
    pub room: Secret,
    pub identity: String,
    pub credential: Secret,
    pub content_id: String,
    pub role: Role,
    pub train: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverControls {
    pub throttle: f64,
    pub train_brake: f64,
    pub independent_brake: Option<f64>,
    pub direction: f64,
    pub headlights: u8,
    pub cab_light: bool,
    pub wipers: bool,
    pub horn: bool,
}
impl DriverControls {
    pub fn valid(&self) -> bool {
        [self.throttle, self.train_brake].into_iter().all(unit)
            && [0.0, 0.5, 1.0].contains(&self.direction)
            && self.headlights <= 2
            && self.independent_brake.is_none_or(unit)
    }
}
fn unit(x: f64) -> bool {
    x.is_finite() && (0.0..=1.0).contains(&x)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiremanControls {
    pub automatic: bool,
    pub firing: f64,
    pub damper: f64,
    pub injector1: bool,
    pub injector2: bool,
    pub blower: bool,
}
impl FiremanControls {
    pub fn valid(&self) -> bool {
        unit(self.firing) && unit(self.damper)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteCommand {
    Drive {
        controls: DriverControls,
    },
    SteamDriver {
        cutoff: f64,
        cylinder_cocks: bool,
    },
    ResumeRegulators,
    Fire {
        controls: FiremanControls,
    },
    Switch {
        node: String,
        position: openrailsrs_track::SwitchPosition,
    },
    Signal {
        id: String,
        aspect: Option<openrailsrs_track::SignalAspect>,
    },
    Priority {
        train: String,
        priority: i32,
    },
    Role {
        role: Role,
        train: Option<String>,
    },
    Follow {
        train: String,
    },
    Ping {
        client_millis: u64,
    },
}
impl RemoteCommand {
    pub fn valid(&self) -> bool {
        match self {
            Self::Drive { controls } => controls.valid(),
            Self::SteamDriver { cutoff, .. } => cutoff.is_finite() && (0.0..=0.75).contains(cutoff),
            Self::ResumeRegulators => true,
            Self::Fire { controls } => controls.valid(),
            Self::Switch { node, .. } => valid_id(node),
            Self::Signal { id, .. } => valid_id(id),
            Self::Priority { train, priority } => valid_id(train) && (-5..=5).contains(priority),
            Self::Role { role, train } => {
                train.as_deref().is_none_or(valid_id) && (!role.needs_train() || train.is_some())
            }
            Self::Follow { train } => valid_id(train),
            Self::Ping { .. } => true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Hello {
        hello: Hello,
    },
    Command {
        epoch: u64,
        lease: u64,
        sequence: u64,
        action: RemoteCommand,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainInfo {
    pub id: String,
    pub steam: bool,
    pub cars: usize,
    pub departed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrainView {
    pub id: String,
    pub time_s: f64,
    pub edge: String,
    pub path_edges: Vec<String>,
    pub edge_index: usize,
    pub path_reversed: bool,
    pub position_m: f64,
    pub velocity_mps: f64,
    pub odometer_m: f64,
    pub headlights: u8,
    pub wipers: bool,
    pub driver: DriverControls,
    pub fireman: Option<FiremanControls>,
    pub boiler_pressure_bar: Option<f64>,
    pub boiler_water_kg: Option<f64>,
    pub cutoff: Option<f64>,
    pub cylinder_cocks: Option<bool>,
    pub has_local_regulators: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldFrame {
    pub epoch: u64,
    pub tick: u64,
    pub time_s: f64,
    /// Spectators receive public poses, never full script/controller/save data.
    pub world: Option<Box<super::world::WorldCheckpoint>>,
    pub trains: Vec<TrainView>,
    pub roster: Vec<super::authority::RosterEntry>,
    pub environment: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        epoch: u64,
        lease: u64,
        last_sequence: u64,
        role: Role,
        train: Option<String>,
        trains: Vec<TrainInfo>,
    },
    Ack {
        sequence: u64,
        accepted: bool,
        processed: bool,
        message: String,
    },
    Frame {
        frame: Box<WorldFrame>,
    },
    Rejected {
        message: String,
    },
}
