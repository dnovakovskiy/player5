# ADR-0010: Web delivery: runtime fallbacks, single-file artifact, offline PWA, browser time mapping

Status: Accepted (web product session). Amends ADR-0004 (the AudioWorklet
stays the preferred runtime; this adds fallbacks and a second artifact).

## Context

ADR-0004 put the whole engine in an AudioWorklet running `core/ffi` as
WebAssembly. The web app is now the full instrument: ten voices, clock
following (bridge, MIDI, tap), installable and offline. It must also work
as a *shareable try-it page* opened inside sandboxed viewers and embeds we
do not control. Those environments break the ADR-0004 assumptions in
predictable ways:

- A strict Content-Security-Policy (`default-src 'none'; script-src
  'unsafe-inline'`) forbids compiling WebAssembly (no `'wasm-unsafe-eval'`)
  and loading worklet modules from `blob:` or any URL.
- Insecure origins (plain `http://` on a LAN, e.g. a page served by
  `apps/bridge --web`) have no `AudioWorklet` and no service worker.
- The viewer serves one file: no sibling `.wasm`, no `worklet.js`, no
  network at all, only bare `#anchors` reach `location.hash`, clipboard
  reads never work, and `WebSocket`/device APIs are blocked.

Following an external clock in a browser also needs one precise mapping
from event times (performance clock) to the engine's sample clock.

## Decision

### 1. One engine host, three runtimes

`src/engine/host.js` drives one lockstep `P5Engine` block by block: it
applies control messages between blocks, renders, computes the meter and
posts playhead/status. The same file runs in every runtime; the page picks
the first that works (or the one forced with `?engine=`):

| mode | core | where it renders | when |
|------|------|------------------|------|
| `worklet` | WebAssembly | AudioWorklet | default |
| `script` | WebAssembly | ScriptProcessorNode on the main thread | `addModule` fails (insecure origin, CSP) |
| `js` | wasm2js build of the same module | ScriptProcessorNode | WebAssembly compilation fails (CSP) |

- The worklet module is not a URL on the server: the bundle contains the
  host and processor sources (Vite `?raw`), joined into a `Blob` URL for
  `audioWorklet.addModule`. The compiled `WebAssembly.Module` travels in
  `processorOptions`.
- In the fallbacks the audio callback is still the engine's clock: the
  scheduler runs in lockstep with rendering and keeps its 100 ms
  lookahead, so timing stays sample-accurate. Main-thread jank can cause
  dropouts there, which is why it is only a fallback.
- The JS core is produced at build time from `public/player5.wasm` by
  Binaryen (`binaryen` npm dev-dependency): rustc's bulk-memory,
  non-trapping float-to-int and sign-extension ops are lowered exactly,
  unused exports are dropped, then `wasm2js` translates. JavaScript
  numbers plus `Math.fround` reproduce IEEE f32 arithmetic, and
  ADR-0002's `dsp::math` avoids platform transcendental functions, so the
  output is bit-identical: `npm run verify-wasm` renders every pattern in
  `patterns/` through both the wasm module and the JS core and compares
  them with the native golden hashes. CI runs it.
- wasm2js cannot pass `i64` across its boundary, so `core/ffi` gains
  additive web variants: `p5_engine_position_f64` and
  `p5_engine_set_stop_after_u32`. The `u64` originals are unchanged; no ABI
  version bump.

### 2. Time mapping (browser clocks → engine samples)

- **Engine samples** count frames the host rendered since the engine was
  created. At the start of each block the host knows the context frame of
  that block (`AudioWorkletGlobalScope.currentFrame`, or
  `playbackTime × sampleRate` in a ScriptProcessor) and reports
  `offset = contextFrame − enginePosition` whenever it changes (it is
  constant while the graph runs).
- **Main thread:** `AudioContext.getOutputTimestamp()` pairs
  `{contextTime, performanceTime}`: the context time being heard at that
  performance time. For an event at performance time `t` (ms):
  `contextTime_at(t) = contextTime + (t − performanceTime) / 1000`,
  `frame = contextTime_at(t) × sampleRate`,
  `engine sample = frame − offset`. The pair jitters by about a callback
  period, so `src/engine/timemap.ts` keeps a smoothed estimate of
  `contextTime − performanceTime` and only jumps on real discontinuities.
- An observation therefore lands on the engine sample whose audio is
  audible at `t`; the latency-offset control covers the rest of the path
  (interface, mixer). Clock messages (`observe`, `midi`, `tap`) carry
  engine samples and are applied by the host between blocks.
- **BridgeClock** (`docs/protocols/bridge-websocket.md`): ping burst then
  one ping every 2 s, offset from the lowest-RTT recent exchange; every
  timeline and a 50 ms timer produce an observation for "now"
  (`Bar` phase when `bar_aligned`, else `Beat`) with the timeline's
  precision as clock mode. Reconnects with exponential backoff.
- **WebMidiClock:** Start/Continue/Stop/Clock with the event's own
  `timeStamp` go to `p5_engine_midi`; follow mode `jittery`.
- **Tap:** the pointer event's `timeStamp` goes to `p5_engine_tap`; the
  tempo estimate is also stored in the pattern.

### 3. Two build artifacts from one source

- `npm run build` → `dist/`: the PWA for static hosting. A hand-written
  service worker template (`src/sw.js`) is filled at build time with the
  list of everything the build emitted and a cache name derived from a
  hash of those files. Install precaches the shell; activate deletes older
  `player5-*` caches; navigations are network-first (4 s timeout) with the
  cached shell as offline fallback; hashed assets and other precached
  files are cache-first (matching ignores `Vary`, which static servers set
  for CORS). Registered only in production builds on http(s) origins.
  Manifest icons (192, 512, maskable 512) are drawn procedurally and
  PNG-encoded with Node's zlib at build time.
- `npm run build:single` → `dist-single/player5.html`: one file with all
  JS and CSS inline, the wasm base64-embedded and the JS core inline. No
  service worker, manifest or icons; bridge and discovery code is compiled
  out (`__P5_SINGLE__`). The build fails if the output contains `http:`,
  `https:`, `fetch(`, `importScripts`, `XMLHttpRequest` or `sendBeacon`,
  or if `<title>` is not within the first 8 KB. It paints its own dark
  theme (`color-scheme: dark`, explicit backgrounds) because the host may
  paint its own behind it. Patterns travel as a *pattern code* (the
  base64url payload of a `#p=` link) with an Import field that accepts a
  code or a full link by typing or the paste event.

### 4. Pattern state

The `#p=<base64url(JSON)>` hash stays exactly as in ADR-0004 and old
kick-only links load (missing fields take the core's defaults). The JSON
is compacted: silent voices with default controls and default-valued
fields are omitted, which the core fills back identically. The hash is
written at most every 120 ms (browsers throttle `history.replaceState`).

## Consequences

- One host script and one set of FFI calls serve every runtime, so a page
  that cannot run WebAssembly still plays the same instrument, proven
  bit-identical by CI.
- The JS core is ~290 KB minified (a lazy chunk in the PWA, inline in the
  single file). It is regenerated whenever `public/player5.wasm` is newer.
- `apps/bridge --web` serves the app over plain `http://` on the LAN: no
  AudioWorklet, service worker or Web MIDI there, so the app runs in
  `script` mode. Booths that want the worklet open the app from
  `localhost` or HTTPS.
- Single-file viewers get Internal and Tap clocks only.
- Parsing pattern JSON still happens on the audio thread between blocks
  (ADR-0004); unchanged.

## Sources

- AudioWorkletGlobalScope `currentFrame`, `AudioContext.getOutputTimestamp()`
  and AudioWorklet's secure-context requirement: Web Audio API,
  https://webaudio.github.io/web-audio-api/
- `'wasm-unsafe-eval'`: Content Security Policy Level 3,
  https://www.w3.org/TR/CSP3/
- wasm2js and the lowering passes: Binaryen, https://github.com/WebAssembly/binaryen
- `CacheQueryOptions.ignoreVary`: Service Workers,
  https://w3c.github.io/ServiceWorker/
- `MIDIMessageEvent` timestamps: Web MIDI API, https://www.w3.org/TR/webmidi/
