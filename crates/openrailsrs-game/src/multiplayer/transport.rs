//! One owned nonblocking worker per endpoint. Dropping it joins the worker and
//! closes its sockets; no child process, detached task, or global runtime.
use super::protocol::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum TransportEvent {
    Request {
        peer: PeerId,
        message: ClientMessage,
    },
    Message(ServerMessage),
    Disconnected {
        peer: PeerId,
    },
    Notice(String),
}
enum HostAction {
    Send(PeerId, ServerMessage),
    Close(PeerId),
}
struct Worker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
pub struct HostTransport {
    worker: Worker,
    events: Mutex<Receiver<TransportEvent>>,
    out: SyncSender<HostAction>,
    address: SocketAddr,
    connected: Arc<Mutex<HashSet<PeerId>>>,
}
pub struct ClientTransport {
    worker: Worker,
    events: Mutex<Receiver<TransportEvent>>,
    out: SyncSender<ClientMessage>,
    latest: Arc<Mutex<Option<ServerMessage>>>,
}
impl HostTransport {
    pub fn bind(address: SocketAddr) -> Result<Self, String> {
        let listener = TcpListener::bind(address)
            .map_err(|_| "No se puede abrir ese puerto de la sala".to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let (tx, events) = mpsc::sync_channel(256);
        let (out, rx) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let connected = Arc::new(Mutex::new(HashSet::new()));
        let live = connected.clone();
        let join = thread::Builder::new()
            .name("rail-network-host".into())
            .spawn(move || host_loop(listener, flag, tx, rx, live))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            worker: Worker {
                stop,
                join: Some(join),
            },
            events: Mutex::new(events),
            out,
            address,
            connected,
        })
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn poll(&self) -> Option<TransportEvent> {
        self.events.lock().ok()?.try_recv().ok()
    }
    pub fn send(&self, peer: PeerId, message: ServerMessage) -> Result<(), String> {
        self.out
            .try_send(HostAction::Send(peer, message))
            .map_err(|_| "La cola de red está ocupada".into())
    }
    pub fn close(&self, peer: PeerId) {
        let _ = self.out.try_send(HostAction::Close(peer));
    }
    /// Separate from the bounded event queue: losing a notification cannot
    /// leave a disconnected human's train running under its last command.
    pub fn connected_peers(&self) -> HashSet<PeerId> {
        self.connected
            .lock()
            .map(|ids| ids.clone())
            .unwrap_or_default()
    }
    pub fn is_running(&self) -> bool {
        !self.worker.stop.load(Ordering::Acquire)
    }
}
impl ClientTransport {
    pub fn connect(address: SocketAddr, hello: Hello) -> Result<Self, String> {
        let (tx, events) = mpsc::sync_channel(64);
        let (out, rx) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let latest = Arc::new(Mutex::new(None));
        let state = latest.clone();
        let join = thread::Builder::new()
            .name("rail-network-client".into())
            .spawn(move || client_loop(address, hello, flag, tx, rx, state))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            worker: Worker {
                stop,
                join: Some(join),
            },
            events: Mutex::new(events),
            out,
            latest,
        })
    }
    pub fn poll(&self) -> Option<TransportEvent> {
        if let Ok(event) = self.events.lock().ok()?.try_recv() {
            return Some(event);
        }
        self.latest.lock().ok()?.take().map(TransportEvent::Message)
    }
    pub fn send(&self, message: ClientMessage) -> Result<(), String> {
        self.out
            .try_send(message)
            .map_err(|_| "La cola de mandos está ocupada".into())
    }
    pub fn is_running(&self) -> bool {
        !self.worker.stop.load(Ordering::Acquire)
    }
}
struct Pending {
    bytes: Vec<u8>,
    offset: usize,
    frame: bool,
}
struct Framed {
    socket: TcpStream,
    input: Vec<u8>,
    output: VecDeque<Pending>,
    limit: usize,
    last_read: Instant,
    quota_at: Instant,
    quota: usize,
}
impl Drop for Framed {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}
impl Framed {
    fn new(socket: TcpStream, limit: usize) -> std::io::Result<Self> {
        socket.set_nonblocking(true)?;
        socket.set_nodelay(true)?;
        Ok(Self {
            socket,
            input: Vec::new(),
            output: VecDeque::new(),
            limit,
            last_read: Instant::now(),
            quota_at: Instant::now(),
            quota: 0,
        })
    }
    fn queue<T: Serialize>(&mut self, message: &T, frame: bool) -> Result<(), String> {
        let output_limit = if self.limit == MAX_FRAME_BYTES {
            MAX_REQUEST_BYTES
        } else {
            MAX_FRAME_BYTES
        };
        let bytes = encode(message, output_limit)?;
        if frame {
            self.output.retain(|p| !p.frame || p.offset > 0);
        }
        if self.output.len() >= 64
            || self
                .output
                .iter()
                .map(|p| p.bytes.len() - p.offset)
                .sum::<usize>()
                + bytes.len()
                > 2 * MAX_FRAME_BYTES
        {
            return Err("Receptor de red demasiado lento".into());
        }
        self.output.push_back(Pending {
            bytes,
            offset: 0,
            frame,
        });
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        let mut budget = 64 * 1024;
        while let Some(next) = self.output.front_mut() {
            let end = (next.offset + budget).min(next.bytes.len());
            match self.socket.write(&next.bytes[next.offset..end]) {
                Ok(0) => return Err("Conexión cerrada".into()),
                Ok(n) => {
                    next.offset += n;
                    budget -= n;
                    if next.offset == next.bytes.len() {
                        self.output.pop_front();
                    }
                    if budget == 0 {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return Err("Conexión interrumpida".into()),
            }
        }
        Ok(())
    }
    fn read<T: DeserializeOwned>(&mut self, request: bool) -> Result<Vec<T>, String> {
        let mut block = [0u8; 8192];
        for _ in 0..8 {
            match self.socket.read(&mut block) {
                Ok(0) => return Err("Conexión cerrada".into()),
                Ok(n) => {
                    self.last_read = Instant::now();
                    self.input.extend_from_slice(&block[..n]);
                    if self.input.len() > self.limit + 64 * 1024 {
                        return Err("Trama excedida".into());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return Err("Conexión interrumpida".into()),
            }
        }
        let mut messages = vec![];
        while self.input.len() >= 4 && messages.len() < 8 {
            let size = u32::from_be_bytes(self.input[..4].try_into().expect("four bytes")) as usize;
            if size == 0 || size > self.limit {
                return Err("Tamaño de trama inválido".into());
            }
            if self.input.len() < size + 4 {
                break;
            }
            if self.quota_at.elapsed() >= Duration::from_secs(1) {
                self.quota_at = Instant::now();
                self.quota = 0;
            }
            self.quota += 1;
            if request && self.quota > 40 {
                return Err("Demasiados mandos por segundo".into());
            }
            let parsed = serde_json::from_slice(&self.input[4..size + 4])
                .map_err(|_| "Mensaje de red inválido")?;
            self.input.drain(..size + 4);
            messages.push(parsed);
        }
        if self.last_read.elapsed() > Duration::from_secs(10) {
            return Err("Conexión sin respuesta".into());
        }
        Ok(messages)
    }
}
pub fn encode<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(value).map_err(|_| "Mensaje no serializable".to_string())?;
    if payload.is_empty() || payload.len() > limit || payload.len() > u32::MAX as usize {
        return Err("Mensaje de red demasiado grande".into());
    }
    let mut bytes = Vec::with_capacity(payload.len() + 4);
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend(payload);
    Ok(bytes)
}
fn host_loop(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    events: SyncSender<TransportEvent>,
    actions: Receiver<HostAction>,
    connected: Arc<Mutex<HashSet<PeerId>>>,
) {
    let mut peers = HashMap::<PeerId, Framed>::new();
    let mut serial = 1u64;
    while !stop.load(Ordering::Acquire) {
        for _ in 0..4 {
            match listener.accept() {
                Ok((socket, _)) => {
                    if peers.len() < MAX_CLIENTS + 2
                        && serial < u64::MAX
                        && let Ok(peer) = Framed::new(socket, MAX_REQUEST_BYTES)
                    {
                        peers.insert(serial, peer);
                        if let Ok(mut live) = connected.lock() {
                            live.insert(serial);
                        }
                        serial += 1;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        for _ in 0..64 {
            match actions.try_recv() {
                Ok(HostAction::Send(id, message)) => {
                    if let Some(peer) = peers.get_mut(&id) {
                        let frame = matches!(&message, ServerMessage::Frame { .. });
                        if peer.queue(&message, frame).is_err() {
                            peers.remove(&id);
                            let _ = events.try_send(TransportEvent::Disconnected { peer: id });
                        }
                    }
                }
                Ok(HostAction::Close(id)) => {
                    peers.remove(&id);
                    let _ = events.try_send(TransportEvent::Disconnected { peer: id });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
        let mut remove = vec![];
        for (&id, peer) in &mut peers {
            let result = peer.read::<ClientMessage>(true).and_then(|messages| {
                for message in messages {
                    events
                        .try_send(TransportEvent::Request { peer: id, message })
                        .map_err(|_| "Cola de entrada completa".to_string())?;
                }
                peer.flush()
            });
            if result.is_err() {
                remove.push(id);
            }
        }
        for id in remove {
            peers.remove(&id);
            let _ = events.try_send(TransportEvent::Disconnected { peer: id });
        }
        if let Ok(mut live) = connected.lock() {
            *live = peers.keys().copied().collect();
        }
        thread::sleep(Duration::from_millis(5));
    }
    if let Ok(mut live) = connected.lock() {
        live.clear();
    }
}
fn client_loop(
    address: SocketAddr,
    hello: Hello,
    stop: Arc<AtomicBool>,
    events: SyncSender<TransportEvent>,
    actions: Receiver<ClientMessage>,
    latest: Arc<Mutex<Option<ServerMessage>>>,
) {
    let mut peer = None::<Framed>;
    let mut authorized = None::<(u64, u64)>;
    let mut retry = Instant::now();
    while !stop.load(Ordering::Acquire) {
        if peer.is_none() && Instant::now() >= retry {
            match TcpStream::connect_timeout(&address, Duration::from_millis(500))
                .and_then(|s| Framed::new(s, MAX_FRAME_BYTES))
            {
                Ok(mut next) => {
                    if next
                        .queue(
                            &ClientMessage::Hello {
                                hello: hello.clone(),
                            },
                            false,
                        )
                        .is_ok()
                    {
                        peer = Some(next);
                        authorized = None;
                    }
                }
                Err(_) => {
                    retry = Instant::now() + Duration::from_millis(750);
                }
            }
        }
        for _ in 0..64 {
            match actions.try_recv() {
                Ok(message) => {
                    if let (Some(peer), Some((epoch, lease))) = (&mut peer, authorized)
                        && matches!(&message,ClientMessage::Command {epoch:e,lease:l,..} if *e==epoch&&*l==lease)
                    {
                        let _ = peer.queue(&message, false);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
        if let Some(socket) = &mut peer {
            let result = socket.read::<ServerMessage>(false).and_then(|messages| {
                for message in messages {
                    if let ServerMessage::Welcome { epoch, lease, .. } = &message {
                        authorized = Some((*epoch, *lease));
                    }
                    if matches!(&message, ServerMessage::Frame { .. }) {
                        // One latest state, independently bounded from command
                        // receipts. A slow UI never accumulates railway saves.
                        *latest
                            .lock()
                            .map_err(|_| "Estado cliente no disponible".to_string())? =
                            Some(message);
                    } else {
                        events
                            .try_send(TransportEvent::Message(message))
                            .map_err(|_| "Cola cliente completa".to_string())?;
                    }
                }
                socket.flush()
            });
            if result.is_err() {
                peer = None;
                authorized = None;
                if let Ok(mut frame) = latest.lock() {
                    *frame = None;
                }
                retry = Instant::now() + Duration::from_millis(250);
                let _ = events.try_send(TransportEvent::Disconnected { peer: 0 });
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}
