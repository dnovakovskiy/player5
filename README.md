# player5

A TR-inspired drum machine for the DJ booth. It feeds a channel of a
Pioneer/AlphaTheta mixer and locks tempo and phase to the rig over the
network. Pro DJ Link decks are numbered 1–4; this is the fifth device.

Ten synthesized voices (kick, snare, three toms, rimshot, clap, closed and
open hats, cowbell) with accent, flam, shuffle and per-voice tune, decay,
tone, snappy and level. One Rust core runs everywhere: in the browser
(WebAssembly), in the macOS and iOS apps (static library), and in the
headless bridge.

## Try it

- **In a browser:** build and run the web app (below), or open the
  single-file build `apps/web/dist-single/player5.html` straight from disk.
- **In the booth:** run the bridge on a laptop wired to the DJ network and
  open the app it serves:

  ```sh
  scripts/build-wasm.sh && (cd apps/web && npm install && npm run build)
  cargo run --release -p player5-bridge -- --source prolink --web apps/web/dist
  # then open http://localhost:17505/ and pick Bridge as the clock source
  ```

  `--source opus` follows an Opus Quad; `--source sim --sim-bpm 124` needs no
  hardware. See `apps/bridge/README.md`.

## Develop

```sh
rustup target add wasm32-unknown-unknown
scripts/build-wasm.sh
cd apps/web && npm install && npm run dev      # http://localhost:5173

cargo test --workspace                          # core, bridge, goldens
cargo run -p render -- patterns/kit-warehouse.json out.wav --fingerprint
```

macOS/iOS: `scripts/build-xcframework.sh`, then `apps/mac` (Swift package)
and `apps/ios` (XcodeGen). See their READMEs.

## Where things are

- `CLAUDE.md`: decisions, conventions, commands, status.
- `docs/adr/`: architecture decision records.
- `docs/protocols/`: digested protocol notes (Pro DJ Link, Opus Quad,
  Ableton Link, MIDI clock, the bridge's WebSocket protocol), each fact with
  its source.
