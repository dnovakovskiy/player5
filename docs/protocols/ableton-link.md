# Ableton Link

Status: digested. Implemented as `sync::link` (`core/sync/src/link.rs`)
behind the `ableton-link` cargo feature, which is **off by default** for
licensing reasons (ADR-0005).

What was read, and how:

- **Link SDK**, tag `Link-3.1.5`, which is the release `rusty_link` 0.4.8
  bundles (its changelog says so:
  https://github.com/anzbert/rusty_link/blob/master/CHANGELOG.md#048).
  `README.md` and `LICENSE.md` were also checked on `master` (2026-10-09);
  `LICENSE.md` is identical on both. Files were read through
  `raw.githubusercontent.com`, because github.com HTML pages and
  `ableton.github.io` (the concept docs) are not reachable from the
  development container. The links below are the canonical GitHub
  locations with line anchors for that tag.
- **rusty_link 0.4.8** as published on crates.io: `.crate` downloaded from
  `static.crates.io` and read in full. Links point at its docs.rs source
  view.
- **LinkKit** (the iOS SDK) `README.md` on `master`.
- **Ableton Link Integration Guidelines** PDF from the Link repository.

Facts measured on our own machine are marked *Measured here*; they are
observations, not protocol guarantees.

## What Link does

- Applications on a local network discover each other and form a session
  that shares beat, tempo and phase. Anyone can change the tempo and the
  others follow; anyone can join or leave without disrupting the session.
  [README, intro](https://github.com/Ableton/link/blob/Link-3.1.5/README.md#ableton-link)
- A Link instance is disabled after construction and does no networking.
  Once enabled it looks for peers; discovered peers immediately become part
  of one shared session.
  [Link.hpp L48–52](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L48-L52)

## Session state: timeline and start/stop

- Each instance has its own session state: a beat timeline plus a
  transport start/stop state. The timeline starts at beat 0 at the initial
  tempo when the instance is constructed and always advances at the current
  tempo, even when transport is stopped.
  [Link.hpp L39–46](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L39-L46)
- There are two capture/commit pairs: one for the audio thread, one for
  other application threads. The app-thread capture is thread-safe but not
  realtime-safe, and returns a snapshot meant for local use only.
  [Link.hpp L61–70](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L61-L70),
  [L196–208](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L196-L208)
- A `SessionState` is for one thread in a local scope. None of its methods
  are thread-safe; all of them are non-blocking.
  [Link.hpp L221–228](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L221-L228)

**What we do:** the source thread owns one `SessionState`, calls the
app-thread capture every poll and never commits. It is a listener: it
cannot change the session's tempo or grid.

## Tempo

- `tempo()` is a stable value meant for display. Beat time does not
  necessarily progress at exactly that rate, because of clock-drift
  compensation.
  [Link.hpp L247–253](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L247-L253)
- The session tempo is clamped to 20–999 BPM.
  [ClientSessionTimelines.hpp L30–38](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/link/ClientSessionTimelines.hpp#L30-L38)
- Ableton's test plan requires that enabling Link never changes the tempo
  of a session that already exists
  ([TEMPO-2](https://github.com/Ableton/link/blob/Link-3.1.5/TEST-PLAN.md#tempo-2-opening-an-app-with-link-enabled-should-not-change-the-tempo-of-an-existing-link-session)).
  It also asks that apps that cannot play the full tempo range stay in sync
  by switching to a multiple of the session tempo
  ([TEMPO-4](https://github.com/Ableton/link/blob/Link-3.1.5/TEST-PLAN.md#tempo-4-tempo-range-handling)).
  The README asks every Link app to comply with the test plan
  ([README, "Test Plan"](https://github.com/Ableton/link/blob/Link-3.1.5/README.md#test-plan)).

**What we do:** report `tempo()` as the observation's `bpm`.
`LinkConfig::initial_bpm` only matters when no session exists, which is how
TEMPO-2 works. `FollowerClock` accepts 20–400 BPM, so a session above
400 BPM is clamped instead of being followed at a multiple. That part of
TEMPO-4 is open.

## Quantum, beat and phase

- `beatAtTime(t, q)` gives the beat at Link time `t`. Its magnitude belongs
  to this instance, but its phase with respect to the quantum `q` is shared
  by all peers in the session. For non-negative beats,
  `fmod(beatAtTime(t, q), q) == phaseAtTime(t, q)`.
  [Link.hpp L260–269](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L260-L269)
- `phaseAtTime(t, q)` lies in `[0, q)` and handles negative beat values
  correctly, which `fmod` does not.
  [Link.hpp L271–281](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L271-L281)
- `timeAtBeat` is the inverse of `beatAtTime` at constant tempo.
  [Link.hpp L283–289](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L283-L289)
- `requestBeatAtTime` gives quantized launch: with peers present, the beat
  is mapped to the next time with a matching phase rather than remapping
  the session.
  [Link.hpp L291–319](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L291-L319)
  `forceBeatAtTime` remaps the session for everyone. It is described as
  anti-social, with one legitimate use: bridging an external clock into
  Link.
  [Link.hpp L321–342](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L321-L342)

**What we do:** quantum 4, so the shared phase is the position in a 4-beat
bar. The observation is
`Phase::Bar(beatAtTime(t, 4).rem_euclid(4))`. `rem_euclid` handles negative
beats the way `phaseAtTime` does, and a result that rounds up to exactly
4.0 becomes 0.0. Using Link as a sink for the booth clock (Pro DJ Link →
Link via `forceBeatAtTime`) is possible but not built. It needs care: it
would override everyone else's session.

## Clock

- Link works on a platform system clock. `Link::clock().micros()` reads it
  as `std::chrono::microseconds`; it is thread-safe and realtime-safe.
  [Link.hpp L160–169](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L160-L169),
  [README, "Time and Clocks"](https://github.com/Ableton/link/blob/Link-3.1.5/README.md#time-and-clocks).
  `rusty_link`'s `AblLink::clock_micros()` is that call through `abl_link`
  ([abl_link.cpp L82–85](https://github.com/Ableton/link/blob/Link-3.1.5/extensions/abl_link/src/abl_link.cpp#L82-L85)).
- **Linux:** `clock_gettime(CLOCK_MONOTONIC_RAW)`, with nanoseconds
  truncated to microseconds.
  [platforms/Config.hpp L85](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/platforms/Config.hpp#L85),
  [platforms/linux/Clock.hpp L38–52](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/platforms/linux/Clock.hpp#L38-L52)
- **macOS/iOS:** `mach_absolute_time()` scaled by the timebase and
  rounded to microseconds with `llround`.
  [platforms/Config.hpp L77](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/platforms/Config.hpp#L77),
  [platforms/darwin/Clock.hpp L38–60](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/platforms/darwin/Clock.hpp#L38-L60)
- Our `host_time::now_ns()` is `mach_absolute_time` on Apple platforms
  (`core/sync/src/host_time.rs`), so there Link time and host time are the
  same clock, apart from Link's microsecond rounding. Elsewhere it is
  `std::time::Instant`, which on Linux is
  `clock_gettime(CLOCK_MONOTONIC)`
  ([Rust std `Instant`, "Underlying System calls"](https://doc.rust-lang.org/1.94.1/std/time/struct.Instant.html#underlying-system-calls),
  read via `library/std/src/time.rs` L114 at tag 1.94.1).
  `CLOCK_MONOTONIC` is affected by NTP and `adjtime` incremental
  adjustments; `CLOCK_MONOTONIC_RAW` is not
  ([clock_gettime(2)](https://man7.org/linux/man-pages/man2/clock_gettime.2.html),
  read via the man-pages source `man2/clock_getres.2`). On Linux the two
  clocks can therefore run at slightly different rates, and their offset
  drifts.

**What we do:** every poll reads host, Link, host back to back and pairs
the Link reading with the midpoint of the two host reads. If the two host
reads are more than 20 µs apart (the thread was preempted), it retries, up
to three readings, and keeps the tightest. The observation's `host_ns` is
that midpoint, and its phase is `beatAtTime` at that Link reading.
Re-pairing on every poll tracks the Linux rate difference. What is left is
a tempo error the size of the NTP slew, which the follower's PLL absorbs.
Link time's resolution adds at most 1 µs.

## Audio clock and latency (consumer side)

- The audio sample clock is independent of the system clock, and Link maps
  system time to beats. On Apple platforms the render callback's host time
  gives the system time a buffer reaches the hardware. Elsewhere Link
  provides `HostTimeFilter`, a linear regression of system time against
  sample time.
  [README, "Time and Clocks"](https://github.com/Ableton/link/blob/Link-3.1.5/README.md#time-and-clocks)
- To play in time, devices must align the moment sound leaves the output,
  so the output latency should be added to system time before it is used
  with Link.
  [README, "Latency Compensation"](https://github.com/Ableton/link/blob/Link-3.1.5/README.md#latency-compensation)

**What we do:** nothing in the source. Observations are in host time. The
consumer maps host time onto its sample clock
(`lookahead-scheduling.md`) and applies the output latency through the
global latency offset (`ClockControls`, ADR-0001). Ableton's
AUDIOENGINE-1 test (onset within 3 ms of LinkHut) is the acceptance check
for a shell that plays to Link.
[TEST-PLAN AUDIOENGINE-1](https://github.com/Ableton/link/blob/Link-3.1.5/TEST-PLAN.md#audioengine-1-correct-alignment-of-app-audio-with-shared-session)

## Peers and discovery

- `numPeers()` is thread-safe and realtime-safe.
  [Link.hpp L116–120](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L116-L120)
- Discovery uses UDP multicast to `224.76.78.75:20808` (IPv4) and
  `ff12::8080%<scope>` port 20808 (IPv6 link-local).
  [discovery/IpInterface.hpp L30–39](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/discovery/IpInterface.hpp#L30-L39)
- On POSIX, Link uses every running IPv4 interface (`IFF_RUNNING`), plus
  the non-loopback link-local IPv6 addresses of those interfaces.
  [platforms/posix/ScanIpIfAddrs.hpp L74–130](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/platforms/posix/ScanIpIfAddrs.hpp#L74-L130)
- Peer-state messages carry a TTL of 5 s and are re-broadcast at a nominal
  TTL/20 = 250 ms, never more often than every 50 ms.
  [discovery/PeerGateway.hpp L240–241](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/discovery/PeerGateway.hpp#L240-L241),
  [discovery/UdpMessenger.hpp L204–214](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/discovery/UdpMessenger.hpp#L204-L214)
  A peer that is not heard from again within its TTL is pruned.
  [discovery/PeerGateway.hpp L106–121](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/discovery/PeerGateway.hpp#L106-L121)
- A peer that shuts down multicasts a ByeBye message.
  [discovery/UdpMessenger.hpp L118–125](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/discovery/UdpMessenger.hpp#L118-L125),
  [L178–199](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/discovery/UdpMessenger.hpp#L178-L199)
- *Measured here* (Linux container, `cargo test -p sync --features
  ableton-link`): two instances in one process found each other in about
  0.5 s over the container's interface with multicast loopback. They then
  agreed on phase to within about 2·10⁻⁶ beats. When one left, the other's
  peer count dropped to 0 within the test's 3 s window.

**What we do:** poll `numPeers()` every poll and send
`SourceEvent::Status` ("Ableton Link: no peers" / "1 peer" / "N peers")
when it changes. Nothing else identifies peers: Link exposes no peer names
or addresses through `abl_link`, so there is no `SourceEvent::Devices`.

## Start/stop sync

- Following the session's start/stop state is optional for every peer, and
  the state is only shared between peers that enable start/stop sync.
  [Link.hpp L43–46](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L43-L46)
  It is disabled by default.
  [link/Controller.hpp L145](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/link/Controller.hpp#L145)
- Only the user changes the start/stop state, and it persists when a peer
  joins or leaves a session. A peer that observes a change should start or
  stop as if its own user had asked at that time.
  [Link.hpp L230–240](https://github.com/Ableton/link/blob/Link-3.1.5/include/ableton/Link.hpp#L230-L240),
  [TEST-PLAN STARTSTOPSTATE-1/2](https://github.com/Ableton/link/blob/Link-3.1.5/TEST-PLAN.md#start-stop-states)

**What we do:** leave it disabled. `SourceEvent` has no transport variant
yet. Adding one is a separate change to `sync::net`.

## The `rusty_link` bindings

- `rusty_link` wraps Ableton's official C wrapper `abl_link`, function for
  function.
  [README](https://docs.rs/crate/rusty_link/0.4.8/source/README.md)
- It is licensed `GPL-2.0-or-later`, and its README says it has to be,
  because it builds Link.
  [Cargo.toml](https://docs.rs/crate/rusty_link/0.4.8/source/Cargo.toml.orig),
  [README, "License"](https://docs.rs/crate/rusty_link/0.4.8/source/README.md)
- The `.crate` includes the Link sources (and Link's bundled Asio, under
  the Boost Software License 1.0). `build.rs` compiles them with CMake into
  a static library, generates bindings with bindgen (which needs libclang),
  and links `libstdc++` on Linux or `libc++` on macOS. It needs CMake 3.14
  or newer.
  [build.rs](https://docs.rs/crate/rusty_link/0.4.8/source/build.rs),
  [cmake/CMakeLists.txt](https://docs.rs/crate/rusty_link/0.4.8/source/cmake/CMakeLists.txt),
  [README, "Requirements"](https://docs.rs/crate/rusty_link/0.4.8/source/README.md)
- It uses Rust edition 2024, so building it needs Rust 1.85 or newer.
  [Cargo.toml](https://docs.rs/crate/rusty_link/0.4.8/source/Cargo.toml.orig)
- 0.4.9 moves to Link 4.0.0b3, a beta.
  [CHANGELOG](https://github.com/anzbert/rusty_link/blob/master/CHANGELOG.md#049)
- The `set_num_peers_callback`, `set_tempo_callback` and
  `set_start_stop_callback` methods pass `abl_link` a pointer to the
  closure argument, a local of the setter. The closure is dropped when the
  setter returns, so the pointer dangles when Link later calls back.
  [src/abl_link.rs](https://docs.rs/crate/rusty_link/0.4.8/source/src/abl_link.rs)
  This is still the case on `master` (read 2026-10-09).

**What we do:** pin `=0.4.8`. Never register callbacks: poll `num_peers()`
and capture the session state instead. Built and tested here (Ubuntu
24.04) with CMake 3.28, g++ 13 and libclang 18.

## iOS

- The Link README tells iOS developers not to use the Link repository but
  LinkKit, the iOS SDK.
  [README L90–91](https://github.com/Ableton/link/blob/Link-3.1.5/README.md?plain=1#L90-L91)
- LinkKit says Link finds peers with UDP multicast on the LAN, that since
  iOS 14 this needs a special entitlement, and that since iOS 17 an app
  needs the multicast entitlement to detect Link peers. (The README spells
  it `com.apple.developer.multicast`.)
  [LinkKit README](https://github.com/Ableton/LinkKit/blob/master/README.md#ios-14-compatibility)
  Apple's key is `com.apple.developer.networking.multicast`: "whether an
  app can send or receive IP multicast traffic".
  [Apple entitlement reference](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.networking.multicast)
  This is the same entitlement Pro DJ Link needs; see
  `docs/ios-multicast-entitlement.md`.
- LinkKit is used under the Ableton Link SDK license. Custom builds fall
  under Link's license. Its README states that the GPL is not compatible
  with the iOS App Store.
  [LinkKit README](https://github.com/Ableton/LinkKit/blob/master/README.md#building)

**Implication:** an App Store build with Link needs LinkKit under Ableton's
license and the multicast entitlement. `rusty_link` under the GPL is not an
option there. See ADR-0005.

## Naming in the UI

- Ableton's integration guidelines ask that the name be written out in
  full as "Ableton Link", as two words with capital A and L. The settings
  list should show whether it is "Enabled" or "Disabled".
  [Ableton Link Integration Guidelines, pp. 08 and 14](https://github.com/Ableton/link/blob/Link-3.1.5/Ableton%20Link%20Guidelines.pdf)

**What we do:** status messages say "Ableton Link: …". A shell toggle
should read "Ableton Link — Enabled/Disabled".

## What the source reports

| Event | When | Content |
|-------|------|---------|
| `Status` | once at start | "Ableton Link enabled; N BPM until a session is joined" |
| `Status` | peer count changed (and first poll) | "Ableton Link: no peers" / "1 peer" / "N peers" |
| `Observation` | every poll (default 5 ms) | `host_ns`, `Phase::Bar(0..4)`, `bpm: Some(tempo)`, `Precision::Exact`, `device: None` |

`SourceCommand::Follow` is accepted and ignored: a Link session has no
devices to choose between.

## Open items

- TEMPO-4: follow sessions above 400 BPM at a sub-multiple.
- Start/stop sync, once `SourceEvent` can carry transport state.
- The bridge and FFI expose Link only through a feature that forwards
  `sync/ableton-link`; any such build is a GPL build (ADR-0005).
- iOS: LinkKit integration is a separate project, gated on ADR-0005.
