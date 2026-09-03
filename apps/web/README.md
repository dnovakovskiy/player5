# apps/web

Vite + TypeScript + AudioWorklet PWA. Arrives in session 6.

Plan: the Rust core compiled to WASM and instantiated inside a
single-threaded AudioWorklet (no SharedArrayBuffer, so no COOP/COEP
headers); the scheduler ticks on the main thread and posts events through
the worklet port. Installable, offline-capable, patterns in the URL hash,
static hosting only. `BridgeClock` (WebSocket to `apps/bridge`) and
`WebMidiClock` (Chromium only; feature-detect).
