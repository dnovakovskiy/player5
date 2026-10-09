//! End-to-end: a real server on 127.0.0.1 with the simulated clock, driven
//! by a hand-rolled WebSocket client and plain HTTP requests.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use player5_bridge::ws::{self, Opcode};
use player5_bridge::{start, Config, SourceKind, SourceOptions};
use serde_json::Value;

fn server(web: Option<std::path::PathBuf>) -> player5_bridge::Server {
    start(Config {
        bind: "127.0.0.1".parse().unwrap(),
        port: 0,
        web,
        source: SourceKind::Sim,
        options: SourceOptions {
            sim_bpm: 124.0,
            ..SourceOptions::default()
        },
        allowed_origins: Vec::new(),
        verbose: false,
    })
    .unwrap()
}

struct Client {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Client {
    fn connect(addr: SocketAddr) -> Self {
        Self::connect_with(addr, &addr.to_string(), "")
    }

    /// Handshake with this `Host` and extra header lines (each ending in CRLF).
    fn connect_with(addr: SocketAddr, host: &str, extra: &str) -> Self {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let req = format!(
            "GET /ws HTTP/1.1\r\nHost: {host}\r\n{extra}Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut buf = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        let head_end = loop {
            if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break p;
            }
            assert!(Instant::now() < deadline, "no handshake response");
            let mut chunk = [0u8; 1024];
            if let Ok(n) = stream.read(&mut chunk) {
                buf.extend_from_slice(&chunk[..n]);
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        assert!(head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        buf.drain(..head_end + 4);
        Self { stream, buf }
    }

    fn send(&mut self, text: &str) {
        let frame = ws::client_frame(Opcode::Text, true, text.as_bytes(), [1, 2, 3, 4]);
        self.stream.write_all(&frame).unwrap();
    }

    fn next_frame(&mut self, timeout: Duration) -> Option<ws::Frame> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(f) = ws::parse_server_frame(&mut self.buf) {
                return Some(f);
            }
            if Instant::now() > deadline {
                return None;
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(_) => {}
            }
        }
    }

    /// Next JSON message of the given type (skipping others).
    fn next_of(&mut self, kind: &str, timeout: Duration) -> Option<(Value, Instant)> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let f = self.next_frame(deadline - Instant::now())?;
            if f.opcode != Opcode::Text {
                continue;
            }
            let v: Value = serde_json::from_slice(&f.payload).unwrap();
            if v["type"] == kind {
                return Some((v, Instant::now()));
            }
        }
        None
    }
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let end = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&out[..end]).to_string();
    let status = head[9..12].parse().unwrap();
    (status, head, out[end + 4..].to_vec())
}

#[test]
fn websocket_session_follows_the_simulated_clock() {
    let srv = server(None);
    let mut c = Client::connect(srv.local_addr());

    let (hello, _) = c.next_of("hello", Duration::from_secs(2)).expect("hello");
    assert_eq!(hello["protocol"], 1);
    assert_eq!(hello["source"], "sim");
    assert!(hello["server_us"].as_u64().is_some());

    // Wait for a locked timeline.
    let deadline = Instant::now() + Duration::from_secs(3);
    let (t1, _) = loop {
        let (t, at) = c
            .next_of("timeline", Duration::from_secs(1))
            .expect("timeline");
        if t["locked"] == true {
            break (t, at);
        }
        assert!(Instant::now() < deadline, "never locked");
    };
    assert_eq!(t1["source"], "sim");
    assert_eq!(t1["bar_aligned"], true);
    assert_eq!(t1["precision"], "exact");
    assert!((t1["bpm"].as_f64().unwrap() - 124.0).abs() < 1e-6);

    // Beat advances at 124 BPM between two timeline messages, consistent with
    // their own anchors (both are on the server clock).
    std::thread::sleep(Duration::from_millis(300));
    let (t2, _) = c.next_of("timeline", Duration::from_secs(1)).unwrap();
    let du = (t2["anchor_us"].as_u64().unwrap() - t1["anchor_us"].as_u64().unwrap()) as f64;
    let db = t2["anchor_beat"].as_f64().unwrap() - t1["anchor_beat"].as_f64().unwrap();
    let expected = du / 1e6 * 124.0 / 60.0;
    // Allow 2 ms of beat time for the simulator's own sampling.
    assert!(
        (db - expected).abs() < 0.002 * 124.0 / 60.0 + 1e-3,
        "beat advanced {db}, expected {expected}"
    );

    // Ping is answered with the fields echoed.
    c.send(r#"{"type":"ping","id":7,"client_ms":1234.5}"#);
    let (pong, _) = c.next_of("pong", Duration::from_secs(2)).expect("pong");
    assert_eq!(pong["id"], 7);
    assert_eq!(pong["client_ms"], 1234.5);
    assert!(pong["server_us"].as_u64().unwrap() >= t2["anchor_us"].as_u64().unwrap());

    // Follow commands and unknown messages are accepted silently.
    c.send(r#"{"type":"follow","target":3}"#);
    c.send(r#"{"type":"follow","target":"master"}"#);
    c.send(r#"{"type":"something-new"}"#);
    c.send("not json");
    assert!(c.next_of("timeline", Duration::from_secs(1)).is_some());

    // WebSocket ping control frame gets a pong frame.
    c.stream
        .write_all(&ws::client_frame(Opcode::Ping, true, b"hi", [9, 9, 9, 9]))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let f = c.next_frame(Duration::from_secs(1)).unwrap();
        if f.opcode == Opcode::Pong {
            assert_eq!(f.payload, b"hi");
            break;
        }
        assert!(Instant::now() < deadline);
    }

    // Close handshake.
    c.stream
        .write_all(&ws::client_frame(
            Opcode::Close,
            true,
            &1000u16.to_be_bytes(),
            [5, 6, 7, 8],
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let f = c.next_frame(Duration::from_secs(1)).expect("close reply");
        if f.opcode == Opcode::Close {
            assert_eq!(&f.payload[..2], &1000u16.to_be_bytes());
            break;
        }
        assert!(Instant::now() < deadline);
    }
    srv.stop();
}

#[test]
fn protocol_violation_closes_with_1002() {
    let srv = server(None);
    let mut c = Client::connect(srv.local_addr());
    // An unmasked client frame.
    c.stream.write_all(&[0x81, 0x01, b'x']).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let f = c.next_frame(Duration::from_secs(2)).expect("close");
        if f.opcode == Opcode::Close {
            assert_eq!(&f.payload[..2], &1002u16.to_be_bytes());
            break;
        }
        assert!(Instant::now() < deadline);
    }
    srv.stop();
}

#[test]
fn http_endpoints_and_static_files() {
    let dir = std::env::temp_dir().join(format!("p5-bridge-web-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<title>player5</title>").unwrap();
    std::fs::write(dir.join("assets/core.wasm"), [0u8, 97, 115, 109]).unwrap();
    let srv = server(Some(dir.clone()));
    let addr = srv.local_addr();

    let (status, head, body) = http_get(addr, "/bridge.json");
    assert_eq!(status, 200);
    assert!(head.contains("Access-Control-Allow-Origin: *"));
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["protocol"], 1);
    assert_eq!(v["ws"], "/ws");

    let (status, head, body) = http_get(addr, "/");
    assert_eq!(status, 200);
    assert!(head.contains("text/html"));
    assert_eq!(body, b"<title>player5</title>");

    let (status, head, _) = http_get(addr, "/assets/core.wasm?v=1");
    assert_eq!(status, 200);
    assert!(head.contains("application/wasm"));

    assert_eq!(http_get(addr, "/../Cargo.toml").0, 404);
    assert_eq!(http_get(addr, "/%2e%2e/etc/passwd").0, 404);
    assert_eq!(http_get(addr, "/nope").0, 404);
    // /ws without the upgrade headers.
    assert_eq!(http_get(addr, "/ws").0, 400);

    // Garbage request line.
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(b"\x00\x01garbage\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 400"), "{out}");

    srv.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(feature = "ableton-link"))]
#[test]
fn unavailable_source_still_serves_with_an_error_status() {
    // Without the ableton-link feature, the Link source cannot start.
    let srv = start(Config {
        bind: "127.0.0.1".parse().unwrap(),
        port: 0,
        web: None,
        source: SourceKind::Link,
        options: SourceOptions::default(),
        allowed_origins: Vec::new(),
        verbose: false,
    })
    .unwrap();
    let mut c = Client::connect(srv.local_addr());
    // The error status arrives as backlog or broadcast.
    let (status, _) = c.next_of("status", Duration::from_secs(2)).expect("status");
    assert_eq!(status["level"], "error");
    let (t, _) = c.next_of("timeline", Duration::from_secs(2)).unwrap();
    assert_eq!(t["locked"], false);
    srv.stop();
}

/// A page from another origin (any site open on the DJ laptop) must not
/// be able to drive the bridge: its WebSocket is closed with 1008 and a
/// reason before any clock data, and its follow command never arrives.
/// The app the bridge serves, local pages and non-browser clients work.
#[test]
fn foreign_page_origins_are_refused() {
    let srv = server(None);
    let addr = srv.local_addr();
    let host = addr.to_string();

    let mut foreign = Client::connect_with(addr, &host, "Origin: https://elsewhere.example\r\n");
    let f = foreign
        .next_frame(Duration::from_secs(2))
        .expect("a close frame");
    assert_eq!(f.opcode, Opcode::Close);
    assert_eq!(u16::from_be_bytes([f.payload[0], f.payload[1]]), 1008);
    assert!(String::from_utf8_lossy(&f.payload[2..]).contains("--allow-origin"));
    let _ = foreign.stream.write_all(&ws::client_frame(
        Opcode::Text,
        true,
        br#"{"type":"ping","id":1,"client_ms":0}"#,
        [1, 2, 3, 4],
    ));
    assert!(foreign.next_frame(Duration::from_millis(300)).is_none());

    // DNS rebinding: Origin and Host agree, but the name is public.
    let mut rebound = Client::connect_with(
        addr,
        "rebound.elsewhere.example:17505",
        "Origin: http://rebound.elsewhere.example:17505\r\n",
    );
    assert_eq!(
        rebound.next_frame(Duration::from_secs(2)).unwrap().opcode,
        Opcode::Close
    );

    // The bridge's own page, and a page on localhost (a dev server).
    for origin in [
        format!("http://{host}"),
        "http://localhost:5173".to_string(),
    ] {
        let mut ok = Client::connect_with(addr, &host, &format!("Origin: {origin}\r\n"));
        assert!(
            ok.next_of("hello", Duration::from_secs(2)).is_some(),
            "{origin}"
        );
        assert!(ok.next_of("timeline", Duration::from_secs(2)).is_some());
    }
    srv.stop();

    // --allow-origin lets a hosted copy of the app in.
    let srv = start(Config {
        bind: "127.0.0.1".parse().unwrap(),
        port: 0,
        web: None,
        source: SourceKind::Sim,
        options: SourceOptions::default(),
        allowed_origins: vec!["https://player5.example".into()],
        verbose: false,
    })
    .unwrap();
    let addr = srv.local_addr();
    let mut hosted = Client::connect_with(
        addr,
        &addr.to_string(),
        "Origin: https://player5.example\r\n",
    );
    assert!(hosted.next_of("hello", Duration::from_secs(2)).is_some());
    srv.stop();
}

/// The served app may not be framed by other pages (clickjacking the
/// follow menu would bypass the origin check).
#[test]
fn static_files_refuse_framing() {
    let dir = std::env::temp_dir().join(format!("p5-bridge-frame-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<p>app</p>").unwrap();
    let srv = server(Some(dir.clone()));
    let (status, head, _) = http_get(srv.local_addr(), "/");
    assert_eq!(status, 200);
    assert!(head.contains("frame-ancestors 'self'"), "{head}");
    assert!(head.contains("X-Frame-Options: SAMEORIGIN"), "{head}");
    srv.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn many_clients_receive_broadcasts() {
    let srv = server(None);
    let mut clients: Vec<Client> = (0..8).map(|_| Client::connect(srv.local_addr())).collect();
    for c in &mut clients {
        assert!(c.next_of("timeline", Duration::from_secs(2)).is_some());
    }
    assert_eq!(srv.clients(), 8);
    drop(clients);
    let deadline = Instant::now() + Duration::from_secs(3);
    while srv.clients() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(srv.clients(), 0, "dropped clients are unregistered");
    srv.stop();
}

/// Pro DJ Link end to end over loopback: CDJ beat packets in, a locked
/// bar-aligned timeline out.
#[test]
fn follows_pro_dj_link_beat_packets() {
    use std::net::UdpSocket;
    use sync::prolink::{build_beat_packet, BeatPacket};

    // A port block unlikely to collide; the bridge binds base..base+2.
    let base = 40_000 + (std::process::id() % 5_000) as u16 * 3;
    let srv = start(Config {
        bind: "127.0.0.1".parse().unwrap(),
        port: 0,
        web: None,
        source: SourceKind::Prolink,
        options: SourceOptions {
            passive: true,
            prolink_port_base: Some(base),
            ..SourceOptions::default()
        },
        allowed_origins: Vec::new(),
        verbose: false,
    })
    .unwrap();
    let mut c = Client::connect(srv.local_addr());

    // A CDJ at 125 BPM, pitch 0 %: a beat every 480 ms, bar position 1-4.
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    let feeder = std::thread::spawn(move || {
        for k in 0..12u32 {
            let p = BeatPacket {
                device: 2,
                name: "CDJ-3000".into(),
                next_beat_ms: 480,
                second_beat_ms: 960,
                next_bar_ms: 480 * (4 - k % 4),
                fourth_beat_ms: 1_440,
                second_bar_ms: 480 * (8 - k % 4),
                eighth_beat_ms: 3_360,
                pitch: 0x0010_0000,
                bpm_x100: 12_500,
                beat_within_bar: (k % 4 + 1) as u8,
            };
            sender
                .send_to(&build_beat_packet(&p), ("127.0.0.1", base + 1))
                .unwrap();
            std::thread::sleep(Duration::from_millis(480));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    let t = loop {
        let (t, _) = c
            .next_of("timeline", Duration::from_secs(1))
            .expect("timeline");
        if t["locked"] == true && (t["bpm"].as_f64().unwrap() - 125.0).abs() < 0.5 {
            break t;
        }
        assert!(Instant::now() < deadline, "never locked: {t}");
    };
    assert_eq!(t["source"], "prolink");
    assert_eq!(t["bar_aligned"], true);
    assert_eq!(t["device"], 2);
    feeder.join().unwrap();
    srv.stop();
}
