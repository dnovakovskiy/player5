# Protocol notes

Digested, not copied: each file states what we rely on, and every fact
carries a link to the source it came from. Packet captures used as test
fixtures live next to the notes. Nothing here is folklore; if a fact has no
source, it does not go in.

| Note | Covers | Status |
|------|--------|--------|
| [lookahead-scheduling.md](lookahead-scheduling.md) | The two-clock scheduling pattern the sequencer is built on | Digested; `sequencer`, `engine::Control` |
| [pro-dj-link.md](pro-dj-link.md) | CDJ / XDJ / DJM beat, status and precise-position packets, joining as a fifth device | Digested; `sync::prolink`, fixtures in [`fixtures/prolink/`](fixtures/prolink/) |
| [opus-quad.md](opus-quad.md) | Opus Quad via rekordbox-lighting impersonation: deck status, coarse beat timing | Digested; `sync::opus`, fixtures in [`fixtures/opus-quad/`](fixtures/opus-quad/) |
| [ableton-link.md](ableton-link.md) | Link timeline, session tempo, phase alignment, `rusty_link` | Digested; `sync::link` (feature `ableton-link`, ADR-0005) |
| [midi-clock.md](midi-clock.md) | MIDI 1.0 clock, Web MIDI, CoreMIDI Universal MIDI Packet input | Digested; `sync::midi`, `apps/web/src/clock/midi.ts`, `apps/mac` `MIDIClockInput` |
| [bridge-websocket.md](bridge-websocket.md) | Our own bridge ↔ browser WebSocket protocol (not third-party) | Specified, version 1; `apps/bridge`, `apps/web/src/clock/bridge.ts` |

Sources that could not be read from the build container are named as such
in each note (midi.org, w3.org, djl-analysis.deepsymmetry.org and github.com
HTML pages were unreachable; their repositories were read through
`raw.githubusercontent.com` where one exists). Hardware behaviour nobody
has published stays under each note's open questions.

## Reference material

- Deep Symmetry `beat-link` (Java reference implementation):
  https://github.com/Deep-Symmetry/beat-link
- Deep Symmetry `dysentery` (protocol analysis, the "Packet Analysis" PDF):
  https://github.com/Deep-Symmetry/dysentery —
  https://djl-analysis.deepsymmetry.org/
- `prolink-connect` (TypeScript implementation):
  https://github.com/EvanPurkhiser/prolink-connect
- `kyleawayan/opus-quad-pro-dj-link-analysis` (Opus Quad packet captures):
  https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis
- Ableton Link C++ SDK: https://github.com/Ableton/link
- `rusty_link` crate: https://github.com/anzbert/rusty_link
- Chris Wilson, "A Tale of Two Clocks – Scheduling Web Audio with Precision":
  https://web.dev/articles/audio-scheduling
- Apple CoreMIDI reference: https://developer.apple.com/documentation/coremidi
- MIDI Association, Universal MIDI Packet (UMP) Format and MIDI 2.0
  Protocol (M2-104-UM):
  https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification
  (not reachable so far; see midi-clock.md)
