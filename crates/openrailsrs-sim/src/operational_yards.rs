//! Shared physical yard machinery and service ownership. This is simulation
//! state: menus and cameras can only issue commands and read the resulting pose.
use openrailsrs_scenarios::operational_yards::YardOperationsDef;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct YardWorldState {
    pub definition: YardOperationsDef,
    pub devices: BTreeMap<String, MovingDeviceState>,
    pub pool_members: BTreeMap<String, PoolMemberState>,
    pub completed_pool_jobs: BTreeSet<(String, String)>,
    pub rendezvous: BTreeMap<String, MeetingState>,
    pub run_arounds: BTreeMap<String, RunAroundState>,
    pub released: BTreeSet<String>,
    pub gravity_routes: BTreeMap<String, usize>,
    pub free_rolling_services: BTreeSet<String>,
    /// Actual post-contact travel along (false) or against (true) each path.
    pub rolling_backwards: BTreeMap<String, bool>,
    pub retarders: BTreeMap<String, RetarderState>,
    pub recovery: BTreeMap<String, RecoveryState>,
    pub contacts: Vec<YardContact>,
    pub actor_id: String,
    /// Ephemeral authority hint. Network assignments belong to Authority.
    #[serde(skip)]
    pub human_driven: bool,
    pub notice: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MovingDeviceState {
    pub position: f64,
    pub aligned_port: usize,
    pub target_port: Option<usize>,
    pub carrier: Option<CarrierState>,
    pub started_s: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CarrierState {
    pub service: String,
    pub vehicle_ids: Vec<String>,
    pub source_port: usize,
    pub head_on_deck_m: f64,
    #[serde(default)]
    pub vehicle_reversed: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PoolMemberState {
    pub pool: String,
    pub reserved_job: Option<String>,
    pub dispatched: bool,
    pub reserved_at_s: Option<f64>,
    pub checkout_s: Option<f64>,
    pub last_departure_s: f64,
    pub returns: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingState {
    pub waiting_since_s: Option<f64>,
    pub last_attempt_s: Option<f64>,
    pub complete: bool,
    pub failed: bool,
    pub notice: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunAroundState {
    pub active: bool,
    pub completed: bool,
    pub phase: usize,
    pub notice: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetarderState {
    pub fraction: f64,
    pub absorbed_j: BTreeMap<String, f64>,
    #[serde(default)]
    pub last_forces: Vec<(usize, f64, f64)>,
    #[serde(default)]
    pub last_positions: Vec<(usize, f64)>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoveryState {
    pub protected_at_s: Option<f64>,
    pub recovered_at_s: Option<f64>,
    pub failed: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct YardContact {
    pub time_s: f64,
    pub left: String,
    pub right: String,
    pub relative_speed_mps: f64,
    pub impulse_ns: f64,
    pub absorbed_j: f64,
    pub unsafe_impact: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum YardCommand {
    MoveDevice { device: String, port: usize },
    ReservePool { pool: String, job: String },
    DispatchPool { member: String },
    ReturnPool { member: String },
    StartRunAround { id: String },
    ReleaseGravity { id: String },
    RouteGravity { id: String, route: usize },
    SecureGravity { id: String },
    SetRetarder { id: String, fraction: f64 },
    ProtectRecovery { id: String },
    SecureRecovery { id: String },
}
impl YardWorldState {
    pub fn from_definition(definition: YardOperationsDef) -> Self {
        let devices = definition
            .moving_devices
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    MovingDeviceState {
                        position: d.ports.get(d.initial_port).map_or(0., |p| p.position),
                        aligned_port: d.initial_port,
                        target_port: None,
                        carrier: None,
                        started_s: 0.,
                    },
                )
            })
            .collect();
        let pool_members = definition
            .pools
            .iter()
            .flat_map(|p| {
                p.members.iter().map(move |id| {
                    (
                        id.clone(),
                        PoolMemberState {
                            pool: p.id.clone(),
                            ..Default::default()
                        },
                    )
                })
            })
            .collect();
        let retarders = definition
            .retarders
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    RetarderState {
                        fraction: r.default_fraction,
                        absorbed_j: BTreeMap::new(),
                        last_forces: vec![],
                        last_positions: vec![],
                    },
                )
            })
            .collect();
        let rendezvous = definition
            .rendezvous
            .iter()
            .map(|r| (r.id.clone(), MeetingState::default()))
            .collect();
        let run_arounds = definition
            .run_arounds
            .iter()
            .map(|r| (r.id.clone(), RunAroundState::default()))
            .collect();
        let recovery = definition
            .recovery_exercises
            .iter()
            .map(|r| (r.id.clone(), RecoveryState::default()))
            .collect();
        Self {
            definition,
            devices,
            pool_members,
            retarders,
            rendezvous,
            run_arounds,
            recovery,
            actor_id: "Jugador".into(),
            ..Default::default()
        }
    }
    pub fn valid_for(&self, expected: &YardOperationsDef) -> bool {
        self.definition.valid()
            && &self.definition == expected
            && self.notice.len() <= 2048
            && self.actor_id.len() <= 256
            && self.contacts.len() <= 256
            && self.devices.len() == expected.moving_devices.len()
            && self.devices.iter().all(|(id, s)| {
                expected
                    .moving_devices
                    .iter()
                    .find(|d| &d.id == id)
                    .is_some_and(|d| {
                        s.position.is_finite()
                            && s.aligned_port < d.ports.len()
                            && s.target_port.is_none_or(|p| p < d.ports.len())
                            && s.started_s.is_finite()
                            && s.started_s >= 0.
                            && (s.carrier.is_none() || s.target_port.is_some())
                            && s.carrier.as_ref().is_none_or(|c| {
                                !c.service.is_empty()
                                    && c.service.len() <= 256
                                    && c.source_port < d.ports.len()
                                    && c.source_port == s.aligned_port
                                    && !c.vehicle_ids.is_empty()
                                    && c.vehicle_ids.len() <= 256
                                    && c.vehicle_ids
                                        .iter()
                                        .all(|id| !id.is_empty() && id.len() <= 512)
                                    && c.vehicle_ids.iter().collect::<BTreeSet<_>>().len()
                                        == c.vehicle_ids.len()
                                    && c.head_on_deck_m.is_finite()
                                    && (0. ..=d.deck_length_m).contains(&c.head_on_deck_m)
                            })
                            && if let Some(target) = s.target_port {
                                let a = d.ports[s.aligned_port].position;
                                let b = d.ports[target].position;
                                s.position >= a.min(b) - 1e-9 && s.position <= a.max(b) + 1e-9
                            } else {
                                (s.position - d.ports[s.aligned_port].position).abs() < 1e-9
                            }
                    })
            })
            && self.pool_members.len()
                == expected
                    .pools
                    .iter()
                    .map(|p| p.members.len())
                    .sum::<usize>()
            && self.pool_members.iter().all(|(id, s)| {
                expected
                    .pools
                    .iter()
                    .find(|p| p.id == s.pool)
                    .is_some_and(|p| {
                        p.members.contains(id)
                            && s.reserved_job
                                .as_ref()
                                .is_none_or(|j| p.jobs.iter().any(|job| &job.id == j))
                            && s.reserved_at_s.is_none_or(|t| t.is_finite() && t >= 0.)
                            && s.checkout_s.is_none_or(|t| t.is_finite() && t >= 0.)
                            && s.last_departure_s.is_finite()
                            && s.last_departure_s >= 0.
                            && (!s.dispatched || s.reserved_job.is_some() && s.checkout_s.is_some())
                            && s.returns <= 1_000_000
                    })
            })
            && self.completed_pool_jobs.iter().all(|(p, j)| {
                expected
                    .pools
                    .iter()
                    .find(|d| &d.id == p)
                    .is_some_and(|d| d.jobs.iter().any(|job| &job.id == j))
            })
            && self
                .pool_members
                .values()
                .filter_map(|s| s.reserved_job.as_ref().map(|j| (&s.pool, j)))
                .collect::<BTreeSet<_>>()
                .len()
                == self
                    .pool_members
                    .values()
                    .filter(|s| s.reserved_job.is_some())
                    .count()
            && self.rendezvous.len() <= expected.rendezvous.len()
            && self.rendezvous.iter().all(|(id, m)| {
                expected.rendezvous.iter().any(|r| &r.id == id)
                    && m.waiting_since_s.is_none_or(|t| t.is_finite() && t >= 0.)
                    && m.last_attempt_s.is_none_or(|t| t.is_finite() && t >= 0.)
                    && m.notice.len() <= 2048
                    && !(m.complete && m.failed)
            })
            && self.run_arounds.len() <= expected.run_arounds.len()
            && self.run_arounds.iter().all(|(id, r)| {
                expected
                    .run_arounds
                    .iter()
                    .find(|p| &p.id == id)
                    .is_some_and(|p| {
                        r.phase <= p.legs.len()
                            && r.notice.len() <= 2048
                            && !(r.active && r.completed)
                    })
            })
            && self.gravity_routes.iter().all(|(id, i)| {
                expected
                    .gravity_releases
                    .iter()
                    .find(|r| &r.id == id)
                    .is_some_and(|r| *i < r.routes.len())
            })
            && self
                .released
                .iter()
                .all(|id| expected.gravity_releases.iter().any(|r| &r.id == id))
            && self.free_rolling_services.iter().all(|id| {
                expected.gravity_releases.iter().any(|r| &r.service == id)
                    || self
                        .contacts
                        .iter()
                        .any(|c| &c.left == id || &c.right == id)
            })
            && self
                .rolling_backwards
                .keys()
                .all(|id| self.free_rolling_services.contains(id))
            && self.retarders.len() == expected.retarders.len()
            && self.retarders.iter().all(|(id, r)| {
                expected.retarders.iter().any(|d| &d.id == id)
                    && r.fraction.is_finite()
                    && (0. ..=1.).contains(&r.fraction)
                    && r.absorbed_j.len() <= 256
                    && r.absorbed_j
                        .values()
                        .all(|v| v.is_finite() && *v >= 0. && *v <= 1e15)
                    && r.last_positions.len() <= 256
                    && r.last_positions
                        .iter()
                        .all(|(i, p)| *i < 256 && p.is_finite() && p.abs() <= 1e15)
                    && r.last_forces.len() <= 256
                    && r.last_forces.iter().all(|(i, f, v)| {
                        *i < 256
                            && f.is_finite()
                            && *f >= 0.
                            && *f <= 2e6
                            && v.is_finite()
                            && *v >= 0.
                            && *v <= 1000.
                    })
            })
            && self.recovery.len() <= expected.recovery_exercises.len()
            && self.recovery.iter().all(|(id, r)| {
                expected.recovery_exercises.iter().any(|d| &d.id == id)
                    && r.protected_at_s.is_none_or(|t| t.is_finite() && t >= 0.)
                    && r.recovered_at_s.is_none_or(|t| t.is_finite() && t >= 0.)
                    && r.failed.as_ref().is_none_or(|s| s.len() <= 2048)
            })
            && self.contacts.iter().all(|c| {
                [c.time_s, c.relative_speed_mps, c.impulse_ns, c.absorbed_j]
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0.)
                    && c.left.len() <= 256
                    && c.right.len() <= 512
            })
    }
}
impl crate::LiveTraffic {
    pub(crate) fn publish_yard_world(
        &mut self,
        player: &mut crate::LiveDriveSession,
        mut world: YardWorldState,
    ) {
        for session in std::iter::once(&*player).chain(self.services.iter().map(|s| &s.session)) {
            for (id, r) in &session.state.yard_operations.retarders {
                if let Some(shared) = world.retarders.get_mut(id) {
                    for (service, work) in &r.absorbed_j {
                        let total = shared.absorbed_j.entry(service.clone()).or_default();
                        *total = total.max(*work);
                    }
                }
            }
        }
        let player_human = player.state.yard_operations.human_driven;
        world.actor_id = player.service_id.clone();
        player.state.yard_operations = world.clone();
        player.state.yard_operations.human_driven = player_human;
        for service in &mut self.services {
            let samples = service
                .session
                .state
                .yard_operations
                .retarders
                .iter()
                .map(|(id, r)| (id.clone(), r.last_forces.clone(), r.last_positions.clone()))
                .collect::<Vec<_>>();
            let human = service.session.state.yard_operations.human_driven;
            world.actor_id = service.id.clone();
            service.session.state.yard_operations = world.clone();
            service.session.state.yard_operations.human_driven = human;
            // The shared authority has one pose/ledger, while forces and
            // pre-step positions belong to the actor that was integrated.
            for (id, forces, positions) in samples {
                if let Some(r) = service.session.state.yard_operations.retarders.get_mut(&id) {
                    r.last_forces = forces;
                    r.last_positions = positions;
                }
            }
        }
    }
    pub fn command_yard(
        &mut self,
        player: &mut crate::LiveDriveSession,
        command: YardCommand,
    ) -> Result<(), String> {
        let result = match command {
            YardCommand::MoveDevice { device, port } => {
                self.request_moving_device(player, &device, port)
            }
            YardCommand::ReservePool { pool, job } => {
                self.reserve_pool_material(player, &pool, &job).map(|_| ())
            }
            YardCommand::DispatchPool { member } => self.dispatch_pool_material(player, &member),
            YardCommand::ReturnPool { member } => self.return_pool_material(player, &member),
            YardCommand::StartRunAround { id } => self.start_ai_runaround(player, &id),
            YardCommand::ReleaseGravity { id } => self.release_gravity_train(player, &id),
            YardCommand::RouteGravity { id, route } => self.route_gravity_train(player, &id, route),
            YardCommand::SecureGravity { id } => self.secure_gravity_train(player, &id),
            YardCommand::SetRetarder { id, fraction } => {
                self.set_yard_retarder(player, &id, fraction)
            }
            YardCommand::ProtectRecovery { id } => self.protect_recovery(player, &id),
            YardCommand::SecureRecovery { id } => self.secure_recovery(player, &id),
        };
        if let Err(error) = &result {
            player.state.yard_operations.notice = error.clone();
        }
        result
    }
    pub(crate) fn tick_yard_operations(&mut self, player: &mut crate::LiveDriveSession, dt: f64) {
        if player.state.yard_operations.definition.is_empty() || !dt.is_finite() || dt <= 0. {
            return;
        }
        let mut world = player.state.yard_operations.clone();
        self.advance_moving_devices(player, &mut world, dt);
        self.publish_yard_world(player, world);
        self.tick_yard_gravity(player);
        self.tick_mobile_meetings(player);
        self.tick_ai_runarounds(player);
        let world = player.state.yard_operations.clone();
        self.publish_yard_world(player, world);
    }
}

/// Validate references once before loading any actors. An absent author train
/// is a content error, not a request to synthesize a similarly named vehicle.
pub fn validate_yard_actor_definitions(
    s: &openrailsrs_scenarios::ScenarioFile,
) -> Result<(), String> {
    let d = &s.simulation.yard_operations;
    if !d.valid() {
        return Err("Definición del patio inválida".into());
    }
    let known = |id: &str| id == "Jugador" || s.extra_trains.iter().any(|t| t.id == id);
    for id in d
        .rendezvous
        .iter()
        .flat_map(|r| [&r.receiver, &r.donor])
        .chain(d.run_arounds.iter().map(|r| &r.service))
        .chain(d.gravity_releases.iter().map(|r| &r.service))
        .chain(d.recovery_exercises.iter().map(|r| &r.service))
    {
        if !known(id) {
            return Err(format!("Patio: servicio original ausente: {id}"));
        }
    }
    for pool in &d.pools {
        for id in &pool.members {
            if s.extra_trains
                .iter()
                .find(|t| &t.id == id)
                .is_none_or(|t| !t.parked)
            {
                return Err(format!(
                    "Pool {}: {id} debe ser material físico estacionado",
                    pool.id
                ));
            }
        }
    }
    Ok(())
}

/// World machinery and allocations are authoritative once for all trains.
/// Per-actor transient retarder samples and its name are deliberately excluded;
/// poses, reservations, impacts, work ledgers and completion must be identical.
pub fn validate_shared_yard_snapshots<'a>(
    snapshots: impl IntoIterator<Item = &'a crate::SessionSnapshot>,
) -> Result<(), String> {
    let mut canonical = None;
    let mut actors = BTreeMap::new();
    let snapshots = snapshots.into_iter().collect::<Vec<_>>();
    for snapshot in &snapshots {
        let mut world = snapshot.state.yard_operations.clone();
        if !world.valid_for(&world.definition) {
            return Err("Patio compartido guardado inválido".into());
        }
        actors.insert(world.actor_id.clone(), *snapshot);
        world.actor_id.clear();
        for r in world.retarders.values_mut() {
            r.last_forces.clear();
            r.last_positions.clear();
        }
        let bytes = serde_json::to_vec(&world).map_err(|e| e.to_string())?;
        if canonical.as_ref().is_some_and(|c| c != &bytes) {
            return Err(
                "Las formaciones guardaron poses o asignaciones incompatibles del mismo patio"
                    .into(),
            );
        }
        canonical = Some(bytes);
    }
    // A caller may validate the traffic-only subset first; resolve each carrier
    // against this set when available, then the combined player+traffic set.
    for snapshot in &snapshots {
        for d in snapshot.state.yard_operations.devices.values() {
            if let Some(c) = &d.carrier
                && let Some(owner) = actors.get(&c.service)
            {
                let ids = if owner.formation.vehicle_ids.is_empty() {
                    (0..owner.formation.cars.len())
                        .map(|i| format!("{}/car/{i}", c.service))
                        .collect()
                } else {
                    owner.formation.vehicle_ids.clone()
                };
                if ids != c.vehicle_ids
                    || owner.formation.coupled_count != owner.formation.cars.len()
                {
                    return Err(
                        "La carga de la mesa guardada no coincide con su formación física".into(),
                    );
                }
            }
        }
    }
    Ok(())
}
