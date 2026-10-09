//! A fake booth on loopback, for end-to-end tests of the bridge and the web
//! app without hardware: two players announce themselves and send beat
//! packets to a bridge started with `--prolink-port-base <base>`.
//!
//! ```sh
//! cargo run -p player5-bridge --example fake_booth -- 41000 &
//! cargo run -p player5-bridge -- --source prolink --passive --prolink-port-base 41000 --web apps/web/dist
//! ```
//!
//! Device 2 plays at 125 BPM, device 3 at 128 BPM and starts 0.7 s later;
//! both count bars 1-4.
//! For each player it prints `downbeat <device> <unix ms> <period ms>`: the
//! wall-clock time of its first downbeat (every fourth beat after it is
//! one), so a test can check bar alignment end to end. Runs until killed.

use std::io::Write;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sync::prolink::{build_beat_packet, build_keep_alive, BeatPacket, KeepAlive};

struct Player {
    number: u8,
    bpm: f64,
    next_beat: Instant,
    beat: u32,
}

fn main() {
    let base: u16 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .expect("usage: fake_booth <prolink-port-base>");
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
    let start = Instant::now();
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs_f64()
        * 1_000.0;
    let mut players = [
        Player {
            number: 2,
            bpm: 125.0,
            next_beat: start,
            beat: 0,
        },
        Player {
            number: 3,
            bpm: 128.0,
            next_beat: start + Duration::from_millis(700),
            beat: 0,
        },
    ];
    let mut out = std::io::stdout().lock();
    for p in &players {
        let first = wall + p.next_beat.duration_since(start).as_secs_f64() * 1_000.0;
        let _ = writeln!(
            out,
            "downbeat {} {first:.3} {:.6}",
            p.number,
            60_000.0 / p.bpm
        );
    }
    let _ = out.flush();
    drop(out);
    let mut next_keep_alive = start;
    loop {
        let now = Instant::now();
        if now >= next_keep_alive {
            for p in &players {
                let k = KeepAlive {
                    number: p.number,
                    name: "CDJ-3000".into(),
                    mac: [0x02, 0, 0, 0, 0, p.number],
                    ip: Ipv4Addr::LOCALHOST,
                    peers: 2,
                    device_type: 1,
                    first_on_network: false,
                    cdj3000_compatible: true,
                };
                let _ = socket.send_to(&build_keep_alive(&k), ("127.0.0.1", base));
            }
            next_keep_alive = now + Duration::from_millis(1_500);
        }
        for p in &mut players {
            if now < p.next_beat {
                continue;
            }
            let period_ms = 60_000.0 / p.bpm;
            let ms = |beats: f64| (period_ms * beats).round() as u32;
            let in_bar = p.beat % 4;
            let packet = BeatPacket {
                device: p.number,
                name: "CDJ-3000".into(),
                next_beat_ms: ms(1.0),
                second_beat_ms: ms(2.0),
                next_bar_ms: ms(f64::from(4 - in_bar)),
                fourth_beat_ms: ms(4.0),
                second_bar_ms: ms(f64::from(8 - in_bar)),
                eighth_beat_ms: ms(8.0),
                pitch: 0x0010_0000,
                bpm_x100: (p.bpm * 100.0).round() as u16,
                beat_within_bar: (in_bar + 1) as u8,
            };
            let _ = socket.send_to(&build_beat_packet(&packet), ("127.0.0.1", base + 1));
            p.beat += 1;
            p.next_beat += Duration::from_secs_f64(period_ms / 1_000.0);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
