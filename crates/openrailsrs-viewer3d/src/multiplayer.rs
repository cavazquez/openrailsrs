//! The viewer is a client of the host's railway authority, including its own
//! local seat. Ordinary input systems are gated while this plugin is active.
use crate::live::LiveDrive;
use crate::route_bootstrap::ViewerAppState;
use bevy::prelude::*;
use openrailsrs_game::multiplayer::*;
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf, time::Instant};

mod panel;
mod persistence;
mod presentation;
#[cfg(test)]
mod tests;
pub use persistence::{ServerSave, save_path};

pub struct MultiplayerPlugin;
impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MultiplayerState>()
            .init_resource::<NetworkLaunch>()
            .add_systems(
                Update,
                (panel::keyboard, panel::buttons, poll, panel::rebuild)
                    .chain()
                    .after(crate::live::enable_live_defaults)
                    .after(crate::weather_state::update)
                    .before(crate::live::live_driver_input)
                    .before(crate::live::sync_live_render_clock)
                    .before(crate::wet_surfaces::update),
            )
            .add_systems(
                PostUpdate,
                presentation::camera.after(crate::camera::follow_train_camera),
            )
            .add_systems(OnExit(ViewerAppState::Playing), disconnect_on_exit);
    }
}
/// CLI adapter: main may populate this resource before the route is loaded.
/// A key file is read privately; credentials are never command-line arguments.
#[derive(Resource)]
pub struct NetworkLaunch {
    pub host: Option<String>,
    pub join: Option<String>,
    pub identity: Option<String>,
    pub role: Option<Role>,
    pub train: Option<String>,
    pub key_file: Option<PathBuf>,
    pub restore: bool,
    pub consumed: bool,
}
impl Default for NetworkLaunch {
    fn default() -> Self {
        Self::from_env()
    }
}
impl NetworkLaunch {
    pub fn from_env() -> Self {
        let value = |key: &str| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.is_empty() && value.len() <= 4096)
        };
        Self {
            host: value("OPENRAILSRS_NETWORK_HOST"),
            join: value("OPENRAILSRS_NETWORK_JOIN"),
            identity: value("OPENRAILSRS_NETWORK_IDENTITY"),
            role: value("OPENRAILSRS_NETWORK_ROLE").and_then(|value| {
                match value.to_ascii_lowercase().as_str() {
                    "driver" | "conductor" => Some(Role::Driver),
                    "fireman" | "fogonero" => Some(Role::Fireman),
                    "dispatcher" | "despachador" => Some(Role::Dispatcher),
                    "spectator" | "espectador" => Some(Role::Spectator),
                    _ => None,
                }
            }),
            train: value("OPENRAILSRS_NETWORK_TRAIN"),
            key_file: value("OPENRAILSRS_NETWORK_KEY_FILE").map(PathBuf::from),
            restore: value("OPENRAILSRS_NETWORK_RESTORE").is_some_and(|v| v == "1" || v == "true"),
            consumed: false,
        }
    }
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ObserverCamera {
    #[default]
    Exterior,
    Orbit,
    Free,
}
pub struct NetworkForm {
    pub address: String,
    pub identity: String,
    pub room: Secret,
    pub role: Role,
    pub train: Option<String>,
    pub show_room: bool,
    /// In-memory identity for embedded clients; UI otherwise uses a private file.
    pub credential: Option<Secret>,
}
impl Default for NetworkForm {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:30500".into(),
            identity: "Jugador".into(),
            room: Secret(String::new()),
            role: Role::Spectator,
            train: None,
            show_room: false,
            credential: None,
        }
    }
}
enum Runtime {
    Host(HostRuntime),
    Client(ClientRuntime),
}
struct HostRuntime {
    transport: HostTransport,
    authority: Authority,
    tick: u64,
    last_frame: Instant,
}
struct Pending {
    action: RemoteCommand,
    sent: Instant,
}
struct ClientRuntime {
    transport: ClientTransport,
    epoch: u64,
    lease: u64,
    next_sequence: u64,
    pending: BTreeMap<u64, Pending>,
    last_tick: Option<(u64, u64)>,
    last_ping: Instant,
}
#[derive(Clone)]
pub enum NetworkAction {
    Host,
    Join,
    Disconnect,
    Save,
    Restore,
    Command(RemoteCommand),
}
#[derive(Resource)]
pub struct MultiplayerState {
    pub open: bool,
    pub form: NetworkForm,
    pub notice: String,
    pub connected: bool,
    pub role: Role,
    pub train: Option<String>,
    pub followed: Option<String>,
    pub catalog: Vec<TrainInfo>,
    pub roster: Vec<RosterEntry>,
    pub views: Vec<TrainView>,
    pub rtt_ms: Option<f64>,
    pub frames: u64,
    pub render_ui: bool,
    pub camera: ObserverCamera,
    pub actions: Vec<NetworkAction>,
    pub dispatch_switch: String,
    pub dispatch_signal: String,
    pub steam_cutoff: f64,
    pub cylinder_cocks: bool,
    pub save_file: PathBuf,
    runtime: Option<Runtime>,
    pub(crate) revision: u64,
    pub(crate) focused: Option<panel::Field>,
    pub(crate) root: Option<Entity>,
    pub(crate) ui_revision: u64,
    pub(crate) scroll_y: f32,
    draft_driver: Option<DriverControls>,
    draft_fireman: Option<FiremanControls>,
}
impl Default for MultiplayerState {
    fn default() -> Self {
        Self {
            open: false,
            form: NetworkForm::default(),
            notice: String::new(),
            connected: false,
            role: Role::Spectator,
            train: None,
            followed: None,
            catalog: vec![],
            roster: vec![],
            views: vec![],
            rtt_ms: None,
            frames: 0,
            render_ui: true,
            camera: ObserverCamera::Exterior,
            actions: vec![],
            runtime: None,
            revision: 1,
            dispatch_switch: String::new(),
            dispatch_signal: String::new(),
            steam_cutoff: 0.75,
            cylinder_cocks: false,
            save_file: save_path(),
            focused: None,
            root: None,
            ui_revision: 0,
            scroll_y: 0.,
            draft_driver: None,
            draft_fireman: None,
        }
    }
}
impl MultiplayerState {
    pub fn active(&self) -> bool {
        self.runtime.is_some()
    }
    pub fn host(&self) -> bool {
        matches!(self.runtime, Some(Runtime::Host(_)))
    }
    pub fn queue(&mut self, command: RemoteCommand) {
        if self.actions.len() < MAX_PENDING_COMMANDS {
            // Only a UI draft. The host remains the sole writer of train physics.
            match &command {
                RemoteCommand::Drive { controls } => self.draft_driver = Some(controls.clone()),
                RemoteCommand::Fire { controls } => self.draft_fireman = Some(controls.clone()),
                RemoteCommand::SteamDriver {
                    cutoff,
                    cylinder_cocks,
                } => {
                    self.steam_cutoff = *cutoff;
                    self.cylinder_cocks = *cylinder_cocks;
                }
                RemoteCommand::Role { .. } | RemoteCommand::Follow { .. } => {
                    self.draft_driver = None;
                    self.draft_fireman = None;
                }
                _ => {}
            }
            self.actions.push(NetworkAction::Command(command));
        }
    }
    pub(crate) fn driver_draft(&self) -> Option<DriverControls> {
        self.draft_driver
            .clone()
            .or_else(|| self.selected_view().map(|view| view.driver.clone()))
    }
    pub(crate) fn fireman_draft(&self) -> Option<FiremanControls> {
        self.draft_fireman
            .clone()
            .or_else(|| self.selected_view().and_then(|view| view.fireman.clone()))
    }
    pub fn selected_view(&self) -> Option<&TrainView> {
        let selected = self.train.as_ref().or(self.followed.as_ref());
        self.views
            .iter()
            .find(|view| Some(&view.id) == selected)
            .or_else(|| self.views.first())
    }
    pub fn dirty(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}
pub fn open_panel(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<MultiplayerState>() {
        state.open = true;
        state.dirty();
    }
}
pub fn standalone_controls(state: Option<Res<MultiplayerState>>) -> bool {
    state.is_none_or(|s| !s.active() && !s.open)
}
pub fn local_simulation(state: Option<Res<MultiplayerState>>) -> bool {
    state.is_none_or(|s| !matches!(s.runtime, Some(Runtime::Client(_))))
}
pub fn panel_closed(state: Option<Res<MultiplayerState>>) -> bool {
    state.is_none_or(|s| !s.open)
}
pub fn deny_local_mutation(state: Option<&MultiplayerState>) -> Option<&'static str> {
    state.filter(|s|s.active()).map(|_|"Usá los mandos de Multijugador: el servidor comprueba tu rol. F9/B y las órdenes del mapa local están desactivadas en red.")
}
fn content_id(live: &LiveDrive) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for (id, signature) in std::iter::once((
        &live.session.service_id,
        live.session.snapshot().content_signature,
    ))
    .chain(
        live.traffic
            .services
            .iter()
            .map(|s| (&s.id, s.session.snapshot().content_signature)),
    ) {
        hash.update((id.len() as u64).to_be_bytes());
        hash.update(id.as_bytes());
        hash.update(signature.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
fn ensure_network_ready(live: &LiveDrive) -> Result<(), String> {
    if live.session.uses_script_tcs()
        || live
            .traffic
            .services
            .iter()
            .any(|service| service.session.uses_script_tcs())
    {
        return Err("Multijugador requiere TCS Rust en todos los servicios. Los scripts C# siguen disponibles en partidas individuales".into());
    }
    if live.traffic.services.len() + 1 > MAX_TRAINS {
        return Err(
            "La sala admite hasta dieciséis servicios, incluidos los actores originales".into(),
        );
    }
    Ok(())
}
fn hello(state: &MultiplayerState, live: &LiveDrive) -> Result<Hello, String> {
    ensure_network_ready(live)?;
    let train = if state.form.role.needs_train() {
        Some(
            state
                .form
                .train
                .clone()
                .unwrap_or_else(|| live.session.service_id.clone()),
        )
    } else {
        None
    };
    Ok(Hello {
        version: PROTOCOL_VERSION,
        room: state.form.room.clone(),
        identity: state.form.identity.clone(),
        credential: match &state.form.credential {
            Some(secret) => secret.clone(),
            None => persistence::identity_secret(&state.form.identity)?,
        },
        content_id: content_id(live),
        role: state.form.role,
        train,
    })
}
fn handle_client(
    state: &mut MultiplayerState,
    client: &mut ClientRuntime,
    message: ServerMessage,
    live: &mut LiveDrive,
    environment: &mut Option<serde_json::Value>,
) -> Result<(), String> {
    match message {
        ServerMessage::Welcome {
            epoch,
            lease,
            last_sequence,
            role,
            train,
            trains,
        } => {
            if client.epoch != 0 && client.epoch != epoch {
                client.pending.clear();
                client.next_sequence = last_sequence + 1;
            }
            client.epoch = epoch;
            client.lease = lease;
            client.next_sequence = client.next_sequence.max(last_sequence + 1);
            client
                .pending
                .retain(|sequence, _| *sequence > last_sequence);
            state.connected = true;
            state.role = role;
            state.train = train.clone();
            state.followed = train;
            state.catalog = trains;
            for (&sequence, pending) in &mut client.pending {
                client.transport.send(ClientMessage::Command {
                    epoch,
                    lease,
                    sequence,
                    action: pending.action.clone(),
                })?;
                pending.sent = Instant::now();
            }
            state.notice = "Conectado: física y mandos validados por el servidor".into();
        }
        ServerMessage::Ack {
            sequence,
            accepted,
            processed,
            message,
        } => {
            if processed && let Some(pending) = client.pending.remove(&sequence) {
                state.rtt_ms = Some(pending.sent.elapsed().as_secs_f64() * 1000.);
                if accepted && let RemoteCommand::Role { role, train } = pending.action {
                    state.role = role;
                    state.train = train;
                }
            }
            if !accepted {
                state.notice = message;
                state.draft_driver = None;
                state.draft_fireman = None;
            }
        }
        ServerMessage::Frame { frame } => {
            if frame.epoch != client.epoch
                || client
                    .last_tick
                    .is_some_and(|last| last >= (frame.epoch, frame.tick))
            {
                return Ok(());
            }
            if frame.trains.len() > MAX_TRAINS || frame.roster.len() > MAX_CLIENTS {
                return Err("Vista de red demasiado grande".into());
            }
            if let Some(saved) = frame.world {
                openrailsrs_game::multiplayer::restore_world(
                    &mut live.session,
                    &mut live.traffic,
                    *saved,
                )?;
            } else {
                presentation::apply_observer(live, &frame.trains)?;
            }
            state.views = frame.trains;
            state.roster = frame.roster;
            state.frames += 1;
            state.connected = true;
            if !client
                .pending
                .values()
                .any(|p| matches!(&p.action, RemoteCommand::Drive { .. }))
            {
                state.draft_driver = None;
            }
            if !client
                .pending
                .values()
                .any(|p| matches!(&p.action, RemoteCommand::Fire { .. }))
            {
                state.draft_fireman = None;
            }
            client.last_tick = Some((frame.epoch, frame.tick));
            *environment = Some(frame.environment);
            if let Some(member) = state
                .roster
                .iter()
                .find(|m| m.identity == state.form.identity)
            {
                state.role = member.role;
                state.train = member.train.clone();
                state.followed = member.followed.clone();
            }
            if !client
                .pending
                .values()
                .any(|p| matches!(&p.action, RemoteCommand::SteamDriver { .. }))
                && let Some(view) = state.selected_view()
            {
                let (cutoff, cocks) = (view.cutoff, view.cylinder_cocks);
                if let Some(cutoff) = cutoff {
                    state.steam_cutoff = cutoff;
                }
                if let Some(cocks) = cocks {
                    state.cylinder_cocks = cocks;
                }
            }
            live.reset_presentation_clock();
        }
        ServerMessage::Rejected { message } => {
            state.connected = false;
            state.notice = message;
        }
    }
    state.dirty();
    Ok(())
}
fn sync_humans(host: &Authority, live: &mut LiveDrive) -> Result<(), String> {
    let mut humans = host.human_trains();
    live.session.state.yard_operations.human_driven = humans.contains(&live.session.service_id);
    humans.remove(&live.session.service_id);
    live.traffic.set_remote_drivers(humans)?;
    for id in host.disconnected_drivers() {
        openrailsrs_game::multiplayer::world::safe_stop(&mut live.session, &mut live.traffic, &id);
    }
    for member in host
        .roster()
        .into_iter()
        .filter(|m| m.role == Role::Fireman && !m.connected)
    {
        if let Some(id) = member.train {
            openrailsrs_game::multiplayer::world::set_automatic_fireman(
                &mut live.session,
                &mut live.traffic,
                &id,
                true,
            );
        }
    }
    Ok(())
}
fn submit_host(
    host: &mut HostRuntime,
    peer: PeerId,
    epoch: u64,
    lease: u64,
    sequence: u64,
    command: RemoteCommand,
    live: &mut LiveDrive,
) -> ServerMessage {
    let before = host.authority.human_trains();
    let catalog = train_catalog(&live.session, &live.traffic);
    let firemen = host
        .authority
        .roster()
        .into_iter()
        .filter(|m| m.role == Role::Fireman)
        .filter_map(|m| m.train)
        .collect::<std::collections::HashSet<_>>();
    let response = host.authority.command(
        peer,
        epoch,
        lease,
        sequence,
        command,
        &catalog,
        |member, command| apply_command(&mut live.session, &mut live.traffic, member, command),
    );
    for id in before.symmetric_difference(&host.authority.human_trains()) {
        openrailsrs_game::multiplayer::world::safe_stop(&mut live.session, &mut live.traffic, id);
    }
    let remaining = host
        .authority
        .roster()
        .into_iter()
        .filter(|m| m.role == Role::Fireman)
        .filter_map(|m| m.train)
        .collect::<std::collections::HashSet<_>>();
    for id in firemen.difference(&remaining) {
        openrailsrs_game::multiplayer::world::set_automatic_fireman(
            &mut live.session,
            &mut live.traffic,
            id,
            true,
        );
    }
    let _ = sync_humans(&host.authority, live);
    response
}
fn start_host(
    state: &mut MultiplayerState,
    live: &mut LiveDrive,
    restore: Option<(AuthorityCheckpoint, u64)>,
) -> Result<(), String> {
    if state.runtime.is_some() {
        return Err("Desconectá la sala actual antes de abrir otra".into());
    }
    ensure_network_ready(live)?;
    let address: SocketAddr = state
        .form
        .address
        .parse()
        .map_err(|_| "Dirección inválida; ejemplo 127.0.0.1:30500")?;
    let catalog = train_catalog(&live.session, &live.traffic);
    let (mut authority, tick) = if let Some((saved, tick)) = restore {
        (Authority::restore(saved, &catalog)?, tick)
    } else {
        if state.form.room.0.is_empty() {
            state.form.room = persistence::random_secret()?;
        }
        (
            Authority::new(state.form.room.clone(), content_id(live))?,
            0,
        )
    };
    let mut request = hello(state, live)?;
    request.room = authority.checkpoint().room;
    let welcome = authority.connect(0, request, &catalog)?;
    if let ServerMessage::Welcome { role, train, .. } = welcome {
        state.role = role;
        state.train = train.clone();
        state.followed = train;
    }
    let transport = HostTransport::bind(address)?;
    state.form.address = transport.address().to_string();
    for id in authority.human_trains() {
        openrailsrs_game::multiplayer::world::safe_stop(&mut live.session, &mut live.traffic, &id);
    }
    sync_humans(&authority, live)?;
    state.connected = true;
    state.catalog = catalog;
    state.runtime = Some(Runtime::Host(HostRuntime {
        transport,
        authority,
        tick,
        last_frame: Instant::now() - std::time::Duration::from_secs(1),
    }));
    state.notice =
        "Sala abierta. Compartí dirección y clave; cada persona elige un puesto libre".into();
    Ok(())
}
fn disconnect_on_exit(mut state: ResMut<MultiplayerState>, mut live: Option<ResMut<LiveDrive>>) {
    if let Some(live) = &mut live {
        stop_online_trains(&state, live);
        live.traffic.remote_drivers.clear();
        live.session.state.yard_operations.human_driven = false;
    }
    state.runtime = None;
    state.connected = false;
    state.actions.clear();
    state.dirty();
}
fn stop_online_trains(state: &MultiplayerState, live: &mut LiveDrive) {
    let ids = if let Some(Runtime::Host(host)) = &state.runtime {
        host.authority.human_trains()
    } else {
        state.train.iter().cloned().collect()
    };
    for id in ids {
        openrailsrs_game::multiplayer::world::safe_stop(&mut live.session, &mut live.traffic, &id);
    }
}

fn poll(world: &mut World) {
    let Some(mut state) = world.remove_resource::<MultiplayerState>() else {
        return;
    };
    let Some(mut live) = world.remove_resource::<LiveDrive>() else {
        world.insert_resource(state);
        return;
    };
    let playing = world
        .get_resource::<State<ViewerAppState>>()
        .is_some_and(|s| *s.get() == ViewerAppState::Playing);
    if !playing {
        world.insert_resource(live);
        world.insert_resource(state);
        return;
    }
    if let Some(mut launch) = world.get_resource_mut::<NetworkLaunch>()
        && !launch.consumed
        && (launch.host.is_some() || launch.join.is_some() || launch.restore)
    {
        launch.consumed = true;
        if let Some(address) = launch.host.as_ref().or(launch.join.as_ref()) {
            state.form.address = address.clone();
        }
        if let Some(identity) = &launch.identity {
            state.form.identity = identity.clone();
        }
        if let Some(role) = launch.role {
            state.form.role = role;
        }
        state.form.train = launch.train.clone();
        let key = launch.key_file.as_ref().map(|path| {
            if std::fs::metadata(path)
                .map_err(|_| "No se puede leer la clave de sala")?
                .len()
                > 130
            {
                return Err("Archivo de clave demasiado grande");
            }
            std::fs::read_to_string(path).map_err(|_| "No se puede leer la clave de sala")
        });
        if let Some(Ok(key)) = &key {
            state.form.room = Secret(key.trim().into());
        } else if key.is_some() {
            state.notice = "No se pudo leer la clave de sala".into();
            state.open = true;
        }
        if key.as_ref().is_none_or(|result| result.is_ok()) {
            state.actions.push(if launch.restore {
                NetworkAction::Restore
            } else if launch.host.is_some() {
                NetworkAction::Host
            } else {
                NetworkAction::Join
            });
        }
    }
    if state.catalog.is_empty() {
        state.catalog = train_catalog(&live.session, &live.traffic);
        state.dirty();
    }
    let mut environment = None;
    let actions = std::mem::take(&mut state.actions);
    for action in actions {
        let result = (|| -> Result<(), String> {
            match action {
                NetworkAction::Host => start_host(&mut state, &mut live, None),
                NetworkAction::Join => {
                    if state.active() {
                        return Err("Desconectá la sala actual primero".into());
                    }
                    if !state.form.room.valid() {
                        return Err("La clave de sala necesita entre 12 y 128 caracteres".into());
                    }
                    let address = state
                        .form
                        .address
                        .parse()
                        .map_err(|_| "Dirección de servidor inválida")?;
                    let transport = ClientTransport::connect(address, hello(&state, &live)?)?;
                    state.runtime = Some(Runtime::Client(ClientRuntime {
                        transport,
                        epoch: 0,
                        lease: 0,
                        next_sequence: 1,
                        pending: BTreeMap::new(),
                        last_tick: None,
                        last_ping: Instant::now(),
                    }));
                    state.notice = "Conectando con el servidor…".into();
                    Ok(())
                }
                NetworkAction::Disconnect => {
                    stop_online_trains(&state, &mut live);
                    state.runtime = None;
                    state.connected = false;
                    state.draft_driver = None;
                    state.draft_fireman = None;
                    live.traffic.remote_drivers.clear();
                    live.session.state.yard_operations.human_driven = false;
                    state.notice = "Desconectado".into();
                    Ok(())
                }
                NetworkAction::Command(command) => {
                    let Some(mut runtime) = state.runtime.take() else {
                        return Err("Abrí o conectá una sala primero".into());
                    };
                    let result = (|| -> Result<(), String> {
                        match &mut runtime {
                            Runtime::Host(host) => {
                                let member = host
                                    .authority
                                    .member(0)
                                    .ok_or("Puesto local desconectado")?;
                                let (epoch, lease, sequence) = (
                                    host.authority.epoch(),
                                    member.lease,
                                    member.last_sequence + 1,
                                );
                                let reply = submit_host(
                                    host, 0, epoch, lease, sequence, command, &mut live,
                                );
                                if let ServerMessage::Ack {
                                    accepted, message, ..
                                } = reply
                                {
                                    state.notice = message;
                                    if !accepted {
                                        state.open = true;
                                    }
                                }
                                if let Some(member) = host.authority.member(0) {
                                    state.role = member.role;
                                    state.train = member.train.clone();
                                    state.followed = member.followed.clone();
                                }
                                Ok(())
                            }
                            Runtime::Client(client) => {
                                if !state.connected || client.epoch == 0 {
                                    return Err(
                                        "Esperá la reconexión antes de enviar mandos".into()
                                    );
                                }
                                if client.pending.len() >= MAX_PENDING_COMMANDS {
                                    return Err(
                                        "Esperá la confirmación de los mandos pendientes".into()
                                    );
                                }
                                let sequence = client.next_sequence;
                                client.next_sequence = client
                                    .next_sequence
                                    .checked_add(1)
                                    .ok_or("Secuencia agotada")?;
                                client.pending.insert(
                                    sequence,
                                    Pending {
                                        action: command.clone(),
                                        sent: Instant::now(),
                                    },
                                );
                                client.transport.send(ClientMessage::Command {
                                    epoch: client.epoch,
                                    lease: client.lease,
                                    sequence,
                                    action: command,
                                })
                            }
                        }
                    })();
                    state.runtime = Some(runtime);
                    result
                }
                NetworkAction::Save => save_server(world, &mut state, &mut live),
                NetworkAction::Restore => restore_server(world, &mut state, &mut live),
            }
        })();
        if let Err(error) = result {
            state.notice = error;
            state.open = true;
        }
        state.dirty();
    }
    if let Some(mut runtime) = state.runtime.take() {
        match &mut runtime {
            Runtime::Host(host) => {
                for _ in 0..128 {
                    let Some(event) = host.transport.poll() else {
                        break;
                    };
                    match event {
                        TransportEvent::Request { peer, message } => {
                            let result = match message {
                                ClientMessage::Hello { hello } => {
                                    let before = host.authority.human_trains();
                                    let result = host.authority.connect(
                                        peer,
                                        hello,
                                        &train_catalog(&live.session, &live.traffic),
                                    );
                                    if result.is_ok() {
                                        for id in host.authority.human_trains().difference(&before)
                                        {
                                            openrailsrs_game::multiplayer::world::safe_stop(
                                                &mut live.session,
                                                &mut live.traffic,
                                                id,
                                            );
                                        }
                                    }
                                    result
                                }
                                ClientMessage::Command {
                                    epoch,
                                    lease,
                                    sequence,
                                    action,
                                } => Ok(submit_host(
                                    host, peer, epoch, lease, sequence, action, &mut live,
                                )),
                            };
                            let response = result
                                .unwrap_or_else(|message| ServerMessage::Rejected { message });
                            let _ = host.transport.send(peer, response);
                        }
                        TransportEvent::Disconnected { peer } => {
                            host.authority.disconnect(peer);
                        }
                        _ => {}
                    }
                }
                let connected = host.transport.connected_peers();
                for peer in host
                    .authority
                    .peers()
                    .into_iter()
                    .filter(|peer| *peer != 0 && !connected.contains(peer))
                {
                    host.authority.disconnect(peer);
                }
                if let Err(error) = sync_humans(&host.authority, &mut live) {
                    state.notice = error;
                }
                if host.last_frame.elapsed().as_secs_f64() >= FRAME_INTERVAL_S {
                    host.last_frame = Instant::now();
                    host.tick = host.tick.saturating_add(1);
                    let snapshot = capture_world(&mut live.session, &mut live.traffic, false);
                    if let Err(error) = &snapshot {
                        state.notice = format!("No se pudo publicar el mundo compartido: {error}");
                    }
                    let environment = capture_environment(world, &live);
                    let roster = host.authority.roster();
                    state.roster = roster.clone();
                    state.views = openrailsrs_game::multiplayer::world::train_views(
                        &live.session,
                        &live.traffic,
                        state.followed.as_deref(),
                        false,
                    );
                    for peer in host.authority.peers().into_iter().filter(|p| *p != 0) {
                        if let Some(member) = host.authority.member(peer) {
                            let spectator = member.role == Role::Spectator;
                            if !spectator && snapshot.is_err() {
                                host.transport.close(peer);
                                continue;
                            }
                            let frame = WorldFrame {
                                epoch: host.authority.epoch(),
                                tick: host.tick,
                                time_s: live.session.time_s(),
                                world: if spectator {
                                    None
                                } else {
                                    snapshot.as_ref().ok().map(|w| Box::new(w.clone()))
                                },
                                trains: openrailsrs_game::multiplayer::world::train_views(
                                    &live.session,
                                    &live.traffic,
                                    member.followed.as_deref().or(member.train.as_deref()),
                                    spectator,
                                ),
                                roster: roster.clone(),
                                environment: environment.clone(),
                            };
                            let _ = host.transport.send(
                                peer,
                                ServerMessage::Frame {
                                    frame: Box::new(frame),
                                },
                            );
                        }
                    }
                    state.draft_driver = None;
                    state.draft_fireman = None;
                    if let Some(view) = state.selected_view() {
                        let (cutoff, cocks) = (view.cutoff, view.cylinder_cocks);
                        if let Some(cutoff) = cutoff {
                            state.steam_cutoff = cutoff;
                        }
                        if let Some(cocks) = cocks {
                            state.cylinder_cocks = cocks;
                        }
                    }
                    state.frames += 1;
                    state.dirty();
                }
            }
            Runtime::Client(client) => {
                for _ in 0..64 {
                    let Some(event) = client.transport.poll() else {
                        break;
                    };
                    match event {
                        TransportEvent::Message(message) => {
                            if let Err(error) = handle_client(
                                &mut state,
                                client,
                                message,
                                &mut live,
                                &mut environment,
                            ) {
                                state.notice = error;
                                state.connected = false;
                            }
                        }
                        TransportEvent::Disconnected { .. } => {
                            state.connected = false;
                            state.notice="Conexión perdida; el servidor frena el tren y se intenta reconectar".into();
                            state.dirty();
                        }
                        _ => {}
                    }
                }
                if state.connected && client.last_ping.elapsed().as_secs_f64() > 2. {
                    client.last_ping = Instant::now();
                    state.queue(RemoteCommand::Ping { client_millis: 0 });
                }
            }
        }
        state.runtime = Some(runtime);
    }
    if let Some(value) = environment
        && let Err(error) = apply_environment(world, value, &mut live)
    {
        state.notice = error;
    }
    world.insert_resource(live);
    world.insert_resource(state);
}
#[derive(serde::Serialize, serde::Deserialize)]
struct NetworkEnvironment {
    weather: Option<crate::player_launch::PlayerWeather>,
    selection: Option<crate::environment::EnvironmentSelection>,
    checkpoint: Option<crate::weather_state::WeatherCheckpoint>,
    start_clock_s: f64,
    season: String,
    live: Option<NetworkLocalEnvironment>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct NetworkLocalEnvironment {
    utc: Option<i64>,
    timezone: Option<String>,
    location: Option<crate::environment::RouteLocation>,
    sample: Option<crate::environment::WeatherSample>,
    status: String,
}
fn capture_environment(world: &World, live: &LiveDrive) -> serde_json::Value {
    let content = world.get_resource::<crate::player_launch::ActivePlayerContent>();
    let environment = world.get_resource::<crate::environment::LiveEnvironment>();
    serde_json::json!(NetworkEnvironment {
        weather: content.map(|c| c.weather),
        selection: content.map(|c| c.environment),
        checkpoint: world
            .get_resource::<crate::weather_state::WeatherState>()
            .and_then(|s| s.checkpoint()),
        start_clock_s: live.start_clock_s,
        season: live.season.clone(),
        live: environment.map(|env| NetworkLocalEnvironment {
            utc: env.utc.map(|utc| utc.timestamp()),
            timezone: env.timezone.map(|zone| zone.to_string()),
            location: env.location,
            sample: env.sample.clone(),
            status: env.network_status.chars().take(512).collect()
        }),
    })
}
fn validate_environment(value: serde_json::Value) -> Result<NetworkEnvironment, String> {
    let value: NetworkEnvironment =
        serde_json::from_value(value).map_err(|_| "Hora o clima de red inválidos")?;
    if !value.start_clock_s.is_finite()
        || !valid_id(&value.season)
        || value.checkpoint.as_ref().is_some_and(|c| !c.valid())
    {
        return Err("Hora o clima de red fuera de rango".into());
    }
    if let Some(env) = &value.live
        && (env
            .utc
            .is_some_and(|utc| chrono::DateTime::from_timestamp(utc, 0).is_none())
            || env
                .timezone
                .as_ref()
                .is_some_and(|zone| zone.parse::<chrono_tz::Tz>().is_err())
            || env.location.is_some_and(|location| !location.valid())
            || env.sample.as_ref().is_some_and(|sample| !sample.valid())
            || env.status.len() > 2048)
    {
        return Err("Entorno geográfico del servidor inválido".into());
    }
    Ok(value)
}
fn apply_environment(
    world: &mut World,
    value: serde_json::Value,
    live: &mut LiveDrive,
) -> Result<(), String> {
    let value = validate_environment(value)?;
    if let Some(checkpoint) = value.checkpoint {
        world.insert_resource(checkpoint.restore(live.session.time_s()));
    }
    if let Some(mut content) = world.get_resource_mut::<crate::player_launch::ActivePlayerContent>()
    {
        if let Some(weather) = value.weather {
            content.weather = weather;
        }
        if let Some(selection) = value.selection {
            content.environment = selection;
        }
    }
    live.start_clock_s = value.start_clock_s;
    live.season = value.season;
    let mut environment = crate::environment::LiveEnvironment::default();
    if let Some(env) = value.live {
        environment.utc = env
            .utc
            .and_then(|utc| chrono::DateTime::from_timestamp(utc, 0));
        environment.timezone = env.timezone.and_then(|zone| zone.parse().ok());
        environment.location = env.location;
        environment.sample = env.sample;
        environment.network_status = format!("Servidor · {}", env.status);
    } else {
        environment.network_status = "Hora y clima del servidor".into();
    }
    world.insert_resource(environment);
    Ok(())
}
fn save_server(
    world: &World,
    state: &mut MultiplayerState,
    live: &mut LiveDrive,
) -> Result<(), String> {
    let Some(Runtime::Host(host)) = &state.runtime else {
        return Err("Sólo el anfitrión guarda el mundo compartido".into());
    };
    let content = world
        .get_resource::<crate::player_launch::ActivePlayerContent>()
        .ok_or("Falta contenido activo")?;
    let camera = crate::saved_game::SavedCamera {
        follow: "ChaseCam".into(),
        focus: [0.; 3],
        yaw: 0.,
        pitch: 0.35,
        distance: 35.,
        look_yaw: 0.,
        look_pitch: 0.,
        pose: None,
    };
    let game = crate::saved_game::SavedGame::capture(
        live,
        content,
        camera,
        world.get_resource::<crate::weather_state::WeatherState>(),
        world.get_resource::<crate::player_settings::PlayerSettings>(),
    )?;
    let saved = ServerSave {
        version: 1,
        tick: host.tick,
        content_id: content_id(live),
        authority: host.authority.checkpoint(),
        game,
        environment: capture_environment(world, live),
    };
    persistence::private_write(
        &state.save_file,
        &serde_json::to_vec(&saved).map_err(|e| e.to_string())?,
    )?;
    state.notice = format!("Sesión completa guardada en {}", state.save_file.display());
    Ok(())
}
fn restore_server(
    world: &mut World,
    state: &mut MultiplayerState,
    live: &mut LiveDrive,
) -> Result<(), String> {
    if state.active() {
        return Err("Desconectá la sala antes de recuperar el guardado".into());
    }
    ensure_network_ready(live)?;
    let saved = persistence::read_save(&state.save_file)?;
    if saved.content_id != content_id(live) {
        return Err("Elegí el mismo escenario y contenido antes de recuperar la sesión".into());
    }
    validate_environment(saved.environment.clone())?;
    let mut candidate = LiveDrive::from_scenario_path(live.scenario_path())?;
    let weather = saved.game.weather;
    let selection = saved.game.environment;
    let checkpoint = saved.game.weather_checkpoint.clone();
    let camera = saved.game.restore(&mut candidate)?;
    let catalog = train_catalog(&candidate.session, &candidate.traffic);
    Authority::restore(saved.authority.clone(), &catalog)?;
    state.form.room = saved.authority.room.clone();
    start_host(state, &mut candidate, Some((saved.authority, saved.tick)))?;
    candidate.paused = false;
    *live = candidate;
    if let Some(mut content) = world.get_resource_mut::<crate::player_launch::ActivePlayerContent>()
    {
        content.weather = weather;
        if let Some(selection) = selection {
            content.environment = selection;
        }
    }
    if let Some(checkpoint) = checkpoint {
        world.insert_resource(checkpoint.restore(live.session.time_s()));
    }
    apply_environment(world, saved.environment, live)?;
    world.insert_resource(crate::saved_game::PendingSavedCamera(camera));
    state.notice =
        "Sesión recuperada: roles, trenes, clima y reservas. Las personas pueden reconectarse"
            .into();
    Ok(())
}
