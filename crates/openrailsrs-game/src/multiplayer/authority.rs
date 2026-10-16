use super::protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub identity: String,
    pub credential: Secret,
    pub role: Role,
    pub train: Option<String>,
    pub followed: Option<String>,
    pub last_sequence: u64,
    pub lease: u64,
    #[serde(default)]
    pub receipts: Vec<CommandReceipt>,
    #[serde(skip)]
    pub peer: Option<PeerId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub sequence: u64,
    pub accepted: bool,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RosterEntry {
    pub identity: String,
    pub role: Role,
    pub train: Option<String>,
    pub followed: Option<String>,
    pub connected: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthorityCheckpoint {
    pub version: u32,
    pub room: Secret,
    pub content_id: String,
    pub epoch: u64,
    pub next_lease: u64,
    pub members: BTreeMap<String, Member>,
}
pub struct Authority {
    saved: AuthorityCheckpoint,
    peers: HashMap<PeerId, String>,
}
impl Authority {
    pub fn new(room: Secret, content_id: String) -> Result<Self, String> {
        if !room.valid() || !valid_id(&content_id) {
            return Err("Clave de sala o contenido inválidos".into());
        }
        Ok(Self {
            saved: AuthorityCheckpoint {
                version: PROTOCOL_VERSION,
                room,
                content_id,
                epoch: 1,
                next_lease: 1,
                members: BTreeMap::new(),
            },
            peers: HashMap::new(),
        })
    }
    pub fn restore(saved: AuthorityCheckpoint, trains: &[TrainInfo]) -> Result<Self, String> {
        let mut authority = Self::new(saved.room.clone(), saved.content_id.clone())?;
        if saved.version != PROTOCOL_VERSION
            || saved.members.len() > MAX_CLIENTS
            || saved.epoch == u64::MAX
            || saved.next_lease == 0
            || saved.next_lease == u64::MAX
        {
            return Err("Sesión de red incompatible".into());
        }
        let mut slots = HashSet::new();
        let mut leases = HashSet::new();
        for (id, member) in &saved.members {
            if id != &member.identity
                || !valid_id(id)
                || !member.credential.valid()
                || member.lease == 0
                || member.lease >= saved.next_lease
                || !leases.insert(member.lease)
                || member.last_sequence == u64::MAX
                || member.receipts.len() > MAX_PENDING_COMMANDS
                || member.receipts.iter().any(|r| {
                    r.sequence == 0 || r.sequence > member.last_sequence || r.message.len() > 1024
                })
                || member
                    .receipts
                    .windows(2)
                    .any(|pair| pair[0].sequence >= pair[1].sequence)
                || member
                    .followed
                    .as_ref()
                    .is_some_and(|id| !trains.iter().any(|t| &t.id == id))
            {
                return Err("Identidad o concesión de red guardada inválida".into());
            }
            check_role(member.role, member.train.as_deref(), trains)?;
            if let Some(slot) = role_slot(member.role, member.train.as_deref())
                && !slots.insert(slot)
            {
                return Err("La partida concede un mismo puesto a dos personas".into());
            }
        }
        authority.saved = saved;
        authority.saved.epoch += 1;
        for member in authority.saved.members.values_mut() {
            member.peer = None;
        }
        Ok(authority)
    }
    pub fn checkpoint(&self) -> AuthorityCheckpoint {
        self.saved.clone()
    }
    pub fn epoch(&self) -> u64 {
        self.saved.epoch
    }
    pub fn content_id(&self) -> &str {
        &self.saved.content_id
    }
    pub fn member(&self, peer: PeerId) -> Option<&Member> {
        self.saved.members.get(self.peers.get(&peer)?)
    }
    pub fn member_by_id(&self, id: &str) -> Option<&Member> {
        self.saved.members.get(id)
    }
    pub fn roster(&self) -> Vec<RosterEntry> {
        self.saved
            .members
            .values()
            .map(|m| RosterEntry {
                identity: m.identity.clone(),
                role: m.role,
                train: m.train.clone(),
                followed: m.followed.clone(),
                connected: m.peer.is_some(),
            })
            .collect()
    }
    pub fn peers(&self) -> Vec<PeerId> {
        self.peers.keys().copied().collect()
    }
    pub fn connect(
        &mut self,
        peer: PeerId,
        hello: Hello,
        trains: &[TrainInfo],
    ) -> Result<ServerMessage, String> {
        if hello.version != PROTOCOL_VERSION || !hello.room.matches(&self.saved.room) {
            return Err("Sala o versión del protocolo incorrecta".into());
        }
        if hello.content_id != self.saved.content_id {
            return Err("La ruta, el servicio o la versión del contenido no coinciden".into());
        }
        if !valid_id(&hello.identity)
            || !hello.credential.valid()
            || self.peers.contains_key(&peer)
            || self.saved.next_lease == u64::MAX
        {
            return Err("Identidad o conexión inválida".into());
        }
        let next = self.saved.next_lease;
        if let Some(member) = self.saved.members.get_mut(&hello.identity) {
            if !hello.credential.matches(&member.credential) {
                return Err("Esa identidad pertenece a otra persona".into());
            }
            if let Some(old) = member.peer {
                self.peers.remove(&old);
            }
            member.peer = Some(peer);
            member.lease = next;
        } else {
            if self.saved.members.len() >= MAX_CLIENTS {
                return Err("La sala admite hasta ocho identidades".into());
            }
            self.check_available(&hello.identity, hello.role, hello.train.as_deref(), trains)?;
            self.saved.members.insert(
                hello.identity.clone(),
                Member {
                    identity: hello.identity.clone(),
                    credential: hello.credential,
                    role: hello.role,
                    train: hello.train.clone(),
                    followed: hello.train,
                    last_sequence: 0,
                    lease: next,
                    receipts: vec![],
                    peer: Some(peer),
                },
            );
        }
        self.saved.next_lease += 1;
        self.peers.insert(peer, hello.identity.clone());
        let member = &self.saved.members[&hello.identity];
        Ok(ServerMessage::Welcome {
            epoch: self.saved.epoch,
            lease: member.lease,
            last_sequence: member.last_sequence,
            role: member.role,
            train: member.train.clone(),
            trains: trains.to_vec(),
        })
    }
    pub fn disconnect(&mut self, peer: PeerId) -> Option<Member> {
        let id = self.peers.remove(&peer)?;
        let member = self.saved.members.get_mut(&id)?;
        if member.peer == Some(peer) {
            member.peer = None;
        }
        Some(member.clone())
    }
    /// Includes disconnected drivers: their parked authority is not given to AI.
    pub fn human_trains(&self) -> HashSet<String> {
        self.saved
            .members
            .values()
            .filter(|m| m.role == Role::Driver)
            .filter_map(|m| m.train.clone())
            .collect()
    }
    pub fn disconnected_drivers(&self) -> Vec<String> {
        self.saved
            .members
            .values()
            .filter(|m| m.role == Role::Driver && m.peer.is_none())
            .filter_map(|m| m.train.clone())
            .collect()
    }
    fn check_available(
        &self,
        identity: &str,
        role: Role,
        train: Option<&str>,
        trains: &[TrainInfo],
    ) -> Result<(), String> {
        check_role(role, train, trains)?;
        if let Some(slot) = role_slot(role, train)
            && self.saved.members.values().any(|m| {
                m.identity != identity
                    && role_slot(m.role, m.train.as_deref()) == Some(slot.clone())
            })
        {
            return Err("Ese puesto ya está asignado; su desconexión no lo libera".into());
        }
        Ok(())
    }
    /// Epoch + peer lease revoke old sockets. Sequence survives role changes,
    /// reconnect and disk restore; a retry cannot repeat a physical command.
    #[allow(clippy::too_many_arguments)] // Keep the wire envelope and railway callback explicit at this boundary.
    pub fn command<F>(
        &mut self,
        peer: PeerId,
        epoch: u64,
        lease: u64,
        sequence: u64,
        action: RemoteCommand,
        trains: &[TrainInfo],
        mut apply: F,
    ) -> ServerMessage
    where
        F: FnMut(&Member, &RemoteCommand) -> Result<(), String>,
    {
        let reject = |message: String| ServerMessage::Ack {
            sequence,
            accepted: false,
            processed: false,
            message,
        };
        let Some(member) = self.member(peer).cloned() else {
            return reject("Conexión sin autoridad".into());
        };
        if epoch != self.saved.epoch || lease != member.lease {
            return reject("Concesión vencida; esperá la reconexión".into());
        }
        if sequence == 0 || sequence == u64::MAX {
            return reject("Secuencia inválida".into());
        }
        if sequence <= member.last_sequence {
            return if let Some(receipt) = member.receipts.iter().find(|r| r.sequence == sequence) {
                ServerMessage::Ack {
                    sequence,
                    accepted: receipt.accepted,
                    processed: true,
                    message: receipt.message.clone(),
                }
            } else {
                ServerMessage::Ack {
                    sequence,
                    accepted: false,
                    processed: true,
                    message: "Secuencia ya procesada; su recibo venció y no se repite".into(),
                }
            };
        }
        if sequence != member.last_sequence + 1 {
            return reject("Falta un comando anterior".into());
        }
        let result = if !action.valid() {
            Err("Mando o límite numérico inválido".into())
        } else {
            match &action {
                RemoteCommand::Role { role, train } => {
                    self.check_available(&member.identity, *role, train.as_deref(), trains)
                }
                RemoteCommand::Follow { train } => {
                    if trains.iter().any(|t| &t.id == train) {
                        Ok(())
                    } else {
                        Err("Servicio desconocido".into())
                    }
                }
                RemoteCommand::Ping { .. } => Ok(()),
                RemoteCommand::Drive { .. }
                | RemoteCommand::SteamDriver { .. }
                | RemoteCommand::ResumeRegulators
                    if member.role == Role::Driver =>
                {
                    apply(&member, &action)
                }
                RemoteCommand::Fire { .. } if member.role == Role::Fireman => {
                    apply(&member, &action)
                }
                RemoteCommand::Switch { .. }
                | RemoteCommand::Signal { .. }
                | RemoteCommand::Priority { .. }
                    if member.role == Role::Dispatcher =>
                {
                    apply(&member, &action)
                }
                _ => Err(format!(
                    "{} no tiene autoridad para ese mando",
                    member.role.label()
                )),
            }
        };
        let current = self
            .saved
            .members
            .get_mut(&member.identity)
            .expect("validated member");
        current.last_sequence = sequence;
        if result.is_ok() {
            match action {
                RemoteCommand::Role { role, train } => {
                    current.role = role;
                    current.train = train.clone();
                    current.followed = train;
                }
                RemoteCommand::Follow { train } => current.followed = Some(train),
                _ => {}
            }
        }
        let accepted = result.is_ok();
        let message = result
            .err()
            .unwrap_or_else(|| "Aplicado por el servidor".into());
        current.receipts.push(CommandReceipt {
            sequence,
            accepted,
            message: message.clone(),
        });
        if current.receipts.len() > MAX_PENDING_COMMANDS {
            current.receipts.remove(0);
        }
        ServerMessage::Ack {
            sequence,
            accepted,
            processed: true,
            message,
        }
    }
}
fn role_slot(role: Role, train: Option<&str>) -> Option<(u8, String)> {
    match role {
        Role::Driver => Some((0, train?.into())),
        Role::Fireman => Some((1, train?.into())),
        Role::Dispatcher => Some((2, String::new())),
        Role::Spectator => None,
    }
}
fn check_role(role: Role, train: Option<&str>, trains: &[TrainInfo]) -> Result<(), String> {
    let mut ids = HashSet::new();
    if trains.is_empty()
        || trains.len() > MAX_TRAINS
        || trains
            .iter()
            .any(|t| !valid_id(&t.id) || t.cars == 0 || !ids.insert(&t.id))
    {
        return Err(
            "La sala necesita entre uno y dieciséis servicios distintos con vehículos".into(),
        );
    }
    if role.needs_train() {
        let selected = train
            .and_then(|id| trains.iter().find(|t| t.id == id))
            .ok_or("Elegí un servicio existente")?;
        if role == Role::Fireman && !selected.steam {
            return Err("El fogonero requiere una locomotora de vapor".into());
        }
    } else if train.is_some() {
        return Err("Elegí el seguimiento después de asignar este rol".into());
    }
    Ok(())
}
