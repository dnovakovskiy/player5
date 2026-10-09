# player5-bridge

Browsers cannot open UDP sockets, so they cannot hear Pro DJ Link, the
Opus Quad or Ableton Link directly. The bridge is a small headless program
that does: it joins the booth network, follows the clock, and serves tempo,
beat and phase to the web app over WebSocket. It can also serve the built
web app itself, so a browser on the booth network opens one address and gets
both.

Protocol: [`docs/protocols/bridge-websocket.md`](../../docs/protocols/bridge-websocket.md).
Design: [ADR-0007](../../docs/adr/0007-bridge.md).

## Run it

```sh
# Build the web app once (see apps/web/README.md), then:
cargo run --release -p player5-bridge -- --source prolink --web apps/web/dist
```

Open `http://<this-machine's-ip>:17505/` on any browser in the booth (or
`http://localhost:17505/` on the same machine), choose **Bridge** as the
clock source, and the app follows the rig.

Without hardware, try the simulated clock:

```sh
cargo run -p player5-bridge -- --source sim --sim-bpm 124 --web apps/web/dist
```

## Options

| Option | Meaning |
|---|---|
| `--source sim\|prolink\|opus\|link` | Clock to follow (default `prolink`) |
| `--sim-bpm <bpm>` | Tempo of the simulated clock |
| `--port <port>` | TCP port (default 17505) |
| `--bind <address>` | Listen address (default all interfaces) |
| `--web <dir>` | Serve the built web app from `<dir>` |
| `--allow-origin <origin>` | Also let pages from `<origin>` (e.g. `https://example.org`) use the WebSocket; repeatable, `*` = any |
| `--device-number <n>` | Pro DJ Link device number to claim (default 5) |
| `--interface <ipv4>` | Address of the interface on the booth network |
| `--passive` | Listen only; do not announce a virtual device |
| `--prolink-port-base <port>` | Testing: Pro DJ Link on 127.0.0.1, ports `<port>`..`<port>+2` |
| `--verbose` | Log connections and source messages |

## In the booth

- Wire the laptop running the bridge to the same switch (or the mixer's
  link port) as the players. Pro DJ Link uses UDP ports 50000–50002; make
  sure the OS firewall allows them, plus TCP 17505 for browsers.
- Do not run rekordbox on the same machine: it owns the Pro DJ Link ports.
- Browsers on other machines must use `http://` to the bridge (an `https://`
  page cannot open a plain `ws://` connection to another host). That is why
  the bridge serves the app itself.
- Plain `http://` from another machine is not a *secure context*, so the
  browser withholds AudioWorklet, Web MIDI and the offline service worker.
  The app still plays, using its main-thread audio fallback, which is more
  prone to dropouts under heavy UI load. For the best timing, run the
  browser on the bridge machine itself and open `http://localhost:17505/`:
  `localhost` counts as a secure context, so the full engine runs.
- Only the app the bridge serves, pages on `localhost` and non-browser
  clients may use the WebSocket; any other web page open on the laptop is
  refused (it could otherwise switch the follow target mid-set). To use a
  copy of the app hosted elsewhere, start the bridge with
  `--allow-origin <that site's origin>` (ADR-0011).
- Stop it with Ctrl-C.

## Endpoints

| Path | What |
|---|---|
| `GET /ws` | WebSocket upgrade (the clock protocol) |
| `GET /bridge.json` | `{"protocol":1,"ws":"/ws","source":"…"}`, CORS-enabled, for discovery |
| anything else | Static files from `--web`, or a short landing page |

## Tests

```sh
cargo test -p player5-bridge
```

Unit tests cover SHA-1 (FIPS 180 vectors), base64 (RFC 4648 vectors), the
RFC 6455 handshake and framing rules, request parsing, static-path
traversal, the origin policy and the connection caps. Integration tests start a real server on a free port with the
simulated clock and drive it with a hand-rolled WebSocket client.

`examples/fake_booth.rs` plays two Pro DJ Link players on loopback for a
bridge started with `--prolink-port-base`; the web app's Playwright suite
uses it to test the whole chain in a browser (`apps/web/README.md`).
