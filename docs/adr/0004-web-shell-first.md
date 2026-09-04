# ADR-0004: Web shell first; the browser consumes the C ABI as WASM

Status: Accepted (session 2). Amends the platform order in ADR-0001.

## Context

ADR-0001 set the platform order macOS → web → iOS, with macOS as the
protocol bench. After session 1 the only way to hear the instrument was the
offline CLI. Sound design and sequencer feel need a playable surface, and a
browser is the fastest one to ship, share and iterate on: no signing, no
audio-device setup, one link.

## Decision

1. **The web shell ships before the mac shell.** macOS remains the bench for
   the network clock sources (it needs UDP and a wired booth LAN), so
   sessions 3–5 still land there first; but UI, voices and feel iterate in
   the browser from now on.
2. **The browser consumes `core/ffi` directly.** The `cdylib` compiled to
   `wasm32-unknown-unknown` exports the same C functions Swift links. No
   `wasm-bindgen`, no glue crate: the module has zero imports, and the host
   moves bytes through `p5_alloc` / `p5_free`. One ABI, two consumers, so
   the FFI stays honest.
3. **The whole engine runs inside the AudioWorklet.** Control and render
   halves step in lockstep off the worklet's own sample clock, exactly like
   the offline renderer. The main thread posts pattern bytes and transport
   through the port and receives the playhead back. This keeps "nothing
   audible from the UI thread" (ADR-0001) and needs no SharedArrayBuffer.
   Clock sources that live on the main thread (`BridgeClock`,
   `WebMidiClock`) will post timing updates through the same port.
4. **Verification.** `apps/web/scripts/verify-wasm.mjs` renders every
   pattern through the WASM module in Node and requires the PCM hash to equal
   the native golden master. CI runs it, plus a Playwright smoke test that
   starts the worklet in headless Chromium and watches the playhead move.

## Consequences

- Pattern edits reach the core as a whole-pattern JSON reload. Parsing runs
  on the worklet thread between blocks; it is small (~100 bytes) and
  allocation happens off the render path, but a per-step FFI call may
  replace it if it ever shows up in profiles.
- `p5_abi_version` is now 2 (allocation helpers, `set_stop_after`,
  `position`, `playing_step`).
- The `wasm` Cargo profile (small, stripped, `panic = "abort"`) builds the
  browser module; `scripts/build-wasm.sh` drops it in `apps/web/public/`.
  The file is generated and not committed.
- Not covered yet: service worker for offline/installable PWA. The manifest
  and relative-path build are in place.
