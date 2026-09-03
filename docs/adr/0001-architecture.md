# ADR-0001: One Rust core, lookahead scheduling, pluggable clocks

Status: Accepted (session 1)

## Context

player5 is a TR-inspired drum machine for the DJ booth on macOS, web and iOS.
It feeds a channel of a Pioneer/AlphaTheta mixer and follows the rig's tempo
and phase over the network. Timing is the product: the sequencer must be
sample-accurate and the audio render path must never block. Three platforms
with three audio APIs must sound identical and stay in step.

## Decision

1. **One Rust core** holds the DSP voices, the sequencer and the clock/sync
   layer. Platform shells contain only audio-device plumbing and UI.
2. **Targets:** a static library with a C ABI (headers via `cbindgen`)
   consumed by Swift; macOS and iOS share one SwiftUI multiplatform package
   and play audio through AVAudioEngine. The web app is WASM + AudioWorklet.
3. **Lookahead scheduler.** The control thread schedules events about 100 ms
   ahead against the audio sample clock and pushes them into a lock-free
   single-producer/single-consumer queue. The render callback consumes the
   queue and fires voices at exact sample offsets. Nothing audible is ever
   triggered from the UI thread. (Pattern: "A Tale of Two Clocks", see
   `docs/protocols/lookahead-scheduling.md`.)
4. **Pluggable clock sources** behind one trait (`sync::ClockSource`), all
   feeding a PLL: `Internal`, `AbletonLink`, `ProDjLink` (CDJ-3000/XDJ beat
   packets + high-precision position), `OpusQuad` (rekordbox-lighting
   impersonation; BPM + beat count only, phase coarse to ±200 ms, so
   interpolate), `MidiClockIn` (tempo only, jittery; smooth it),
   `BridgeClock` and `WebMidiClock` (browser variants), `Tap`. Global
   controls regardless of source: phase nudge, latency offset in ms,
   quantized re-sync.
5. **Voices are synthesized**, never sampled: 808-style kick (bridged-T
   resonator + click transient), snare (two detuned oscillators + filtered
   noise), open/closed hats (six detuned squares → band-pass/high-pass),
   plus toms, clap, rimshot, cowbell. TR-style Accent, flam, shuffle and
   per-voice Tune/Decay/Snappy behaviour. A sample layer for 909-style
   hats/cymbals may come later using only our own recordings.
6. **Dumb master output:** full-range, mono-compatible, default peaks ≈ −6
   dBFS, a single output-gain control, optional soft safety limiter. No
   master EQ or compression.
7. **Web scope:** a real product surface (installable, offline-capable PWA;
   patterns serialized into the URL hash; pure static hosting). No
   SharedArrayBuffer: a single-threaded worklet avoids COOP/COEP headers. The
   AudioContext resumes on first user gesture. Browsers cannot open UDP
   sockets, so `apps/bridge/` (a headless binary built from the same `sync`
   crate) joins the booth LAN and serves tempo/beat/phase over WebSocket.
   `WebMidiClock` covers Chromium (DJM MIDI clock over USB); Safari has no
   Web MIDI, so feature-detect and hide.
8. **Platform order: macOS → web → iOS.** macOS is the protocol dev bench
   (wired Ethernet, class-compliant audio interfaces, no multicast
   entitlement). The iOS multicast entitlement is requested immediately
   anyway because approval takes weeks.
9. **No Roland trademarks or trade dress** in code, UI or docs.

## Consequences

- The render path is reviewed against real-time rules; violations block.
- The FFI surface stays tiny and C-ABI stable.
- Protocol knowledge is digested into `docs/protocols/` with a source per
  fact, never left in code comments.
- Rendering offline through the same engine as the shells makes
  golden-master tests possible (session 1) and keeps the CLI harness honest.
- Session 1 delivers only the `Internal` clock and the kick voice; the trait
  and event model are shaped for the rest.
