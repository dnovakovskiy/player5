# apps/web

The browser product surface: a Vite + TypeScript app around the Rust core
compiled to WebAssembly and hosted inside an AudioWorklet.

```sh
scripts/build-wasm.sh        # from the repo root; needs the wasm32 target
cd apps/web
npm install
npm run dev                  # http://localhost:5173
npm run check                # tsc
npm run verify-wasm          # wasm render == native golden hashes
npm run build && npm test    # production build + Playwright smoke test
```

How it fits together (ADR-0004):

- `core/ffi` compiled to `wasm32-unknown-unknown` is the *same* C ABI the
  Swift shells link. No wasm-bindgen; the module has no imports.
- `public/worklet.js` instantiates it inside the AudioWorklet and drives the
  whole engine (scheduler + renderer) in lockstep off the worklet's own
  sample clock. Single-threaded, so no SharedArrayBuffer and no COOP/COEP.
- `src/audio.ts` owns the AudioContext (created on the first Play, per
  autoplay policy) and posts pattern bytes and transport through the port.
- `src/spec.ts` is the pattern format and the `#p=` URL-hash codec: the
  whole pattern lives in the link.
- `vite.config.ts` uses relative paths: the `dist/` folder works on any
  static host.

Not yet: a service worker for offline/installable PWA, `BridgeClock` and
`WebMidiClock` (they arrive with the sync sessions), voices beyond the kick.
