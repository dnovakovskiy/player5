//! The server: an accept loop, a hub thread that follows the clock source
//! and broadcasts, and one thread per connection.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

fn accept_loop(listener: &TcpListener, hub: &Arc<Hub>, stop: &AtomicBool, config: &Config) {
    let active = Arc::new(AtomicUsize::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_nodelay(true);
                if active.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                    let mut stream = stream;
                    let _ = http::respond(
                        &mut stream,
                        503,
                        "Service Unavailable",
                        &[("Content-Type", "text/plain")],
                        b"too many connections\n",
                        false,
                    );
                    continue;
                }
                active.fetch_add(1, Ordering::Relaxed);
                let hub = Arc::clone(hub);
                let active = Arc::clone(&active);
                let web = config.web.clone();
                let verbose = config.verbose;
                let stop_flag = Arc::clone(&stop_flag);
                let spawned = std::thread::Builder::new()
                    .name("bridge-conn".into())
                    .spawn(move || {
                        if verbose {
                            eprintln!("player5-bridge: connection from {peer}");
                        }
                        handle_connection(stream, &hub, web.as_deref(), &stop_flag, verbose);
                        active.fetch_sub(1, Ordering::Relaxed);
                    });
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
                if http::respond_upgrade(&mut stream, &accept).is_ok() {
                    serve_websocket(stream, rest, hub, stop, verbose);
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
