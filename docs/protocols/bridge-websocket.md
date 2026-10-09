# Bridge WebSocket protocol (player5 ↔ browser)

This is *our* protocol, not a third-party one: the contract between
`apps/bridge` (server) and the web app's `BridgeClock` (client). It is
specified here so both sides can be built and tested independently. The
design rationale is in ADR-0007. Version: **1**.

## Transport

- HTTP/1.1 on one TCP port, default **17505**, all interfaces.
- `GET /ws` with a standard WebSocket upgrade (RFC 6455) opens the session.
  Text frames only, each carrying one JSON object with a `"type"` field.
- `GET /bridge.json` returns `{"protocol":1,"ws":"/ws"}` so a page served by
  the bridge can discover it.
- If the bridge was started with `--web <dir>`, every other `GET` serves
  static files from that directory (the built web app), so booth machines
  can open `http://<bridge-host>:17505/` and get app and clock from one
  origin (no mixed-content or private-network-access problems).
- Pages from other origins are refused (ADR-0011): when the upgrade
  request carries an `Origin` that is not a loopback host, not the
  bridge's own host (IP literal, single-label or `.local` name, equal to
  `Host`) and not listed with `--allow-origin`, the server completes the
  upgrade and immediately closes with code **1008** and a reason that
  names `--allow-origin`. Requests without `Origin` (non-browser clients)
  are accepted.
- Unknown message types must be ignored by both sides (forward
  compatibility). Numbers are JSON numbers; times are integers.

## Time base

The server's clock is a monotonic microsecond counter (`server_us`) with an
arbitrary epoch. The client estimates `offset = server_us − client_us`
from ping/pong exchanges and maps server times onto its own clock
(`performance.now()`, then the AudioContext).

## Messages: server → client

### `hello` (first message after the upgrade)

```json
{"type":"hello","protocol":1,"app":"player5-bridge","version":"0.1.0",
 "server_us":123456789,"source":"prolink"}
```

### `pong` (reply to every `ping`)

```json
{"type":"pong","id":17,"client_ms":5123.25,"server_us":123459000}
```

`client_ms` echoes the ping verbatim. The client computes
`rtt = now_ms − client_ms` and
`offset_us = server_us − (client_ms + rtt / 2) × 1000`, keeping the estimate
from the lowest-RTT exchange among recent ones.

### `timeline` (at least 10 Hz, and immediately on any change)

```json
{"type":"timeline","source":"prolink","locked":true,"bpm":124.0,
 "anchor_us":123460000,"anchor_beat":1033.25,"bar_aligned":true,
 "precision":"fine","device":2}
```

- The beat at server time `t` is
  `anchor_beat + (t − anchor_us) / 1e6 × bpm / 60`.
- `bar_aligned`: when `true`, multiples of 4 are downbeats (bar starts);
  when `false`, only the beat phase (fractional part) is meaningful.
- `locked`: `false` while searching or after the source went quiet; the
  client should keep its own clock running and show the state.
- `precision`: `"exact" | "fine" | "coarse" | "jittery"` — the follower
  tuning the client should use (see `sync::Precision`).
- `source`: `"prolink" | "opus" | "link" | "sim" | "none"`.
- `device`: the device number being followed, or `null`.

### `devices` (on change, and once after `hello`)

```json
{"type":"devices","devices":[
  {"number":2,"name":"CDJ-3000","address":"169.254.12.34","kind":"player",
   "bpm":124.0,"playing":true,"master":true,"on_air":true}]}
```

`kind`: `"player" | "mixer" | "rekordbox" | "all-in-one" | "other"`. Any of
`bpm`, `playing`, `master`, `on_air` may be `null` when unknown.

### `status`

```json
{"type":"status","level":"warn","message":"no Pro DJ Link traffic on 169.254.0.0/16"}
```

`level`: `"info" | "warn" | "error"`.

## Messages: client → server

### `ping`

```json
{"type":"ping","id":17,"client_ms":5123.25}
```

Clients send a burst of ~8 pings on connect, then one every 2 s.

### `follow`

```json
{"type":"follow","target":"master"}
{"type":"follow","target":3}
```

Selects which device the bridge follows (default `"master"`). Applies to
all clients (one bridge, one booth). When the followed device changes
(this command, or a new tempo master), the next `timeline` carries the
new `device` and that device's own bar at once; it is not a phase jump of
the previous device.

## Client behaviour (web app)

1. Connect, send the ping burst, wait for an offset estimate.
2. On every `timeline` (and on a local ~50 ms timer between them), compute
   the beat at a chosen server time, map that time to an AudioContext
   sample position, and feed the engine one observation
   (`Bar` phase when `bar_aligned`, else `Beat`) with the timeline's BPM and
   precision.
3. Show `locked`, the followed device and `status` messages in the UI.
4. When following a specific device number, ignore timelines whose
   `device` is another one (a restarted bridge follows the master until
   the client's `follow` arrives). When the `device` of a locked timeline
   changes, re-sync rather than slew: it is another deck's bar.
