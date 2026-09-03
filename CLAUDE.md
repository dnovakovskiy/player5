# player5 — project guide

TR-inspired drum machine for the DJ booth: macOS, web and iOS shells over one
Rust core. It feeds a channel of a Pioneer/AlphaTheta mixer (DJM series or
Opus Quad) and syncs tempo/phase with the rig over the network. Pro DJ Link
decks number 1–4; this app is the fifth device.

## Non-negotiables

- **Timing is the product.** Sample-accurate sequencing. The render path is
  real-time safe: no allocations, locks, syscalls, blocking or logging.
- **Synthesized voices only.** No ripped samples. A sample layer for
  909-style hats/cymbals may come later, using only recordings we make.
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
- Clock sources implement `sync::ClockSource`; all feed a PLL (arrives with
  the first network source). Global controls regardless of source: phase
  nudge, latency offset (ms), quantized re-sync.
- Deterministic DSP math: the render path uses `dsp::math`, not `libm`, so
  golden-master hashes are identical on every platform (ADR-0002).
- Platform order: macOS → web → iOS.

## Layout

```
Cargo.toml         # workspace root (crates live in core/)
CLAUDE.md
core/dsp           # voices + master; real-time safe; forbid(unsafe_code)
core/sequencer     # Pattern, Event, queue (the only unsafe), Scheduler
core/sync          # ClockSource trait, InternalClock, ClockControls
core/engine        # Control / Renderer / Engine, JSON pattern spec
core/ffi           # C ABI (lib name `player5`), cbindgen.toml
core/render        # `render` CLI, WAV output, analysis, golden tests
patterns/          # example pattern files; also the golden-test fixtures
apps/mac           # SwiftUI + AVAudioEngine shell (session 2)
apps/web           # Vite + TS + AudioWorklet PWA (session 6)
apps/bridge        # headless sync → WebSocket relay (session 4)
apps/ios           # shares the Swift package with mac (session 7)
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
```

CI runs fmt, clippy and tests on Linux and macOS; both must produce the same
golden hashes.

## Pattern files

JSON, documented in `core/engine/src/spec.rs`. Steps are a 16-character
string: `-`/`.` off, `x` hit, `X` accented hit; spaces are ignored. Voice
controls and render settings are all `0..=1` with defaults.

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
- **Dependencies.** Ask before adding anything beyond `serde`, `serde_json`
  and `hound`. `cpal`, `rusty_link`, WebSocket and WASM tooling arrive in
  their own sessions.
- **Conventions.** `cargo fmt` defaults; clippy clean with `-D warnings`;
  `missing_docs` on public items; unit tests next to the code, integration
  tests in `tests/`.

## Roadmap (one concern per session)

1. ✅ Scaffold, CLAUDE.md, ADR-0001; 16-step sequencer with accent + shuffle,
   internal clock, kick voice, offline render, golden tests, CLI, CI.
2. mac shell: AVAudioEngine playback, minimal SwiftUI step grid.
3. Ableton Link source (mac) + PLL layer.
4. Pro DJ Link source + `apps/bridge/` WebSocket relay.
5. Opus Quad mode.
6. Web shell: WASM + AudioWorklet PWA, URL-hash patterns, `BridgeClock` +
   `WebMidiClock`.
7. iOS shell (needs the multicast entitlement — see
   `docs/ios-multicast-entitlement.md`); remaining voices fill in along the way.
