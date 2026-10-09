# ADR-0007: A headless bridge serves the network clock (and the app) to browsers

Status: Accepted

## Context

The web app must follow the booth's tempo and phase, but browsers cannot
open UDP sockets: Pro DJ Link, the Opus Quad and Ableton Link are all
UDP broadcast or multicast protocols. ADR-0001 already decided that a
headless binary built from the same `sync` crate joins the booth network and
relays the clock over WebSocket. This ADR records how.

Constraints:

- Browsers on other machines reach the bridge over the LAN. A page served
  over `https://` (GitHub Pages, any CDN) may not open a plain `ws://`
  connection to another host (mixed content), and Chromium increasingly
  restricts public pages from reaching private-network addresses.
- The dependency rule (CLAUDE.md) asks before adding crates; WebSocket
  tooling was expected to arrive with this work.
- Nothing about the bridge is real-time audio, but the timing it relays is.

## Decision

1. **The bridge serves the web app itself** when started with
   `--web <dir>`: one origin, `http://<bridge>:17505/`, for both the app and
   `ws://<bridge>:17505/ws`. No mixed content, no cross-origin private-network
   requests, no certificates. The web app discovers this case through
   `GET /bridge.json` on its own origin.
2. **Standard library only.** The HTTP/1.1 head parser, the RFC 6455
   framing, SHA-1 and base64 are implemented in the crate (a few hundred
   lines, unit-tested against the RFC and FIPS vectors). A booth has a
   handful of clients, so one thread per connection with blocking sockets
   and short read timeouts is simpler and adequate. No new dependencies.
3. **The bridge runs its own follower.** Source observations (host time in
   nanoseconds) feed a `sync::FollowerClock` in a microsecond domain. The
   bridge broadcasts a `timeline` (anchor time, anchor beat, BPM, bar
   alignment, lock, precision) at 20 Hz and on lock changes, so a client
   can compute the beat at any server time without per-beat messages.
4. **Clients do the time mapping.** Each client estimates
   `server_us − client_us` from ping/pong round trips (lowest-RTT sample
   wins), maps timeline times onto `performance.now()` and then onto the
   AudioContext, and feeds its own engine follower. The bridge never needs
   to know a client's audio latency.
5. **One booth, one target.** A `follow` command from any client changes
   which device the bridge follows for everyone.
6. **Failure is visible, not fatal.** If a source cannot start (missing
   permissions, a build without that source), the bridge still serves the
   app and reports an `error` status with an unlocked timeline.

## Consequences

- The web app works from any static host for internal-clock use, and from
  the bridge for synced use in the booth.
- Two followers sit in series (bridge, then browser). Each is tuned by the
  source's precision; the bridge forwards that precision so the browser
  picks the same tuning.
- The server trusts the LAN: any page a DJ visits could connect to a bridge
  on `localhost` and read the tempo or change the follow target. That is
  acceptable for a booth tool; revisit if the bridge ever exposes more.
- Each connection costs a thread. The bridge caps connections at 64.
