# apps/web

The browser product surface: the instrument a DJ uses in the booth and a
page anyone can try from a shared link. Vanilla TypeScript + CSS around the
Rust core (`core/ffi`) compiled to WebAssembly. Decisions: ADR-0004 (web
shell, worklet) and ADR-0010 (runtime fallbacks, single-file build, service
worker, time mapping).

```sh
scripts/build-wasm.sh          # from the repo root; needs the wasm32 target
cd apps/web
npm install
npm run dev                    # http://localhost:5173
npm run check                  # tsc
npm run verify-wasm            # wasm AND JS core renders == native golden hashes
npm run build                  # dist/: the PWA
npm run build:single           # dist-single/player5.html: one self-contained file
npm test                       # Playwright (needs both builds)
```

## The instrument

- Ten voice rows (BD SD LT MT HT RS CP CH OH CB) × 16 steps, grouped in
  fours, live playhead. Tap a step: off → hit → accent → off. **Flam edit**
  makes taps toggle flams (`f`/`F` in the pattern). Each row has a label
  (select the voice for its controls panel) and a mute.
- Transport: Play/Stop (Space), BPM (number, −/+, Shift for 0.1, Tap or
  `T`), shuffle, accent, flam amount, presets, clear, undo (Ctrl/Cmd+Z,
  redo with Shift or Ctrl+Y), share.
- Master: output gain in dB, safety limiter, peak meter in dBFS (computed
  from the rendered blocks inside the audio runtime) with the −6 dBFS mark.
- Clock: Internal, Tap, Bridge (`apps/bridge` over WebSocket, see
  `docs/protocols/bridge-websocket.md`) and MIDI (Web MIDI clock in);
  engine tempo, lock and bar position; phase nudge (1 ms, Shift 10 ms),
  latency offset (remembered), re-sync.
- The pattern lives in the URL (`#p=<base64url JSON>`, old kick-only links
  still load) and as a copyable *pattern code*; the Import field takes
  either. No `alert`/`confirm`/`prompt`; copying falls back to a selected
  read-only field. Preferences use `localStorage` defensively.

## How it fits together

| file | role |
|------|------|
| `src/engine/host.js` | drives one lockstep engine: control messages between blocks, render, meter, status. Runs in every runtime. |
| `src/engine/worklet.js` | the AudioWorkletProcessor; joined with `host.js` into a Blob module at runtime |
| `src/engine/audio-engine.ts` | AudioContext, runtime choice (`worklet` → `script` → `js`, or `?engine=`), message routing |
| `src/engine/timemap.ts` | performance clock → engine sample mapping (`getOutputTimestamp` + the host's frame offset) |
| `src/clock/bridge.ts`, `src/clock/midi.ts` | BridgeClock and WebMidiClock clients |
| `src/spec.ts` | pattern format (shared with `core/engine/src/spec.rs`), URL hash and pattern code |
| `src/ui/*` | page template, grid, clock panel, meter |
| `src/sw.js` | service worker template (filled in at build time) |
| `scripts/gen-core.mjs` | `public/player5.wasm` → `src/generated/core.js` via Binaryen wasm2js (gitignored, regenerated when stale) |
| `scripts/icons.mjs` | procedural PNG icons (Node zlib, no image deps) |
| `scripts/verify-wasm.mjs` | renders every `patterns/*.json` through wasm and JS core; both must match the goldens |
| `vite.player5.ts` | build plugin: virtual modules, icons, service worker, single-file inlining and its no-network check |

Runtimes (ADR-0010): the default is WebAssembly in an AudioWorklet. If the
worklet cannot load (insecure origin, CSP) the same host renders on the
main thread in a ScriptProcessorNode; if WebAssembly is blocked, it uses the
wasm2js build of the same core. The header shows which one is active. The
core is compiled when the page loads; audio starts only from Play (or
Space). In the PWA build the wasm ships content-hashed
(`assets/player5-<hash>.wasm`), like the JS, so the service worker can
never pair one build's JavaScript with another build's core.

The bar readout and the playhead show what is *heard*: the runtime reports
the render position, which leads the speakers by the output latency, and
`AudioEngine.beatHeardAt()` maps it back through the same time mapping the
clock sources use.

The single-file build has no service worker, manifest or network code at
all; Bridge and MIDI are hidden there. It is published next to the PWA as
`player5-standalone.html` by `.github/workflows/pages.yml`.

## Tests

`npm test` runs Playwright against `vite preview` (bound to 127.0.0.1:4173)
and against `dist-single/player5.html` (via `file://` and under a strict
CSP with `page.setContent`). Headless Chromium plays real audio with
`--autoplay-policy=no-user-gesture-required`. Covered: every voice, flams,
mute, presets, undo, old links, garbage in the hash and the import field,
master, share/import, phone layout, the skip link, all three runtimes (the
playhead steps forward one step at a time in each), Play then Stop while
audio is still starting, an insecure LAN origin like `apps/bridge --web`
(ScriptProcessor, MIDI hidden with the reason), MIDI clock (fake
`requestMIDIAccess` at 125 BPM: tempo, lock and phase), the bridge
protocol (an in-test RFC 6455 server at 128 BPM: lock, phase within 0.1
beat in the worklet and the ScriptProcessor, devices, follow, reconnect
with the follow target re-sent, discovery only on demand), offline reload
through the service worker, a new deploy winning over the cache, and the
single-file page making zero requests. The artifact fragment
(`dist-single/player5-artifact.html`) is loaded inside a strict-CSP host
page at 1280 px and as an emulated phone: no horizontal scroll, no
requests, share/import, and every voice alone reaching the meter. Against
the real bridge binary (`cargo build -p player5-bridge --bins --examples`
first; skipped otherwise): the simulated clock, and Pro DJ Link from
`apps/bridge/examples/fake_booth.rs` (two players on loopback) with lock,
tempo, bar alignment within 1/20 beat of the players' own downbeats, a
follow switch that lands in under 0.9 s, and a bridge restart the page
rides through without leaving the followed deck's bar. Every test fails on page errors,
console errors (except ones it expects, like a refused bridge connection)
and alert/confirm/prompt dialogs. A pre-installed Chromium at
`/opt/pw-browsers/chromium` is used when present (or `PW_CHROMIUM`); CI
installs Playwright's own.
