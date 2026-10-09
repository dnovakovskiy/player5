//! The server: an accept loop, a hub thread that follows the clock source
//! and broadcasts, and one thread per connection.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;
use sync::net::{FollowTarget, SourceCommand};

use crate::clock::{self, BridgeClock};
use crate::http;
use crate::sources::{self, SourceKind, SourceOptions};
use crate::ws::{self, Assembler, Event};

/// Most simultaneous connections (HTTP and WebSocket together).
pub const MAX_CONNECTIONS: usize = 64;
/// Most simultaneous connections from one non-loopback address, so one
/// machine on the LAN (or a slow-loris client) cannot take every slot. A
/// browser opens about six at once.
pub const MAX_PER_PEER: usize = 16;
/// Timeline broadcast period (20 Hz).
pub const TIMELINE_PERIOD: Duration = Duration::from_millis(50);
/// A WebSocket client that sends nothing for this long is dropped (clients
/// ping every 2 s).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Server configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Address to listen on.
    pub bind: IpAddr,
    /// Port (0 = pick a free one).
    pub port: u16,
    /// Directory with the built web app to serve, if any.
    pub web: Option<PathBuf>,
    /// Clock source.
    pub source: SourceKind,
    /// Source options.
    pub options: SourceOptions,
    /// Extra page origins allowed to open the WebSocket
    /// (`--allow-origin`; `*` = any). See [`http::origin_allowed`].
    pub allowed_origins: Vec<String>,
    /// Log every connection.
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: IpAddr::from([0, 0, 0, 0]),
            port: crate::DEFAULT_PORT,
            web: None,
            source: SourceKind::Prolink,
            options: SourceOptions::default(),
            allowed_origins: Vec::new(),
            verbose: false,
        }
    }
}

enum HubCommand {
    Follow(FollowTarget),
}

#[derive(Default)]
struct HubState {
    clients: Vec<(u64, Sender<Arc<str>>)>,
    devices: Option<Arc<str>>,
    status: Option<Arc<str>>,
    timeline: Option<Arc<str>>,
}

struct Hub {
    state: Mutex<HubState>,
    commands: Mutex<Sender<HubCommand>>,
    next_id: AtomicU64,
    source_name: &'static str,
}

impl Hub {
    fn broadcast(&self, msg: Arc<str>) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.clients
            .retain(|(_, tx)| tx.send(Arc::clone(&msg)).is_ok());
    }

    fn register(&self) -> (u64, Receiver<Arc<str>>, Vec<Arc<str>>) {
        let (tx, rx) = mpsc::channel();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.clients.push((id, tx));
        let mut backlog = Vec::new();
        backlog.extend(st.devices.clone());
        backlog.extend(st.status.clone());
        backlog.extend(st.timeline.clone());
        (id, rx, backlog)
    }

    fn unregister(&self, id: u64) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.clients.retain(|(c, _)| *c != id);
    }

    fn client_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clients
            .len()
    }
}

/// A running server. Dropping it stops everything.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    hub: Arc<Hub>,
}

impl Server {
    /// The address actually bound (useful with port 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Number of connected WebSocket clients.
    #[must_use]
    pub fn clients(&self) -> usize {
        self.hub.client_count()
    }

    /// Blocks until the server stops (it does not stop on its own).
    pub fn wait(mut self) {
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }

    /// Stops the accept loop and the hub, and waits for them.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Binds, starts the clock source and serves. Returns once listening; a
/// source that fails to start is reported to clients as an error status
/// and the server keeps serving (an unlocked timeline).
pub fn start(config: Config) -> io::Result<Server> {
    let listener = TcpListener::bind((config.bind, config.port))?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let hub = Arc::new(Hub {
        state: Mutex::new(HubState::default()),
        commands: Mutex::new(cmd_tx),
        next_id: AtomicU64::new(1),
        source_name: config.source.name(),
    });

    let hub_thread = {
        let hub = Arc::clone(&hub);
        let stop = Arc::clone(&stop);
        let source = config.source;
        let options = config.options.clone();
        let verbose = config.verbose;
        std::thread::Builder::new()
            .name("bridge-hub".into())
            .spawn(move || run_hub(&hub, &stop, source, &options, &cmd_rx, verbose))?
    };

    let accept_thread = {
        let hub = Arc::clone(&hub);
        let stop = Arc::clone(&stop);
        let config = config.clone();
        std::thread::Builder::new()
            .name("bridge-accept".into())
            .spawn(move || accept_loop(&listener, &hub, &stop, &config))?
    };

    Ok(Server {
        addr,
        stop,
        threads: vec![accept_thread, hub_thread],
        hub,
    })
}

fn run_hub(
    hub: &Hub,
    stop: &AtomicBool,
    kind: SourceKind,
    options: &SourceOptions,
    commands: &Receiver<HubCommand>,
    verbose: bool,
) {
    let mut clock = BridgeClock::new(kind.name());
    let source = match sources::start_source(kind, options) {
        Ok(s) => {
            eprintln!("player5-bridge: following {}", kind.name());
            Some(s)
        }
        Err(e) => {
            eprintln!("player5-bridge: {e}");
            let msg: Arc<str> = clock::status_message("error", &e).into();
            hub.state.lock().unwrap_or_else(|p| p.into_inner()).status = Some(Arc::clone(&msg));
            hub.broadcast(msg);
            None
        }
    };
    let mut last_timeline = Instant::now() - TIMELINE_PERIOD;
    let mut last_locked = false;
    while !stop.load(Ordering::Relaxed) {
        let now = clock::now_us();
        if let Some(src) = source.as_ref() {
            while let Some(event) = src.try_recv() {
                if let Some(msg) = clock.handle(event, now) {
                    let msg: Arc<str> = msg.into();
                    {
                        let mut st = hub.state.lock().unwrap_or_else(|p| p.into_inner());
                        if msg.contains("\"type\":\"devices\"") {
                            st.devices = Some(Arc::clone(&msg));
                        } else {
                            st.status = Some(Arc::clone(&msg));
                        }
                    }
                    if verbose {
                        eprintln!("player5-bridge: {msg}");
                    }
                    hub.broadcast(msg);
                }
            }
        }
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                HubCommand::Follow(target) => {
                    if let Some(src) = source.as_ref() {
                        src.command(SourceCommand::Follow(target));
                    }
                }
            }
        }
        clock.advance(now);
        let locked = clock.locked();
        if locked != last_locked || last_timeline.elapsed() >= TIMELINE_PERIOD {
            let msg: Arc<str> = clock.timeline_message(now).into();
            hub.state.lock().unwrap_or_else(|p| p.into_inner()).timeline = Some(Arc::clone(&msg));
            hub.broadcast(msg);
            last_timeline = Instant::now();
            last_locked = locked;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Counts a connection while alive (also when its thread never starts).
struct Slot {
    active: Arc<Mutex<Counts>>,
    peer: IpAddr,
}

#[derive(Default)]
struct Counts {
    total: usize,
    per_peer: Vec<(IpAddr, usize)>,
}

impl Counts {
    fn of(&self, peer: IpAddr) -> usize {
        self.per_peer
            .iter()
            .find(|(p, _)| *p == peer)
            .map_or(0, |(_, n)| *n)
    }

    /// Takes a slot for `peer` unless a cap is reached.
    fn admit(&mut self, peer: IpAddr) -> bool {
        if self.total >= MAX_CONNECTIONS || (!peer.is_loopback() && self.of(peer) >= MAX_PER_PEER) {
            return false;
        }
        self.total += 1;
        match self.per_peer.iter_mut().find(|(p, _)| *p == peer) {
            Some((_, n)) => *n += 1,
            None => self.per_peer.push((peer, 1)),
        }
        true
    }

    fn release(&mut self, peer: IpAddr) {
        self.total = self.total.saturating_sub(1);
        if let Some(i) = self.per_peer.iter().position(|(p, _)| *p == peer) {
            self.per_peer[i].1 -= 1;
            if self.per_peer[i].1 == 0 {
                self.per_peer.swap_remove(i);
            }
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .release(self.peer);
    }
}

fn accept_loop(listener: &TcpListener, hub: &Arc<Hub>, stop: &AtomicBool, config: &Config) {
    let active = Arc::new(Mutex::new(Counts::default()));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let origins: Arc<[String]> = config.allowed_origins.clone().into();
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_nodelay(true);
                let admitted = active
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .admit(peer.ip());
                if !admitted {
                    let mut stream = stream;
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                    let _ = http::respond(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        &[("Content-Type", "text/plain"), ("Retry-After", "5")],
                        b"too many connections\n",
                        false,
                    );
                    continue;
                }
                let slot = Slot {
                    active: Arc::clone(&active),
                    peer: peer.ip(),
                };
                let hub = Arc::clone(hub);
                let web = config.web.clone();
                let verbose = config.verbose;
                let stop_flag = Arc::clone(&stop_flag);
                let origins = Arc::clone(&origins);
                let spawned = std::thread::Builder::new()
                    .name("bridge-conn".into())
                    .spawn(move || {
                        let _slot = slot;
                        if verbose {
                            eprintln!("player5-bridge: connection from {peer}");
                        }
                        handle_connection(
                            stream,
                            &hub,
                            web.as_deref(),
                            &origins,
                            &stop_flag,
                            verbose,
                        );
                    });
                // On failure the closure (and with it the slot) is dropped.
                if spawned.is_err() {
                    eprintln!("player5-bridge: could not spawn a connection thread");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                eprintln!("player5-bridge: accept failed: {e}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    // Ask connection threads to finish; they poll this every 10 ms.
    stop_flag.store(true, Ordering::Relaxed);
}

fn handle_connection(
    mut stream: TcpStream,
    hub: &Hub,
    web: Option<&std::path::Path>,
    origins: &[String],
    stop: &AtomicBool,
    verbose: bool,
) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let (req, rest) = match http::read_request(&mut stream) {
        Ok(r) => r,
        Err(http::ReadError::TooLarge) => {
            let _ = http::respond(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                &[],
                b"",
                false,
            );
            return;
        }
        Err(http::ReadError::Malformed) => {
            let _ = http::respond(&mut stream, 400, "Bad Request", &[], b"", false);
            return;
        }
        Err(_) => return,
    };
    let head_only = req.method == "HEAD";
    match req.path.as_str() {
        "/ws" => match http::websocket_accept(&req) {
            Ok(accept) => {
                if http::respond_upgrade(&mut stream, &accept).is_err() {
                    return;
                }
                match http::origin_allowed(&req, origins) {
                    Ok(()) => serve_websocket(stream, rest, hub, stop, verbose),
                    Err(origin) => {
                        // Upgrade, then close with a reason the page can
                        // show (a refused handshake tells a page nothing).
                        if verbose {
                            eprintln!("player5-bridge: refused WebSocket from origin {origin:?}");
                        }
                        let _ = stream.write_all(&ws::close_frame(1008, ORIGIN_REFUSED));
                    }
                }
            }
            Err((status, reason)) => {
                let headers: &[(&str, &str)] = if status == 426 {
                    &[("Sec-WebSocket-Version", "13")]
                } else {
                    &[]
                };
                let _ = http::respond(&mut stream, status, reason, headers, b"", false);
            }
        },
        _ if req.method != "GET" && req.method != "HEAD" => {
            let _ = http::respond(
                &mut stream,
                405,
                "Method Not Allowed",
                &[("Allow", "GET, HEAD")],
                b"",
                false,
            );
        }
        "/bridge.json" => {
            let body = format!(
                "{{\"protocol\":1,\"ws\":\"/ws\",\"source\":\"{}\"}}",
                hub.source_name
            );
            let _ = http::respond(
                &mut stream,
                200,
                "OK",
                &[
                    ("Content-Type", "application/json"),
                    ("Access-Control-Allow-Origin", "*"),
                    ("Cache-Control", "no-store"),
                ],
                body.as_bytes(),
                head_only,
            );
        }
        path => serve_static(&mut stream, web, path, head_only),
    }
}

/// Close reason for a refused page origin (at most 123 bytes).
pub const ORIGIN_REFUSED: &str =
    "this page's origin may not use the bridge; start the bridge with --allow-origin <origin>";

const LANDING: &str = "<!doctype html><meta charset=utf-8><title>player5 bridge</title>\
<body style=\"font:16px ui-monospace,monospace;background:#0f1115;color:#e8e8e8;padding:2rem\">\
<h1>player5 bridge</h1><p>The clock relay is running. Open the player5 web app, choose \
<b>Bridge</b> as the clock source and connect to <code>ws://&lt;this host&gt;:PORT/ws</code>.</p>\
<p>Start the bridge with <code>--web &lt;dir&gt;</code> to serve the web app from here.</p>";

fn serve_static(
    stream: &mut TcpStream,
    web: Option<&std::path::Path>,
    path: &str,
    head_only: bool,
) {
    let Some(root) = web else {
        if path == "/" {
            let port = stream.local_addr().map(|a| a.port()).unwrap_or(0);
            let body = LANDING.replace("PORT", &port.to_string());
            let _ = http::respond(
                stream,
                200,
                "OK",
                &[("Content-Type", "text/html; charset=utf-8")],
                body.as_bytes(),
                head_only,
            );
        } else {
            let _ = http::respond(stream, 404, "Not Found", &[], b"not found\n", head_only);
        }
        return;
    };
    match http::resolve_static(root, path).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((file, body)) => {
            let _ = http::respond(
                stream,
                200,
                "OK",
                &[
                    ("Content-Type", http::mime_type(&file)),
                    ("Cache-Control", http::cache_control(&file)),
                    ("X-Content-Type-Options", "nosniff"),
                    // The served app may follow the booth clock; no other
                    // page may frame it and click its controls.
                    ("Content-Security-Policy", "frame-ancestors 'self'"),
                    ("X-Frame-Options", "SAMEORIGIN"),
                ],
                &body,
                head_only,
            );
        }
        None => {
            let _ = http::respond(stream, 404, "Not Found", &[], b"not found\n", head_only);
        }
    }
}

fn serve_websocket(
    mut stream: TcpStream,
    mut buf: Vec<u8>,
    hub: &Hub,
    stop: &AtomicBool,
    verbose: bool,
) {
    let (id, outgoing, backlog) = hub.register();
    let result = websocket_loop(&mut stream, &mut buf, hub, &outgoing, backlog, stop);
    hub.unregister(id);
    if verbose {
        eprintln!("player5-bridge: client {id} left ({result:?})");
    }
}

/// Why a WebSocket session ended (read through `Debug` in verbose logs).
#[derive(Debug)]
#[allow(dead_code)]
enum End {
    Closed,
    Protocol(u16),
    Io,
    Idle,
    Shutdown,
}

fn websocket_loop(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    hub: &Hub,
    outgoing: &Receiver<Arc<str>>,
    backlog: Vec<Arc<str>>,
    stop: &AtomicBool,
) -> End {
    let send = |stream: &mut TcpStream, frame: &[u8]| stream.write_all(frame).is_ok();
    if !send(
        stream,
        ws::text_frame(&clock::hello_message(hub.source_name, clock::now_us())).as_slice(),
    ) {
        return End::Io;
    }
    for msg in backlog {
        if !send(stream, &ws::text_frame(&msg)) {
            return End::Io;
        }
    }
    if stream
        .set_read_timeout(Some(Duration::from_millis(10)))
        .is_err()
    {
        return End::Io;
    }
    let mut assembler = Assembler::default();
    let mut chunk = [0u8; 4096];
    let mut last_rx = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = send(stream, &ws::close_frame(1001, "server shutting down"));
            return End::Shutdown;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return End::Io,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                last_rx = Instant::now();
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return End::Io,
        }
        loop {
            let frame = match ws::parse_client_frame(buf) {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    let _ = send(stream, &ws::close_frame(e.code, e.reason));
                    return End::Protocol(e.code);
                }
            };
            match assembler.push(frame) {
                Ok(Event::Text(text)) => {
                    if let Some(reply) = handle_client_message(&text, hub) {
                        if !send(stream, &ws::text_frame(&reply)) {
                            return End::Io;
                        }
                    }
                }
                Ok(Event::Ping(payload)) => {
                    if !send(stream, &ws::encode_frame(ws::Opcode::Pong, &payload)) {
                        return End::Io;
                    }
                }
                Ok(Event::Close(code)) => {
                    let _ = send(stream, &ws::close_frame(code, ""));
                    return End::Closed;
                }
                Ok(Event::None) => {}
                Err(e) => {
                    let _ = send(stream, &ws::close_frame(e.code, e.reason));
                    return End::Protocol(e.code);
                }
            }
        }
        while let Ok(msg) = outgoing.try_recv() {
            if !send(stream, &ws::text_frame(&msg)) {
                return End::Io;
            }
        }
        if last_rx.elapsed() > IDLE_TIMEOUT {
            let _ = send(stream, &ws::close_frame(1001, "idle"));
            return End::Idle;
        }
    }
}

/// Handles one client JSON message; returns an immediate reply if any.
fn handle_client_message(text: &str, hub: &Hub) -> Option<String> {
    let msg: Value = serde_json::from_str(text).ok()?;
    match msg.get("type")?.as_str()? {
        "ping" => Some(clock::pong_message(
            msg.get("id").unwrap_or(&Value::Null),
            msg.get("client_ms").unwrap_or(&Value::Null),
            clock::now_us(),
        )),
        "follow" => {
            let target = match msg.get("target")? {
                Value::String(s) if s == "master" => FollowTarget::Master,
                Value::Number(n) => {
                    let n = n.as_u64().filter(|n| (1..=255).contains(n))?;
                    FollowTarget::Device(n as u8)
                }
                _ => return None,
            };
            let tx = hub.commands.lock().unwrap_or_else(|p| p.into_inner());
            let _ = tx.send(HubCommand::Follow(target));
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_caps() {
        let mut c = Counts::default();
        let lan: IpAddr = [192, 168, 1, 9].into();
        let other: IpAddr = [192, 168, 1, 10].into();
        let local: IpAddr = [127, 0, 0, 1].into();
        for _ in 0..MAX_PER_PEER {
            assert!(c.admit(lan));
        }
        assert!(
            !c.admit(lan),
            "one LAN host cannot take more than its share"
        );
        assert!(c.admit(other));
        // Loopback is not capped per peer (the browser on the bridge
        // machine, local tests), only by the total.
        while c.total < MAX_CONNECTIONS {
            assert!(c.admit(local));
        }
        assert!(!c.admit(local));
        assert!(!c.admit([10, 0, 0, 1].into()));
        c.release(lan);
        assert!(c.admit(lan));
    }

    #[test]
    fn a_slot_is_released_when_dropped_unused() {
        let active = Arc::new(Mutex::new(Counts::default()));
        let peer: IpAddr = [192, 168, 1, 9].into();
        assert!(active.lock().unwrap().admit(peer));
        // The connection closure owns the slot; dropping it unrun (thread
        // spawn failed) must give the slot back.
        let slot = Slot {
            active: Arc::clone(&active),
            peer,
        };
        let job = move || drop(slot);
        drop(job);
        let counts = active.lock().unwrap();
        assert_eq!(counts.total, 0);
        assert_eq!(counts.of(peer), 0);
    }
}
