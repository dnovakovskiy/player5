//! The source thread: two UDP sockets around a [`Session`].

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

use super::session::{Action, Session, Settings, Via};
use crate::host_time;
use crate::net::{SourceCommand, SourceContext, SourceEvent};

/// How long a receive on the update socket blocks; bounds both the stop
/// latency and the tick period.
pub const POLL: Duration = Duration::from_millis(20);

/// Largest datagram read; status packets are at most 0x200 bytes.
const BUF_LEN: usize = 2048;

/// Repeated send errors are reported at most this often.
const SEND_ERROR_INTERVAL_NS: u64 = 10_000_000_000;

/// Most datagrams read from the announce socket per loop.
const MAX_ANNOUNCE_READS: usize = 64;

/// After failing to find our address toward the unit, wait this long
/// before trying again (each try opens a socket).
const INTERFACE_RETRY_NS: u64 = 1_000_000_000;

/// Our address on the interface that routes to `peer` (no packet is
/// sent: `connect` on UDP only picks a route).
#[must_use]
pub fn local_ip_toward(peer: Ipv4Addr, port: u16) -> Option<Ipv4Addr> {
    let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    probe.connect((peer, port.max(1))).ok()?;
    match probe.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

fn is_timeout(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

/// Runs the source until the owner stops it or hangs up. `update` must
/// have a read timeout of about [`POLL`], `announce` must be
/// non-blocking.
pub fn run(ctx: SourceContext, announce: UdpSocket, update: UdpSocket, settings: Settings) {
    let peer_update_port = settings.peer_update_port;
    let mut session = Session::new(settings, host_time::now_ns());
    let mut out = Vec::new();
    let mut buf = [0u8; BUF_LEN];
    let mut last_send_error: Option<(String, u64)> = None;
    let mut next_interface_probe = 0u64;
    if let Some(ip) = session.interface() {
        session.set_interface(ip, &mut out);
    }
    while !ctx.should_stop() {
        match update.recv_from(&mut buf) {
            Ok((n, SocketAddr::V4(from))) => {
                let now = host_time::now_ns();
                session.on_update(&buf[..n], from, now, &mut out);
            }
            Ok(_) => {}
            Err(e) if is_timeout(e.kind()) => {}
            Err(_) => {
                // e.g. ICMP-induced resets on some platforms; keep going
                // without spinning.
                std::thread::sleep(POLL);
            }
        }
        // Bounded, so a flood on the announce port cannot keep the loop
        // from checking `should_stop`.
        for _ in 0..MAX_ANNOUNCE_READS {
            match announce.recv_from(&mut buf) {
                Ok((n, SocketAddr::V4(from))) => {
                    let now = host_time::now_ns();
                    session.on_announce(&buf[..n], from, now, &mut out);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        while let Ok(command) = ctx.commands.try_recv() {
            match command {
                SourceCommand::Follow(target) => session.set_follow(target, &mut out),
            }
        }
        let now = host_time::now_ns();
        if let Some(unit) = session.interface_wanted() {
            if now >= next_interface_probe {
                match local_ip_toward(unit, peer_update_port) {
                    Some(ip) => session.set_interface(ip, &mut out),
                    None => {
                        next_interface_probe = now.saturating_add(INTERFACE_RETRY_NS);
                        session.interface_failed(&mut out);
                    }
                }
            }
        }
        session.tick(now, &mut out);
        for action in out.drain(..) {
            match action {
                Action::Send { via, to, bytes } => {
                    let socket = match via {
                        Via::Announce => &announce,
                        Via::Update => &update,
                    };
                    if let Err(e) = socket.send_to(&bytes, to) {
                        let message = format!("cannot send to {to}: {e}");
                        let repeat = last_send_error.as_ref().is_some_and(|(m, t)| {
                            *m == message && now.saturating_sub(*t) < SEND_ERROR_INTERVAL_NS
                        });
                        if !repeat {
                            last_send_error = Some((message.clone(), now));
                            if !ctx.send(SourceEvent::Status {
                                warning: true,
                                message,
                            }) {
                                return;
                            }
                        }
                    }
                }
                Action::Event(event) => {
                    if !ctx.send(event) {
                        return;
                    }
                }
            }
        }
    }
}

/// Binds a UDP socket on `bind:port` with a helpful error.
pub fn bind(ip: Ipv4Addr, port: u16, what: &str) -> std::io::Result<UdpSocket> {
    UdpSocket::bind(SocketAddrV4::new(ip, port)).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "cannot open the {what} port {ip}:{port} ({e}); is rekordbox or another \
                 DJ Link program running on this computer?"
            ),
        )
    })
}
