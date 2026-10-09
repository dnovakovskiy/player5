# player5 — project guide

TR-inspired drum machine for the DJ booth: macOS, web and iOS shells over one
Rust core. It feeds a channel of a Pioneer/AlphaTheta mixer (DJM series or
Opus Quad) and syncs tempo/phase with the rig over the network. Pro DJ Link
decks number 1–4; this app is the fifth device.

## Non-negotiables

- **Timing is the product.** Sample-accurate sequencing. The render path is
  real-time safe: no allocations, locks, syscalls, blocking or logging.
- **Synthesized voices only.** No ripped samples. A sample layer for
  sampled-era hats/cymbals may come later, using only recordings we make.
- **No Roland trademarks or trade dress** anywhere: code, UI, docs, commit
  messages. "TR-inspired" is the ceiling. Never name a specific Roland model.
- **Dumb master output.** Full-range, mono-compatible, default peaks ≈ −6
  dBFS, one output-gain control, optional soft safety limiter. No master EQ or
  compression; the mixer's channel strip does that.

## Architecture (decided; see ADR-0001, don't relitigate)

- One Rust core: `dsp` (voices, master), `sequencer` (pattern, events,
  lock-free queue, lookahead scheduler), `sync` (clock sources, controls),
  `engine` (wires them into a control half and a render half), `ffi` (C ABI
  for Swift), `render` (offline CLI + analysis for golden tests).
- Targets: static lib + C ABI → Swift (macOS/iOS share one SwiftUI package,
  audio via AVAudioEngine); WASM + AudioWorklet → web PWA.
- Lookahead scheduling: the control thread schedules ~100 ms ahead on the
  audio sample clock into an SPSC event queue; the render callback consumes
  it. Parameter changes travel through the same queue (ADR-0003). Nothing
  audible is ever triggered from the UI thread.
- Clock sources: the internal clock implements `sync::ClockSource`; every
  external source (Pro DJ Link, Opus Quad, Ableton Link, MIDI clock, tap,
  the browser bridge) produces `sync::Observation`s that steer a
  `sync::FollowerClock` (the PLL, ADR-0006). Global controls regardless of
  source: phase nudge, latency offset (ms), quantized re-sync.
- Browsers get the network clock from `apps/bridge` over WebSocket
  (`docs/protocols/bridge-websocket.md`, ADR-0007); the bridge can also serve
  the built web app.
- Ableton Link is behind the off-by-default `ableton-link` feature because
  the Link SDK is GPLv2+ (ADR-0005). Distribution with it on is the owner's
  call.
- The kit mix carries one fixed headroom trim so busy grooves peak near
  −6 dBFS (ADR-0009).
- Deterministic DSP math: the render path uses `dsp::math`, not `libm`, so
  golden-master hashes are identical on every platform (ADR-0002).
- The browser runs `core/ffi` compiled to WASM inside an AudioWorklet, whole
  engine in lockstep, no wasm-bindgen (ADR-0004). The WASM render must stay
  bit-identical to the native goldens (`npm run verify-wasm`).
- Platform order: web (UI, voices, feel) and macOS (network clock bench) in
  parallel, then iOS (ADR-0004 amends ADR-0001).

## Layout

```
Cargo.toml         # workspace root (crates live in core/)
CLAUDE.md
core/dsp           # ten voices, kit mixer, master; real-time safe; forbid(unsafe_code)
core/sequencer     # Pattern (accent, flam, mute, shuffle), Event, SPSC queue, Scheduler
core/sync          # clocks, FollowerClock (PLL), MIDI clock, tap, host time,
                   #   prolink/, opus/, link.rs (feature), net (source threads)
core/engine        # Control / Renderer / Engine, SharedTiming, JSON pattern spec
core/ffi           # C ABI (lib name `player5`): P5Engine (browser), P5Control/P5Renderer (Apple)
core/render        # `render` CLI, WAV output, analysis, golden tests
patterns/          # pattern files; also the golden-test fixtures
apps/web           # Vite + TS app: AudioWorklet / ScriptProcessor / JS-core runtimes, PWA
apps/bridge        # player5-bridge: network clock -> WebSocket (+ serves the web app)
apps/mac           # Swift package (Player5Kit) + macOS app; scripts/build-xcframework.sh
apps/ios           # XcodeGen iOS app over the same Swift package
docs/adr           # numbered, append-only decision records
docs/protocols     # digested protocol notes + packet fixtures, with sources
```

## Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p render -- patterns/four-on-the-floor.json out.wav --fingerprint
UPDATE_GOLDEN=1 cargo test -p render --test golden   # after an intended sound change
scripts/gen-header.sh                                 # C header (needs cbindgen)

scripts/build-wasm.sh                                 # core/ffi → apps/web/public/player5.wasm
cd apps/web && npm install && npm run dev             # web UI at http://localhost:5173
cargo build -p player5-bridge                         # needed by the real-bridge Playwright test
cd apps/web && npm run check && npm run verify-wasm && npm run build && npm run build:single && npm test
cd apps/web && npm run build:artifact                 # single file as a host-wrapped fragment
cargo run -p player5-bridge -- --source sim --web apps/web/dist   # try the bridge without hardware
cargo test -p sync -p player5-bridge -p player5-ffi --features "sync/ableton-link player5-bridge/ableton-link player5-ffi/ableton-link"
scripts/build-xcframework.sh && (cd apps/mac && swift build && swift test)   # macOS only
```

CI (`.github/workflows/`): `ci.yml` runs fmt, clippy and tests on Linux and
macOS (both must produce the same golden hashes), the ableton-link feature
build, and the web job (WASM + JS core vs goldens, builds, Playwright incl.
the real bridge). `apple.yml` builds the XCFramework, the Swift package and
its tests, and the iOS and macOS apps. `pages.yml` builds the web app and
deploys it to GitHub Pages once Pages is enabled (see the file header).

## Pattern files

JSON, documented in `core/engine/src/spec.rs`. Ten voices: `kick`, `snare`,
`low_tom`, `mid_tom`, `high_tom`, `rim`, `clap`, `closed_hat`, `open_hat`,
`cowbell`. Steps are a 16-character string: `-`/`.` off, `x` hit, `X`
accented hit, `f`/`F` flammed hit (grace note before the grid); spaces are
ignored. Per-voice `tune`, `decay`, `tone`, `snappy`, `level` (`0..=1`) and
`mute`; pattern-level `bpm`, `shuffle`, `accent`, `flam`.

## Working agreements

- **Real-time rules are review-blocking.** Anything reachable from
  `Renderer::process` or a `Voice::process` must not allocate, lock, block,
  log, do I/O, or call `std` transcendental math. Use `dsp::math`.
- **Golden masters.** Any change that alters rendered audio must regenerate
  `core/render/tests/golden/` in the same commit and say why. The test
  distinguishes "inaudible numeric change" (hash only) from "the sound
  changed" (fingerprint too).
- **FFI stays tiny and C-ABI stable.** Add functions only when a shell needs
  them; bump `p5_abi_version` on breaking changes; regenerate the header.
- **Protocol knowledge lives in `docs/protocols/`** with a source link per
  fact. Never as folklore in code comments.
- **Significant decisions get an ADR.** Superseding beats editing.
- **Dependencies.** Ask before adding anything beyond `serde`, `serde_json`,
  `hound` and the feature-gated `rusty_link` (ADR-0005). The bridge's
  WebSocket server is std-only on purpose. npm dev tooling: vite,
  typescript, @playwright/test, binaryen (wasm2js for the JS core).
- **Conventions.** `cargo fmt` defaults; clippy clean with `-D warnings`;
  `missing_docs` on public items; unit tests next to the code, integration
  tests in `tests/`.

## Status

Built: the ten-voice core with flam/mute/accent/shuffle, the phase-locked
follower, Pro DJ Link, Opus Quad, Ableton Link (feature), MIDI clock and tap,
the bridge, the web app (PWA, offline, three audio runtimes, single-file
build), and the macOS and iOS shells (CI-built, not yet run on hardware).

Still open:

1. Hardware verification in a booth: CDJ-3000/XDJ and DJM over Pro DJ Link,
   an Opus Quad, Link peers, a DJM's MIDI clock over USB, output latency
   compensation on real interfaces (see ADR-0006/0008 and the protocol notes'
   limitations sections).
2. iOS networking waits for the multicast entitlement
   (`docs/ios-multicast-entitlement.md`).
3. Ableton Link distribution licence decision (ADR-0005).
4. GitHub Pages: a repo admin enables Pages (Source: GitHub Actions) and sets
   the repository variable `PAGES_ENABLED=true`.
5. A sample layer for sampled-era hats/cymbals, from our own recordings.
